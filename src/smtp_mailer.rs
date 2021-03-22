use anyhow::{anyhow, Context};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{
    message::{header, MultiPart, SinglePart},
    Message, SmtpTransport, Transport,
};
use serde::Deserialize;
use std::ffi::OsStr;
use std::fmt::{self, Display};
use std::fs;
use std::path::Path;
use std::str::FromStr;
use toml;

#[derive(Debug)]
enum ContentType {
    Html,
    Plain,
}

pub type MailAddress = String;
pub type Attachments = Vec<Attachment>;

// Directly represented via a TOML file in which the user can configure the corresponding attributes
#[derive(Deserialize, Debug)]
pub struct MailConfiguration {
    username: String,
    password: String,
    sender: MailAddress,
    reply_to: MailAddress,
    mailserver: String,
}

pub struct SmtpMailer {
    email: lettre::Message,
    lettre_mailer: lettre::SmtpTransport,
}

#[derive(Debug)]
pub struct MailContent {
    subject: String,
    body: String,
    content_type: ContentType,
}

#[derive(Debug)]
pub struct Attachment {
    filename: String,
    content: Vec<u8>, // idiomatic rust binary content representation
}

impl Display for MailContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Content Type: {:#?}\n\n{}\n---\n{}", self.content_type, self.subject, self.body)
    }
}

impl Display for Attachment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.filename)
    }
}

impl SmtpMailer {
    // The default errors from lettre are very short and don't prodive much information,
    // thus this function performs a parse and returns a more useful error message
    fn parse_pretty_error<T>(mail: &String) -> Result<T, anyhow::Error>
    where
        T: FromStr,
        <T as FromStr>::Err: std::error::Error + Send + Sync + 'static,
    {
        mail.parse()
            .with_context(|| format!("Invalid email address: {}", mail))
    }

    fn add_attachment(a: &Attachment, m: MultiPart) -> MultiPart {
        m.singlepart(
            SinglePart::builder()
                .header(header::ContentType(
                    "application/octet-stream".parse().unwrap(),
                ))
                .header(header::ContentDisposition {
                    disposition: header::DispositionType::Attachment,
                    parameters: vec![header::DispositionParam::Filename(
                        header::Charset::Ext("utf-8".into()),
                        None,
                        a.filename.as_bytes().into(),
                    )],
                })
                .body(a.content.clone()),
        )
    }

    fn create_mail(
        recipient: &MailAddress,
        content: &MailContent,
        config: &MailConfiguration,
        attachments: &Attachments,
    ) -> anyhow::Result<Message> {
        // Mail with preliminary settings (from, reply to,...), content to be added
        let mail_prelude = Message::builder()
            .from(Self::parse_pretty_error(&config.sender)?)
            .reply_to(Self::parse_pretty_error(&config.reply_to)?)
            .to(Self::parse_pretty_error(&recipient)?)
            .subject(content.subject.clone());

        let mail_builder = MultiPart::mixed();
        // Add Mail body
        let header_content_type = match content.content_type {
            ContentType::Html => header::ContentType("text/html; charset=utf8".parse().unwrap()),
            ContentType::Plain => header::ContentType("text/plain; charset=utf8".parse().unwrap()),
        };
        // MultiPart::mixed() gives us a mail builder, but after applying singlepart on it,
        // we get a MultiPart, so this is a bit messy. I would ideally like to reuse the mail
        // builder and just incrementally build on the single variable.
        let mut mail_multipart = mail_builder.singlepart(
            SinglePart::builder()
                .header(header_content_type)
                .body(content.body.clone()),
        );

        // Add attachments
        for att in attachments {
            mail_multipart = Self::add_attachment(att, mail_multipart);
        }
        Ok(mail_prelude.multipart(mail_multipart)?)
    }

    pub fn new(
        recipient: &MailAddress,
        content: &MailContent,
        config: &MailConfiguration,
        attachments: &Attachments,
    ) -> anyhow::Result<SmtpMailer> {
        let email = Self::create_mail(recipient, content, config, attachments)?;
        let creds = Credentials::new(config.username.to_string(), config.password.to_string());

        let mailer = SmtpTransport::relay(&config.mailserver)
            .with_context(|| "Could not connect to mail server")?
            .credentials(creds)
            .build();
        Ok(SmtpMailer {
            email: email,
            lettre_mailer: mailer,
        })
    }

    pub fn send(&self) -> anyhow::Result<()> {
        self.lettre_mailer
            .send(&self.email)
            .with_context(|| "Could not send mail.")?;
        Ok(())
    }
}

// Reads path and dumps full file contents into a string, error if the file is not found
fn get_file_content<P>(path: P) -> anyhow::Result<String>
where
    P: AsRef<Path> + std::fmt::Debug,
{
    fs::read_to_string(&path).with_context(|| format!("Could not find file at: {:#?}", path))
}

pub fn parse_recipients<P>(recipient_file: P) -> anyhow::Result<Vec<MailAddress>>
where
    P: AsRef<Path> + std::fmt::Debug,
{
    Ok(get_file_content(recipient_file)?
        .lines()
        .map(str::to_string)
        .collect())
}

pub fn parse_config<P>(config_file: P) -> anyhow::Result<MailConfiguration>
where
    P: AsRef<Path> + std::fmt::Debug,
{
    let file_content = get_file_content(&config_file)?;
    Ok(toml::from_str(&file_content).with_context(|| {
        format!(
            "Error parsing configuration file at {:#?} with content \n{}",
            config_file, file_content
        )
    })?)
}

fn get_content_type<P>(file_path: P) -> anyhow::Result<ContentType>
where
    P: AsRef<Path> + std::fmt::Debug,
{
    match file_path.as_ref().extension().and_then(OsStr::to_str) {
        Some("html") => Ok(ContentType::Html),
        Some("txt") => Ok(ContentType::Plain),
        _ => Err(anyhow!(
            "Unrecognized content file type: {:#?}. Only .txt and .html is allowed.",
            file_path
        )),
    }
}

pub fn parse_mail_content<P>(content_file: P) -> anyhow::Result<MailContent>
where
    P: AsRef<Path> + std::fmt::Debug,
{
    let file_content = get_file_content(&content_file)?;
    let content_type = get_content_type(&content_file)?;

    // Parse content for correct format
    let premature_end_msg = "Error while parsing mail content file: Premature end of content file. Content file needs to have format: Subject line, blank line, body.";
    let mut lines = file_content.lines();
    let subject = lines.next().with_context(|| premature_end_msg)?;
    let sep = lines.next().with_context(|| premature_end_msg)?;
    let body = lines.collect::<Vec<&str>>().join("\n");

    if !(sep.is_empty() || sep == "---") {
        return Err(anyhow!("Error while parsing mail content file: Line separator missing. \nSubject header and body must be separated by a blank line or three dashes (---)."));
    }

    Ok(MailContent {
        subject: subject.to_string(),
        body: body,
        content_type: content_type,
    })
}

pub fn parse_attachments<P>(attachment_paths: &Option<Vec<P>>) -> anyhow::Result<Attachments>
where
    P: AsRef<Path> + std::fmt::Debug,
{
    let mut res = vec![];
    for p in attachment_paths.as_ref().unwrap_or(&vec![]) {
        let content_binary =
            fs::read(&p).with_context(|| format!("Error parsing attachment at {:#?}", p))?;
        let name = p.as_ref().file_name().unwrap(); // line above returns err if file nonexistant
        res.push(Attachment {
            filename: name
                .to_str()
                .ok_or(anyhow!("Could not parse attachment at {:#?}", p))?
                .to_string(),
            content: content_binary,
        });
    }
    Ok(res)
}

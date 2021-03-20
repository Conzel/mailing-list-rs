use anyhow::{anyhow, Context};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use serde::Deserialize;
use std::fmt::{self, Display};
use std::fs;
use std::path::Path;
use std::str::FromStr;
use toml;

pub type MailAddress = String;

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
}

impl Display for MailContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}\n---\n{}", self.subject, self.body)
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

    pub fn new(
        recipient: &MailAddress,
        content: &MailContent,
        config: &MailConfiguration,
    ) -> anyhow::Result<SmtpMailer> {
        let email = Message::builder()
            .from(Self::parse_pretty_error(&config.sender)?)
            .reply_to(Self::parse_pretty_error(&config.reply_to)?)
            .to(Self::parse_pretty_error(&recipient)?)
            // implement content
            .subject(content.subject.clone())
            .body(content.body.clone())?;

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

pub fn parse_mail_content<P>(content_file: P) -> anyhow::Result<MailContent>
where
    P: AsRef<Path> + std::fmt::Debug,
{
    let file_content = get_file_content(&content_file)?;
    let premature_end_msg = "Error while parsing mail content file: Premature end of content file. Content file needs to have format: Subject line, blank line, body.";
    let mut lines = file_content.lines();
    let subject = lines.next().with_context(|| premature_end_msg)?;
    let sep = lines.next().with_context(|| premature_end_msg)?;
    let body = lines.collect();

    if !(sep.is_empty() || sep == "---") {
        return Err(anyhow!("Error while parsing mail content file: Line separator missing. \nSubject header and body must be separated by a blank line or three dashes (---)."));
    }

    Ok(MailContent {
        subject: subject.to_string(),
        body: body,
    })
}

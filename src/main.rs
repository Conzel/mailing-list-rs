// File System
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use serde::Deserialize;
use std::env;
use std::fs;
use std::fmt::{self, Display};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use structopt::StructOpt;
use text_io::read;
use anyhow::{Context, anyhow};
use rayon::prelude::*;
use indicatif::ParallelProgressIterator;
use toml;

const CONFIG_FILENAME: &str = "mailsend.toml";
type MailAddress = String;

#[derive(StructOpt, Debug)]
#[structopt(name = "TTC Mail Sending")]
struct CliOptions {
    /// Full or relative path to configuration file (from executable)
    #[structopt(short = "c", long)]
    config_file: Option<PathBuf>,

    /// File containing email addresses (one address on each line)
    #[structopt(short, long, parse(from_os_str))]
    recipients_file: PathBuf,

    /// File containing email addresses (one address on each line)
    #[structopt(short, long, parse(from_os_str))]
    text_file: PathBuf,

    /// Enables debugging mode (does not send mail but just prints output)
    #[structopt(long)]
    debug: bool,
}

#[derive(Deserialize, Debug)]
struct MailConfiguration {
    username: String,
    password: String,
    sender: MailAddress,
    reply_to: MailAddress,
    mailserver: String,
}

struct SmtpMailer {
    email: lettre::Message,
    lettre_mailer: lettre::SmtpTransport,
}

#[derive(Debug)]
struct MailContent {
    subject: String,
    body: String,
}

impl Display for MailContent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}\n---\n{}", self.subject, self.body)
    }
}

impl SmtpMailer {
    fn parse_pretty_error<T>(mail: &String) -> Result<T, anyhow::Error> 
    where T: FromStr,
          <T as FromStr>::Err: std::error::Error + Send + Sync + 'static {
        mail.parse().with_context(|| format!("Invalid email address: {}", mail))
    }

    fn new(
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

        // Open a remote connection to gmail
        let mailer = SmtpTransport::relay(&config.mailserver)
            .with_context(|| "Could not connect to mail server")?
            .credentials(creds)
            .build();
        Ok(SmtpMailer {
            email: email,
            lettre_mailer: mailer,
        })
    }

    fn send(&self) -> anyhow::Result<()> {
        self.lettre_mailer.send(&self.email).with_context(|| "Could not send mail.")?;
        Ok(())
    }
}

fn get_default_configpath() -> io::Result<PathBuf> {
    let mut buf = env::current_exe()?;
    buf.pop(); // Removes executable file name itself and gives us folder of executable
    buf.push(CONFIG_FILENAME);
    Ok(buf)
}

fn get_file_content<P>(path: P) -> anyhow::Result<String> 
where P: AsRef<Path> + std::fmt::Debug {
    fs::read_to_string(&path).with_context(|| format!("Could not find file at: {:#?}", path))
}

fn parse_recipients<P>(recipient_file: P) -> anyhow::Result<Vec<MailAddress>> 
where P: AsRef<Path> + std::fmt::Debug {
    Ok(get_file_content(recipient_file)?.lines().map(str::to_string).collect())
}

fn parse_config<P>(config_file: P) -> anyhow::Result<MailConfiguration> 
where P: AsRef<Path> + std::fmt::Debug {
    let file_content = get_file_content(&config_file)?;
    Ok(toml::from_str(&file_content).with_context(|| format!("Error parsing configuration file at {:#?} with content \n{}", config_file, file_content))?)
}

fn parse_mail_content<P>(content_file: P) -> anyhow::Result<MailContent> 
where P: AsRef<Path> + std::fmt::Debug {
    let file_content = get_file_content(&content_file)?;
    let premature_end_msg = "Error while parsing mail content file: Premature end of content file. Content file needs to have format: Subject line, blank line, body.";
    let mut lines = file_content.lines();
    let subject = lines.next().with_context(|| premature_end_msg)?;
    let sep = lines.next().with_context(|| premature_end_msg)?;
    let body = lines.collect();

    if ! (sep.is_empty() || sep == "---")  {
        return Err(anyhow!("Error while parsing mail content file: Line separator missing. \nSubject header and body must be separated by a blank line or three dashes (---)."));
    }

    Ok(MailContent {
        subject: subject.to_string(),
        body: body
    })
}

fn main() -> anyhow::Result<()> {
    // Setting up configuration files from Cli arguments
    let opt = CliOptions::from_args();
    let text = parse_mail_content(&opt.text_file)?;
    let recipients = parse_recipients(&opt.recipients_file)?;
    let config = parse_config(
        &opt.config_file
            .as_ref()
            .unwrap_or(&get_default_configpath()?),
    )?;

    // Partition into successful mailers and errors
    let mut correct_mailers: Vec<SmtpMailer> = vec![];
    let mut errors: Vec<anyhow::Error> = vec![];
    for addr in &recipients {
        match SmtpMailer::new(&addr, &text, &config) {
            Ok(mailer) => correct_mailers.push(mailer),
            Err(e) => errors.push(e),
        }
    }

    // Error handling for wrongly parsed email addresses
    println!(
        "Found {} email addresses. {} parsed successfully, {} error(s) occured.",
        recipients.len(),
        correct_mailers.len(),
        errors.len()
    );
    if errors.len() > 0 {
        println!("Errors:");
        errors.iter().for_each(|e| eprintln!("\t{}", e));
        println!("");
    }

    // Early return in debug case
    if opt.debug {
        println!("Recipients: {:#?}\n Config: {:#?}\nCli Options: {:#?}\nText: \n{:#?}", recipients, config, opt, text);
        return Ok(());
    }

    // Asking for final confirm
    println!("Will now send the following email to the successfully parsed addresses: \n\n{}\n", text);

    // User input for finishing sendmail
    loop {
        print!("Proceed? [y/n] ");
        io::stdout().flush()?;
        let input: String = read!("{}\n");
        if input == "y" || input == "Y" {
            let num_correct_mails = correct_mailers.len() as u64;
            let send_result = correct_mailers.into_par_iter().progress_count(num_correct_mails).try_for_each(|mailer| mailer.send());
            match send_result {
                Err(e) => println!("Failure occured during sending: {:#?}. \nSome mails may have been sent and others not.", e),
                _ => println!("Successfully sent all emails"),
            }
            break;
        } else if input == "n" || input == "N" {
            println!("Sending cancelled.");
            break;
        } else {
            println!("Unexpected input.");
        }
    }
    Ok(())
}

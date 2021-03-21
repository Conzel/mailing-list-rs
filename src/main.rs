use indicatif::ParallelProgressIterator;
use rayon::prelude::*;
use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use structopt::StructOpt;
use text_io::read;
mod smtp_mailer;
use smtp_mailer::*;

const CONFIG_FILENAME: &str = "mailsend.toml";

#[derive(StructOpt, Debug)]
#[structopt(name = "mailing-list-rs")]
struct CliOptions {
    /// Full or relative path to configuration file (from executable)
    #[structopt(short = "c", long)]
    config_file: Option<PathBuf>,

    /// File containing email addresses (one address on each line)
    #[structopt(short, long, parse(from_os_str))]
    recipients_file: PathBuf,

    /// File containing content of email (format: subject line, blank line, mail text)
    #[structopt(short, long, parse(from_os_str))]
    text_file: PathBuf,

    /// Enables debugging mode (does not send mail but just prints output)
    #[structopt(long)]
    debug: bool,
}

// Returns the configuration path: /abs/path/to/exec/CONFIG_FILENAME
fn get_default_configpath() -> io::Result<PathBuf> {
    let mut buf = env::current_exe()?;
    buf.pop(); // Removes executable file name itself and gives us folder of executable
    buf.push(CONFIG_FILENAME);
    Ok(buf)
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
        println!(
            "Recipients: {:#?}\n Config: {:#?}\nCli Options: {:#?}\nText: \n{:#?}",
            recipients, config, opt, text
        );
        return Ok(());
    }

    // Asking for final confirm, handling user input
    println!(
        "Will now send the following email to the successfully parsed addresses: \n\n{}\n",
        text
    );
    loop {
        print!("Proceed? [y/n] ");
        io::stdout().flush()?;
        let input: String = read!("{}\n");
        if input == "y" || input == "Y" {
            let num_correct_mails = correct_mailers.len() as u64;
            // sends all mails in parallel with added progress bar
            let send_result = correct_mailers
                .into_par_iter()
                .progress_count(num_correct_mails)
                .try_for_each(|mailer| mailer.send());
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

# mailing-list-rs
Simple example of how a mailing list can be implemented in Rust. Uses [lettre](https://github.com/lettre/lettre) to deliver mails via 
a remote SMTP server (for example one as provided by GMail). 

## Usage
Either download the binaries for your platforms under releases (Linux and Windows supported) or build them 
yourself.

The binary is in the form of a Command Line Utility, which can be called with `--help` for more details. 
In short: three file paths have to be supplied to the program via command line flags
* -r or --recipients, a text file in which each line is a valid email address representing one recipient
* -t or --text-file, a text file which contains the subject and mail text. The subject is on it's own line and is separated from the mail text body with a blank line (or a line containing only three dashes `---`).
* -c or --config-file, a TOML file containing the configuration information for the mail server. An example for a GMail connection is provided. If this option is left out, the program will search in the directory of the executable for a file called `mailsend.toml`. The required arguments are:
  * `mailserver`: Address of the SMTP Server that the mail should be sent to
  * `username`: Username used to authenticate against the SMTP server
  * `password`: Password used to authenticate against the SMTP server
  * `sender`:   Mail address appearing in the sender field
  * `reply_to`: Mail address appearing in the reply_to field

Example call: 
`./mailing-list-rs --recipients-file ./example-recipients.txt --text-file ./example-content.txt -config-file ./mailsend.toml`
or shorter
`./mailing-list-rs -r ./example-recipients.txt -t ./example-content.txt -c ./mailsend.toml`

## How to build
1. Follow the Rust installation instructions in [the Rust book](https://doc.rust-lang.org/book/ch01-01-installation.html)
2. Clone this repository
3. Navigate into the repository and build the project using `cargo build --release`
4. Locate the executable. For example under /path/to/repo/target/release/mailing-list-rs

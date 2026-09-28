use std::io::Write;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::Subcommand;

use harness_core::ask::answers::{Outcome, read_answered};
use harness_core::ask::asks::{Asks, Locale};
use harness_core::ask::current::current;
use harness_core::ask::serve::{Browser, Serving, SystemBrowser, serve};
use harness_core::error::{Error, Result};

use super::write_envelope_success;

#[derive(Subcommand)]
pub enum AskCommand {
    /// Serve a decision page on 127.0.0.1, open it in the browser, and wait
    /// for one answer set. Exits 0 answered, 1 stale or unanswered
    Serve {
        /// The page: an HTML file, served with the files of its directory
        page: PathBuf,
        /// The asks file (`harnex export schema asks`); its `sources` resolve
        /// against the working directory
        asks: PathBuf,
        /// Minutes to wait for an answer set
        #[arg(long, default_value = "120")]
        within: NonZeroU32,
        /// Serve without opening a browser, for a person who reaches the
        /// address another way, such as a port forwarded to their machine and
        /// opened there as `localhost` or `127.0.0.1` on any port
        #[arg(long)]
        no_open: bool,
    },
    /// Whether answers taken earlier still hold against the asks as they read
    /// now. Exits 0 when every one does, 1 otherwise
    Current {
        /// An answered record: the `data` of an `ask serve` envelope, or the
        /// same shape another transport wrote (`harnex export schema
        /// ask-outcome`)
        answered: PathBuf,
        /// The asks file as it reads now
        asks: PathBuf,
    },
    /// Every sentence `ask serve` may put on a page in one locale, before any
    /// page is served: what the page script shows and what a refusal says.
    /// `{name}` marks a value filled where it is shown
    Words {
        /// A locale an asks file can name as its `locale`
        #[arg(value_parser = locale_values())]
        locale: String,
    },
}

/// Source of truth for the `locale` value_parser — derives from
/// [`Locale::ALL`], so a locale added to the catalog is offered here.
fn locale_values() -> Vec<&'static str> {
    Locale::ALL.iter().map(|l| l.as_str()).collect()
}

pub fn run<W: Write>(cmd: AskCommand, out: &mut W) -> Result<ExitCode> {
    match cmd {
        AskCommand::Serve {
            page,
            asks,
            within,
            no_open,
        } => {
            let base = std::env::current_dir().map_err(|source| Error::IoFailure {
                path: PathBuf::from("."),
                source,
            })?;
            let asks = Asks::read(&asks)?;
            let serving = Serving {
                page: &page,
                asks: &asks,
                base: &base,
                within: Duration::from_secs(u64::from(within.get()) * 60),
            };
            let browser: Option<&dyn Browser> = (!no_open).then_some(&SystemBrowser);
            let outcome = serve(&serving, browser, &mut |url| {
                let _ = writeln!(std::io::stderr(), "serving {url} for {within} minutes");
            })?;
            let exit = match outcome {
                Outcome::Answered { .. } => ExitCode::SUCCESS,
                Outcome::Stale { .. } | Outcome::Unanswered { .. } => ExitCode::from(1),
            };
            write_envelope_success(out, outcome)?;
            Ok(exit)
        }
        AskCommand::Current { answered, asks } => {
            let answers = read_answered(&answered)?;
            let now = Asks::read(&asks)?;
            let standing = current(&answers, &now);
            let exit = if standing.holds() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            };
            write_envelope_success(out, standing)?;
            Ok(exit)
        }
        AskCommand::Words { locale } => {
            let locale = Locale::from_str(&locale).ok_or_else(|| Error::ConfigInvalid {
                message: format!("unknown locale '{locale}'"),
                location: None,
            })?;
            write_envelope_success(out, locale.words())?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

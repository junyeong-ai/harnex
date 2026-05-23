use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;

use harness_core::error::Result;
use harness_core::init::ProjectInitializer;

use super::write_envelope_success;

#[derive(Args)]
pub struct InitArgs {
    /// Target directory (defaults to current working dir)
    #[arg(long, default_value = ".")]
    pub dir: PathBuf,
    /// Project name embedded in CLAUDE.md / README.md scaffolds
    #[arg(long)]
    pub name: Option<String>,
    /// Overwrite existing files
    #[arg(long, default_value_t = false)]
    pub force: bool,
    /// Dry-run: report planned writes without touching disk
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,
    /// Skip hook script generation
    #[arg(long, default_value_t = false)]
    pub no_hooks: bool,
}

pub fn run<W: Write>(args: InitArgs, out: &mut W) -> Result<ExitCode> {
    let project_name = args.name.unwrap_or_else(|| {
        args.dir
            .canonicalize()
            .ok()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_else(|| "my-project".to_string())
    });
    let init =
        ProjectInitializer::new(&args.dir, project_name, args.force).with_hooks(!args.no_hooks);
    let outcome = if args.dry_run {
        init.plan()?
    } else {
        init.apply()?
    };
    write_envelope_success(out, outcome)?;
    Ok(ExitCode::SUCCESS)
}

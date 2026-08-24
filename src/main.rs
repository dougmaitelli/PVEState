use anyhow::Result;
use clap::{Parser, Subcommand};
use pvestate::{apply, capture, config::Repository, plan, recovery};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "pves",
    version,
    about = "Safe desired-state tooling for Proxmox VE"
)]
struct Cli {
    #[arg(long, env = "IAC_CONFIG_DIR", default_value = ".", global = true)]
    config_dir: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Init {
        path: PathBuf,
    },
    Capture,
    Plan,
    Apply,
    Validate,
    Recover {
        #[command(subcommand)]
        action: Recovery,
    },
    Schema {
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum Recovery {
    Plan { target: String },
    BootstrapPve { target: String },
    BootstrapPbs { target: String },
    Restore { target: String },
    Configure { target: String },
    All { target: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { path } => Repository::initialize(&path),
        Command::Capture => capture::run(&Repository::open(&cli.config_dir)?),
        Command::Plan => {
            let p = plan::run(&Repository::open(&cli.config_dir)?)?;
            println!("{}", serde_json::to_string_pretty(&p)?);
            Ok(())
        },
        Command::Apply => apply::run(&Repository::open(&cli.config_dir)?),
        Command::Validate => capture::validate(&Repository::open(&cli.config_dir)?),
        Command::Recover { action } => {
            let (stage, target) = match action {
                Recovery::Plan { target } => (recovery::Stage::Plan, target),
                Recovery::BootstrapPve { target } => (recovery::Stage::BootstrapPve, target),
                Recovery::BootstrapPbs { target } => (recovery::Stage::BootstrapPbs, target),
                Recovery::Restore { target } => (recovery::Stage::Restore, target),
                Recovery::Configure { target } => (recovery::Stage::Configure, target),
                Recovery::All { target } => (recovery::Stage::All, target),
            };
            recovery::run(&Repository::open(&cli.config_dir)?, stage, &target)
        },
        Command::Schema { output } => Repository::write_schema(output.as_deref()),
    }
}

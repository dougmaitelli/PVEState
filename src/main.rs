use anyhow::Result;
use clap::{Parser, Subcommand};
use pvestate::{
    client::{Pbs, Pve, Ssh},
    command::{adopt, apply, capture, plan, recovery},
    config::Repository,
    settings::Settings,
};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "pves",
    version,
    about = "Safe desired-state tooling for Proxmox VE"
)]
struct Cli {
    #[arg(long, env = "PVES_CONFIG_DIR", default_value = ".", global = true)]
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
    Adopt {
        #[arg(long)]
        write: bool,
        #[arg(long = "id", conflicts_with = "all")]
        ids: Vec<String>,
        #[arg(long, requires = "write", conflicts_with = "ids")]
        all: bool,
    },
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
        Command::Capture => {
            let repo = Repository::open(&cli.config_dir)?;
            let settings = Settings::load(&cli.config_dir)?;
            let pve = Pve::discovery(&settings.pve)?;
            let pbs = Pbs::discovery(&settings.pbs)?;
            let ssh = Ssh::new(&settings.ssh.discovery);
            capture::run(&repo, &pve, &pbs, &ssh)
        },
        Command::Plan => {
            let repo = Repository::open(&cli.config_dir)?;
            let settings = Settings::load(&cli.config_dir)?;
            let pve = Pve::discovery(&settings.pve)?;
            let pbs = Pbs::discovery(&settings.pbs)?;
            let p = plan::run(&repo, &pve, &pbs)?;
            println!("{}", serde_json::to_string_pretty(&p)?);
            Ok(())
        },
        Command::Adopt { write, ids, all } => {
            let repo = Repository::open(&cli.config_dir)?;
            adopt::run(&repo, write, all, &ids)
        },
        Command::Apply => {
            let repo = Repository::open(&cli.config_dir)?;
            let settings = Settings::load(&cli.config_dir)?;
            let pve = Pve::mutation(&settings.pve)?;
            let pbs = settings
                .pbs
                .mutation
                .as_ref()
                .map(|_| Pbs::mutation(&settings.pbs))
                .transpose()?;
            let ssh = settings.ssh.mutation.as_ref().map(Ssh::new);
            apply::run(
                &repo,
                &settings.apply,
                &pve,
                pbs.as_ref().map(|client| client as _),
                ssh.as_ref().map(|client| client as _),
            )
        },
        Command::Validate => {
            let repo = Repository::open(&cli.config_dir)?;
            let settings = Settings::load(&cli.config_dir)?;
            let ssh = Ssh::new(&settings.ssh.discovery);
            capture::validate(&repo, &ssh)
        },
        Command::Recover { action } => {
            let (stage, target) = match action {
                Recovery::Plan { target } => (recovery::Stage::Plan, target),
                Recovery::BootstrapPve { target } => (recovery::Stage::BootstrapPve, target),
                Recovery::BootstrapPbs { target } => (recovery::Stage::BootstrapPbs, target),
                Recovery::Restore { target } => (recovery::Stage::Restore, target),
                Recovery::Configure { target } => (recovery::Stage::Configure, target),
                Recovery::All { target } => (recovery::Stage::All, target),
            };
            let repo = Repository::open(&cli.config_dir)?;
            let settings = Settings::load(&cli.config_dir)?;
            let ssh = settings
                .ssh
                .recovery
                .as_ref()
                .map(|template| Ssh::new(&template.for_host(&target)));
            recovery::run(
                &repo,
                stage,
                &target,
                &settings.recovery,
                ssh.as_ref().map(|client| client as _),
            )
        },
        Command::Schema { output } => Repository::write_schema(output.as_deref()),
    }
}

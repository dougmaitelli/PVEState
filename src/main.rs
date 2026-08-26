use anyhow::Result;
use clap::{ArgAction, Parser, Subcommand};
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
    /// Show operation progress; repeat for additional detail
    #[arg(short, long, action = ArgAction::Count, global = true)]
    verbose: u8,
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
    #[command(arg_required_else_help = true)]
    Adopt {
        #[arg(long, conflicts_with_all = ["ids", "all"])]
        preview: bool,
        #[arg(long, conflicts_with_all = ["preview", "ids"])]
        all: bool,
        #[arg(value_name = "ID", num_args = 1.., conflicts_with_all = ["preview", "all"])]
        ids: Vec<String>,
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
    pvestate::utility::progress::set(cli.verbose);
    match cli.command {
        Command::Init { path } => {
            pvestate::utility::progress::section("Initializing configuration repository");
            Repository::initialize(&path)
        },
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
        Command::Adopt { preview, all, ids } => {
            let repo = Repository::open(&cli.config_dir)?;
            adopt::run(&repo, preview, all, &ids)
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
        Command::Schema { output } => {
            pvestate::utility::progress::section("Generating configuration schemas");
            Repository::write_schema(output.as_deref())
        },
    }
}

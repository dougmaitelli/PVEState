use crate::{
    client::{Pbs, Pve, Ssh},
    command::{adopt, apply, capture, plan, recovery},
    config,
    settings::Settings,
    utility::progress::{EventSink, TerminalEventSink},
};
use anyhow::Result;
use clap::{ArgAction, Parser, Subcommand};
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
    Plan {
        /// Print the complete machine-readable plan
        #[arg(long)]
        json: bool,
    },
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
    All { target: String },
}

pub fn run_cli() -> Result<()> {
    let cli = Cli::parse();
    let events = TerminalEventSink::new(cli.verbose);
    let result = run(cli, &events);
    events.finish(result.is_ok());
    result
}

fn run(cli: Cli, events: &dyn EventSink) -> Result<()> {
    match cli.command {
        Command::Init { path } => {
            events.section("Initializing configuration repository");
            config::scaffold::initialize(&path)?;
            events.finish(true);
            println!("initialized configuration repository: {}", path.display());
            Ok(())
        },
        Command::Capture => {
            let repo = config::open(&cli.config_dir)?;
            let settings = Settings::load(&cli.config_dir)?;
            let pve = Pve::discovery(&settings.pve)?;
            let pbs = Pbs::discovery(&settings.pbs)?;
            let ssh = Ssh::new(&settings.ssh.discovery);
            let report = capture::run(&repo, &pve, &pbs, &ssh, events)?;
            println!("{}", render_capture_report(&report));
            Ok(())
        },
        Command::Plan { json } => {
            let repo = config::open(&cli.config_dir)?;
            let p = plan::run(&repo, events)?;
            events.finish(true);
            if json {
                println!("{}", serde_json::to_string_pretty(&p)?);
            } else {
                plan::print_human(&p);
            }
            Ok(())
        },
        Command::Adopt { preview, all, ids } => {
            let repo = config::open(&cli.config_dir)?;
            let report = adopt::run(&repo, preview, all, &ids, events)?;
            println!("{}", render_adoption_report(report)?);
            Ok(())
        },
        Command::Apply => {
            let repo = config::open(&cli.config_dir)?;
            let settings = Settings::load(&cli.config_dir)?;
            let report = apply::run(&repo, &settings, events)?;
            println!("{}", render_apply_report(&report)?);
            Ok(())
        },
        Command::Validate => {
            let repo = config::open(&cli.config_dir)?;
            let settings = Settings::load(&cli.config_dir)?;
            let ssh = Ssh::new(&settings.ssh.discovery);
            let report = capture::validate(&repo, &ssh, events)?;
            println!("{}", render_validation_report(&report));
            Ok(())
        },
        Command::Recover { action } => {
            let (stage, target) = match action {
                Recovery::Plan { target } => (recovery::Stage::Plan, target),
                Recovery::BootstrapPve { target } => (recovery::Stage::BootstrapPve, target),
                Recovery::BootstrapPbs { target } => (recovery::Stage::BootstrapPbs, target),
                Recovery::Restore { target } => (recovery::Stage::Restore, target),
                Recovery::All { target } => (recovery::Stage::All, target),
            };
            let repo = config::open(&cli.config_dir)?;
            let settings = Settings::load(&cli.config_dir)?;
            let ssh = settings
                .ssh
                .recovery
                .as_ref()
                .map(|template| Ssh::new(&template.for_host(&target)));
            let report = recovery::run(
                &repo,
                stage,
                &target,
                &settings.recovery,
                ssh.as_ref().map(|client| client as _),
                events,
            )?;
            println!("{}", render_recovery_report(report)?);
            Ok(())
        },
        Command::Schema { output } => {
            events.section("Generating configuration schemas");
            config::schema::write(output.as_deref())?;
            events.finish(true);
            println!(
                "wrote configuration schemas to {}",
                output
                    .as_deref()
                    .unwrap_or_else(|| std::path::Path::new("schemas"))
                    .display()
            );
            Ok(())
        },
    }
}

fn render_capture_report(report: &capture::CaptureReport) -> String {
    format!("captured live state into {}", report.destination)
}

fn render_adoption_report(report: adopt::AdoptionReport) -> Result<String> {
    if let Some(candidates) = report.candidates {
        return Ok(serde_json::to_string_pretty(&candidates)?);
    }
    let documents = report
        .changed_documents
        .iter()
        .map(|document| document.path())
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!(
        "adopted {} captured value(s) into {documents}",
        report.selected
    ))
}

fn render_apply_report(report: &apply::ApplyReport) -> Result<String> {
    Ok(serde_json::to_string_pretty(report)?)
}

fn render_validation_report(report: &capture::ValidationReport) -> String {
    format!("validation passed: {} checks", report.check_count)
}

fn render_recovery_report(report: recovery::RecoveryReport) -> Result<String> {
    match report {
        recovery::RecoveryReport::Plan { plan } => Ok(serde_json::to_string_pretty(&plan)?),
        recovery::RecoveryReport::Completed { stage, message } => Ok(message.map_or_else(
            || format!("recovery stage completed: {stage}"),
            |message| format!("recovery stage completed: {stage}\n{message}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigDocument;

    #[test]
    fn human_report_rendering_is_stable() {
        assert_eq!(
            render_capture_report(&capture::CaptureReport {
                capture_id: "capture-1".into(),
                endpoint_count: 4,
                failures: Vec::new(),
                destination: "/srv/pveconf".into(),
            }),
            "captured live state into /srv/pveconf"
        );
        assert_eq!(
            render_adoption_report(adopt::AdoptionReport {
                candidates: None,
                selected: 2,
                changed_documents: vec![ConfigDocument::Guests, ConfigDocument::Network],
            })
            .unwrap(),
            "adopted 2 captured value(s) into config/guests.yml, config/network.yml"
        );
        assert_eq!(
            render_validation_report(&capture::ValidationReport {
                check_count: 3,
                failures: Vec::new(),
            }),
            "validation passed: 3 checks"
        );
    }

    #[test]
    fn apply_report_json_has_stable_field_names() {
        let output = render_apply_report(&apply::ApplyReport {
            journal_id: "apply-1".into(),
            journal_path: "/runtime/apply-1.json".into(),
            completed: 3,
            failed: None,
        })
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(
            value.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["completed", "failed", "journal_id", "journal_path"]
        );

        let capture = serde_json::to_value(capture::CaptureReport {
            capture_id: "capture-1".into(),
            endpoint_count: 4,
            failures: Vec::new(),
            destination: "/srv/pveconf".into(),
        })
        .unwrap();
        assert_eq!(
            capture.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["capture_id", "destination", "endpoint_count", "failures"]
        );

        let adoption = serde_json::to_value(adopt::AdoptionReport {
            candidates: None,
            selected: 1,
            changed_documents: vec![ConfigDocument::Guests],
        })
        .unwrap();
        assert_eq!(
            adoption.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["changed_documents", "selected"]
        );
    }
}

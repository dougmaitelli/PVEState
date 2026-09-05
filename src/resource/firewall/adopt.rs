use crate::{
    config::{AdoptionCandidate, ConfigDocument, LocalPatch, LocalState},
    discovery::CapturedNative,
    reconcile::Operation,
    resource::{native::NativeDirective, native_paths},
    utility::yaml_patch::Segment,
};
use anyhow::{Context, Result, bail};

pub(crate) fn candidate(
    local: &LocalState,
    captured: &CapturedNative,
    operation: &Operation,
) -> Result<AdoptionCandidate> {
    let result = adoption_patch(local, captured, &operation.resource().to_string());
    Ok(match result {
        Ok(patch) => AdoptionCandidate::adoptable(
            operation.resource(),
            "file",
            "local rendering",
            "captured file",
            vec![patch],
        ),
        Err(error) => AdoptionCandidate::blocked(
            operation.resource(),
            "file",
            "local rendering",
            "captured file",
            format!("{error:#}"),
        ),
    })
}

fn adoption_patch(
    local: &LocalState,
    captured: &CapturedNative,
    resource: &str,
) -> Result<LocalPatch> {
    let (observed, document, path) = paths(local, captured, resource)?;
    let value = match std::fs::read_to_string(observed) {
        Ok(content) => {
            let parsed = super::native::parse(&content);
            if !parsed.unmodeled.is_empty() {
                bail!(
                    "captured firewall contains syntax that cannot be adopted safely: {}",
                    parsed
                        .unmodeled
                        .iter()
                        .map(NativeDirective::diagnostic)
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            }
            Some(serde_yaml::to_value(parsed.managed)?)
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    Ok(if let Some(value) = value {
        LocalPatch::ReplaceResource {
            document,
            path,
            value,
        }
    } else {
        LocalPatch::RemoveResource { document, path }
    })
}

fn paths(
    local: &LocalState,
    captured: &CapturedNative,
    resource: &str,
) -> Result<(std::path::PathBuf, ConfigDocument, Vec<Segment>)> {
    let observed = captured.path(native_paths::FIREWALL_ARTIFACT_ROOT);
    if resource == "cluster" {
        return Ok((
            observed.join("cluster.fw"),
            ConfigDocument::Cluster,
            vec![Segment::Key("firewall".into())],
        ));
    }
    if let Some(node) = resource.strip_prefix("node/") {
        return Ok((
            observed.join(format!("{node}-host.fw")),
            ConfigDocument::Node,
            vec![Segment::Key("firewall".into())],
        ));
    }
    let id: u32 = resource
        .parse()
        .with_context(|| format!("invalid firewall resource {resource}"))?;
    let collection = if local.guests.lxcs.contains_key(&id) {
        "lxcs"
    } else if local.guests.vms.contains_key(&id) {
        "vms"
    } else {
        bail!("firewall resource {resource} is not a configured guest")
    };
    Ok((
        observed.join(format!("{id}.fw")),
        ConfigDocument::Guests,
        vec![
            Segment::Key(collection.into()),
            Segment::Key(id.to_string()),
            Segment::Key("firewall".into()),
        ],
    ))
}

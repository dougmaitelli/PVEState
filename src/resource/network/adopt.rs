use crate::{
    config::{AdoptionCandidate, ConfigDocument, LocalPatch, LocalState},
    discovery::CapturedState,
    reconcile::Operation,
    utility::yaml_patch::Segment,
};
use anyhow::Result;

pub(crate) fn candidates(
    local: &LocalState,
    captured: &CapturedState,
    operation: &Operation,
) -> Result<Vec<AdoptionCandidate>> {
    let Operation::ApiMutation {
        resource, changes, ..
    } = operation
    else {
        return Ok(Vec::new());
    };
    let actual = captured.node_dns(&local.guests.node)?;
    let search = actual.search.clone().unwrap_or_default();
    let servers = actual.servers.values().cloned().collect::<Vec<_>>();
    let patches = vec![
        LocalPatch::SetScalar {
            document: ConfigDocument::Network,
            path: vec![Segment::Key("dns".into()), Segment::Key("search".into())],
            value: serde_yaml::Value::String(search),
        },
        LocalPatch::ReplaceResource {
            document: ConfigDocument::Network,
            path: vec![Segment::Key("dns".into()), Segment::Key("servers".into())],
            value: serde_yaml::to_value(servers)?,
        },
    ];
    Ok(vec![AdoptionCandidate::adoptable(
        resource,
        changes.keys().cloned().collect::<Vec<_>>().join(","),
        "local DNS settings",
        "captured DNS settings",
        patches,
    )])
}

pub(crate) fn native_candidate(
    local: &LocalState,
    captured: &crate::discovery::CapturedNative,
    operation: &Operation,
) -> Result<AdoptionCandidate> {
    let result = (|| {
        let content = captured.read_to_string(crate::resource::native_paths::NETWORK_ARTIFACT)?;
        let parsed = super::native::parse(&content, &local.network)?;
        if !parsed.unmodeled.is_empty() {
            anyhow::bail!(
                "captured network contains syntax that cannot be adopted safely: {}",
                parsed
                    .unmodeled
                    .iter()
                    .map(crate::resource::native::NativeDirective::diagnostic)
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        }
        Ok::<_, anyhow::Error>(vec![
            LocalPatch::ReplaceResource {
                document: ConfigDocument::Network,
                path: vec![Segment::Key("interfaces".into())],
                value: serde_yaml::to_value(&parsed.managed.interfaces)?,
            },
            LocalPatch::ReplaceResource {
                document: ConfigDocument::Network,
                path: vec![Segment::Key("bridges".into())],
                value: serde_yaml::to_value(&parsed.managed.bridges)?,
            },
        ])
    })();
    Ok(match result {
        Ok(patches) => AdoptionCandidate::adoptable(
            operation.resource(),
            "file",
            "local rendering",
            "captured file",
            patches,
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

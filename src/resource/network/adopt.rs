use crate::{
    command::plan::Operation,
    config::{AdoptionCandidate, ConfigDocument, LocalPatch, LocalState},
    discovery::CapturedState,
    utility::yaml_patch::Segment,
};
use anyhow::Result;
use serde_json::Value;

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
    let actual = captured
        .pve
        .response(&format!("/nodes/{}/dns", local.guests.node))?;
    let search = actual
        .get("search")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut servers = actual
        .as_object()
        .into_iter()
        .flat_map(|object| object.iter())
        .filter_map(|(key, value)| {
            key.strip_prefix("dns")?
                .parse::<usize>()
                .ok()
                .zip(value.as_str())
        })
        .collect::<Vec<_>>();
    servers.sort_by_key(|(index, _)| *index);
    let servers = servers
        .into_iter()
        .map(|(_, server)| server.to_owned())
        .collect::<Vec<_>>();
    let patches = vec![
        LocalPatch::SetScalar {
            document: ConfigDocument::Network,
            path: vec![Segment::Key("dns".into()), Segment::Key("search".into())],
            value: serde_yaml::Value::String(search.into()),
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

use super::{Domain, Operation, ResourceId};
use crate::{
    config::LocalState,
    resource::{firewall, network},
};
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::fs;

pub(super) fn operation(
    repo: &LocalState,
    identity: (Domain, &str),
    paths: (&str, &str),
    wanted: String,
    activate: bool,
    operations: &mut Vec<Operation>,
) -> Result<()> {
    let (domain, resource) = identity;
    let (local, remote) = paths;
    let current = fs::read_to_string(repo.observed().join(local)).ok();
    let current_text = current.as_deref().unwrap_or_default();
    let differs = if domain == Domain::Firewall {
        firewall::render::semantic(&wanted) != firewall::render::semantic(current_text)
    } else {
        network::render::semantic_lines(&wanted) != network::render::semantic_lines(current_text)
    };
    if differs {
        let resource = ResourceId::parse(resource);
        let before_sha256 = current
            .as_ref()
            .map(|value| hex::encode(Sha256::digest(value.as_bytes())));
        operations.push(Operation::WriteFile {
            domain,
            resource,
            path: remote.into(),
            content: wanted,
            before_content: current,
            before_sha256,
            activate,
        });
    }
    Ok(())
}

pub(super) fn deletion(
    repo: &LocalState,
    identity: (Domain, &str),
    paths: (&str, &str),
    operations: &mut Vec<Operation>,
) -> Result<()> {
    let Some(current) = fs::read(repo.observed().join(paths.0)).ok() else {
        return Ok(());
    };
    let domain = identity.0;
    let resource = ResourceId::parse(identity.1);
    operations.push(Operation::DeleteFile {
        domain,
        resource,
        path: paths.1.into(),
        before_sha256: hex::encode(Sha256::digest(&current)),
        before_content: String::from_utf8_lossy(&current).into_owned(),
    });
    Ok(())
}

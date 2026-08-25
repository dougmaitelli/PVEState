use super::Operation;
use crate::{config::Repository, render};
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::fs;

pub(super) fn operation(
    repo: &Repository,
    identity: (&str, &str),
    paths: (&str, &str),
    wanted: String,
    activate: bool,
    operations: &mut Vec<Operation>,
) -> Result<()> {
    let (domain, resource) = identity;
    let (local, remote) = paths;
    let current = fs::read_to_string(repo.observed().join(local)).unwrap_or_default();
    let differs = if domain == "firewall" {
        render::firewall_semantic(&wanted) != render::firewall_semantic(&current)
    } else {
        render::semantic_lines(&wanted) != render::semantic_lines(&current)
    };
    if differs {
        operations.push(Operation::WriteFile {
            domain: domain.into(),
            resource: resource.into(),
            path: remote.into(),
            content: wanted,
            before_sha256: hex::encode(Sha256::digest(current.as_bytes())),
            activate,
        });
    }
    Ok(())
}

use crate::{
    api::Pve,
    config::Repository,
    plan::{Operation, Plan},
    ssh::Ssh,
};
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::Utc;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};
pub fn run(repo: &Repository) -> Result<()> {
    let plan: Plan = serde_json::from_slice(
        &fs::read(repo.runtime().join("production-plan.json")).context("run plan first")?,
    )?;
    plan.verify()?;
    require_eq("IAC_ENABLE_PRODUCTION_APPLY", "YES")?;
    if std::env::var("IAC_CONFIRM_PLAN_SHA")? != plan.plan_sha256 {
        bail!("plan SHA confirmation mismatch")
    }
    if std::env::var("IAC_APPLY_TARGET")? != plan.target {
        bail!("apply target does not match plan target")
    }
    if Utc::now() - plan.created_at > chrono::Duration::minutes(30) {
        bail!("plan is stale; capture and plan again")
    }
    if !plan.blockers.is_empty() {
        bail!("plan has blockers: {}", plan.blockers.join("; "))
    }
    let allowed: BTreeSet<_> = std::env::var("IAC_APPLY_DOMAINS")?
        .split(',')
        .map(str::to_string)
        .collect();
    let needed: BTreeSet<_> = plan
        .operations
        .iter()
        .map(|x| x.domain().to_string())
        .collect();
    if !needed.is_subset(&allowed) {
        bail!(
            "unapproved domains: {:?}",
            needed.difference(&allowed).collect::<Vec<_>>()
        )
    }
    let api = Pve::mutation()?;
    if api.endpoint() != plan.target {
        bail!("mutation API endpoint differs from plan target")
    }
    let mut ssh = None;
    let mut results = Vec::new();
    for op in &plan.operations {
        match op {
            Operation::ApiUpdate {
                endpoint,
                changes,
                digest,
                ..
            } => {
                let mut x = changes.clone();
                if let Some(d) = digest {
                    x.insert("digest".into(), d.clone());
                }
                api.put(endpoint, &x)?
            }
            Operation::GrowDisk {
                endpoint,
                disk,
                size_gb,
                ..
            } => api.put(
                endpoint,
                &BTreeMap::from([
                    ("disk".into(), disk.clone()),
                    ("size".into(), format!("{size_gb}G")),
                ]),
            )?,
            Operation::WriteFile {
                path,
                content,
                before_sha256,
                activate,
                ..
            } => {
                let s = match &ssh {
                    Some(x) => x,
                    None => {
                        ssh = Some(Ssh::mutation()?);
                        ssh.as_ref().unwrap()
                    }
                };
                let current = s
                    .run(&format!("sha256sum {}", quote(path)))?
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string();
                if &current != before_sha256 {
                    bail!("remote file changed after plan: {path}")
                }
                let stamp = Utc::now().format("%Y%m%dT%H%M%SZ");
                let backup = format!("/root/iac-preapply/{stamp}{path}");
                let encoded = STANDARD.encode(content);
                let mode = if path == "/etc/network/interfaces" {
                    "0644"
                } else {
                    "0640"
                };
                let cmd = format!(
                    "install -d {} && cp -a {} {} && base64 -d > {}.iac-new && install -m {} {}.iac-new {}",
                    quote(
                        std::path::Path::new(&backup)
                            .parent()
                            .unwrap()
                            .to_str()
                            .unwrap()
                    ),
                    quote(path),
                    quote(&backup),
                    quote(path),
                    mode,
                    quote(path),
                    quote(path)
                );
                s.stdin(&cmd, encoded.as_bytes())?;
                if *activate && std::env::var("IAC_APPLY_NETWORK_NOW").as_deref() == Ok("YES") {
                    s.run("ifreload -a")?;
                }
            }
        }
        results.push(serde_json::json!({"resource":resource(op),"status":"applied"}));
    }
    fs::write(
        repo.runtime().join(format!(
            "apply-{}.json",
            Utc::now().format("%Y%m%dT%H%M%SZ")
        )),
        serde_json::to_vec_pretty(&results)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&results)?);
    Ok(())
}

fn require_eq(n: &str, w: &str) -> Result<()> {
    if std::env::var(n).unwrap_or_default() != w {
        bail!("{n} must equal {w}")
    }
    Ok(())
}

fn resource(o: &Operation) -> &str {
    match o {
        Operation::ApiUpdate { resource, .. }
        | Operation::GrowDisk { resource, .. }
        | Operation::WriteFile { resource, .. } => resource,
    }
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::quote;
    #[test]
    fn shell_quote_is_safe() {
        assert_eq!(quote("a'b"), "'a'\\''b'")
    }
}

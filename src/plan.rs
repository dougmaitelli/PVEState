use crate::{
    api::Pve,
    config::{Lxc, Repository, Vm},
    render,
};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum Operation {
    ApiUpdate {
        domain: String,
        resource: String,
        endpoint: String,
        changes: BTreeMap<String, String>,
        digest: Option<String>,
    },
    GrowDisk {
        domain: String,
        resource: String,
        endpoint: String,
        disk: String,
        size_gb: u64,
    },
    WriteFile {
        domain: String,
        resource: String,
        path: String,
        content: String,
        before_sha256: String,
        activate: bool,
    },
}
impl Operation {
    pub fn domain(&self) -> &str {
        match self {
            Self::ApiUpdate { domain, .. }
            | Self::GrowDisk { domain, .. }
            | Self::WriteFile { domain, .. } => domain,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub schema_version: u8,
    pub created_at: DateTime<Utc>,
    pub target: String,
    pub operations: Vec<Operation>,
    pub blockers: Vec<String>,
    pub plan_sha256: String,
}
impl Plan {
    pub fn calculate_hash(&self) -> Result<String> {
        let mut x = self.clone();
        x.plan_sha256.clear();
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(&x)?)))
    }
    pub fn verify(&self) -> Result<()> {
        if self.calculate_hash()? != self.plan_sha256 {
            bail!("plan file integrity check failed")
        }
        Ok(())
    }
}
pub fn run(repo: &Repository) -> Result<Plan> {
    validate(repo)?;
    let manifest: Value = serde_json::from_slice(
        &fs::read(repo.observed().join("manifest.json")).context("run capture before plan")?,
    )?;
    let at = DateTime::parse_from_rfc3339(
        manifest["exported_at"]
            .as_str()
            .context("manifest exported_at")?,
    )?
    .with_timezone(&Utc);
    if Utc::now() - at > chrono::Duration::minutes(30) {
        bail!("observed exports are stale; run capture before plan")
    }
    let api = Pve::discovery()?;
    let mut operations = Vec::new();
    let mut blockers = Vec::new();
    for (id, d) in &repo.guests.lxcs {
        let actual = api.get(&format!("/nodes/{}/lxc/{id}/config", repo.guests.node))?;
        guest_lxc(
            &repo.guests.node,
            *id,
            d,
            &actual,
            &mut operations,
            &mut blockers,
        )?
    }
    for (id, d) in &repo.guests.vms {
        let actual = api.get(&format!("/nodes/{}/qemu/{id}/config", repo.guests.node))?;
        guest_vm(
            &repo.guests.node,
            *id,
            d,
            &actual,
            &mut operations,
            &mut blockers,
        )?
    }
    let dns = api.get(&format!("/nodes/{}/dns", repo.guests.node))?;
    let mut changes = BTreeMap::new();
    cmp(&mut changes, "search", &repo.network.dns.search, &dns);
    for (i, v) in repo.network.dns.servers.iter().enumerate() {
        cmp(&mut changes, &format!("dns{}", i + 1), v, &dns)
    }
    if !changes.is_empty() {
        operations.push(Operation::ApiUpdate {
            domain: "dns".into(),
            resource: repo.guests.node.clone(),
            endpoint: format!("/nodes/{}/dns", repo.guests.node),
            changes,
            digest: None,
        })
    }
    let wanted = render::network(&repo.network);
    file_op(
        repo,
        ("network", &repo.guests.node),
        ("network/interfaces", "/etc/network/interfaces"),
        wanted,
        true,
        &mut operations,
    )?;
    let cluster = &repo.firewall["cluster"];
    file_op(
        repo,
        ("firewall", "cluster"),
        ("pve/firewall/cluster.fw", "/etc/pve/firewall/cluster.fw"),
        render::firewall_policy(cluster),
        false,
        &mut operations,
    )?;
    if let Some(gs) = repo.firewall["guests"].as_mapping() {
        for (id, p) in gs {
            let id = id.as_u64().context("firewall VMID")?.to_string();
            file_op(
                repo,
                ("firewall", &id),
                (
                    &format!("pve/firewall/{id}.fw"),
                    &format!("/etc/pve/firewall/{id}.fw"),
                ),
                render::firewall_policy(p),
                false,
                &mut operations,
            )?
        }
    }
    let mut p = Plan {
        schema_version: 1,
        created_at: Utc::now(),
        target: api.endpoint().into(),
        operations,
        blockers,
        plan_sha256: String::new(),
    };
    p.plan_sha256 = p.calculate_hash()?;
    fs::create_dir_all(repo.runtime())?;
    fs::write(
        repo.runtime().join("production-plan.json"),
        serde_json::to_vec_pretty(&p)?,
    )?;
    Ok(p)
}
fn cmp(ch: &mut BTreeMap<String, String>, k: &str, w: &impl ToString, a: &Value) {
    if a.get(k).map(val).as_deref() != Some(&w.to_string()) {
        ch.insert(k.into(), w.to_string());
    }
}
fn val(v: &Value) -> String {
    v.as_str()
        .map(str::to_string)
        .unwrap_or_else(|| v.to_string())
}
fn opts(s: &str) -> BTreeMap<String, String> {
    s.split(',')
        .enumerate()
        .filter_map(|(index, x)| {
            x.split_once('=')
                .map(|(k, v)| (k.into(), v.into()))
                .or_else(|| (index == 0).then(|| ("volume".into(), x.into())))
        })
        .collect()
}
fn changed(w: &BTreeMap<String, String>, a: &Value) -> BTreeMap<String, String> {
    w.iter()
        .filter(|(k, v)| {
            if k.starts_with("net") {
                let have = opts(a.get(*k).and_then(Value::as_str).unwrap_or(""));
                let want = opts(v);
                want.iter().any(|(x, y)| {
                    have.get(x)
                        .map(String::as_str)
                        .unwrap_or(if x == "firewall" { "0" } else { "" })
                        != y
                })
            } else {
                a.get(*k)
                    .map(val)
                    .unwrap_or_else(|| if *k == "agent" { "0".into() } else { "".into() })
                    != **v
            }
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}
fn guest_lxc(
    node: &str,
    id: u32,
    d: &Lxc,
    a: &Value,
    ops: &mut Vec<Operation>,
    block: &mut Vec<String>,
) -> Result<()> {
    let mut w = BTreeMap::from([
        ("hostname".into(), d.hostname.clone()),
        ("cores".into(), d.cores.to_string()),
        ("memory".into(), d.memory_mb.to_string()),
        ("swap".into(), d.swap_mb.to_string()),
        ("onboot".into(), u8::from(d.start.onboot).to_string()),
        (
            "startup".into(),
            format!(
                "order={}{}",
                d.start.order,
                d.start
                    .delay_seconds
                    .map(|x| format!(",up={x}"))
                    .unwrap_or_default()
            ),
        ),
        ("net0".into(), render::lxc_nic(&d.network)),
    ]);
    for (i, n) in d.additional_networks.iter().enumerate() {
        w.insert(format!("net{}", i + 1), render::lxc_nic(n));
    }
    let c = changed(&w, a);
    if !c.is_empty() {
        ops.push(Operation::ApiUpdate {
            domain: "guests".into(),
            resource: format!("lxc/{id}"),
            endpoint: format!("/nodes/{node}/lxc/{id}/config"),
            changes: c,
            digest: a["digest"].as_str().map(str::to_string),
        })
    }
    disk(
        node,
        ("lxc", id),
        ("rootfs", &d.rootfs.storage, d.rootfs.size_gb),
        a,
        ops,
        block,
    )
}
fn guest_vm(
    node: &str,
    id: u32,
    d: &Vm,
    a: &Value,
    ops: &mut Vec<Operation>,
    block: &mut Vec<String>,
) -> Result<()> {
    let mut w = BTreeMap::from([
        ("name".into(), d.name.clone()),
        ("machine".into(), d.machine.clone()),
        ("bios".into(), d.bios.clone()),
        ("cores".into(), d.cpu.cores.to_string()),
        ("sockets".into(), d.cpu.sockets.to_string()),
        ("memory".into(), d.memory_mb.to_string()),
        ("cpu".into(), d.cpu.r#type.clone()),
        ("onboot".into(), u8::from(d.start.onboot).to_string()),
        ("agent".into(), u8::from(d.qemu_guest_agent).to_string()),
        ("startup".into(), format!("order={}", d.start.order)),
    ]);
    for (i, n) in d.networks.iter().enumerate() {
        w.insert(format!("net{i}"), render::vm_nic(n));
    }
    for u in &d.usb_passthrough {
        w.insert(u.slot.clone(), format!("host={}", u.host));
    }
    let c = changed(&w, a);
    if !c.is_empty() {
        ops.push(Operation::ApiUpdate {
            domain: "guests".into(),
            resource: format!("qemu/{id}"),
            endpoint: format!("/nodes/{node}/qemu/{id}/config"),
            changes: c,
            digest: a["digest"].as_str().map(str::to_string),
        })
    }
    disk(
        node,
        ("qemu", id),
        (&d.disk.interface, &d.disk.storage, d.disk.size_gb),
        a,
        ops,
        block,
    )
}
fn disk(
    node: &str,
    identity: (&str, u32),
    desired: (&str, &str, u64),
    a: &Value,
    ops: &mut Vec<Operation>,
    block: &mut Vec<String>,
) -> Result<()> {
    let (kind, id) = identity;
    let (key, store, size) = desired;
    let o = opts(a[key].as_str().context("disk config")?);
    let volume = o.get("volume").cloned().unwrap_or_default();
    let actual_store = volume.split(':').next().unwrap_or("");
    let current = o
        .get("size")
        .and_then(|x| x.trim_end_matches('G').parse::<u64>().ok())
        .unwrap_or(0);
    if actual_store != store {
        block.push(format!("{kind}/{id}: storage moves are not automatic"))
    }
    if size < current {
        block.push(format!("{kind}/{id}: disk shrinking is forbidden"))
    } else if size > current {
        ops.push(Operation::GrowDisk {
            domain: "guests".into(),
            resource: format!("{kind}/{id}/{key}"),
            endpoint: format!("/nodes/{node}/{kind}/{id}/resize"),
            disk: key.into(),
            size_gb: size,
        })
    }
    Ok(())
}
fn file_op(
    repo: &Repository,
    identity: (&str, &str),
    paths: (&str, &str),
    wanted: String,
    activate: bool,
    ops: &mut Vec<Operation>,
) -> Result<()> {
    let (domain, res) = identity;
    let (local, remote) = paths;
    let path = repo.observed().join(local);
    let current = fs::read_to_string(&path).unwrap_or_default();
    let differs = if domain == "firewall" {
        render::firewall_semantic(&wanted) != render::firewall_semantic(&current)
    } else {
        render::semantic_lines(&wanted) != render::semantic_lines(&current)
    };
    if differs {
        ops.push(Operation::WriteFile {
            domain: domain.into(),
            resource: res.into(),
            path: remote.into(),
            content: wanted,
            before_sha256: hex::encode(Sha256::digest(current.as_bytes())),
            activate,
        })
    }
    Ok(())
}
fn validate(repo: &Repository) -> Result<()> {
    let mut macs = std::collections::BTreeSet::new();
    for l in repo.guests.lxcs.values() {
        for n in std::iter::once(&l.network).chain(&l.additional_networks) {
            if !macs.insert(n.mac.to_uppercase()) {
                bail!("duplicate MAC: {}", n.mac)
            }
        }
    }
    for v in repo.guests.vms.values() {
        for n in &v.networks {
            if !macs.insert(n.mac.to_uppercase()) {
                bail!("duplicate MAC: {}", n.mac)
            }
        }
    }
    println!(
        "configuration valid: {} guests, {} NICs",
        repo.guests.lxcs.len() + repo.guests.vms.len(),
        macs.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proxmox_volume_and_options() {
        let parsed = opts("VMs:vm-107-disk-1,discard=on,size=64G");
        assert_eq!(parsed["volume"], "VMs:vm-107-disk-1");
        assert_eq!(parsed["size"], "64G");
    }

    #[test]
    fn plan_hash_detects_tampering() {
        let mut plan = Plan {
            schema_version: 1,
            created_at: Utc::now(),
            target: "https://pve.example:8006".into(),
            operations: Vec::new(),
            blockers: Vec::new(),
            plan_sha256: String::new(),
        };
        plan.plan_sha256 = plan.calculate_hash().unwrap();
        assert!(plan.verify().is_ok());
        plan.target.push_str("/tampered");
        assert!(plan.verify().is_err());
    }

    #[test]
    fn firewall_option_order_is_not_drift() {
        let a = "[OPTIONS]\nenable: 1\nlog_level_in: nolog\n";
        let b = "[OPTIONS]\nlog_level_in: nolog\nenable: 1\n";
        assert_eq!(render::firewall_semantic(a), render::firewall_semantic(b));
    }
}

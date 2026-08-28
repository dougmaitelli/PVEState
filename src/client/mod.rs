mod pbs;
mod pve;
mod ssh;

use anyhow::Result;
use serde_json::Value;
use std::collections::BTreeMap;

pub use pbs::{Pbs, Snapshot as PbsSnapshot, capture as capture_pbs};
pub use pve::Pve;
pub use ssh::{Ssh, SshOutput};

pub trait PveClient {
    fn endpoint(&self) -> &str;
    fn get(&self, path: &str) -> Result<Value>;
    fn put(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()>;
    fn post(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()>;
    fn delete(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()>;
}

pub trait PbsClient {
    fn endpoint(&self) -> &str;
    fn get(&self, path: &str) -> Result<Value>;
    fn put(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()>;
    fn post(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()>;
    fn delete(&self, path: &str, data: &BTreeMap<String, String>) -> Result<()>;
}

pub trait RemoteHost {
    fn host_key_fingerprint(&self) -> Result<String>;
    fn run(&self, remote: &str) -> Result<String>;
    fn probe(&self, remote: &str) -> Result<SshOutput>;
    fn stdin(&self, remote: &str, input: &[u8]) -> Result<()>;
}

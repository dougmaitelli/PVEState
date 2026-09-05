mod cli;
mod client;
mod command;
mod config;
mod discovery;
mod model;
mod resource;
mod scope;
mod settings;
mod utility;

use anyhow::Result;
use std::path::Path;

#[doc(hidden)]
pub use cli::run_cli;

/// Validate a local PVE State configuration repository.
pub fn validate_local_state(path: &Path) -> Result<()> {
    config::open(path).map(drop)
}

/// Create a new local PVE State configuration repository.
pub fn initialize_local_state(path: &Path) -> Result<()> {
    config::scaffold::initialize(path)
}

/// Generate schemas for every supported local configuration document.
pub fn write_schemas(output: Option<&Path>) -> Result<()> {
    config::schema::write(output)
}

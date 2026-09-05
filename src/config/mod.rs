mod adoption;
pub(crate) mod artifacts;
mod layout;
mod loader;
mod local_patch;
mod local_state;
pub(crate) mod scaffold;
pub(crate) mod schema;
pub(crate) mod transaction;

pub(crate) use adoption::AdoptionCandidate;
pub(crate) use layout::RepositoryLayout;
pub(crate) use loader::open;
pub(crate) use local_patch::{ConfigDocument, LocalPatch};
pub(crate) use local_state::{LocalState, RepositoryDocument};

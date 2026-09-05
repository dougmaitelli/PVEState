use super::LocalPatch;
use crate::reconcile::ResourceId;

#[derive(Debug)]
pub(crate) struct AdoptionCandidate {
    pub(crate) id: String,
    pub(crate) resource: String,
    pub(crate) field: String,
    pub(crate) local: String,
    pub(crate) captured: String,
    pub(crate) reason: Option<String>,
    patches: Option<Vec<LocalPatch>>,
}

impl serde::Serialize for AdoptionCandidate {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut state = serializer.serialize_struct("AdoptionCandidate", 7)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("resource", &self.resource)?;
        state.serialize_field("field", &self.field)?;
        state.serialize_field("local", &self.local)?;
        state.serialize_field("captured", &self.captured)?;
        state.serialize_field("adoptable", &self.is_adoptable())?;
        if let Some(reason) = &self.reason {
            state.serialize_field("reason", reason)?;
        }
        state.end()
    }
}

impl AdoptionCandidate {
    pub(crate) fn adoptable(
        resource: &ResourceId,
        field: impl Into<String>,
        local: impl Into<String>,
        captured: impl Into<String>,
        patches: Vec<LocalPatch>,
    ) -> Self {
        let field = field.into();
        Self {
            id: format!("{resource}:{field}"),
            resource: resource.to_string(),
            field,
            local: local.into(),
            captured: captured.into(),
            reason: None,
            patches: Some(patches),
        }
    }

    pub(crate) fn blocked(
        resource: &ResourceId,
        field: impl Into<String>,
        local: impl Into<String>,
        captured: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        let field = field.into();
        Self {
            id: format!("{resource}:{field}"),
            resource: resource.to_string(),
            field,
            local: local.into(),
            captured: captured.into(),
            reason: Some(reason.into()),
            patches: None,
        }
    }

    pub(crate) const fn is_adoptable(&self) -> bool {
        self.patches.is_some()
    }

    pub(crate) fn patches(&self) -> Option<&[LocalPatch]> {
        self.patches.as_deref()
    }
}

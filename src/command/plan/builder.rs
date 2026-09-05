use super::{Operation, Plan};
use crate::utility::plan_envelope;
use anyhow::Result;
use chrono::Utc;

pub struct PlanBuilder {
    capture_id: String,
    target: String,
    pbs_target: String,
    operations: Vec<Operation>,
    blockers: Vec<String>,
}

impl PlanBuilder {
    pub fn new(capture_id: &str, target: &str, pbs_target: &str) -> Self {
        Self {
            capture_id: capture_id.into(),
            target: target.into(),
            pbs_target: pbs_target.into(),
            operations: Vec::new(),
            blockers: Vec::new(),
        }
    }

    pub fn operations(&mut self) -> &mut Vec<Operation> {
        &mut self.operations
    }

    pub fn blockers(&mut self) -> &mut Vec<String> {
        &mut self.blockers
    }

    pub fn parts(&mut self) -> (&mut Vec<Operation>, &mut Vec<String>) {
        (&mut self.operations, &mut self.blockers)
    }

    pub fn finish(self) -> Result<Plan> {
        let mut plan = Plan {
            schema_version: 2,
            created_at: Utc::now(),
            capture_id: self.capture_id,
            target: self.target,
            pbs_target: self.pbs_target,
            operations: self.operations,
            blockers: self.blockers,
            plan_sha256: String::new(),
        };
        plan_envelope::sign(&mut plan)?;
        Ok(plan)
    }
}

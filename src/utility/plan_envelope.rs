use anyhow::{Result, bail};
use serde::Serialize;
use sha2::{Digest, Sha256};

pub(crate) trait PlanEnvelope: Clone + Serialize {
    fn integrity(&self) -> &str;
    fn set_integrity(&mut self, value: String);
}

pub(crate) fn sign<T: PlanEnvelope>(plan: &mut T) -> Result<()> {
    plan.set_integrity(String::new());
    plan.set_integrity(hash(plan)?);
    Ok(())
}

pub(crate) fn verify<T: PlanEnvelope>(plan: &T, message: &str) -> Result<()> {
    let mut unsigned = plan.clone();
    unsigned.set_integrity(String::new());
    if hash(&unsigned)? != plan.integrity() {
        bail!("{message}")
    }
    Ok(())
}

fn hash<T: Serialize>(value: &T) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(value)?)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Clone, Serialize)]
    struct Fixture {
        value: String,
        sha: String,
    }

    impl PlanEnvelope for Fixture {
        fn integrity(&self) -> &str {
            &self.sha
        }
        fn set_integrity(&mut self, value: String) {
            self.sha = value;
        }
    }

    #[test]
    fn signed_envelopes_detect_tampering() {
        let mut fixture = Fixture {
            value: "before".into(),
            sha: String::new(),
        };
        sign(&mut fixture).unwrap();
        verify(&fixture, "tampered").unwrap();
        fixture.value = "after".into();
        assert_eq!(
            verify(&fixture, "tampered").unwrap_err().to_string(),
            "tampered"
        );
    }
}

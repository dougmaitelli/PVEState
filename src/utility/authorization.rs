use anyhow::{Result, bail};
use chrono::{DateTime, Duration, Utc};

pub struct Policy<'a> {
    pub enabled: bool,
    pub enabled_error: &'a str,
    pub confirmation: Option<&'a str>,
    pub confirmation_error: &'a str,
    pub requested_target: &'a str,
    pub target_error: &'a str,
    pub max_age: Duration,
    pub stale_error: &'a str,
    pub blocker_prefix: &'a str,
    pub blocker_separator: &'a str,
}

pub fn authorize(
    plan_sha: &str,
    plan_target: &str,
    created_at: DateTime<Utc>,
    blockers: &[String],
    policy: Policy<'_>,
) -> Result<()> {
    if !policy.enabled {
        bail!(policy.enabled_error.to_string())
    }
    if policy.confirmation != Some(plan_sha) {
        bail!(policy.confirmation_error.to_string())
    }
    if plan_target != policy.requested_target {
        bail!(policy.target_error.to_string())
    }
    if Utc::now() - created_at > policy.max_age {
        bail!(policy.stale_error.to_string())
    }
    if !blockers.is_empty() {
        bail!(
            "{}{}",
            policy.blocker_prefix,
            blockers.join(policy.blocker_separator)
        )
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy<'a>(confirmation: Option<&'a str>) -> Policy<'a> {
        Policy {
            enabled: true,
            enabled_error: "disabled",
            confirmation,
            confirmation_error: "confirmation",
            requested_target: "host",
            target_error: "target",
            max_age: Duration::minutes(30),
            stale_error: "stale",
            blocker_prefix: "blockers: ",
            blocker_separator: ", ",
        }
    }

    #[test]
    fn confirmation_is_required() {
        assert_eq!(
            authorize("sha", "host", Utc::now(), &[], policy(None))
                .unwrap_err()
                .to_string(),
            "confirmation"
        );
    }
}

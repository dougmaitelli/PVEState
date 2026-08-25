use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::{fs, path::Path};

pub fn write_snapshot<T: Serialize>(
    name: &str,
    collected_at: DateTime<Utc>,
    snapshot: &T,
    runtime: &Path,
    observed: &Path,
) -> Result<()> {
    let raw = serde_json::to_value(snapshot)?;
    let raw_bytes = serde_json::to_vec_pretty(&raw)?;
    let stamp = collected_at.format("%Y%m%dT%H%M%SZ");
    fs::create_dir_all(runtime)?;
    fs::write(runtime.join(format!("{name}-{stamp}.json")), &raw_bytes)?;
    fs::write(runtime.join(format!("{name}-latest.json")), &raw_bytes)?;

    let mut safe = raw;
    sanitize(&mut safe);
    if let Some(object) = safe.as_object_mut() {
        object.remove("collected_at");
    }
    fs::create_dir_all(observed)?;
    fs::write(
        observed.join(format!("{name}.json")),
        serde_json::to_vec_pretty(&safe)?,
    )?;
    Ok(())
}

fn sanitize(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                let key = key.to_ascii_lowercase().replace('_', "-");
                if ["password", "secret", "token", "private-key", "access-key"]
                    .iter()
                    .any(|sensitive| key.contains(sensitive))
                {
                    *value = Value::String("[REDACTED]".into());
                } else {
                    sanitize(value);
                }
            }
        },
        Value::Array(items) => items.iter_mut().for_each(sanitize),
        _ => {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_snapshot_redacts_nested_secrets() {
        let mut value = serde_json::json!({
            "token": "one",
            "nested": [{"access-key-id": "two", "bucket": "safe"}]
        });
        sanitize(&mut value);
        assert_eq!(value["token"], "[REDACTED]");
        assert_eq!(value["nested"][0]["access-key-id"], "[REDACTED]");
        assert_eq!(value["nested"][0]["bucket"], "safe");
    }
}

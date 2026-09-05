use crate::utility::progress;
use anyhow::Result;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{collections::BTreeMap, fmt};

pub type ApiObject = BTreeMap<String, Value>;
pub type ApiObjects = Vec<ApiObject>;
pub type RawResponse = CapturedResponse<Value>;
pub type ObjectResponse = CapturedResponse<ApiObject>;
pub type ObjectsResponse = CapturedResponse<ApiObjects>;

#[derive(Debug, Serialize)]
pub struct CapturedResponse<T> {
    pub ok: bool,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<CapturedError>,
}

#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct CapturedError(String);

impl CapturedError {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for CapturedError {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}

impl From<String> for CapturedError {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl fmt::Display for CapturedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<T> CapturedResponse<T> {
    pub fn failure(&self) -> Option<&str> {
        self.error.as_ref().map(CapturedError::as_str)
    }
}

pub fn capture<T>(path: &str, request: impl FnOnce() -> Result<Value>) -> CapturedResponse<T>
where
    T: DeserializeOwned,
{
    progress::operation(format!("GET {path}"));

    let result = request().and_then(|value| {
        serde_json::from_value(value)
            .map_err(anyhow::Error::from)
            .map_err(|error| error.context(format!("decode response from {path}")))
    });

    match result {
        Ok(data) => CapturedResponse {
            ok: true,
            path: path.into(),
            data: Some(data),
            error: None,
        },
        Err(error) => {
            progress::detail(format!("failed: {error:#}"));
            CapturedResponse {
                ok: false,
                path: path.into(),
                data: None,
                error: Some(format!("{error:#}").into()),
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_capture_rejects_an_unexpected_shape() {
        let response: ObjectsResponse = capture("/items", || Ok(serde_json::json!({})));

        assert!(!response.ok);
        assert!(response.data.is_none());
        assert!(
            response
                .error
                .unwrap()
                .as_str()
                .contains("decode response from /items")
        );
    }

    #[test]
    fn envelope_serialization_remains_compatible() {
        let response: ObjectResponse =
            capture("/config", || Ok(serde_json::json!({"name": "primary"})));

        assert_eq!(
            serde_json::to_value(response).unwrap(),
            serde_json::json!({
                "ok": true,
                "path": "/config",
                "data": {"name": "primary"}
            })
        );
    }
}

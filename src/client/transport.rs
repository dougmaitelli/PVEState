use anyhow::{Context, Result};
use reqwest::{
    Certificate, Method, Url,
    blocking::Client,
    header::{ACCEPT, AUTHORIZATION, HeaderValue, USER_AGENT},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{fs, path::Path, time::Duration};

pub(super) struct JsonApiClient {
    endpoint: Url,
    base: Url,
    authorization: HeaderValue,
    http: Client,
}

#[derive(serde::Deserialize)]
struct Envelope<T> {
    #[serde(default)]
    data: Option<T>,
}

impl JsonApiClient {
    pub(crate) fn new(
        endpoint: &Url,
        verify_tls: bool,
        ca_file: Option<&Path>,
        authorization: &str,
    ) -> Result<Self> {
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(30))
            .danger_accept_invalid_certs(!verify_tls);
        if let Some(path) = ca_file {
            let pem =
                fs::read(path).with_context(|| format!("read API CA file {}", path.display()))?;
            builder = builder.add_root_certificate(Certificate::from_pem(&pem)?);
        }
        let mut authorization = HeaderValue::from_str(authorization)
            .context("authorization value is not a valid HTTP header")?;
        authorization.set_sensitive(true);
        let base = Url::parse(&format!(
            "{}/api2/json/",
            endpoint.as_str().trim_end_matches('/')
        ))?;
        Ok(Self {
            endpoint: endpoint.clone(),
            base,
            authorization,
            http: builder.build()?,
        })
    }

    pub(crate) fn endpoint(&self) -> &str {
        self.endpoint.as_str().trim_end_matches('/')
    }

    pub(crate) fn get_data<T: DeserializeOwned + Default>(&self, path: &str) -> Result<T> {
        let response = self
            .http
            .get(self.url(path)?)
            .header(AUTHORIZATION, &self.authorization)
            .header(ACCEPT, "application/json")
            .header(USER_AGENT, concat!("pvestate/", env!("CARGO_PKG_VERSION")))
            .send()?
            .error_for_status()?;
        Ok(response.json::<Envelope<T>>()?.data.unwrap_or_default())
    }

    pub(crate) fn form(&self, method: Method, path: &str, data: &impl Serialize) -> Result<()> {
        self.http
            .request(method, self.url(path)?)
            .header(AUTHORIZATION, &self.authorization)
            .header(USER_AGENT, concat!("pvestate/", env!("CARGO_PKG_VERSION")))
            .form(data)
            .send()?
            .error_for_status()?;
        Ok(())
    }

    fn url(&self, path: &str) -> Result<Url> {
        self.base
            .join(path.trim_start_matches('/'))
            .with_context(|| format!("invalid API path {path}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_paths_remain_below_the_json_base() {
        let client = JsonApiClient::new(
            &Url::parse("https://pve.example:8006").unwrap(),
            true,
            None,
            "Token value",
        )
        .unwrap();

        assert_eq!(
            client.url("/nodes/pve/status").unwrap().as_str(),
            "https://pve.example:8006/api2/json/nodes/pve/status"
        );
    }

    #[test]
    fn rejects_header_injection_in_credentials() {
        let error = JsonApiClient::new(
            &Url::parse("https://pve.example:8006").unwrap(),
            true,
            None,
            "Token value\r\nInjected: yes",
        )
        .err()
        .unwrap();

        assert!(error.to_string().contains("valid HTTP header"));
    }
}

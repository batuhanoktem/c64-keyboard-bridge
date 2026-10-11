use crate::config::Config;
use anyhow::{Context, Result, bail};
use reqwest::{
    Client, StatusCode,
    header::{HeaderMap, HeaderValue},
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const MAX_RESPONSE: usize = 64 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeviceInfo {
    pub product: String,
    pub firmware_version: String,
    pub fpga_version: String,
    #[serde(default)]
    pub core_version: Option<String>,
    pub hostname: String,
    pub unique_id: String,
    #[serde(default)]
    pub errors: Vec<String>,
}

pub struct RestClient {
    client: Client,
    base: String,
}
impl RestClient {
    pub fn new(config: &Config) -> Result<Self> {
        let mut headers = HeaderMap::new();
        let mut password = HeaderValue::from_bytes(config.password.expose().as_bytes())
            .map_err(|_| anyhow::anyhow!("C64_NETWORK_PASSWORD is invalid for an HTTP header"))?;
        password.set_sensitive(true);
        headers.insert("X-Password", password);
        let client = Client::builder()
            .default_headers(headers)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(15))
            .build()
            .context("Cannot initialize REST client")?;
        Ok(Self {
            client,
            base: format!(
                "http://{}:{}/v1",
                config.require_device()?,
                config.device_port
            ),
        })
    }
    async fn bounded_json(&self, mut response: reqwest::Response) -> Result<serde_json::Value> {
        match response.status() {
            StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => {
                bail!("REST authentication failed; check C64_NETWORK_PASSWORD")
            }
            StatusCode::NOT_FOUND => bail!("REST endpoint is unavailable on this firmware"),
            status if !status.is_success() => {
                bail!("REST request failed with HTTP {}", status.as_u16())
            }
            _ => {}
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .context("REST response interrupted")?
        {
            if bytes.len() + chunk.len() > MAX_RESPONSE {
                bail!("REST response exceeded 64 KiB limit");
            }
            bytes.extend_from_slice(&chunk);
        }
        let json: serde_json::Value =
            serde_json::from_slice(&bytes).context("REST returned invalid JSON")?;
        let errors = json
            .get("errors")
            .and_then(|v| v.as_array())
            .context("REST reply omitted the errors array")?;
        if !errors.is_empty() {
            bail!(
                "REST device reported an operation error ({} entries); details withheld to avoid disclosing secrets",
                errors.len()
            );
        }
        Ok(json)
    }
    pub async fn info(&self) -> Result<DeviceInfo> {
        let response = self
            .client
            .get(format!("{}/info", self.base))
            .send()
            .await
            .context("Cannot reach C64 REST API")?;
        let info: DeviceInfo = serde_json::from_value(self.bounded_json(response).await?)
            .map_err(|_| anyhow::anyhow!("Invalid device information response"))?;
        if info.product != "C64 Ultimate" {
            bail!("Unexpected device product; this bridge targets C64 Ultimate");
        }
        Ok(info)
    }
    pub async fn upload(&self, prg: Vec<u8>, run: bool, allow_reset: bool) -> Result<()> {
        if !allow_reset {
            bail!(
                "PRG deployment resets the C64 and interrupts running software; supply --allow-reset explicitly"
            );
        }
        if prg.len() < 4 || prg.len() > 65537 {
            bail!("Invalid PRG file size");
        }
        let action = if run { "run_prg" } else { "load_prg" };
        let response = self.client.post(format!("{}/runners:{action}",self.base))
            .header("Content-Type","application/octet-stream").body(prg).send().await
            .context("PRG request failed; the C64 may already have loaded or run it. Inspect the device before retrying")?;
        self.bounded_json(response).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Overrides;
    use std::collections::BTreeMap;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    async fn server(status: &str, body: &str) -> (Config, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buf = [0; 1024];
            loop {
                let n = socket.read(&mut buf).await.unwrap();
                request.extend_from_slice(&buf[..n]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&request[..end]).to_lowercase();
                    let length = header
                        .lines()
                        .find_map(|l| {
                            l.strip_prefix("content-length: ")
                                .and_then(|v| v.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
                assert_ne!(n, 0);
            }
            socket.write_all(response.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });
        let values = BTreeMap::from([
            ("C64_HOST".into(), "127.0.0.1".into()),
            ("C64_PORT".into(), port.to_string()),
            ("C64_NETWORK_PASSWORD".into(), "a$ #=".into()),
        ]);
        (
            Config::from_values(values, BTreeMap::new(), Overrides::default()).unwrap(),
            handle,
        )
    }
    #[tokio::test]
    async fn request_header_and_authentication_error() {
        let (c, s) = server("403 Forbidden", "{\"errors\":[]}").await;
        let error = RestClient::new(&c)
            .unwrap()
            .info()
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("authentication"));
        assert!(!error.contains("a$ #="));
        let request = s.await.unwrap();
        assert!(request.starts_with("GET /v1/info HTTP/1.1"));
        assert!(request.to_lowercase().contains("x-password: a$ #="));
    }
    #[tokio::test]
    async fn raw_binary_upload_includes_load_address_and_requires_opt_in() {
        let (c, s) = server("200 OK", "{\"errors\":[]}").await;
        let client = RestClient::new(&c).unwrap();
        assert!(
            client
                .upload(vec![1, 8, 65, 66], true, false)
                .await
                .is_err()
        );
        client.upload(vec![1, 8, 65, 66], true, true).await.unwrap();
        let request = s.await.unwrap();
        assert!(request.starts_with("POST /v1/runners:run_prg "));
        assert!(
            request
                .to_lowercase()
                .contains("content-type: application/octet-stream")
        );
        assert!(request.ends_with("\u{1}\u{8}AB"));
    }

    #[tokio::test]
    async fn device_errors_and_invalid_info_cannot_echo_response_values() {
        for body in [
            "{\"errors\":[\"a$ #=\"]}",
            "{\"product\":{},\"firmware_version\":\"a$ #=\",\"errors\":[]}",
            "{\"errors\":[]}",
        ] {
            let (config, server) = server("200 OK", body).await;
            let error = RestClient::new(&config).unwrap().info().await.unwrap_err();
            assert!(!format!("{error:#}").contains("a$ #="));
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn rejects_redirects_and_oversized_replies_without_following_or_retrying() {
        let (config, redirect_server) = server("302 Found", "{\"errors\":[]}").await;
        assert!(
            RestClient::new(&config)
                .unwrap()
                .info()
                .await
                .unwrap_err()
                .to_string()
                .contains("302")
        );
        redirect_server.await.unwrap();
        let (config, server) = server("200 OK", &"x".repeat(MAX_RESPONSE + 1)).await;
        assert!(
            RestClient::new(&config)
                .unwrap()
                .info()
                .await
                .unwrap_err()
                .to_string()
                .contains("64 KiB")
        );
        server.await.unwrap();
    }
}

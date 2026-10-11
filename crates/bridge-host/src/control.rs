use crate::pairing::{Token, read_token};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    net::{Ipv4Addr, SocketAddr},
    path::Path,
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Serialize, Deserialize)]
pub struct RuntimeRecord {
    pub pid: u32,
    pub port: u16,
}
#[derive(Serialize, Deserialize)]
pub struct ControlRequest {
    pub token: String,
    pub command: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    pub running: bool,
    pub connected: bool,
    pub enabled: bool,
    pub backend: String,
    pub device: Option<String>,
    pub events: u64,
    pub blocked_combinations: u64,
    pub detail: String,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            running: true,
            connected: false,
            enabled: false,
            backend: "disabled".into(),
            device: None,
            events: 0,
            blocked_combinations: 0,
            detail: "Waiting for authenticated C64 connection".into(),
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ControlResponse {
    pub ok: bool,
    pub status: Status,
    pub error: Option<String>,
}

pub async fn read_json<T: serde::de::DeserializeOwned>(
    socket: &mut tokio::net::TcpStream,
) -> Result<T> {
    let length = socket.read_u16_le().await? as usize;
    if length == 0 || length > 4096 {
        bail!("Control message exceeded bounds");
    }
    let mut data = vec![0; length];
    socket.read_exact(&mut data).await?;
    serde_json::from_slice(&data).context("Invalid control message")
}
pub async fn write_json(socket: &mut tokio::net::TcpStream, value: &impl Serialize) -> Result<()> {
    let data = serde_json::to_vec(value)?;
    if data.is_empty() || data.len() > 4096 {
        bail!("Control response exceeded bounds");
    }
    socket.write_u16_le(data.len() as u16).await?;
    socket.write_all(&data).await?;
    Ok(())
}
pub async fn send(state_dir: &Path, command: &str) -> Result<ControlResponse> {
    let bytes = std::fs::read(state_dir.join("runtime.json"))
        .context("Bridge is not running (no runtime record)")?;
    if bytes.len() > 4096 {
        bail!("Invalid runtime record");
    }
    let record: RuntimeRecord = serde_json::from_slice(&bytes).context("Invalid runtime record")?;
    let token = read_token(state_dir)?;
    let operation = async {
        let mut socket =
            tokio::net::TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, record.port)))
                .await
                .context("Bridge is not responding; its runtime record may be stale")?;
        write_json(
            &mut socket,
            &ControlRequest {
                token: token.hex(),
                command: command.into(),
            },
        )
        .await?;
        read_json(&mut socket).await
    };
    tokio::time::timeout(Duration::from_secs(3), operation)
        .await
        .context("Bridge control timed out")?
}
pub fn authorized(token: Token, request: &ControlRequest) -> bool {
    Token::from_hex(&request.token).is_ok_and(|other| token.matches(&other.0))
}

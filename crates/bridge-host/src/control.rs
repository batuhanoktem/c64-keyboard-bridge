use crate::pairing::{Token, read_token};
use crate::session_diagnostics::SessionDiagnostics;
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
/// Bounded permission evidence. No portal/session handles or keymap data escape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationPhase {
    #[default]
    NotRequested,
    Pending,
    Ready,
    Denied,
    Cancelled,
    Failed,
}
impl AuthorizationPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotRequested => "not_requested",
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Denied => "denied",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthorizationStatus {
    pub phase: AuthorizationPhase,
    pub enable_requested: bool,
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
    #[serde(default)]
    pub diagnostics: SessionDiagnostics,
    #[serde(default)]
    pub authorization: AuthorizationStatus,
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
            diagnostics: SessionDiagnostics::default(),
            authorization: AuthorizationStatus::default(),
        }
    }
}
impl Status {
    pub(crate) fn snapshot(&self) -> Self {
        let mut snapshot = self.clone();
        snapshot.diagnostics.refresh();
        snapshot
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_diagnostics::{CleanupOutcome, FailureReason};

    #[test]
    fn legacy_status_defaults_diagnostics_and_full_response_stays_bounded() {
        let mut status = Status::default();
        let mut legacy = serde_json::to_value(&status).unwrap();
        legacy.as_object_mut().unwrap().remove("diagnostics");
        legacy.as_object_mut().unwrap().remove("authorization");
        let legacy: Status = serde_json::from_value(legacy).unwrap();
        assert_eq!(legacy.diagnostics.attempts, 0);
        assert_eq!(legacy.authorization, AuthorizationStatus::default());
        assert!(legacy.diagnostics.last_failure.is_none());
        status.diagnostics.begin();
        status.diagnostics.valid_frame();
        status
            .diagnostics
            .record_failure(FailureReason::HeartbeatTimeout, CleanupOutcome::Pending);
        let mut value = serde_json::to_value(&status).unwrap();
        fn maximize(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Number(_) => *value = serde_json::Value::from(u64::MAX),
                serde_json::Value::Object(fields) => fields.values_mut().for_each(maximize),
                _ => {}
            }
        }
        maximize(&mut value);
        let status: Status = serde_json::from_value(value).unwrap();
        let response = ControlResponse {
            ok: true,
            status,
            error: None,
        };
        assert!(serde_json::to_vec(&response).unwrap().len() < 4096);
    }
}

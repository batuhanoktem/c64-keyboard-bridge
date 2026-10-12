use bridge_core::{BackendCapabilities, MappingDocument};
use bridge_host::control::Status;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct GuiSettings {
    pub schema_version: u32,
    pub env_file: String,
    pub device_host: String,
    pub device_port: u16,
    pub bind_address: String,
    pub advertise_ip: String,
    pub bridge_port: u16,
    pub allowed_ips: String,
    pub layout: String,
    pub profile: String,
    pub backend: String,
    pub mappings_file: String,
    pub repeat_delay_ms: u64,
    pub repeat_interval_ms: u64,
    pub log_level: String,
    pub remember_password: bool,
    pub credential_id: String,
}
impl Default for GuiSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            env_file: ".env".into(),
            device_host: String::new(),
            device_port: 80,
            bind_address: String::new(),
            advertise_ip: String::new(),
            bridge_port: 6502,
            allowed_ips: String::new(),
            layout: "US".into(),
            profile: "modern".into(),
            backend: "auto".into(),
            mappings_file: String::new(),
            repeat_delay_ms: 500,
            repeat_interval_ms: 40,
            log_level: "info".into(),
            remember_password: false,
            credential_id: String::new(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum GuiPhase {
    #[default]
    Idle,
    Starting,
    Running,
    Stopping,
}
#[derive(Clone, Debug)]
pub struct GuiBackendInfo {
    pub name: String,
    pub available: bool,
    pub authorized: bool,
    pub layout_matches: Option<bool>,
    pub capabilities: BackendCapabilities,
    pub detail: String,
}
#[derive(Clone, Debug)]
pub struct GuiDeviceInfo {
    pub product: String,
    pub firmware: String,
    pub fpga: String,
    pub core: String,
}
#[derive(Clone, Debug)]
pub struct GuiCheck {
    pub name: String,
    pub state: String,
    pub detail: String,
}
#[derive(Clone, Debug)]
pub struct Activity {
    pub elapsed_ms: u64,
    pub summary: String,
}
#[derive(Clone, Debug, Default)]
pub struct GuiSnapshot {
    pub phase: GuiPhase,
    pub owned: bool,
    pub status: Option<Status>,
    pub countdown: Option<u64>,
    pub backend: Option<GuiBackendInfo>,
    pub device: Option<GuiDeviceInfo>,
    pub checks: Vec<GuiCheck>,
    pub history: Vec<Activity>,
    pub message: String,
    pub quit_ready: bool,
    pub settings_pending: bool,
    pub busy: bool,
    pub tray_available: bool,
}
// Password-bearing commands deliberately have no Debug or Serialize implementation.
#[derive(Clone)]
pub enum GuiAction {
    Start {
        deploy: bool,
        allow_reset: bool,
    },
    Stop,
    Pause,
    Enable,
    CancelCountdown,
    SaveSettings {
        settings: GuiSettings,
        password: String,
    },
    ApplySettings {
        settings: GuiSettings,
        password: String,
    },
    DeviceInfo,
    Deploy {
        run: bool,
        allow_reset: bool,
    },
    Probe {
        allow_reset: bool,
    },
    Doctor,
    RequestPermission,
    RotatePairing,
    TestInput,
    ImportProfile(PathBuf),
    SaveProfile {
        document: MappingDocument,
        path: Option<PathBuf>,
    },
    ExportProfile {
        document: MappingDocument,
        path: PathBuf,
    },
    ApplyProfile {
        document: MappingDocument,
    },
    ExportDiagnostics(PathBuf),
    ForgetPassword,
    RemoveLocalData {
        confirmed: bool,
    },
    FocusChanged(bool),
    TrayAvailable(bool),
    Quit,
}
#[derive(Clone, Debug)]
pub enum GuiEvent {
    Snapshot(Box<GuiSnapshot>),
    SettingsLoaded(GuiSettings),
    ProfileLoaded {
        document: MappingDocument,
        path: PathBuf,
    },
    ProfileDraftLoaded {
        text: String,
        path: PathBuf,
    },
    ProfileSaved {
        path: PathBuf,
    },
    Error(String),
    LocalDataReset(GuiSettings),
    ShowWindow,
}

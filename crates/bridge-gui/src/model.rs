use bridge_core::{BackendCapabilities, MappingDocument};
use bridge_host::control::Status;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Presentation choices follow the posting process, never an imported backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiPlatform {
    MacOs,
    Windows,
    Linux,
}
impl GuiPlatform {
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }
    pub const fn backends(self) -> &'static [&'static str] {
        match self {
            Self::MacOs => &["auto", "macos", "recording"],
            Self::Windows => &["auto", "windows", "recording"],
            Self::Linux => &["auto", "x11", "portal", "uinput", "recording"],
        }
    }
    pub fn supports_backend(self, backend: &str) -> bool {
        self.backends().contains(&backend)
    }
    pub const fn credential_store(self) -> &'static str {
        match self {
            Self::MacOs => "macOS Keychain",
            Self::Windows => "Windows Credential Manager",
            Self::Linux => "Linux Secret Service",
        }
    }
    pub const fn permission_button(self) -> &'static str {
        match self {
            Self::MacOs => "Check GUI input permission",
            Self::Windows => "Check GUI input availability",
            Self::Linux => "Check GUI input access",
        }
    }
    pub const fn permission_help(self) -> &'static str {
        match self {
            Self::MacOs => {
                "For GUI-owned input, allow C64 Keyboard Bridge in System Settings > Privacy & Security > Device Control and Data Access (Accessibility on older macOS). Permission granted to the CLI does not authorize this app. After changing permission, check it again here."
            }
            Self::Windows => {
                "Windows SendInput cannot post into applications running at a higher integrity level, such as an elevated editor. Use a normal user application. This checks the GUI posting process; a separate CLI session checks its own access."
            }
            Self::Linux => {
                "GNOME Wayland uses desktop-approved keyboard control through the portal. X11 uses XTEST. Sway/Hyprland uinput requires explicitly granted /dev/uinput access. Never run the network application as root. Approval belongs to the running posting process."
            }
        }
    }
}

/// No peer or editable text can supply an external navigation target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalTarget {
    ProjectWebsite,
    MacInputSettings,
}
impl ExternalTarget {
    pub const fn url(self) -> &'static str {
        match self {
            Self::ProjectWebsite => "https://github.com/batuhanoktem/c64-keyboard-bridge",
            Self::MacInputSettings => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
        }
    }
    pub const fn failure(self) -> &'static str {
        match self {
            Self::ProjectWebsite => {
                "The project website could not be opened. Open https://github.com/batuhanoktem/c64-keyboard-bridge in your browser manually."
            }
            Self::MacInputSettings => {
                "Open System Settings > Privacy & Security > Device Control and Data Access (Accessibility on older macOS) manually."
            }
        }
    }
}

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrayCommand {
    Stop,
    Enable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandOutcome {
    Completed,
    Rejected,
    Cancelled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct TrayCommandAck {
    pub sequence: u64,
    pub command: TrayCommand,
    pub outcome: CommandOutcome,
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
    pub configuration_warning: String,
    pub busy: bool,
    pub tray_available: bool,
    pub tray_command_ack: Option<TrayCommandAck>,
    pub test_authorization_pending: bool,
    pub credential_pending: bool,
}
// Password-bearing commands deliberately have no Debug or Serialize implementation.
#[derive(Clone)]
pub enum GuiAction {
    Configured {
        settings: GuiSettings,
        password: String,
        operation: ConnectionOperation,
    },
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
    OpenPermissionSettings,
    OpenProjectWebsite,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionOperation {
    Start { deploy: bool, allow_reset: bool },
    DeviceInfo,
    Deploy { run: bool, allow_reset: bool },
    Probe { allow_reset: bool },
    Doctor,
    RequestPermission,
    TestInput,
}
#[derive(Clone, Debug)]
pub enum GuiEvent {
    Snapshot(Box<GuiSnapshot>),
    SettingsLoaded(GuiSettings),
    FormAccepted(GuiSettings),
    CredentialChanged {
        remember_password: bool,
        password_forgotten: bool,
    },
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

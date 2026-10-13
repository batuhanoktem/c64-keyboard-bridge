//! Native output only: no backend installs keyboard hooks or captures host input.
mod codes;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
mod portal;
#[cfg(target_os = "macos")]
pub use macos::main_thread::{MacOsMainThreadContext, run_with_main_thread_loop};
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "linux")]
mod x11_transport;

use bridge_core::{
    BackendCapabilities, BackendError, KeyboardBackend, KeyboardLayout, RecordingBackend,
};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct BackendProbe {
    pub name: String,
    pub available: bool,
    pub authorized: bool,
    /// None means the active native layout could not be verified read-only.
    pub layout_matches: Option<bool>,
    pub detail: String,
    pub capabilities: BackendCapabilities,
}

impl BackendProbe {
    #[cfg(target_os = "linux")]
    fn error(name: &str, error: BackendError, capabilities: BackendCapabilities) -> Self {
        Self {
            name: name.into(),
            available: matches!(error, BackendError::PermissionDenied(_)),
            authorized: false,
            layout_matches: None,
            detail: error.to_string(),
            capabilities,
        }
    }
}

fn selected_backend(selection: &str) -> Result<&str, BackendError> {
    match selection {
        "auto" => {
            #[cfg(target_os = "macos")]
            {
                Ok("macos")
            }
            #[cfg(target_os = "windows")]
            {
                Ok("windows")
            }
            #[cfg(target_os = "linux")]
            {
                Ok(if portal::is_gnome_wayland() {
                    "portal"
                } else if linux::is_wayland_session() {
                    "uinput"
                } else {
                    "x11"
                })
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
            {
                Err(BackendError::Unsupported(
                    "No native backend supports this operating system".into(),
                ))
            }
        }
        "recording" | "macos" | "windows" | "x11" | "uinput" | "portal" => Ok(selection),
        _ => Err(BackendError::Unsupported(
            "Unknown backend; choose auto, recording, macos, windows, x11, uinput, or portal"
                .into(),
        )),
    }
}

fn wrong_platform() -> BackendError {
    BackendError::Unsupported("The selected backend is unavailable on this operating system".into())
}

/// Whether this selection needs an asynchronous OS authorization session.
/// Read-only: this never requests permission or creates a virtual device.
pub fn requires_authorization(selection: &str) -> Result<bool, BackendError> {
    Ok(selected_backend(selection)? == "portal")
}

/// Prepare native output after deliberate authorization. The caller must keep
/// draining disabled transitions and cancel when a newer Pause/Stop/Quit arrives.
/// A successful result has verified layout and emits no input during preparation.
pub async fn prepare_backend(
    selection: &str,
    layout: KeyboardLayout,
    cancellation: tokio::sync::watch::Receiver<bool>,
) -> Result<Box<dyn KeyboardBackend>, BackendError> {
    if *cancellation.borrow() {
        return Err(BackendError::Unavailable(
            "Input authorization cancelled".into(),
        ));
    }
    if selected_backend(selection)? == "portal" {
        #[cfg(target_os = "linux")]
        {
            return portal::prepare(layout, cancellation).await;
        }
        #[cfg(not(target_os = "linux"))]
        {
            return Err(wrong_platform());
        }
    }
    let selection = selection.to_owned();
    let result = tokio::task::spawn_blocking(move || create_backend(&selection, layout))
        .await
        .map_err(|_| BackendError::Unavailable("Native backend preparation failed".into()))?;
    // Blocking native setup cannot be aborted safely. Its bounded operation must
    // finish, and dropping a cancelled result follows its normal cleanup path.
    if *cancellation.borrow() {
        drop(result);
        Err(BackendError::Unavailable(
            "Input authorization cancelled".into(),
        ))
    } else {
        result
    }
}

/// Construct only when injection is explicitly enabled. Construction never
/// posts a key; uinput creates a virtual device only on this deliberate path.
/// Layout tables in bridge-core choose physical codes; native backends verify
/// the active layout before they post a new press.
pub fn create_backend(
    selection: &str,
    layout: KeyboardLayout,
) -> Result<Box<dyn KeyboardBackend>, BackendError> {
    match selected_backend(selection)? {
        "recording" => Ok(Box::new(RecordingBackend::default())),
        "macos" => {
            #[cfg(target_os = "macos")]
            {
                Ok(Box::new(macos::MacOsBackend::new(layout)?))
            }
            #[cfg(not(target_os = "macos"))]
            {
                Err(wrong_platform())
            }
        }
        "windows" => {
            #[cfg(target_os = "windows")]
            {
                Ok(Box::new(windows::WindowsBackend::new(layout)?))
            }
            #[cfg(not(target_os = "windows"))]
            {
                Err(wrong_platform())
            }
        }
        "x11" => {
            #[cfg(target_os = "linux")]
            {
                Ok(Box::new(linux::X11Backend::new(layout)?))
            }
            #[cfg(not(target_os = "linux"))]
            {
                Err(wrong_platform())
            }
        }
        "uinput" => {
            #[cfg(target_os = "linux")]
            {
                Ok(Box::new(linux::UinputBackend::new(layout)?))
            }
            #[cfg(not(target_os = "linux"))]
            {
                Err(wrong_platform())
            }
        }
        "portal" => Err(BackendError::Unsupported(
            "GNOME desktop permission requires asynchronous preparation in the posting process"
                .into(),
        )),
        _ => Err(wrong_platform()),
    }
}

/// Read-only diagnostic: never prompts, creates a virtual device, or posts input.
pub fn probe_backend(selection: &str, layout: KeyboardLayout) -> BackendProbe {
    let no_capabilities = BackendCapabilities {
        key_events: false,
        text: false,
        repeat: false,
    };
    let selected = match selected_backend(selection) {
        Ok(selected) => selected,
        Err(error) => {
            return BackendProbe {
                name: "invalid".into(),
                available: false,
                authorized: false,
                layout_matches: None,
                detail: error.to_string(),
                capabilities: no_capabilities,
            };
        }
    };
    match selected {
        "recording" => BackendProbe {
            name: "recording".into(),
            available: true,
            authorized: true,
            layout_matches: Some(true),
            detail: "Recording backend available; native input is disabled.".into(),
            capabilities: BackendCapabilities {
                key_events: true,
                text: true,
                repeat: true,
            },
        },
        #[cfg(target_os = "macos")]
        "macos" => macos::probe(layout),
        #[cfg(target_os = "windows")]
        "windows" => windows::probe(layout),
        #[cfg(target_os = "linux")]
        "x11" => linux::probe_x11(layout),
        #[cfg(target_os = "linux")]
        "uinput" => linux::probe_uinput(layout),
        #[cfg(target_os = "linux")]
        "portal" => portal::probe(layout),
        _ => BackendProbe {
            name: selected.into(),
            available: false,
            authorized: false,
            layout_matches: None,
            detail: wrong_platform().to_string(),
            capabilities: no_capabilities,
        },
    }
}

/// Explicit opt-in OS prompt. This is never called by construction or doctor.
pub fn request_permission() -> Result<(), BackendError> {
    #[cfg(target_os = "macos")]
    {
        macos::request_permission()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(BackendError::Unsupported("This platform has no Accessibility permission prompt; consult doctor for the selected backend's requirements".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_core::NativeKey;

    #[test]
    fn recording_factory_and_probe_need_no_native_permissions() {
        let probe = probe_backend("recording", KeyboardLayout::Us);
        assert!(probe.available && probe.authorized);
        let mut backend = create_backend("recording", KeyboardLayout::Us).unwrap();
        backend.press(NativeKey::A).unwrap();
        backend.release(NativeKey::A).unwrap();
    }

    #[test]
    fn invalid_selection_is_rejected_without_native_construction() {
        assert!(create_backend("unknown", KeyboardLayout::Us).is_err());
        assert!(!probe_backend("unknown", KeyboardLayout::Us).available);
    }
}

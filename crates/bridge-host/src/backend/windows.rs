use super::{
    BackendProbe,
    codes::{is_modifier, pc_scancode, release_order},
};
use bridge_core::{BackendCapabilities, BackendError, KeyboardBackend, KeyboardLayout, NativeKey};
use windows_sys::Win32::{
    Foundation::{GetLastError, SetLastError},
    UI::{
        Input::KeyboardAndMouse::{
            GetKeyboardLayout, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
            KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, KEYEVENTF_UNICODE, SendInput,
        },
        WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
    },
};

const MARKER: usize = 0x6436_3442;

pub(super) fn capabilities() -> BackendCapabilities {
    BackendCapabilities {
        key_events: true,
        text: true,
        repeat: true,
    }
}

fn layout_from_hkl(hkl: usize) -> Option<KeyboardLayout> {
    // Exact standard layout handles, not just the locale's low word. Dvorak,
    // international/dead-key variants and IMEs can share a language identifier.
    match hkl as u32 {
        0x0409_0409 => Some(KeyboardLayout::Us),
        0x0809_0809 => Some(KeyboardLayout::Uk),
        _ => None,
    }
}

fn active_layout() -> Result<KeyboardLayout, BackendError> {
    let window = unsafe { GetForegroundWindow() };
    if window.is_null() {
        return Err(BackendError::Unavailable(
            "No foreground application has a verifiable keyboard layout".into(),
        ));
    }
    let thread = unsafe { GetWindowThreadProcessId(window, std::ptr::null_mut()) };
    if thread == 0 {
        return Err(BackendError::Unavailable(
            "Cannot read the foreground application's keyboard thread".into(),
        ));
    }
    let hkl = unsafe { GetKeyboardLayout(thread) } as usize;
    layout_from_hkl(hkl).ok_or_else(|| BackendError::Unsupported(format!(
        "Unverified foreground keyboard layout 0x{:08x}; select standard US or United Kingdom (variants and IMEs are unsupported)", hkl as u32
    )))
}

fn require_layout(expected: KeyboardLayout) -> Result<(), BackendError> {
    let current = active_layout()?;
    if current != expected {
        return Err(BackendError::Unsupported(format!(
            "Foreground keyboard layout is {current:?}, but the bridge requires {expected:?}"
        )));
    }
    Ok(())
}

pub(super) fn probe(layout: KeyboardLayout) -> BackendProbe {
    let layout_matches = active_layout().ok().map(|current| current == layout);
    let layout_detail = match require_layout(layout) {
        Ok(()) => "Foreground keyboard layout matches.".into(),
        Err(error) => error.to_string(),
    };
    BackendProbe {
        name: "windows".into(),
        available: true,
        authorized: true,
        layout_matches,
        detail: format!(
            "SendInput is available. {layout_detail} Injection is limited to equal/lower integrity applications; availability does not verify focused-application delivery. Host keyboard modifiers may interact with synthetic input."
        ),
        capabilities: capabilities(),
    }
}

fn send_one(scan: u16, flags: u32) -> Result<(), BackendError> {
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: MARKER,
            },
        },
    };
    // Each call sends exactly one event: a partial array result cannot obscure
    // which key is held. The caller records it only after a successful result.
    let sent = unsafe {
        SetLastError(0);
        SendInput(1, &input, std::mem::size_of::<INPUT>() as i32)
    };
    if sent != 1 {
        let error = unsafe { GetLastError() };
        return Err(BackendError::Injection(format!(
            "SendInput accepted {sent}/1 events (Win32 error {error}). UIPI may block elevated targets without identifying the cause."
        )));
    }
    Ok(())
}

fn send_key(key: NativeKey, down: bool, repeating: bool) -> Result<(), BackendError> {
    let (scan, extended) = pc_scancode(key);
    let flags = KEYEVENTF_SCANCODE
        | if extended { KEYEVENTF_EXTENDEDKEY } else { 0 }
        | if down { 0 } else { KEYEVENTF_KEYUP };
    send_one(scan, flags)?;
    // Acceptance by SendInput does not verify delivery to a particular app.
    tracing::debug!(
        key = ?key,
        pressed = down,
        repeat = repeating,
        "Windows SendInput accepted native key event"
    );
    Ok(())
}

fn release_pending_unicode(
    pending: &mut Option<u16>,
    mut send: impl FnMut(u16, u32) -> Result<(), BackendError>,
) -> Result<(), BackendError> {
    if let Some(unit) = *pending {
        send(unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP)?;
        *pending = None;
    }
    Ok(())
}

pub(super) struct WindowsBackend {
    held: Vec<NativeKey>,
    pending_unicode: Option<u16>,
    layout: KeyboardLayout,
}

impl WindowsBackend {
    pub(super) fn new(layout: KeyboardLayout) -> Result<Self, BackendError> {
        require_layout(layout)?;
        Ok(Self {
            held: Vec::new(),
            pending_unicode: None,
            layout,
        })
    }
}

impl KeyboardBackend for WindowsBackend {
    fn capabilities(&self) -> BackendCapabilities {
        capabilities()
    }

    fn validate_layout(&mut self) -> Result<(), BackendError> {
        require_layout(self.layout)
    }

    fn cleanup_pending_text(&mut self) -> Result<(), BackendError> {
        release_pending_unicode(&mut self.pending_unicode, send_one)
    }

    fn press(&mut self, key: NativeKey) -> Result<(), BackendError> {
        self.cleanup_pending_text()?;
        if self.held.contains(&key) {
            return Ok(());
        }
        require_layout(self.layout)?;
        send_key(key, true, false)?;
        self.held.push(key);
        Ok(())
    }

    fn release(&mut self, key: NativeKey) -> Result<(), BackendError> {
        if !self.held.contains(&key) {
            return Ok(());
        }
        send_key(key, false, false)?;
        self.held.retain(|held| *held != key);
        Ok(())
    }

    fn repeat(&mut self, key: NativeKey) -> Result<(), BackendError> {
        if !self.held.contains(&key) || is_modifier(key) {
            return Err(BackendError::Unsupported(
                "Only a held ordinary key can repeat".into(),
            ));
        }
        require_layout(self.layout)?;
        send_key(key, true, true)
    }

    fn type_text(&mut self, text: &str) -> Result<(), BackendError> {
        if !self.held.is_empty() {
            return Err(BackendError::Unsupported(
                "Text insertion requires all native keys and modifiers released".into(),
            ));
        }
        if text.contains('\0') || text.len() > 4096 {
            return Err(BackendError::Unsupported(
                "Text insertion rejects NUL and input exceeding 4096 bytes".into(),
            ));
        }
        self.cleanup_pending_text()?;
        for unit in text.encode_utf16() {
            require_layout(self.layout)?;
            send_one(unit, KEYEVENTF_UNICODE)?;
            self.pending_unicode = Some(unit);
            self.cleanup_pending_text()?;
        }
        Ok(())
    }
}

impl Drop for WindowsBackend {
    fn drop(&mut self) {
        let _ = self.cleanup_pending_text();
        for key in release_order(&self.held) {
            let _ = self.release(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_unicode_release_retries_exact_unit_and_retains_failure() {
        let mut pending = Some(0xd83d);
        let mut attempts = Vec::new();
        let result = release_pending_unicode(&mut pending, |unit, flags| {
            attempts.push((unit, flags));
            Err(BackendError::Injection("simulated key-up failure".into()))
        });
        assert!(result.is_err());
        assert_eq!(pending, Some(0xd83d));
        release_pending_unicode(&mut pending, |unit, flags| {
            attempts.push((unit, flags));
            Ok(())
        })
        .unwrap();
        assert_eq!(pending, None);
        release_pending_unicode(&mut pending, |_, _| {
            panic!("released text cannot repeat cleanup")
        })
        .unwrap();
        assert_eq!(attempts, [(0xd83d, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP); 2]);
    }

    #[test]
    fn standard_layout_policy_rejects_shared_locale_variants() {
        assert_eq!(layout_from_hkl(0x0409_0409), Some(KeyboardLayout::Us));
        assert_eq!(layout_from_hkl(0x0809_0809), Some(KeyboardLayout::Uk));
        assert_eq!(layout_from_hkl(0xf002_0409), None);
        assert_eq!(layout_from_hkl(0x0452_0809), None);
        assert_eq!(layout_from_hkl(0), None);
    }
}

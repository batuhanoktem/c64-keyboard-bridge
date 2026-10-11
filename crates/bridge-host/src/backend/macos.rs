use super::{
    BackendProbe,
    codes::{is_modifier, mac_keycode, release_order},
};
use bridge_core::{BackendCapabilities, BackendError, KeyboardBackend, KeyboardLayout, NativeKey};
use core_foundation::{
    base::{CFType, CFTypeRef, TCFType},
    string::{CFString, CFStringRef},
};
use core_graphics::{
    event::{CGEvent, CGEventFlags, CGEventTapLocation, EventField},
    event_source::{CGEventSource, CGEventSourceStateID},
};

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
}
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn TISCopyCurrentKeyboardInputSource() -> CFTypeRef;
    fn TISGetInputSourceProperty(source: CFTypeRef, property: CFStringRef) -> CFTypeRef;
    static kTISPropertyInputSourceID: CFStringRef;
}
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightPostEventAccess() -> bool;
    fn CGRequestPostEventAccess() -> bool;
}

const PERMISSION_HELP: &str = "Keyboard injection requires Accessibility permission. Open System Settings > Privacy & Security > Accessibility and authorize the executable or terminal reported by macOS, then run doctor again. No Input Monitoring permission or keyboard capture is used.";

pub(super) fn capabilities() -> BackendCapabilities {
    BackendCapabilities {
        key_events: true,
        text: true,
        repeat: true,
    }
}

pub(super) fn authorized() -> bool {
    // These OS preflight functions do not prompt or listen to keyboard events.
    unsafe { AXIsProcessTrusted() != 0 && CGPreflightPostEventAccess() }
}

fn layout_from_id(identifier: &str) -> Option<KeyboardLayout> {
    match identifier {
        "com.apple.keylayout.US" | "com.apple.keylayout.ABC" => Some(KeyboardLayout::Us),
        "com.apple.keylayout.British" => Some(KeyboardLayout::Uk),
        _ => None,
    }
}

fn active_layout() -> Result<KeyboardLayout, BackendError> {
    // TIS reads the selected input source. It does not install a tap or query
    // host key presses. Reject IMEs, dead-key variants, and unknown layouts.
    let source = unsafe { TISCopyCurrentKeyboardInputSource() };
    if source.is_null() {
        return Err(BackendError::Unavailable(
            "Cannot read the macOS input source".into(),
        ));
    }
    let source = unsafe { CFType::wrap_under_create_rule(source) };
    let property = unsafe {
        TISGetInputSourceProperty(source.as_concrete_TypeRef(), kTISPropertyInputSourceID)
    };
    if property.is_null() {
        return Err(BackendError::Unavailable(
            "macOS input source has no identifier".into(),
        ));
    }
    let identifier = unsafe { CFString::wrap_under_get_rule(property.cast()) }.to_string();
    layout_from_id(&identifier).ok_or_else(|| {
        BackendError::Unsupported(format!(
            "Unverified macOS input source {identifier}; select U.S./ABC for US or British for UK"
        ))
    })
}

fn require_layout(expected: KeyboardLayout) -> Result<(), BackendError> {
    let current = active_layout()?;
    if current != expected {
        return Err(BackendError::Unsupported(format!(
            "Active macOS layout is {current:?}, but the bridge requires {expected:?}; select the matching input source"
        )));
    }
    Ok(())
}

pub(super) fn probe(layout: KeyboardLayout) -> BackendProbe {
    let allowed = authorized();
    let layout_matches = active_layout().ok().map(|current| current == layout);
    let layout_detail = match require_layout(layout) {
        Ok(()) => "Active keyboard layout matches.".into(),
        Err(error) => error.to_string(),
    };
    BackendProbe {
        name: "macos".into(),
        available: true,
        authorized: allowed,
        layout_matches,
        detail: if allowed {
            format!(
                "CoreGraphics posting is authorized. {layout_detail} Delivery to a focused application requires an explicit input test."
            )
        } else {
            format!("{PERMISSION_HELP} {layout_detail}")
        },
        capabilities: capabilities(),
    }
}

pub(super) fn request_permission() -> Result<(), BackendError> {
    // Only the explicit CLI permission action calls the prompting API.
    unsafe {
        CGRequestPostEventAccess();
    }
    if authorized() {
        Ok(())
    } else {
        Err(BackendError::PermissionDenied(PERMISSION_HELP.into()))
    }
}

pub(super) struct MacOsBackend {
    held: Vec<NativeKey>,
    pending_text: Option<char>,
    layout: KeyboardLayout,
}

fn release_pending_text(
    pending: &mut Option<char>,
    mut release: impl FnMut(char) -> Result<(), BackendError>,
) -> Result<(), BackendError> {
    if let Some(character) = *pending {
        release(character)?;
        *pending = None;
    }
    Ok(())
}

impl MacOsBackend {
    pub(super) fn new(layout: KeyboardLayout) -> Result<Self, BackendError> {
        if !authorized() {
            return Err(BackendError::PermissionDenied(PERMISSION_HELP.into()));
        }
        require_layout(layout)?;
        Ok(Self {
            held: Vec::new(),
            pending_text: None,
            layout,
        })
    }

    fn flags_for(held: &[NativeKey]) -> CGEventFlags {
        let mut flags = CGEventFlags::empty();
        if held
            .iter()
            .any(|k| matches!(k, NativeKey::LeftShift | NativeKey::RightShift))
        {
            flags |= CGEventFlags::CGEventFlagShift;
        }
        if held.contains(&NativeKey::Control) {
            flags |= CGEventFlags::CGEventFlagControl;
        }
        if held.contains(&NativeKey::Alt) {
            flags |= CGEventFlags::CGEventFlagAlternate;
        }
        if held.contains(&NativeKey::Meta) {
            flags |= CGEventFlags::CGEventFlagCommand;
        }
        flags
    }

    fn post(
        &self,
        key: NativeKey,
        down: bool,
        repeating: bool,
        held: &[NativeKey],
        text: Option<&str>,
    ) -> Result<(), BackendError> {
        if !authorized() {
            return Err(BackendError::PermissionDenied(PERMISSION_HELP.into()));
        }
        if down {
            require_layout(self.layout)?;
        }
        // No CoreGraphics pointer crosses a thread boundary. Explicit flags carry
        // our own modifier state instead of consulting the host keyboard state.
        let source = CGEventSource::new(CGEventSourceStateID::Private).map_err(|()| {
            BackendError::Injection("CoreGraphics event source allocation failed".into())
        })?;
        let event = CGEvent::new_keyboard_event(source, mac_keycode(key), down).map_err(|()| {
            BackendError::Injection("CoreGraphics keyboard event allocation failed".into())
        })?;
        event.set_flags(Self::flags_for(held));
        event.set_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT, i64::from(repeating));
        event.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, 0x6436_3442);
        if let Some(text) = text {
            event.set_string(text);
        }
        event.post(CGEventTapLocation::HID);
        Ok(())
    }
}

impl KeyboardBackend for MacOsBackend {
    fn capabilities(&self) -> BackendCapabilities {
        capabilities()
    }

    fn validate_layout(&mut self) -> Result<(), BackendError> {
        require_layout(self.layout)
    }

    fn cleanup_pending_text(&mut self) -> Result<(), BackendError> {
        let mut pending = self.pending_text;
        let result = release_pending_text(&mut pending, |character| {
            let mut utf8 = [0; 4];
            let piece = character.encode_utf8(&mut utf8);
            self.post(NativeKey::A, false, false, &[], Some(piece))
        });
        self.pending_text = pending;
        result
    }

    fn press(&mut self, key: NativeKey) -> Result<(), BackendError> {
        self.cleanup_pending_text()?;
        if self.held.contains(&key) {
            return Ok(());
        }
        let mut next = self.held.clone();
        next.push(key);
        self.post(key, true, false, &next, None)?;
        self.held = next;
        Ok(())
    }

    fn release(&mut self, key: NativeKey) -> Result<(), BackendError> {
        if !self.held.contains(&key) {
            return Ok(());
        }
        let mut next = self.held.clone();
        next.retain(|k| *k != key);
        self.post(key, false, false, &next, None)?;
        self.held = next;
        Ok(())
    }

    fn repeat(&mut self, key: NativeKey) -> Result<(), BackendError> {
        if !self.held.contains(&key) || is_modifier(key) {
            return Err(BackendError::Unsupported(
                "Only a held ordinary key can repeat".into(),
            ));
        }
        self.post(key, true, true, &self.held, None)
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
        for character in text.chars() {
            let mut utf8 = [0; 4];
            let piece = character.encode_utf8(&mut utf8);
            self.post(NativeKey::A, true, false, &[], Some(piece))?;
            self.pending_text = Some(character);
            self.cleanup_pending_text()?;
        }
        Ok(())
    }
}

impl Drop for MacOsBackend {
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
    fn temporary_text_release_retries_exact_character_and_retains_failure() {
        let mut pending = Some('\u{1f600}');
        let mut attempts = Vec::new();
        let result = release_pending_text(&mut pending, |character| {
            attempts.push(character);
            Err(BackendError::Injection("simulated key-up failure".into()))
        });
        assert!(result.is_err());
        assert_eq!(pending, Some('\u{1f600}'));
        release_pending_text(&mut pending, |character| {
            attempts.push(character);
            Ok(())
        })
        .unwrap();
        assert_eq!(pending, None);
        release_pending_text(&mut pending, |_| {
            panic!("released text cannot repeat cleanup")
        })
        .unwrap();
        assert_eq!(attempts, ['\u{1f600}', '\u{1f600}']);
    }

    #[test]
    fn flags_follow_only_bridge_owned_modifiers() {
        let flags =
            MacOsBackend::flags_for(&[NativeKey::A, NativeKey::RightShift, NativeKey::Meta]);
        assert!(flags.contains(CGEventFlags::CGEventFlagShift));
        assert!(flags.contains(CGEventFlags::CGEventFlagCommand));
        assert!(!flags.contains(CGEventFlags::CGEventFlagControl));
        assert_eq!(MacOsBackend::flags_for(&[]), CGEventFlags::empty());
    }

    #[test]
    fn input_source_policy_rejects_layout_variants_and_input_methods() {
        assert_eq!(
            layout_from_id("com.apple.keylayout.US"),
            Some(KeyboardLayout::Us)
        );
        assert_eq!(
            layout_from_id("com.apple.keylayout.ABC"),
            Some(KeyboardLayout::Us)
        );
        assert_eq!(
            layout_from_id("com.apple.keylayout.British"),
            Some(KeyboardLayout::Uk)
        );
        assert_eq!(layout_from_id("com.apple.keylayout.British-PC"), None);
        assert_eq!(
            layout_from_id("com.apple.keylayout.USInternational-PC"),
            None
        );
        assert_eq!(
            layout_from_id("com.apple.inputmethod.Kotoeri.Japanese"),
            None
        );
    }
}

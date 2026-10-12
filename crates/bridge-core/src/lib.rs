//! Physical keyboard mapping and ownership of injected native keys.
use bridge_protocol::{
    ALL_KEYS, KeyEvent, MOD_COMMODORE, MOD_CONTROL, MOD_SHIFT, MatrixState, PhysicalKey,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum NativeKey {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    Space,
    Enter,
    Backspace,
    Delete,
    Escape,
    Tab,
    Home,
    End,
    PageUp,
    PageDown,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    LeftShift,
    RightShift,
    Control,
    Alt,
    Meta,
    Minus,
    Equal,
    LeftBracket,
    RightBracket,
    Backslash,
    Semicolon,
    Quote,
    Comma,
    Period,
    Slash,
    Grave,
    IntlBackslash,
}
impl NativeKey {
    /// Native chord choices accepted by the shared mapping validator.
    pub const ALL: &'static [Self] = &[
        Self::A,
        Self::B,
        Self::C,
        Self::D,
        Self::E,
        Self::F,
        Self::G,
        Self::H,
        Self::I,
        Self::J,
        Self::K,
        Self::L,
        Self::M,
        Self::N,
        Self::O,
        Self::P,
        Self::Q,
        Self::R,
        Self::S,
        Self::T,
        Self::U,
        Self::V,
        Self::W,
        Self::X,
        Self::Y,
        Self::Z,
        Self::Num0,
        Self::Num1,
        Self::Num2,
        Self::Num3,
        Self::Num4,
        Self::Num5,
        Self::Num6,
        Self::Num7,
        Self::Num8,
        Self::Num9,
        Self::Space,
        Self::Enter,
        Self::Backspace,
        Self::Delete,
        Self::Escape,
        Self::Tab,
        Self::Home,
        Self::End,
        Self::PageUp,
        Self::PageDown,
        Self::ArrowLeft,
        Self::ArrowRight,
        Self::ArrowUp,
        Self::ArrowDown,
        Self::F1,
        Self::F2,
        Self::F3,
        Self::F4,
        Self::F5,
        Self::F6,
        Self::F7,
        Self::F8,
        Self::F9,
        Self::F10,
        Self::F11,
        Self::F12,
        Self::LeftShift,
        Self::RightShift,
        Self::Control,
        Self::Alt,
        Self::Meta,
        Self::Minus,
        Self::Equal,
        Self::LeftBracket,
        Self::RightBracket,
        Self::Backslash,
        Self::Semicolon,
        Self::Quote,
        Self::Comma,
        Self::Period,
        Self::Slash,
        Self::Grave,
        Self::IntlBackslash,
    ];
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|key| format!("{key:?}").eq_ignore_ascii_case(name))
    }

    pub const fn is_modifier(self) -> bool {
        matches!(
            self,
            Self::LeftShift | Self::RightShift | Self::Control | Self::Alt | Self::Meta
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyboardLayout {
    Us,
    Uk,
}
impl FromStr for KeyboardLayout {
    type Err = MappingError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "us" => Ok(Self::Us),
            "uk" => Ok(Self::Uk),
            _ => Err(MappingError::InvalidLayout),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostPlatform {
    MacOS,
    Windows,
    Linux,
}
impl HostPlatform {
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOS
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendCapabilities {
    pub key_events: bool,
    pub text: bool,
    pub repeat: bool,
}
#[derive(Debug, Error)]
pub enum BackendError {
    #[error("keyboard permission denied: {0}")]
    PermissionDenied(String),
    #[error("keyboard backend unavailable: {0}")]
    Unavailable(String),
    #[error("keyboard operation unsupported: {0}")]
    Unsupported(String),
    #[error("keyboard injection failed: {0}")]
    Injection(String),
}
pub trait KeyboardBackend: Send {
    fn capabilities(&self) -> BackendCapabilities;
    /// Check the active host layout without posting input. Cleanup releases must
    /// remain possible even if this validation fails.
    fn validate_layout(&mut self) -> Result<(), BackendError> {
        Ok(())
    }
    fn press(&mut self, key: NativeKey) -> Result<(), BackendError>;
    fn release(&mut self, key: NativeKey) -> Result<(), BackendError>;
    /// Posts another key-down without changing ownership or releasing the key.
    fn repeat(&mut self, key: NativeKey) -> Result<(), BackendError>;
    fn type_text(&mut self, _text: &str) -> Result<(), BackendError> {
        Err(BackendError::Unsupported("text input".into()))
    }
    /// Retry release of a temporary key owned by a partially failed text action.
    /// Keep that ownership until release succeeds; this must bypass layout checks.
    fn cleanup_pending_text(&mut self) -> Result<(), BackendError> {
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordingEvent {
    Press(NativeKey),
    Release(NativeKey),
    Repeat(NativeKey),
    Text(String),
}
#[derive(Debug, Default)]
pub struct RecordingBackend {
    pub events: Vec<RecordingEvent>,
}
impl KeyboardBackend for RecordingBackend {
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            key_events: true,
            text: true,
            repeat: true,
        }
    }
    fn press(&mut self, key: NativeKey) -> Result<(), BackendError> {
        self.events.push(RecordingEvent::Press(key));
        Ok(())
    }
    fn release(&mut self, key: NativeKey) -> Result<(), BackendError> {
        self.events.push(RecordingEvent::Release(key));
        Ok(())
    }
    fn repeat(&mut self, key: NativeKey) -> Result<(), BackendError> {
        self.events.push(RecordingEvent::Repeat(key));
        Ok(())
    }
    fn type_text(&mut self, text: &str) -> Result<(), BackendError> {
        self.events.push(RecordingEvent::Text(text.into()));
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShiftPolicy {
    Physical,
    Required,
    Suppress,
}
#[derive(Debug, Clone)]
enum Action {
    Keys {
        keys: Vec<NativeKey>,
        shift: ShiftPolicy,
    },
    Text(String),
}
#[derive(Debug, Clone)]
struct Binding {
    key: PhysicalKey,
    modifiers: u8,
    action: Action,
}
#[derive(Debug, Clone)]
pub struct MappingProfile {
    pub name: String,
    pub layout: KeyboardLayout,
    authentic: bool,
    bindings: Vec<Binding>,
    commodore_modifier: Option<NativeKey>,
    control_modifier: NativeKey,
}
/// Editable representation of the existing mapping TOML schema. Construction
/// does not validate a draft; call `validate` before replacing an active profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MappingDocument {
    pub name: String,
    pub profile: String,
    pub layout: String,
    #[serde(default)]
    pub bindings: Vec<BindingDocument>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commodore_modifier: Option<NativeKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control_modifier: Option<NativeKey>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingDocument {
    pub key: PhysicalKey,
    #[serde(default)]
    pub modifiers: Vec<SourceModifier>,
    #[serde(default)]
    pub keys: Vec<NativeKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shift: Option<ShiftPolicy>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceModifier {
    Shift,
    Control,
    Commodore,
}
impl SourceModifier {
    pub const fn mask(self) -> u8 {
        match self {
            Self::Shift => MOD_SHIFT,
            Self::Control => MOD_CONTROL,
            Self::Commodore => MOD_COMMODORE,
        }
    }
}
/// Read-only mapping result. A preview never owns or injects native input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolvedAction {
    Keys {
        keys: Vec<NativeKey>,
        shift: ShiftPolicy,
    },
    Text(String),
    Modifier(NativeKey),
}
impl MappingDocument {
    pub fn builtin(profile: &str, layout: &str) -> Result<Self, MappingError> {
        let parsed = MappingProfile::builtin(profile, layout)?;
        Ok(Self {
            name: parsed.name,
            profile: profile.into(),
            layout: if parsed.layout == KeyboardLayout::Us {
                "US"
            } else {
                "UK"
            }
            .into(),
            bindings: Vec::new(),
            commodore_modifier: None,
            control_modifier: None,
        })
    }
    pub fn from_toml(text: &str) -> Result<Self, MappingError> {
        toml::from_str(text).map_err(|error| MappingError::Configuration(error.to_string()))
    }
    pub fn to_toml(&self) -> Result<String, MappingError> {
        toml::to_string_pretty(self).map_err(|error| MappingError::Configuration(error.to_string()))
    }
    pub fn validate(&self) -> Result<MappingProfile, MappingError> {
        MappingProfile::from_document(self)
    }
}
impl MappingProfile {
    pub fn builtin(profile: &str, layout: &str) -> Result<Self, MappingError> {
        let layout = layout.parse()?;
        let authentic = match profile {
            "modern" => false,
            "authentic" | "authentic-c64" => true,
            _ => return Err(MappingError::InvalidProfile),
        };
        Ok(Self {
            name: format!(
                "{profile}-{}",
                if layout == KeyboardLayout::Us {
                    "us"
                } else {
                    "uk"
                }
            ),
            layout,
            authentic,
            bindings: Vec::new(),
            commodore_modifier: None,
            control_modifier: NativeKey::Control,
        })
    }
    pub fn from_toml(text: &str) -> Result<Self, MappingError> {
        Self::from_document(&MappingDocument::from_toml(text)?)
    }
    pub fn from_document(document: &MappingDocument) -> Result<Self, MappingError> {
        let file = document.clone();
        let mut result = Self::builtin(&file.profile, &file.layout)?;
        if file.name.is_empty() || file.name.len() > 80 || file.bindings.len() > 256 {
            return Err(MappingError::Configuration(
                "profile name or binding count exceeds bounds".into(),
            ));
        }
        result.name = file.name;
        for key in [file.commodore_modifier, file.control_modifier]
            .into_iter()
            .flatten()
        {
            if !matches!(key, NativeKey::Control | NativeKey::Alt | NativeKey::Meta) {
                return Err(MappingError::Configuration(
                    "modifier role must be Control, Alt or Meta".into(),
                ));
            }
        }
        result.commodore_modifier = file.commodore_modifier;
        result.control_modifier = file.control_modifier.unwrap_or(NativeKey::Control);
        let mut seen = BTreeSet::new();
        for binding in file.bindings {
            if binding.key.is_modifier() {
                return Err(MappingError::Configuration(
                    "bind ordinary keys; modifier roles are provided by the profile".into(),
                ));
            }
            let mut modifiers = 0;
            for modifier in binding.modifiers {
                let mask = match modifier {
                    SourceModifier::Shift => MOD_SHIFT,
                    SourceModifier::Control => MOD_CONTROL,
                    SourceModifier::Commodore => MOD_COMMODORE,
                };
                if modifiers & mask != 0 {
                    return Err(MappingError::Configuration(
                        "duplicate source modifier".into(),
                    ));
                }
                modifiers |= mask;
            }
            if !seen.insert((binding.key, modifiers)) {
                return Err(MappingError::Configuration("duplicate binding".into()));
            }
            let action = match binding.text {
                Some(text)
                    if binding.keys.is_empty()
                        && !text.is_empty()
                        && text.chars().count() <= 16
                        && !text.chars().any(char::is_control) =>
                {
                    Action::Text(text)
                }
                None if !binding.keys.is_empty() && binding.keys.len() <= 8 => {
                    if binding.keys.iter().copied().collect::<BTreeSet<_>>().len()
                        != binding.keys.len()
                    {
                        return Err(MappingError::Configuration("duplicate output key".into()));
                    }
                    let has_shift = binding
                        .keys
                        .iter()
                        .any(|key| matches!(key, NativeKey::LeftShift | NativeKey::RightShift));
                    if has_shift && binding.shift == Some(ShiftPolicy::Suppress) {
                        return Err(MappingError::Configuration(
                            "a Shift chord cannot suppress Shift".into(),
                        ));
                    }
                    Action::Keys {
                        keys: binding.keys,
                        shift: if has_shift {
                            ShiftPolicy::Required
                        } else {
                            binding.shift.unwrap_or(ShiftPolicy::Physical)
                        },
                    }
                }
                _ => {
                    return Err(MappingError::Configuration(
                        "binding requires either 1–8 native keys or up to 16 text characters"
                            .into(),
                    ));
                }
            };
            result.bindings.push(Binding {
                key: binding.key,
                modifiers,
                action,
            });
        }
        // Equal-specificity rules which could both match are ambiguous.
        for (index, left) in result.bindings.iter().enumerate() {
            for right in &result.bindings[index + 1..] {
                if left.key == right.key
                    && left.modifiers.count_ones() == right.modifiers.count_ones()
                {
                    return Err(MappingError::Configuration(
                        "equally specific bindings can match the same chord".into(),
                    ));
                }
            }
        }
        Ok(result)
    }
    pub fn preview(
        &self,
        key: PhysicalKey,
        modifiers: u8,
        platform: HostPlatform,
    ) -> Result<ResolvedAction, MappingError> {
        if modifiers & !bridge_protocol::MODIFIER_MASK != 0 {
            return Err(MappingError::Configuration(
                "invalid source modifier mask".into(),
            ));
        }
        if key.is_modifier() {
            let native = match key {
                PhysicalKey::LeftShift => NativeKey::LeftShift,
                PhysicalKey::RightShift => NativeKey::RightShift,
                PhysicalKey::Control => self.control_modifier,
                PhysicalKey::Commodore => {
                    self.commodore_modifier
                        .unwrap_or(if platform == HostPlatform::MacOS {
                            NativeKey::Meta
                        } else {
                            NativeKey::Control
                        })
                }
                _ => unreachable!("modifier checked above"),
            };
            return Ok(ResolvedAction::Modifier(native));
        }
        Ok(match self.action(key, modifiers, platform) {
            Action::Keys { keys, shift } => ResolvedAction::Keys { keys, shift },
            Action::Text(text) => ResolvedAction::Text(text),
        })
    }
    fn action(&self, key: PhysicalKey, modifiers: u8, platform: HostPlatform) -> Action {
        if let Some(binding) = self
            .bindings
            .iter()
            .filter(|b| b.key == key && modifiers & b.modifiers == b.modifiers)
            .max_by_key(|b| b.modifiers.count_ones())
        {
            return binding.action.clone();
        }
        use NativeKey as N;
        use PhysicalKey as P;
        let shifted = modifiers & MOD_SHIFT != 0;
        let physical = |key| Action::Keys {
            keys: vec![key],
            shift: ShiftPolicy::Physical,
        };
        let fixed = |key, shift| Action::Keys {
            keys: vec![key],
            shift: if shift {
                ShiftPolicy::Required
            } else {
                ShiftPolicy::Suppress
            },
        };
        match key {
            P::A => physical(N::A),
            P::B => physical(N::B),
            P::C => physical(N::C),
            P::D => physical(N::D),
            P::E => physical(N::E),
            P::F => physical(N::F),
            P::G => physical(N::G),
            P::H => physical(N::H),
            P::I => physical(N::I),
            P::J => physical(N::J),
            P::K => physical(N::K),
            P::L => physical(N::L),
            P::M => physical(N::M),
            P::N => physical(N::N),
            P::O => physical(N::O),
            P::P => physical(N::P),
            P::Q => physical(N::Q),
            P::R => physical(N::R),
            P::S => physical(N::S),
            P::T => physical(N::T),
            P::U => physical(N::U),
            P::V => physical(N::V),
            P::W => physical(N::W),
            P::X => physical(N::X),
            P::Y => physical(N::Y),
            P::Z => physical(N::Z),
            P::Num0 => {
                if self.authentic && shifted {
                    fixed(N::Num0, false)
                } else {
                    physical(N::Num0)
                }
            }
            P::Num1 => physical(N::Num1),
            P::Num2 if self.authentic && shifted => fixed(
                if self.layout == KeyboardLayout::Us || platform == HostPlatform::MacOS {
                    N::Quote
                } else {
                    N::Num2
                },
                true,
            ),
            P::Num2 => physical(N::Num2),
            P::Num3 if self.authentic && shifted && self.layout == KeyboardLayout::Uk => {
                Action::Keys {
                    keys: if platform == HostPlatform::MacOS {
                        vec![N::Alt, N::Num3]
                    } else {
                        vec![N::Backslash]
                    },
                    shift: ShiftPolicy::Suppress,
                }
            }
            P::Num3 => physical(N::Num3),
            P::Num4 => physical(N::Num4),
            P::Num5 => physical(N::Num5),
            P::Num6 if self.authentic && shifted => fixed(N::Num7, true),
            P::Num6 => physical(N::Num6),
            P::Num7 if self.authentic && shifted => fixed(N::Quote, false),
            P::Num7 => physical(N::Num7),
            P::Num8 if self.authentic && shifted => fixed(N::Num9, true),
            P::Num8 => physical(N::Num8),
            P::Num9 if self.authentic && shifted => fixed(N::Num0, true),
            P::Num9 => physical(N::Num9),
            P::Space => physical(N::Space),
            P::Return => physical(N::Enter),
            P::Delete if shifted => fixed(N::Delete, false),
            P::Delete => physical(N::Backspace),
            P::RunStop => physical(N::Escape),
            P::CursorRight => fixed(if shifted { N::ArrowLeft } else { N::ArrowRight }, false),
            P::CursorDown => fixed(if shifted { N::ArrowUp } else { N::ArrowDown }, false),
            P::F1 => fixed(if shifted { N::F2 } else { N::F1 }, false),
            P::F3 => fixed(if shifted { N::F4 } else { N::F3 }, false),
            P::F5 => fixed(if shifted { N::F6 } else { N::F5 }, false),
            P::F7 => fixed(if shifted { N::F8 } else { N::F7 }, false),
            P::Home => fixed(N::Home, false),
            P::Plus => fixed(N::Equal, true),
            P::Minus => physical(N::Minus),
            P::Equal => fixed(N::Equal, false),
            P::Colon if shifted => fixed(N::LeftBracket, false),
            P::Colon => fixed(N::Semicolon, true),
            P::Semicolon if shifted => fixed(N::RightBracket, false),
            P::Semicolon => fixed(N::Semicolon, false),
            P::At => fixed(
                if self.layout == KeyboardLayout::Us || platform == HostPlatform::MacOS {
                    N::Num2
                } else {
                    N::Quote
                },
                true,
            ),
            P::Asterisk => fixed(N::Num8, true),
            P::Pound if self.layout == KeyboardLayout::Uk => fixed(N::Num3, true),
            P::Pound => Action::Text("£".into()),
            P::UpArrow if self.authentic => Action::Text("↑".into()),
            P::UpArrow => fixed(N::Num6, true),
            P::LeftArrow if self.authentic => Action::Text("←".into()),
            P::LeftArrow => physical(N::Grave),
            P::Comma => physical(N::Comma),
            P::Period => physical(N::Period),
            P::Slash => physical(N::Slash),
            P::LeftShift | P::RightShift | P::Control | P::Commodore => {
                unreachable!("modifiers handled separately")
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Active {
    action: Action,
    order: u64,
}
pub struct Mapper {
    profile: MappingProfile,
    platform: HostPlatform,
    physical: MatrixState,
    active: BTreeMap<PhysicalKey, Active>,
    pressed: BTreeSet<NativeKey>,
    quarantined: bool,
    order: u64,
    blocked: u64,
}
impl Mapper {
    pub fn new(profile: MappingProfile, platform: HostPlatform) -> Self {
        Self {
            profile,
            platform,
            physical: MatrixState::default(),
            active: BTreeMap::new(),
            pressed: BTreeSet::new(),
            quarantined: false,
            order: 0,
            blocked: 0,
        }
    }
    pub fn physical_state(&self) -> MatrixState {
        self.physical
    }
    pub fn is_quarantined(&self) -> bool {
        self.quarantined
    }
    pub fn pressed_keys(&self) -> &BTreeSet<NativeKey> {
        &self.pressed
    }
    pub fn blocked_combinations(&self) -> u64 {
        self.blocked
    }
    pub fn is_key_admitted(&self, key: PhysicalKey) -> bool {
        !self.quarantined
            && (self.active.contains_key(&key) || (key.is_modifier() && self.physical.get(key)))
    }
    pub fn transition(
        &mut self,
        event: KeyEvent,
        backend: &mut dyn KeyboardBackend,
    ) -> Result<(), MappingError> {
        event
            .validate()
            .map_err(|e| MappingError::InvalidEvent(e.to_string()))?;
        if self.physical.get(event.key) == event.pressed {
            return Ok(());
        }
        self.physical.set(event.key, event.pressed);
        if self.quarantined {
            if self.physical.ordinary_any() {
                return Ok(());
            }
            self.quarantined = false;
        }
        let mut text = None;
        if !event.key.is_modifier() {
            if event.pressed {
                let action =
                    self.profile
                        .action(event.key, self.physical.modifiers(), self.platform);
                if let Action::Text(value) = &action {
                    // Unicode text is deliberately separate from physical shortcut chords.
                    if !backend.capabilities().text
                        || self.physical.modifiers() & (MOD_CONTROL | MOD_COMMODORE) != 0
                        || self
                            .pressed
                            .iter()
                            .any(|key| !matches!(key, NativeKey::LeftShift | NativeKey::RightShift))
                        || self.active.values().any(|active| match &active.action {
                            Action::Keys { keys, .. } => keys.iter().any(|key| {
                                matches!(key, NativeKey::Control | NativeKey::Alt | NativeKey::Meta)
                            }),
                            Action::Text(_) => false,
                        })
                    {
                        self.blocked = self.blocked.saturating_add(1);
                        return Ok(());
                    }
                    text = Some(value.clone());
                }
                self.order = self.order.wrapping_add(1);
                self.active.insert(
                    event.key,
                    Active {
                        action,
                        order: self.order,
                    },
                );
            } else {
                self.active.remove(&event.key);
            }
        }
        let desired = match self.desired() {
            Ok(desired) => desired,
            Err(MappingError::ShiftConflict) => {
                self.blocked = self.blocked.saturating_add(1);
                if event.pressed && !event.key.is_modifier() {
                    // Reject only the newly requested chord. Its physical key remains
                    // tracked, so holding it cannot later cause an unexpected admission.
                    self.active.remove(&event.key);
                    return Ok(());
                }
                self.quarantined = true;
                self.active.clear();
                self.release_owned(backend)?;
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        if let Err(e) = self.apply(&desired, backend).and_then(|()| {
            if let Some(text) = text {
                backend.type_text(&text).map_err(MappingError::Backend)?;
            }
            Ok(())
        }) {
            self.active.clear();
            self.quarantined = true;
            let _ = self.release_owned(backend);
            return Err(e);
        }
        Ok(())
    }
    /// Connection/re-enable baseline: held ordinary keys must first be released.
    pub fn synchronize(
        &mut self,
        matrix: MatrixState,
        backend: &mut dyn KeyboardBackend,
    ) -> Result<(), MappingError> {
        self.active.clear();
        self.physical = matrix;
        self.quarantined = matrix.ordinary_any();
        self.release_owned(backend)?;
        if !self.quarantined {
            let desired = self.desired()?;
            if let Err(error) = self.apply(&desired, backend) {
                self.quarantined = true;
                let _ = self.release_owned(backend);
                return Err(error);
            }
        }
        Ok(())
    }
    /// Authoritative snapshots repair missed releases without admitting new held keys.
    pub fn reconcile(
        &mut self,
        matrix: MatrixState,
        backend: &mut dyn KeyboardBackend,
    ) -> Result<(), MappingError> {
        if matrix == self.physical {
            return Ok(());
        }
        // A press missing its transition is ambiguous: release and establish a safe baseline.
        if ALL_KEYS
            .into_iter()
            .any(|key| matrix.get(key) && !self.physical.get(key))
        {
            return self.synchronize(matrix, backend);
        }
        for key in ALL_KEYS {
            if self.physical.get(key) && !matrix.get(key) {
                self.transition(
                    KeyEvent {
                        key,
                        pressed: false,
                        modifiers: matrix.modifiers(),
                    },
                    backend,
                )?;
            }
        }
        Ok(())
    }
    pub fn release_all(&mut self, backend: &mut dyn KeyboardBackend) -> Result<(), MappingError> {
        self.active.clear();
        self.quarantined = self.physical.ordinary_any();
        self.release_owned(backend)
    }
    pub fn repeat(&mut self, backend: &mut dyn KeyboardBackend) -> Result<(), MappingError> {
        if self.quarantined || !backend.capabilities().repeat {
            return Ok(());
        }
        let Some(active) = self
            .active
            .values()
            .filter(|active| match &active.action {
                Action::Keys { keys, .. } => keys.iter().any(|key| !key.is_modifier()),
                Action::Text(_) => true,
            })
            .max_by_key(|a| a.order)
        else {
            return Ok(());
        };
        let result =
            match &active.action {
                Action::Keys { keys, .. } => {
                    if let Some(key) = keys.iter().rev().find(|key| !key.is_modifier()) {
                        backend.repeat(*key)
                    } else {
                        Ok(())
                    }
                }
                Action::Text(text) => {
                    if self.pressed.iter().any(|key| {
                        matches!(key, NativeKey::Control | NativeKey::Alt | NativeKey::Meta)
                    }) {
                        return Ok(());
                    }
                    backend.type_text(text)
                }
            };
        if let Err(error) = result {
            self.active.clear();
            self.quarantined = true;
            let _ = self.release_owned(backend);
            return Err(error.into());
        }
        Ok(())
    }
    fn desired(&self) -> Result<BTreeMap<NativeKey, usize>, MappingError> {
        let mut desired = BTreeMap::new();
        let physical_shift = self.physical.modifiers() & MOD_SHIFT != 0;
        let mut shift_requirement = None;
        for active in self.active.values() {
            let (required, keys) = match &active.action {
                Action::Keys { keys, shift } => (
                    match shift {
                        ShiftPolicy::Physical => physical_shift,
                        ShiftPolicy::Required => true,
                        ShiftPolicy::Suppress => false,
                    },
                    Some(keys),
                ),
                Action::Text(_) => (false, None),
            };
            if shift_requirement.is_some_and(|old| old != required) {
                return Err(MappingError::ShiftConflict);
            }
            shift_requirement = Some(required);
            if let Some(keys) = keys {
                for key in keys {
                    *desired.entry(*key).or_insert(0) += 1;
                }
            }
        }
        let shift = shift_requirement.unwrap_or(physical_shift);
        if shift {
            if self.physical.get(PhysicalKey::RightShift) {
                *desired.entry(NativeKey::RightShift).or_insert(0) += 1;
            }
            if self.physical.get(PhysicalKey::LeftShift)
                || !self.physical.get(PhysicalKey::RightShift)
            {
                *desired.entry(NativeKey::LeftShift).or_insert(0) += 1;
            }
        }
        if self.physical.get(PhysicalKey::Control) {
            *desired.entry(self.profile.control_modifier).or_insert(0) += 1;
        }
        if self.physical.get(PhysicalKey::Commodore) {
            let key = self.profile.commodore_modifier.unwrap_or(
                if self.platform == HostPlatform::MacOS {
                    NativeKey::Meta
                } else {
                    NativeKey::Control
                },
            );
            *desired.entry(key).or_insert(0) += 1;
        }
        Ok(desired)
    }
    fn apply(
        &mut self,
        desired: &BTreeMap<NativeKey, usize>,
        backend: &mut dyn KeyboardBackend,
    ) -> Result<(), MappingError> {
        if !backend.capabilities().key_events && !desired.is_empty() {
            return Err(BackendError::Unsupported("physical key events".into()).into());
        }
        let released: Vec<_> = self
            .pressed
            .iter()
            .copied()
            .filter(|key| !desired.contains_key(key))
            .collect();
        // Release ordinary keys before their supporting modifiers.
        for modifiers in [false, true] {
            for key in released
                .iter()
                .copied()
                .filter(|key| key.is_modifier() == modifiers)
            {
                backend.release(key)?;
                self.pressed.remove(&key);
            }
        }
        // Establish modifier flags before posting the ordinary key-down.
        for modifiers in [true, false] {
            for key in desired
                .keys()
                .copied()
                .filter(|key| key.is_modifier() == modifiers)
            {
                if !self.pressed.contains(&key) {
                    // A failed native call may have posted an event before reporting failure.
                    self.pressed.insert(key);
                    backend.press(key)?;
                }
            }
        }
        Ok(())
    }
    fn release_owned(&mut self, backend: &mut dyn KeyboardBackend) -> Result<(), MappingError> {
        let keys: Vec<_> = self.pressed.iter().copied().collect();
        // Temporary Unicode keys are ordinary keys too. Attempt their release
        // before dropping modifiers, even when another release later fails.
        let mut first_error = backend.cleanup_pending_text().err();
        for modifiers in [false, true] {
            for key in keys
                .iter()
                .copied()
                .filter(|key| key.is_modifier() == modifiers)
            {
                match backend.release(key) {
                    Ok(()) => {
                        self.pressed.remove(&key);
                    }
                    Err(e) => {
                        if first_error.is_none() {
                            first_error = Some(e);
                        }
                    }
                }
            }
        }
        first_error.map_or(Ok(()), |e| Err(e.into()))
    }
}

#[derive(Debug, Error)]
pub enum MappingError {
    #[error("keyboard layout must be US or UK")]
    InvalidLayout,
    #[error("keyboard profile must be modern or authentic")]
    InvalidProfile,
    #[error("invalid mapping: {0}")]
    Configuration(String),
    #[error("invalid physical event: {0}")]
    InvalidEvent(String),
    #[error("held keys require conflicting Shift states")]
    ShiftConflict,
    #[error(transparent)]
    Backend(#[from] BackendError),
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mapper(platform: HostPlatform) -> Mapper {
        Mapper::new(MappingProfile::builtin("modern", "US").unwrap(), platform)
    }
    fn send(
        mapper: &mut Mapper,
        backend: &mut dyn KeyboardBackend,
        key: PhysicalKey,
        pressed: bool,
    ) {
        let mut state = mapper.physical_state();
        state.set(key, pressed);
        mapper
            .transition(
                KeyEvent {
                    key,
                    pressed,
                    modifiers: state.modifiers(),
                },
                backend,
            )
            .unwrap();
    }
    #[test]
    fn keys_are_owned_until_release_and_duplicate_events_are_ignored() {
        let mut m = mapper(HostPlatform::MacOS);
        let mut b = RecordingBackend::default();
        send(&mut m, &mut b, PhysicalKey::A, true);
        send(&mut m, &mut b, PhysicalKey::A, true);
        send(&mut m, &mut b, PhysicalKey::A, false);
        assert_eq!(
            b.events,
            vec![
                RecordingEvent::Press(NativeKey::A),
                RecordingEvent::Release(NativeKey::A)
            ]
        );
    }
    #[test]
    fn shared_windows_control_is_not_released_by_one_physical_owner() {
        let mut m = mapper(HostPlatform::Windows);
        let mut b = RecordingBackend::default();
        send(&mut m, &mut b, PhysicalKey::Control, true);
        send(&mut m, &mut b, PhysicalKey::Commodore, true);
        send(&mut m, &mut b, PhysicalKey::Control, false);
        assert_eq!(b.events, vec![RecordingEvent::Press(NativeKey::Control)]);
        send(&mut m, &mut b, PhysicalKey::Commodore, false);
        assert_eq!(
            b.events.last(),
            Some(&RecordingEvent::Release(NativeKey::Control))
        );
    }
    #[test]
    fn cursor_consumes_shift_and_release_uses_original_direction() {
        let mut m = mapper(HostPlatform::MacOS);
        let mut b = RecordingBackend::default();
        send(&mut m, &mut b, PhysicalKey::LeftShift, true);
        send(&mut m, &mut b, PhysicalKey::CursorRight, true);
        send(&mut m, &mut b, PhysicalKey::LeftShift, false);
        send(&mut m, &mut b, PhysicalKey::CursorRight, false);
        assert_eq!(
            b.events,
            vec![
                RecordingEvent::Press(NativeKey::LeftShift),
                RecordingEvent::Release(NativeKey::LeftShift),
                RecordingEvent::Press(NativeKey::ArrowLeft),
                RecordingEvent::Release(NativeKey::ArrowLeft)
            ]
        );
    }
    #[test]
    fn modifier_change_conflict_releases_and_quarantines_until_all_ordinary_up() {
        let mut m = mapper(HostPlatform::MacOS);
        let mut b = RecordingBackend::default();
        send(&mut m, &mut b, PhysicalKey::A, true);
        send(&mut m, &mut b, PhysicalKey::CursorRight, true);
        send(&mut m, &mut b, PhysicalKey::LeftShift, true);
        assert!(m.is_quarantined());
        assert!(m.pressed_keys().is_empty());
        send(&mut m, &mut b, PhysicalKey::A, false);
        assert!(m.is_quarantined());
        send(&mut m, &mut b, PhysicalKey::CursorRight, false);
        assert!(!m.is_quarantined());
        send(&mut m, &mut b, PhysicalKey::B, true);
        assert!(m.pressed_keys().contains(&NativeKey::B));
    }
    #[test]
    fn reconnect_baseline_does_not_type_previously_held_keys() {
        let mut m = mapper(HostPlatform::MacOS);
        let mut b = RecordingBackend::default();
        let mut matrix = MatrixState::default();
        matrix.set(PhysicalKey::A, true);
        matrix.set(PhysicalKey::LeftShift, true);
        m.synchronize(matrix, &mut b).unwrap();
        assert!(b.events.is_empty());
        send(&mut m, &mut b, PhysicalKey::B, true);
        assert!(b.events.is_empty());
        send(&mut m, &mut b, PhysicalKey::A, false);
        send(&mut m, &mut b, PhysicalKey::B, false);
        assert_eq!(b.events, vec![RecordingEvent::Press(NativeKey::LeftShift)]);
    }
    #[test]
    fn new_shift_conflicting_chord_is_rejected_without_releasing_earlier_keys() {
        for (first, rejected, native) in [
            (PhysicalKey::A, PhysicalKey::CursorRight, NativeKey::A),
            (
                PhysicalKey::CursorRight,
                PhysicalKey::A,
                NativeKey::ArrowLeft,
            ),
        ] {
            let mut m = mapper(HostPlatform::MacOS);
            let mut b = RecordingBackend::default();
            send(&mut m, &mut b, PhysicalKey::LeftShift, true);
            send(&mut m, &mut b, first, true);
            let before = b.events.clone();
            send(&mut m, &mut b, rejected, true);
            assert_eq!(b.events, before);
            assert!(!m.is_quarantined());
            assert_eq!(m.blocked_combinations(), 1);
            assert!(m.is_key_admitted(first));
            assert!(!m.is_key_admitted(rejected));
            assert!(m.pressed_keys().contains(&native));
            send(&mut m, &mut b, first, false);
            assert!(!m.is_key_admitted(rejected));
            send(&mut m, &mut b, rejected, false);
            send(&mut m, &mut b, rejected, true);
            assert!(m.is_key_admitted(rejected));
        }
    }
    #[test]
    fn text_overlap_is_rejected_but_shift_alone_is_suppressed_and_restored() {
        let mut m = mapper(HostPlatform::MacOS);
        let mut b = RecordingBackend::default();
        send(&mut m, &mut b, PhysicalKey::A, true);
        send(&mut m, &mut b, PhysicalKey::Pound, true);
        assert_eq!(m.blocked_combinations(), 1);
        assert!(!m.is_key_admitted(PhysicalKey::Pound));
        assert!(m.pressed_keys().contains(&NativeKey::A));
        assert!(
            !b.events
                .iter()
                .any(|e| matches!(e, RecordingEvent::Text(_)))
        );
        send(&mut m, &mut b, PhysicalKey::A, false);
        send(&mut m, &mut b, PhysicalKey::Pound, false);
        send(&mut m, &mut b, PhysicalKey::LeftShift, true);
        send(&mut m, &mut b, PhysicalKey::Pound, true);
        assert_eq!(
            &b.events[b.events.len() - 2..],
            &[
                RecordingEvent::Release(NativeKey::LeftShift),
                RecordingEvent::Text("£".into())
            ]
        );
        send(&mut m, &mut b, PhysicalKey::Pound, false);
        assert_eq!(
            b.events.last(),
            Some(&RecordingEvent::Press(NativeKey::LeftShift))
        );
    }
    #[test]
    fn uk_punctuation_differs_between_mac_and_pc_layouts() {
        for (platform, at, quote) in [
            (HostPlatform::MacOS, NativeKey::Num2, NativeKey::Quote),
            (HostPlatform::Windows, NativeKey::Quote, NativeKey::Num2),
            (HostPlatform::Linux, NativeKey::Quote, NativeKey::Num2),
        ] {
            let mut m = Mapper::new(
                MappingProfile::builtin("authentic", "UK").unwrap(),
                platform,
            );
            let mut b = RecordingBackend::default();
            send(&mut m, &mut b, PhysicalKey::At, true);
            assert!(m.pressed_keys().contains(&at));
            send(&mut m, &mut b, PhysicalKey::At, false);
            send(&mut m, &mut b, PhysicalKey::LeftShift, true);
            send(&mut m, &mut b, PhysicalKey::Num2, true);
            assert!(m.pressed_keys().contains(&quote));
        }
    }
    #[test]
    fn authentic_uk_hash_consumes_shift_and_owns_its_platform_chord_until_release() {
        for platform in [
            HostPlatform::MacOS,
            HostPlatform::Windows,
            HostPlatform::Linux,
        ] {
            let mut m = Mapper::new(
                MappingProfile::builtin("authentic", "UK").unwrap(),
                platform,
            );
            let mut b = RecordingBackend::default();
            send(&mut m, &mut b, PhysicalKey::LeftShift, true);
            send(&mut m, &mut b, PhysicalKey::Num3, true);
            let expected = if platform == HostPlatform::MacOS {
                BTreeSet::from([NativeKey::Alt, NativeKey::Num3])
            } else {
                BTreeSet::from([NativeKey::Backslash])
            };
            assert_eq!(m.pressed_keys(), &expected);
            send(&mut m, &mut b, PhysicalKey::LeftShift, false);
            assert_eq!(m.pressed_keys(), &expected);
            send(&mut m, &mut b, PhysicalKey::Num3, false);
            assert!(m.pressed_keys().is_empty());
            assert!(!m.is_quarantined());
            assert_eq!(m.blocked_combinations(), 0);
        }
    }
    #[test]
    fn known_unsupported_text_is_blocked_until_release_without_disabling_other_keys() {
        #[derive(Default)]
        struct PhysicalOnly(RecordingBackend);
        impl KeyboardBackend for PhysicalOnly {
            fn capabilities(&self) -> BackendCapabilities {
                BackendCapabilities {
                    key_events: true,
                    text: false,
                    repeat: true,
                }
            }
            fn press(&mut self, key: NativeKey) -> Result<(), BackendError> {
                self.0.press(key)
            }
            fn release(&mut self, key: NativeKey) -> Result<(), BackendError> {
                self.0.release(key)
            }
            fn repeat(&mut self, key: NativeKey) -> Result<(), BackendError> {
                self.0.repeat(key)
            }
        }
        let mut m = Mapper::new(
            MappingProfile::builtin("authentic", "US").unwrap(),
            HostPlatform::Linux,
        );
        let mut b = PhysicalOnly::default();
        // A standalone text action must be rejected before invoking the trait's
        // default Unsupported method, rather than becoming a session failure.
        send(&mut m, &mut b, PhysicalKey::Pound, true);
        assert!(b.0.events.is_empty());
        assert!(!m.is_key_admitted(PhysicalKey::Pound));
        assert!(!m.is_quarantined());
        send(&mut m, &mut b, PhysicalKey::Pound, false);
        send(&mut m, &mut b, PhysicalKey::A, true);
        for key in [
            PhysicalKey::Pound,
            PhysicalKey::UpArrow,
            PhysicalKey::LeftArrow,
        ] {
            let before = b.0.events.clone();
            send(&mut m, &mut b, key, true);
            send(&mut m, &mut b, key, true);
            assert_eq!(b.0.events, before);
            assert!(m.physical_state().get(key));
            assert!(!m.is_key_admitted(key));
            assert!(m.is_key_admitted(PhysicalKey::A));
            assert!(m.pressed_keys().contains(&NativeKey::A));
            assert!(!m.is_quarantined());
            send(&mut m, &mut b, key, false);
        }
        assert_eq!(m.blocked_combinations(), 4);
        m.repeat(&mut b).unwrap();
        assert_eq!(
            b.0.events.last(),
            Some(&RecordingEvent::Repeat(NativeKey::A))
        );
        send(&mut m, &mut b, PhysicalKey::A, false);
        send(&mut m, &mut b, PhysicalKey::B, true);
        assert!(m.is_key_admitted(PhysicalKey::B));
    }
    #[test]
    fn repeat_only_last_admitted_ordinary_key_and_snapshot_repairs_releases() {
        let mut m = mapper(HostPlatform::MacOS);
        let mut b = RecordingBackend::default();
        send(&mut m, &mut b, PhysicalKey::A, true);
        send(&mut m, &mut b, PhysicalKey::B, true);
        m.repeat(&mut b).unwrap();
        assert_eq!(b.events.last(), Some(&RecordingEvent::Repeat(NativeKey::B)));
        send(&mut m, &mut b, PhysicalKey::B, false);
        m.repeat(&mut b).unwrap();
        assert_eq!(b.events.last(), Some(&RecordingEvent::Repeat(NativeKey::A)));
        m.reconcile(MatrixState::default(), &mut b).unwrap();
        assert!(m.pressed_keys().is_empty());
    }
    #[test]
    fn custom_chord_and_profile_validation() {
        let profile = MappingProfile::from_toml("name='custom'\nprofile='modern'\nlayout='US'\n[[bindings]]\nkey='A'\nkeys=['Alt','F4']\nshift='suppress'\n").unwrap();
        let mut m = Mapper::new(profile, HostPlatform::Windows);
        let mut b = RecordingBackend::default();
        send(&mut m, &mut b, PhysicalKey::A, true);
        send(&mut m, &mut b, PhysicalKey::A, false);
        assert_eq!(
            b.events,
            vec![
                RecordingEvent::Press(NativeKey::Alt),
                RecordingEvent::Press(NativeKey::F4),
                RecordingEvent::Release(NativeKey::F4),
                RecordingEvent::Release(NativeKey::Alt)
            ]
        );
        assert!(
            MappingProfile::from_toml(
                "name='bad'\nprofile='modern'\nlayout='US'\n[[bindings]]\nkey='A'\nkeys=['A','A']"
            )
            .is_err()
        );
    }
    #[test]
    fn every_builtin_key_has_a_mapping_on_both_layouts() {
        for profile in ["modern", "authentic"] {
            for layout in ["US", "UK"] {
                let p = MappingProfile::builtin(profile, layout).unwrap();
                for key in ALL_KEYS.into_iter().filter(|key| !key.is_modifier()) {
                    for modifiers in 0..8 {
                        for platform in [
                            HostPlatform::MacOS,
                            HostPlatform::Windows,
                            HostPlatform::Linux,
                        ] {
                            let _ = p.action(key, modifiers, platform);
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn modifier_roles_and_explicit_shift_chords_are_configurable() {
        let profile = MappingProfile::from_toml("name='roles'\nprofile='modern'\nlayout='UK'\ncommodore_modifier='Alt'\n[[bindings]]\nkey='A'\nkeys=['LeftShift','B']\n").unwrap();
        let mut m = Mapper::new(profile, HostPlatform::MacOS);
        let mut b = RecordingBackend::default();
        send(&mut m, &mut b, PhysicalKey::Commodore, true);
        assert!(m.pressed_keys().contains(&NativeKey::Alt));
        send(&mut m, &mut b, PhysicalKey::A, true);
        assert!(m.pressed_keys().contains(&NativeKey::LeftShift));
        assert!(m.pressed_keys().contains(&NativeKey::B));
        assert!(!m.pressed_keys().contains(&NativeKey::Meta));
    }
    #[test]
    fn unicode_is_separate_from_shortcuts_and_held_modifier_releases_do_not_type() {
        let mut m = mapper(HostPlatform::MacOS);
        let mut b = RecordingBackend::default();
        send(&mut m, &mut b, PhysicalKey::Pound, true);
        assert_eq!(b.events, vec![RecordingEvent::Text("£".into())]);
        send(&mut m, &mut b, PhysicalKey::Pound, false);
        send(&mut m, &mut b, PhysicalKey::Commodore, true);
        send(&mut m, &mut b, PhysicalKey::Pound, true);
        send(&mut m, &mut b, PhysicalKey::Commodore, false);
        m.repeat(&mut b).unwrap();
        assert_eq!(
            b.events
                .iter()
                .filter(|e| matches!(e, RecordingEvent::Text(_)))
                .count(),
            1
        );
    }
    #[test]
    fn unicode_repeat_does_not_emit_while_shortcut_modifier_is_held() {
        let mut m = mapper(HostPlatform::MacOS);
        let mut b = RecordingBackend::default();
        send(&mut m, &mut b, PhysicalKey::Pound, true);
        send(&mut m, &mut b, PhysicalKey::Control, true);
        m.repeat(&mut b).unwrap();
        assert_eq!(
            b.events
                .iter()
                .filter(|e| matches!(e, RecordingEvent::Text(_)))
                .count(),
            1
        );
    }
    #[test]
    fn every_release_is_attempted_after_backend_failure() {
        struct Failing {
            releases: Vec<NativeKey>,
        }
        impl KeyboardBackend for Failing {
            fn capabilities(&self) -> BackendCapabilities {
                BackendCapabilities {
                    key_events: true,
                    text: false,
                    repeat: true,
                }
            }
            fn press(&mut self, _: NativeKey) -> Result<(), BackendError> {
                Ok(())
            }
            fn release(&mut self, key: NativeKey) -> Result<(), BackendError> {
                self.releases.push(key);
                if key == NativeKey::A {
                    Err(BackendError::Injection("test".into()))
                } else {
                    Ok(())
                }
            }
            fn repeat(&mut self, _: NativeKey) -> Result<(), BackendError> {
                Ok(())
            }
        }
        let mut m = mapper(HostPlatform::MacOS);
        let mut b = Failing {
            releases: Vec::new(),
        };
        for key in [PhysicalKey::LeftShift, PhysicalKey::A, PhysicalKey::B] {
            let mut state = m.physical_state();
            state.set(key, true);
            m.transition(
                KeyEvent {
                    key,
                    pressed: true,
                    modifiers: state.modifiers(),
                },
                &mut b,
            )
            .unwrap();
        }
        assert!(m.release_all(&mut b).is_err());
        assert_eq!(
            b.releases,
            vec![NativeKey::A, NativeKey::B, NativeKey::LeftShift]
        );
        assert_eq!(m.pressed_keys(), &BTreeSet::from([NativeKey::A]));
    }
    #[test]
    fn pending_text_release_precedes_modifiers_and_is_retried_after_failure() {
        struct PendingText {
            events: Vec<&'static str>,
            fail: bool,
        }
        impl KeyboardBackend for PendingText {
            fn capabilities(&self) -> BackendCapabilities {
                BackendCapabilities {
                    key_events: true,
                    text: true,
                    repeat: true,
                }
            }
            fn press(&mut self, _: NativeKey) -> Result<(), BackendError> {
                Ok(())
            }
            fn release(&mut self, key: NativeKey) -> Result<(), BackendError> {
                assert!(key.is_modifier());
                self.events.push("modifier up");
                Ok(())
            }
            fn repeat(&mut self, _: NativeKey) -> Result<(), BackendError> {
                Ok(())
            }
            fn cleanup_pending_text(&mut self) -> Result<(), BackendError> {
                self.events.push("text up");
                if self.fail {
                    Err(BackendError::Injection(
                        "temporary text release refused".into(),
                    ))
                } else {
                    Ok(())
                }
            }
        }
        let mut m = mapper(HostPlatform::MacOS);
        let mut b = PendingText {
            events: Vec::new(),
            fail: true,
        };
        m.transition(
            KeyEvent {
                key: PhysicalKey::LeftShift,
                pressed: true,
                modifiers: MOD_SHIFT,
            },
            &mut b,
        )
        .unwrap();
        assert!(m.release_all(&mut b).is_err());
        assert_eq!(b.events, vec!["text up", "modifier up"]);
        b.fail = false;
        m.release_all(&mut b).unwrap();
        assert_eq!(b.events, vec!["text up", "modifier up", "text up"]);
    }
    #[test]
    fn shipped_profile_files_are_valid() {
        for text in [
            include_str!("../../../config/layouts/modern-us.toml"),
            include_str!("../../../config/layouts/modern-uk.toml"),
            include_str!("../../../config/layouts/authentic-c64.toml"),
        ] {
            MappingProfile::from_toml(text).unwrap();
        }
    }
    #[test]
    fn mapping_documents_round_trip_and_share_validation() {
        let text = "name='custom'\nprofile='modern'\nlayout='UK'\ncommodore_modifier='Alt'\n[[bindings]]\nkey='A'\nmodifiers=['Shift']\nkeys=['Control','B']\nshift='suppress'\n[[bindings]]\nkey='Pound'\ntext='£'\n";
        let draft = MappingDocument::from_toml(text).unwrap();
        let round_trip = MappingDocument::from_toml(&draft.to_toml().unwrap()).unwrap();
        assert_eq!(draft, round_trip);
        let profile = draft.validate().unwrap();
        assert_eq!(
            profile
                .preview(PhysicalKey::A, MOD_SHIFT, HostPlatform::Windows)
                .unwrap(),
            ResolvedAction::Keys {
                keys: vec![NativeKey::Control, NativeKey::B],
                shift: ShiftPolicy::Suppress
            }
        );
        assert_eq!(
            profile
                .preview(PhysicalKey::Commodore, 0, HostPlatform::MacOS)
                .unwrap(),
            ResolvedAction::Modifier(NativeKey::Alt)
        );
        assert_eq!(
            profile
                .preview(PhysicalKey::Pound, 0, HostPlatform::Linux)
                .unwrap(),
            ResolvedAction::Text("£".into())
        );
        assert!(
            profile
                .preview(PhysicalKey::A, 128, HostPlatform::Linux)
                .is_err()
        );
    }
    #[test]
    fn invalid_document_stays_editable_without_bypassing_cli_validator() {
        let mut draft = MappingDocument::builtin("modern", "US").unwrap();
        draft.bindings.push(BindingDocument {
            key: PhysicalKey::A,
            modifiers: vec![],
            keys: vec![NativeKey::LeftShift, NativeKey::A],
            text: None,
            shift: Some(ShiftPolicy::Suppress),
        });
        let text = draft.to_toml().unwrap();
        assert_eq!(MappingDocument::from_toml(&text).unwrap(), draft);
        assert_eq!(
            draft.validate().unwrap_err().to_string(),
            MappingProfile::from_toml(&text).unwrap_err().to_string()
        );
        assert!(
            MappingDocument::from_toml("name='x'\nprofile='modern'\nlayout='US'\nunknown=true")
                .is_err()
        );
    }
    #[test]
    fn preview_resolves_inherited_shift_and_platform_defaults() {
        let profile = MappingDocument::builtin("modern", "US")
            .unwrap()
            .validate()
            .unwrap();
        assert_eq!(
            profile
                .preview(PhysicalKey::CursorRight, MOD_SHIFT, HostPlatform::Linux)
                .unwrap(),
            ResolvedAction::Keys {
                keys: vec![NativeKey::ArrowLeft],
                shift: ShiftPolicy::Suppress
            }
        );
        assert_eq!(
            profile
                .preview(PhysicalKey::Commodore, 0, HostPlatform::MacOS)
                .unwrap(),
            ResolvedAction::Modifier(NativeKey::Meta)
        );
        assert_eq!(
            profile
                .preview(PhysicalKey::Commodore, 0, HostPlatform::Linux)
                .unwrap(),
            ResolvedAction::Modifier(NativeKey::Control)
        );
        for key in NativeKey::ALL {
            assert_eq!(NativeKey::from_name(&format!("{key:?}")), Some(*key));
        }
    }
}

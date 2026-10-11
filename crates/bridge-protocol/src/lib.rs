//! Small, bounded wire protocol for physical C64 keyboard events.
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAGIC: [u8; 2] = *b"KB";
pub const VERSION: u8 = 1;
pub const HEADER_LEN: usize = 8;
pub const MAX_PAYLOAD: usize = 64;
pub const MAX_FRAME: usize = HEADER_LEN + MAX_PAYLOAD;
pub const MOD_SHIFT: u8 = 1;
pub const MOD_CONTROL: u8 = 2;
pub const MOD_COMMODORE: u8 = 4;
pub const MODIFIER_MASK: u8 = 7;
pub const CAP_MATRIX_STATE: u16 = 1;

/// IDs are CIA1 port-A selector * 8 + port-B bit. A set matrix bit means pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum PhysicalKey {
    Delete = 0,
    Return,
    CursorRight,
    F7,
    F1,
    F3,
    F5,
    CursorDown,
    Num3,
    W,
    A,
    Num4,
    Z,
    S,
    E,
    LeftShift,
    Num5,
    R,
    D,
    Num6,
    C,
    F,
    T,
    X,
    Num7,
    Y,
    G,
    Num8,
    B,
    H,
    U,
    V,
    Num9,
    I,
    J,
    Num0,
    M,
    K,
    O,
    N,
    Plus,
    P,
    L,
    Minus,
    Period,
    Colon,
    At,
    Comma,
    Pound,
    Asterisk,
    Semicolon,
    Home,
    RightShift,
    Equal,
    UpArrow,
    Slash,
    Num1,
    LeftArrow,
    Control,
    Num2,
    Space,
    Commodore,
    Q,
    RunStop,
}

pub const ALL_KEYS: [PhysicalKey; 64] = [
    PhysicalKey::Delete,
    PhysicalKey::Return,
    PhysicalKey::CursorRight,
    PhysicalKey::F7,
    PhysicalKey::F1,
    PhysicalKey::F3,
    PhysicalKey::F5,
    PhysicalKey::CursorDown,
    PhysicalKey::Num3,
    PhysicalKey::W,
    PhysicalKey::A,
    PhysicalKey::Num4,
    PhysicalKey::Z,
    PhysicalKey::S,
    PhysicalKey::E,
    PhysicalKey::LeftShift,
    PhysicalKey::Num5,
    PhysicalKey::R,
    PhysicalKey::D,
    PhysicalKey::Num6,
    PhysicalKey::C,
    PhysicalKey::F,
    PhysicalKey::T,
    PhysicalKey::X,
    PhysicalKey::Num7,
    PhysicalKey::Y,
    PhysicalKey::G,
    PhysicalKey::Num8,
    PhysicalKey::B,
    PhysicalKey::H,
    PhysicalKey::U,
    PhysicalKey::V,
    PhysicalKey::Num9,
    PhysicalKey::I,
    PhysicalKey::J,
    PhysicalKey::Num0,
    PhysicalKey::M,
    PhysicalKey::K,
    PhysicalKey::O,
    PhysicalKey::N,
    PhysicalKey::Plus,
    PhysicalKey::P,
    PhysicalKey::L,
    PhysicalKey::Minus,
    PhysicalKey::Period,
    PhysicalKey::Colon,
    PhysicalKey::At,
    PhysicalKey::Comma,
    PhysicalKey::Pound,
    PhysicalKey::Asterisk,
    PhysicalKey::Semicolon,
    PhysicalKey::Home,
    PhysicalKey::RightShift,
    PhysicalKey::Equal,
    PhysicalKey::UpArrow,
    PhysicalKey::Slash,
    PhysicalKey::Num1,
    PhysicalKey::LeftArrow,
    PhysicalKey::Control,
    PhysicalKey::Num2,
    PhysicalKey::Space,
    PhysicalKey::Commodore,
    PhysicalKey::Q,
    PhysicalKey::RunStop,
];

impl PhysicalKey {
    pub const fn id(self) -> u8 {
        self as u8
    }
    pub fn from_id(id: u8) -> Result<Self, ProtocolError> {
        ALL_KEYS
            .get(id as usize)
            .copied()
            .ok_or(ProtocolError::InvalidKey(id))
    }
    pub const fn is_modifier(self) -> bool {
        matches!(
            self,
            Self::LeftShift | Self::RightShift | Self::Control | Self::Commodore
        )
    }
    pub fn named(name: &str) -> Option<Self> {
        ALL_KEYS
            .into_iter()
            .find(|key| format!("{key:?}").eq_ignore_ascii_case(name))
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatrixState(pub [u8; 8]);
impl MatrixState {
    pub const fn from_bytes(bytes: [u8; 8]) -> Self {
        Self(bytes)
    }
    pub const fn as_bytes(&self) -> &[u8; 8] {
        &self.0
    }
    pub fn get(self, key: PhysicalKey) -> bool {
        self.0[key.id() as usize / 8] & (1 << (key.id() % 8)) != 0
    }
    pub fn key(self, key: PhysicalKey) -> bool {
        self.get(key)
    }
    pub fn set(&mut self, key: PhysicalKey, pressed: bool) {
        let byte = &mut self.0[key.id() as usize / 8];
        let mask = 1 << (key.id() % 8);
        if pressed {
            *byte |= mask;
        } else {
            *byte &= !mask;
        }
    }
    pub fn modifiers(self) -> u8 {
        let shift = self.get(PhysicalKey::LeftShift) || self.get(PhysicalKey::RightShift);
        u8::from(shift)
            | (u8::from(self.get(PhysicalKey::Control)) << 1)
            | (u8::from(self.get(PhysicalKey::Commodore)) << 2)
    }
    pub fn ordinary_any(self) -> bool {
        ALL_KEYS
            .into_iter()
            .any(|key| !key.is_modifier() && self.get(key))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: PhysicalKey,
    pub pressed: bool,
    pub modifiers: u8,
}
impl KeyEvent {
    pub fn validate(self) -> Result<Self, ProtocolError> {
        if self.modifiers & !MODIFIER_MASK != 0 {
            return Err(ProtocolError::InvalidModifiers);
        }
        Ok(self)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum Message {
    Hello {
        token: [u8; 16],
        capabilities: u16,
        device_id: String,
    },
    HelloAck {
        enabled: bool,
    },
    KeyDown {
        key: PhysicalKey,
        modifiers: u8,
    },
    KeyUp {
        key: PhysicalKey,
        modifiers: u8,
    },
    StateSync(MatrixState),
    Heartbeat(MatrixState),
    HostHeartbeat {
        enabled: bool,
    },
    Error {
        code: u8,
        text: String,
    },
    Disconnect {
        reason: u8,
    },
}
impl std::fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hello {
                capabilities,
                device_id,
                ..
            } => f
                .debug_struct("Hello")
                .field("token", &"[REDACTED]")
                .field("capabilities", capabilities)
                .field("device_id", device_id)
                .finish(),
            Self::HelloAck { enabled } => f
                .debug_struct("HelloAck")
                .field("enabled", enabled)
                .finish(),
            Self::KeyDown { key, modifiers } => f
                .debug_struct("KeyDown")
                .field("key", key)
                .field("modifiers", modifiers)
                .finish(),
            Self::KeyUp { key, modifiers } => f
                .debug_struct("KeyUp")
                .field("key", key)
                .field("modifiers", modifiers)
                .finish(),
            Self::StateSync(_) => f.write_str("StateSync([REDACTED])"),
            Self::Heartbeat(_) => f.write_str("Heartbeat([REDACTED])"),
            Self::HostHeartbeat { enabled } => f
                .debug_struct("HostHeartbeat")
                .field("enabled", enabled)
                .finish(),
            Self::Error { code, .. } => f
                .debug_struct("Error")
                .field("code", code)
                .finish_non_exhaustive(),
            Self::Disconnect { reason } => f
                .debug_struct("Disconnect")
                .field("reason", reason)
                .finish(),
        }
    }
}
impl Message {
    pub fn key_event(&self) -> Option<KeyEvent> {
        match self {
            Self::KeyDown { key, modifiers } => Some(KeyEvent {
                key: *key,
                pressed: true,
                modifiers: *modifiers,
            }),
            Self::KeyUp { key, modifiers } => Some(KeyEvent {
                key: *key,
                pressed: false,
                modifiers: *modifiers,
            }),
            _ => None,
        }
    }
    pub fn type_id(&self) -> u8 {
        match self {
            Self::Hello { .. } => 1,
            Self::HelloAck { .. } => 2,
            Self::KeyDown { .. } => 3,
            Self::KeyUp { .. } => 4,
            Self::StateSync(_) => 5,
            Self::Heartbeat(_) | Self::HostHeartbeat { .. } => 6,
            Self::Error { .. } => 7,
            Self::Disconnect { .. } => 8,
        }
    }
    fn payload(&self) -> Result<Vec<u8>, ProtocolError> {
        let mut out = Vec::with_capacity(MAX_PAYLOAD);
        match self {
            Self::Hello {
                token,
                capabilities,
                device_id,
            } => {
                if device_id.is_empty()
                    || device_id.len() > 32
                    || !printable_ascii(device_id.as_bytes())
                {
                    return Err(ProtocolError::InvalidDeviceId);
                }
                out.extend(token);
                out.extend(capabilities.to_le_bytes());
                out.push(device_id.len() as u8);
                out.extend(device_id.as_bytes());
            }
            Self::HelloAck { enabled } | Self::HostHeartbeat { enabled } => {
                out.push(u8::from(*enabled))
            }
            Self::KeyDown { key, modifiers } | Self::KeyUp { key, modifiers } => {
                KeyEvent {
                    key: *key,
                    pressed: true,
                    modifiers: *modifiers,
                }
                .validate()?;
                out.extend([key.id(), *modifiers]);
            }
            Self::StateSync(matrix) | Self::Heartbeat(matrix) => out.extend(matrix.0),
            Self::Error { code, text } => {
                if text.len() > MAX_PAYLOAD - 1 || !printable_ascii(text.as_bytes()) {
                    return Err(ProtocolError::InvalidErrorText);
                }
                out.push(*code);
                out.extend(text.as_bytes());
            }
            Self::Disconnect { reason } => out.push(*reason),
        }
        Ok(out)
    }
    fn decode(kind: u8, p: &[u8]) -> Result<Self, ProtocolError> {
        let exact = |len| {
            if p.len() == len {
                Ok(())
            } else {
                Err(ProtocolError::InvalidLength)
            }
        };
        Ok(match kind {
            1 => {
                if p.len() < 19
                    || p[18] == 0
                    || p[18] > 32
                    || p.len() != 19 + p[18] as usize
                    || !printable_ascii(&p[19..])
                {
                    return Err(ProtocolError::InvalidDeviceId);
                }
                Self::Hello {
                    token: p[..16].try_into().expect("checked length"),
                    capabilities: u16::from_le_bytes([p[16], p[17]]),
                    device_id: String::from_utf8(p[19..].to_vec()).expect("checked ASCII"),
                }
            }
            2 => {
                exact(1)?;
                Self::HelloAck {
                    enabled: decode_bool(p[0])?,
                }
            }
            3 | 4 => {
                exact(2)?;
                let key = PhysicalKey::from_id(p[0])?;
                KeyEvent {
                    key,
                    pressed: kind == 3,
                    modifiers: p[1],
                }
                .validate()?;
                if kind == 3 {
                    Self::KeyDown {
                        key,
                        modifiers: p[1],
                    }
                } else {
                    Self::KeyUp {
                        key,
                        modifiers: p[1],
                    }
                }
            }
            5 => {
                exact(8)?;
                Self::StateSync(MatrixState(p.try_into().expect("checked length")))
            }
            6 if p.len() == 1 => Self::HostHeartbeat {
                enabled: decode_bool(p[0])?,
            },
            6 => {
                exact(8)?;
                Self::Heartbeat(MatrixState(p.try_into().expect("checked length")))
            }
            7 => {
                if p.is_empty() || !printable_ascii(&p[1..]) {
                    return Err(ProtocolError::InvalidErrorText);
                }
                Self::Error {
                    code: p[0],
                    text: String::from_utf8(p[1..].to_vec()).expect("checked ASCII"),
                }
            }
            8 => {
                exact(1)?;
                Self::Disconnect { reason: p[0] }
            }
            _ => return Err(ProtocolError::UnknownMessage(kind)),
        })
    }
}
fn printable_ascii(bytes: &[u8]) -> bool {
    bytes.iter().all(|b| (32..=126).contains(b))
}
fn decode_bool(value: u8) -> Result<bool, ProtocolError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(ProtocolError::InvalidBoolean),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub sequence: u16,
    pub message: Message,
}
impl Frame {
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        let payload = self.message.payload()?;
        let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
        out.extend(MAGIC);
        out.extend([VERSION, self.message.type_id()]);
        out.extend((payload.len() as u16).to_le_bytes());
        out.extend(self.sequence.to_le_bytes());
        out.extend(payload);
        Ok(out)
    }
}

/// Accumulates at most one bounded incomplete frame, even for very large input slices.
#[derive(Debug, Default)]
pub struct Decoder {
    pending: Vec<u8>,
    failed: bool,
}
impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn buffered_len(&self) -> usize {
        self.pending.len()
    }
    pub fn push(&mut self, mut bytes: &[u8]) -> Result<Vec<Frame>, ProtocolError> {
        if self.failed {
            return Err(ProtocolError::DecoderFailed);
        }
        let result = self.push_inner(&mut bytes);
        if result.is_err() {
            self.failed = true;
            self.pending.clear();
        }
        result
    }
    fn push_inner(&mut self, bytes: &mut &[u8]) -> Result<Vec<Frame>, ProtocolError> {
        let mut frames = Vec::new();
        loop {
            if self.pending.len() < HEADER_LEN {
                let n = (HEADER_LEN - self.pending.len()).min(bytes.len());
                self.pending.extend_from_slice(&bytes[..n]);
                *bytes = &bytes[n..];
                if self.pending.len() < HEADER_LEN {
                    break;
                }
            }
            if self.pending[..2] != MAGIC {
                return Err(ProtocolError::InvalidMagic);
            }
            if self.pending[2] != VERSION {
                return Err(ProtocolError::UnsupportedVersion(self.pending[2]));
            }
            let length = u16::from_le_bytes([self.pending[4], self.pending[5]]) as usize;
            if length > MAX_PAYLOAD {
                return Err(ProtocolError::PayloadTooLarge(length));
            }
            let total = HEADER_LEN + length;
            let n = (total - self.pending.len()).min(bytes.len());
            self.pending.extend_from_slice(&bytes[..n]);
            *bytes = &bytes[n..];
            if self.pending.len() < total {
                break;
            }
            let sequence = u16::from_le_bytes([self.pending[6], self.pending[7]]);
            let message = Message::decode(self.pending[3], &self.pending[HEADER_LEN..])?;
            frames.push(Frame { sequence, message });
            self.pending.clear();
            if bytes.is_empty() {
                break;
            }
        }
        Ok(frames)
    }
}

/// Every connection starts at zero, with independently ordered directions.
#[derive(Debug, Default, Clone)]
pub struct Sequencer {
    expected: u16,
}
impl Sequencer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn expected(&self) -> u16 {
        self.expected
    }
    pub fn check(&mut self, sequence: u16) -> Result<(), ProtocolError> {
        if sequence != self.expected {
            return Err(ProtocolError::Sequence {
                expected: self.expected,
                actual: sequence,
            });
        }
        self.expected = self.expected.wrapping_add(1);
        Ok(())
    }
    pub fn next_sequence(&mut self) -> u16 {
        let value = self.expected;
        self.expected = self.expected.wrapping_add(1);
        value
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("invalid frame signature")]
    InvalidMagic,
    #[error("unsupported protocol version {0}")]
    UnsupportedVersion(u8),
    #[error("payload length {0} exceeds bound")]
    PayloadTooLarge(usize),
    #[error("invalid payload length")]
    InvalidLength,
    #[error("unknown message type {0}")]
    UnknownMessage(u8),
    #[error("invalid physical key {0}")]
    InvalidKey(u8),
    #[error("invalid modifier flags")]
    InvalidModifiers,
    #[error("invalid device identification")]
    InvalidDeviceId,
    #[error("invalid error payload")]
    InvalidErrorText,
    #[error("invalid boolean")]
    InvalidBoolean,
    #[error("sequence mismatch: expected {expected}, received {actual}")]
    Sequence { expected: u16, actual: u16 },
    #[error("decoder rejected an earlier frame")]
    DecoderFailed,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample_frames() -> Vec<Frame> {
        vec![
            Frame {
                sequence: 0,
                message: Message::Hello {
                    token: [0x95; 16],
                    capabilities: 1,
                    device_id: "C64-Ultimate".into(),
                },
            },
            Frame {
                sequence: 1,
                message: Message::KeyDown {
                    key: PhysicalKey::C,
                    modifiers: MOD_COMMODORE,
                },
            },
            Frame {
                sequence: 2,
                message: Message::Heartbeat(MatrixState([0; 8])),
            },
        ]
    }
    #[test]
    fn framing_every_split_and_coalesced_frames() {
        let frames = sample_frames();
        let bytes: Vec<u8> = frames.iter().flat_map(|f| f.encode().unwrap()).collect();
        for split in 0..=bytes.len() {
            let mut d = Decoder::new();
            let mut got = d.push(&bytes[..split]).unwrap();
            got.extend(d.push(&bytes[split..]).unwrap());
            assert_eq!(got, frames);
            assert_eq!(d.buffered_len(), 0);
        }
        let mut d = Decoder::new();
        let mut got = Vec::new();
        for byte in bytes {
            got.extend(d.push(&[byte]).unwrap());
        }
        assert_eq!(got, frames);
    }
    #[test]
    fn reject_oversize_before_reading_payload_and_poison_connection() {
        let mut d = Decoder::new();
        assert_eq!(
            d.push(&[b'K', b'B', 1, 3, 65, 0, 0, 0]),
            Err(ProtocolError::PayloadTooLarge(65))
        );
        assert_eq!(d.buffered_len(), 0);
        assert_eq!(d.push(&[]), Err(ProtocolError::DecoderFailed));
    }
    #[test]
    fn invalid_fields_and_direction_lengths() {
        for raw in [
            [b'K', b'B', 1, 3, 2, 0, 0, 0, 64, 0],
            [b'K', b'B', 1, 3, 2, 0, 0, 0, 1, 8],
        ] {
            assert!(Decoder::new().push(&raw).is_err());
        }
        assert!(
            Frame {
                sequence: 0,
                message: Message::Hello {
                    token: [0; 16],
                    capabilities: 0,
                    device_id: "\nsecret".into()
                }
            }
            .encode()
            .is_err()
        );
        assert!(
            Decoder::new()
                .push(&[b'K', b'B', 1, 6, 1, 0, 0, 0, 2])
                .is_err()
        );
    }
    #[test]
    fn every_key_and_modifier_matrix_roundtrip() {
        let mut m = MatrixState::default();
        for key in ALL_KEYS {
            assert_eq!(PhysicalKey::from_id(key.id()).unwrap(), key);
            m.set(key, true);
            assert!(m.get(key));
        }
        assert_eq!(m.modifiers(), 7);
        assert!(m.ordinary_any());
        for key in ALL_KEYS {
            m.set(key, false);
        }
        assert_eq!(m, MatrixState::default());
        assert_eq!(
            (
                PhysicalKey::LeftShift.id(),
                PhysicalKey::RightShift.id(),
                PhysicalKey::Control.id(),
                PhysicalKey::Commodore.id()
            ),
            (15, 52, 58, 61)
        );
    }
    #[test]
    fn sequencing_rejects_duplicates_gaps_and_wraps() {
        let mut s = Sequencer::new();
        s.check(0).unwrap();
        assert!(s.check(0).is_err());
        assert!(s.check(2).is_err());
        for i in 1..=u16::MAX {
            s.check(i).unwrap();
        }
        s.check(0).unwrap();
    }
    #[test]
    fn all_message_types_roundtrip_and_secrets_are_redacted() {
        let messages = [
            Message::Hello {
                token: [222; 16],
                capabilities: CAP_MATRIX_STATE,
                device_id: "C64-Ultimate".into(),
            },
            Message::HelloAck { enabled: true },
            Message::KeyDown {
                key: PhysicalKey::Control,
                modifiers: MOD_CONTROL,
            },
            Message::KeyUp {
                key: PhysicalKey::Control,
                modifiers: 0,
            },
            Message::StateSync(MatrixState([0xff; 8])),
            Message::Heartbeat(MatrixState([0; 8])),
            Message::HostHeartbeat { enabled: false },
            Message::Error {
                code: 5,
                text: "x".repeat(63),
            },
            Message::Disconnect { reason: 2 },
        ];
        for message in messages {
            let frame = Frame {
                sequence: 20,
                message,
            };
            assert_eq!(
                Decoder::new().push(&frame.encode().unwrap()).unwrap(),
                vec![frame]
            );
        }
        assert!(!format!("{:?}", sample_frames()[0]).contains("149"));
    }
}

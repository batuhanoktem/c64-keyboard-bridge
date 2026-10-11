use super::{
    BackendProbe,
    codes::{linux_keycode, release_order, x11_keysym},
    x11_transport::{self, BoundedStream},
};
use bridge_core::{BackendCapabilities, BackendError, KeyboardBackend, KeyboardLayout, NativeKey};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use x11rb::{
    connection::Connection,
    protocol::{
        xkb::{ConnectionExt as _, ID, KeyName, NameDetail},
        xproto::{ConnectionExt as _, GetKeyboardMappingReply, KEY_PRESS_EVENT, KEY_RELEASE_EVENT},
        xtest::ConnectionExt as _,
    },
    rust_connection::RustConnection,
};

pub(super) fn capabilities() -> BackendCapabilities {
    BackendCapabilities {
        key_events: true,
        text: false,
        repeat: false,
    }
}

fn unavailable(context: &str, error: impl std::fmt::Display) -> BackendError {
    BackendError::Unavailable(format!("{context}: {error}"))
}

fn layout_from_x11_mapping(
    first_code: u8,
    symbols: &[u32],
    stride: usize,
    first_name: u8,
    names: &[KeyName],
) -> Option<KeyboardLayout> {
    if stride < 2 {
        return None;
    }
    let pair = |name: &[u8; 4]| -> Option<(u32, u32)> {
        let name_index = names.iter().position(|entry| &entry.name == name)?;
        let code = usize::from(first_name).checked_add(name_index)?;
        let symbol_index = code
            .checked_sub(usize::from(first_code))?
            .checked_mul(stride)?;
        Some((*symbols.get(symbol_index)?, *symbols.get(symbol_index + 1)?))
    };
    // Check physical XKB row names, not merely the presence of Latin symbols;
    // Dvorak/Colemak can share punctuation while changing shortcut positions.
    for (row, letters) in [
        (b"AD", b"qwertyuiop".as_slice()),
        (b"AC", b"asdfghjkl".as_slice()),
        (b"AB", b"zxcvbnm".as_slice()),
    ] {
        for (index, letter) in letters.iter().copied().enumerate() {
            let name = [
                row[0],
                row[1],
                b'0' + ((index + 1) / 10) as u8,
                b'0' + ((index + 1) % 10) as u8,
            ];
            if pair(&name) != Some((u32::from(letter), u32::from(letter.to_ascii_uppercase()))) {
                return None;
            }
        }
    }
    let punctuation = (
        pair(b"AE02")?,
        pair(b"AE03")?,
        pair(b"AC11")?,
        pair(b"TLDE")?,
        pair(b"BKSL")?,
    );
    match punctuation {
        ((50, 64), (51, 35), (39, 34), (96, 126), (92, 124)) => Some(KeyboardLayout::Us),
        ((50, 34), (51, 163), (39, 64), (96, 172), (35, 126))
            if pair(b"LSGT") == Some((92, 124)) =>
        {
            Some(KeyboardLayout::Uk)
        }
        _ => None,
    }
}

fn resolve_keysym(key: NativeKey, layout: KeyboardLayout) -> u32 {
    if layout == KeyboardLayout::Uk && key == NativeKey::Backslash {
        u32::from(b'#')
    } else {
        x11_keysym(key)
    }
}

pub(super) fn is_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v.eq_ignore_ascii_case("wayland"))
}

pub(super) fn probe_x11(layout: KeyboardLayout) -> BackendProbe {
    if is_wayland_session() {
        return BackendProbe {
            name: "x11".into(), available: false, authorized: false,
            layout_matches: None,
            detail: "XTEST is not selected in a Wayland session: XWayland cannot establish injection into native Wayland applications. Select uinput after granting device access.".into(),
            capabilities: capabilities(),
        };
    }
    match X11Backend::connect(layout) {
        Ok(backend) => {
            let current = backend.active_layout();
            let matches = current.as_ref().ok().map(|current| *current == layout);
            let detail = match current {
                Ok(current) if current == layout => "Active layout fingerprint matches.".into(),
                Ok(current) => {
                    format!("Active X11 layout is {current:?}, but the bridge requires {layout:?}.")
                }
                Err(error) => error.to_string(),
            };
            BackendProbe {
                name: "x11".into(),
                available: true,
                authorized: true,
                layout_matches: matches,
                detail: format!(
                    "Authenticated local Unix X11 connection, XTEST and XKB available. {detail} Socket operations have a 500 ms deadline; remote/TCP/SSH DISPLAY is unsupported. No input was synthesized. Repetition uses desktop settings."
                ),
                capabilities: capabilities(),
            }
        }
        Err(error) => BackendProbe::error("x11", error, capabilities()),
    }
}

pub(super) struct X11Backend {
    connection: RustConnection<BoundedStream>,
    held: Vec<(NativeKey, u8)>,
    layout: KeyboardLayout,
}

impl X11Backend {
    pub(super) fn new(layout: KeyboardLayout) -> Result<Self, BackendError> {
        let backend = Self::connect(layout)?;
        backend.require_layout()?;
        Ok(backend)
    }

    fn connect(layout: KeyboardLayout) -> Result<Self, BackendError> {
        if is_wayland_session() {
            return Err(BackendError::Unsupported(
                "XTEST cannot serve native Wayland applications; select uinput".into(),
            ));
        }
        let connection = x11_transport::connect()?;
        connection.stream().begin_operation();
        connection
            .xtest_get_version(2, 2)
            .map_err(|e| unavailable("XTEST extension unavailable", e))?
            .reply()
            .map_err(|e| unavailable("XTEST extension unavailable", e))?;
        let extension = connection
            .xkb_use_extension(1, 0)
            .map_err(|e| unavailable("XKB extension unavailable", e))?
            .reply()
            .map_err(|e| unavailable("XKB extension unavailable", e))?;
        if !extension.supported {
            return Err(BackendError::Unavailable(
                "XKB 1.0 is required to verify the active layout group".into(),
            ));
        }
        Ok(Self {
            connection,
            held: Vec::new(),
            layout,
        })
    }

    fn mapping(&self) -> Result<GetKeyboardMappingReply, BackendError> {
        let setup = self.connection.setup();
        let first = setup.min_keycode;
        let count = setup.max_keycode - first + 1;
        // Read mappings, not pressed-key state. Resolve on every new press so a
        // desktop layout change cannot leave cached key-up identities stale.
        self.connection
            .get_keyboard_mapping(first, count)
            .map_err(|e| unavailable("Cannot read X11 keyboard mapping", e))?
            .reply()
            .map_err(|e| unavailable("Cannot read X11 keyboard mapping", e))
    }

    fn active_layout(&self) -> Result<KeyboardLayout, BackendError> {
        self.connection.stream().begin_operation();
        let state = self
            .connection
            .xkb_get_state(ID::USE_CORE_KBD.into())
            .map_err(|e| unavailable("Cannot read active XKB layout group", e))?
            .reply()
            .map_err(|e| unavailable("Cannot read active XKB layout group", e))?;
        // The core mapping's first two columns describe group zero. Refuse a
        // switched group rather than treating the first layout as still active.
        if u8::from(state.group) != 0 {
            return Err(BackendError::Unsupported(
                "X11 layout verification requires the matching US/UK layout in active group zero"
                    .into(),
            ));
        }
        let mapping = self.mapping()?;
        let names = self
            .connection
            .xkb_get_names(ID::USE_CORE_KBD.into(), NameDetail::KEY_NAMES)
            .map_err(|e| unavailable("Cannot read XKB physical key names", e))?
            .reply()
            .map_err(|e| unavailable("Cannot read XKB physical key names", e))?;
        let key_names = names.value_list.key_names.as_deref().ok_or_else(|| {
            BackendError::Unavailable("XKB physical key names are unavailable".into())
        })?;
        layout_from_x11_mapping(self.connection.setup().min_keycode, &mapping.keysyms, usize::from(mapping.keysyms_per_keycode), names.first_key, key_names)
            .ok_or_else(|| BackendError::Unsupported("Active X11 map has an unknown US/UK punctuation fingerprint; select standard US or UK without dead-key variants".into()))
    }

    fn require_layout(&self) -> Result<(), BackendError> {
        let current = self.active_layout()?;
        if current != self.layout {
            return Err(BackendError::Unsupported(format!(
                "Active X11 layout is {current:?}, but the bridge requires {:?}",
                self.layout
            )));
        }
        Ok(())
    }

    fn resolve(&self, key: NativeKey) -> Result<u8, BackendError> {
        let first = self.connection.setup().min_keycode;
        let mapping = self.mapping()?;
        let stride = usize::from(mapping.keysyms_per_keycode);
        if stride == 0 {
            return Err(BackendError::Unavailable(
                "X11 returned an empty keyboard mapping".into(),
            ));
        }
        let symbol = resolve_keysym(key, self.layout);
        mapping.keysyms.chunks(stride)
            .position(|symbols| symbols.first() == Some(&symbol))
            .and_then(|index| first.checked_add(u8::try_from(index).ok()?))
            .ok_or_else(|| BackendError::Unsupported("Required unshifted key is absent from the current X11 map; select a matching US/UK host layout".into()))
    }

    fn emit(&self, code: u8, down: bool) -> Result<(), BackendError> {
        self.connection.stream().begin_operation();
        self.connection
            .xtest_fake_input(
                if down {
                    KEY_PRESS_EVENT
                } else {
                    KEY_RELEASE_EVENT
                },
                code,
                x11rb::CURRENT_TIME,
                x11rb::NONE,
                0,
                0,
                0,
            )
            .map_err(|e| BackendError::Injection(format!("XTEST request failed: {e}")))?
            .check()
            .map_err(|e| BackendError::Injection(format!("XTEST server rejected input: {e}")))?;
        self.connection
            .flush()
            .map_err(|e| BackendError::Injection(format!("X11 flush failed: {e}")))
    }
}

impl KeyboardBackend for X11Backend {
    fn capabilities(&self) -> BackendCapabilities {
        capabilities()
    }

    fn validate_layout(&mut self) -> Result<(), BackendError> {
        self.require_layout()
    }

    fn press(&mut self, key: NativeKey) -> Result<(), BackendError> {
        if self.held.iter().any(|(held, _)| *held == key) {
            return Ok(());
        }
        self.require_layout()?;
        let code = self.resolve(key)?;
        // Distinct logical keys may resolve to one X11 code (ISO backslash).
        // Keep ownership until the last logical owner releases that code.
        if !self.held.iter().any(|(_, held_code)| *held_code == code) {
            // Conservatively retain ownership on a transport error: the server
            // may have received a press before the connection error appeared.
            self.held.push((key, code));
            self.emit(code, true)?;
        } else {
            self.held.push((key, code));
        }
        Ok(())
    }

    fn release(&mut self, key: NativeKey) -> Result<(), BackendError> {
        let Some((_, code)) = self.held.iter().find(|(held, _)| *held == key).copied() else {
            return Ok(());
        };
        if self
            .held
            .iter()
            .filter(|(_, held_code)| *held_code == code)
            .count()
            == 1
        {
            self.emit(code, false)?;
        }
        self.held.retain(|(held, _)| *held != key);
        Ok(())
    }

    fn repeat(&mut self, _key: NativeKey) -> Result<(), BackendError> {
        Err(BackendError::Unsupported(
            "X11 uses native desktop repeat; host-generated repeats are disabled".into(),
        ))
    }
}

impl Drop for X11Backend {
    fn drop(&mut self) {
        let keys: Vec<_> = self.held.iter().map(|(key, _)| *key).collect();
        for key in release_order(&keys) {
            let _ = self.release(key);
        }
    }
}

// Linux generic ioctl encoding, valid for the supported x86_64/aarch64 targets.
const fn ioctl_write(number: u8, size: usize) -> libc::c_ulong {
    ((1_u64 << 30) | ((size as u64) << 16) | ((b'U' as u64) << 8) | number as u64) as libc::c_ulong
}
const UI_SET_EVBIT: libc::c_ulong = ioctl_write(100, std::mem::size_of::<libc::c_int>());
const UI_SET_KEYBIT: libc::c_ulong = ioctl_write(101, std::mem::size_of::<libc::c_int>());
const UI_DEV_SETUP: libc::c_ulong = ioctl_write(3, std::mem::size_of::<libc::uinput_setup>());
const UI_DEV_CREATE: libc::c_ulong = 0x5501;
const UI_DEV_DESTROY: libc::c_ulong = 0x5502;
const EV_KEY: u16 = 1;
const EV_REP: u16 = 20;

fn open_uinput() -> Result<File, BackendError> {
    let mut last = None;
    for path in ["/dev/uinput", "/dev/input/uinput"] {
        match OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)
        {
            Ok(file) => return Ok(file),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                return Err(BackendError::PermissionDenied("uinput device exists but access is denied. Grant this user access to /dev/uinput using an explicit administrator-configured device rule; do not run the bridge network server as root.".into()));
            }
            Err(error) => last = Some(error),
        }
    }
    Err(unavailable(
        "No usable uinput device; enable the uinput kernel module and explicitly grant device access",
        last.expect("two paths checked"),
    ))
}

const VIRTUAL_KEYBOARD_NAME: &str = "C64 Keyboard Bridge";

fn compositor_command() -> Result<Command, BackendError> {
    let sway = std::env::var_os("SWAYSOCK").is_some_and(|value| !value.is_empty());
    let hyprland =
        std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some_and(|value| !value.is_empty());
    if !is_wayland_session() || (sway && hyprland) {
        return Err(BackendError::Unsupported("uinput requires an unambiguous current Wayland compositor session; use XTEST in X11 or remove stale nested-compositor environment variables".into()));
    }
    if sway {
        let mut command = Command::new("swaymsg");
        command.args(["-r", "-t", "get_inputs"]);
        Ok(command)
    } else if hyprland {
        let mut command = Command::new("hyprctl");
        command.args(["devices", "-j"]);
        Ok(command)
    } else {
        Err(BackendError::Unsupported("uinput layout verification currently requires a Sway or Hyprland session with its read-only query tool. Unknown Wayland compositors are refused; use X11/XTEST in a true X11 session.".into()))
    }
}

fn bounded_compositor_query(mut command: Command) -> Result<serde_json::Value, BackendError> {
    struct ChildGuard(std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| unavailable("Cannot run the compositor's read-only layout query", e))?;
    let mut child = ChildGuard(child);
    let mut stdout = child.0.stdout.take().expect("stdout pipe requested");
    // Never wait on a child with a full pipe or leave cleanup behind an
    // unbounded command. No shell is involved and stderr is not logged.
    let flags = unsafe { libc::fcntl(stdout.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(stdout.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(unavailable(
            "Cannot bound compositor query output",
            std::io::Error::last_os_error(),
        ));
    }
    let deadline = Instant::now() + Duration::from_millis(500);
    let mut bytes = Vec::new();
    let mut finished: Option<bool> = None;
    loop {
        if Instant::now() >= deadline {
            return Err(BackendError::Unavailable(
                "Compositor layout query exceeded its 500 ms deadline".into(),
            ));
        }
        let mut buffer = [0; 4096];
        loop {
            match stdout.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    if bytes.len() + count > 65_536 {
                        return Err(BackendError::Unavailable(
                            "Compositor layout query exceeded its 64 KiB limit".into(),
                        ));
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    return Err(unavailable("Cannot read compositor layout query", error));
                }
            }
        }
        if let Some(status) = finished {
            if !status {
                return Err(BackendError::Unavailable(
                    "Compositor rejected its read-only layout query".into(),
                ));
            }
            return serde_json::from_slice(&bytes).map_err(|_| {
                BackendError::Unavailable("Compositor layout query returned invalid JSON".into())
            });
        }
        if let Some(status) = child
            .0
            .try_wait()
            .map_err(|e| unavailable("Cannot poll compositor layout query", e))?
        {
            // Drain once more after observing exit, without blocking on a pipe
            // retained by a descendant. The next iteration keeps the deadline.
            finished = Some(status.success());
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn layout_from_compositor(value: &serde_json::Value) -> Result<KeyboardLayout, BackendError> {
    let (devices, field) = if let Some(inputs) = value.as_array() {
        (inputs, "xkb_active_layout_name")
    } else if let Some(keyboards) = value.get("keyboards").and_then(|v| v.as_array()) {
        (keyboards, "active_keymap")
    } else {
        return Err(BackendError::Unavailable(
            "Compositor query did not describe keyboard devices".into(),
        ));
    };
    let mut selected = devices.iter().filter(|device| {
        matches!(
            device.get("name").and_then(|v| v.as_str()),
            Some(VIRTUAL_KEYBOARD_NAME | "c64-keyboard-bridge")
        )
    });
    let device = selected.next().ok_or_else(|| BackendError::Unavailable("The compositor has not exposed the bridge virtual keyboard; its layout cannot yet be verified".into()))?;
    if selected.next().is_some() {
        return Err(BackendError::Unsupported(
            "Multiple bridge virtual keyboards prevent unambiguous layout verification".into(),
        ));
    }
    match device.get(field).and_then(|v| v.as_str()) {
        Some("English (US)") => Ok(KeyboardLayout::Us),
        Some("English (UK)") => Ok(KeyboardLayout::Uk),
        _ => Err(BackendError::Unsupported("The bridge virtual keyboard has an unknown active layout; configure standard us or gb without variants in the compositor".into())),
    }
}

fn compositor_layout() -> Result<KeyboardLayout, BackendError> {
    layout_from_compositor(&bounded_compositor_query(compositor_command()?)?)
}

pub(super) fn probe_uinput(layout: KeyboardLayout) -> BackendProbe {
    match open_uinput() {
        Ok(_) => {
            let current = compositor_layout();
            let matches = current.as_ref().ok().map(|current| *current == layout);
            let detail = match current {
                Ok(current) if current == layout => {
                    "Existing bridge virtual keyboard layout matches.".into()
                }
                Ok(current) => format!(
                    "Existing bridge keyboard layout is {current:?}, but the bridge requires {layout:?}."
                ),
                Err(error) => error.to_string(),
            };
            BackendProbe {
                name: "uinput".into(),
                available: true,
                authorized: true,
                layout_matches: matches,
                detail: format!(
                    "uinput device can be opened; no virtual device or input was created. {detail} Enabling creates the device, then verifies its actual compositor layout before posting any key. Desktop acceptance and native repeat require an explicit focused-application test."
                ),
                capabilities: capabilities(),
            }
        }
        Err(error) => BackendProbe::error("uinput", error, capabilities()),
    }
}

pub(super) struct UinputBackend {
    file: File,
    created: bool,
    held: Vec<NativeKey>,
    layout: KeyboardLayout,
    last_layout_check: Option<Instant>,
}

impl UinputBackend {
    pub(super) fn new(layout: KeyboardLayout) -> Result<Self, BackendError> {
        // Refuse unsupported sessions before creating a device. A matching
        // per-device layout can be checked only after the compositor sees it.
        compositor_command()?;
        let file = open_uinput()?;
        let mut backend = Self {
            file,
            created: false,
            held: Vec::new(),
            layout,
            last_layout_check: None,
        };
        backend.set_bit(UI_SET_EVBIT, EV_KEY)?;
        backend.set_bit(UI_SET_EVBIT, EV_REP)?;
        // A bounded normal keyboard capability range includes all mapped keys
        // and allows desktops to classify the virtual device as a keyboard.
        for code in 1..=125 {
            backend.set_bit(UI_SET_KEYBIT, code)?;
        }
        let mut setup: libc::uinput_setup = unsafe { std::mem::zeroed() };
        setup.id.bustype = 0x06; // BUS_VIRTUAL; no physical device is impersonated.
        setup.id.version = 1;
        for (dest, source) in setup.name.iter_mut().zip(VIRTUAL_KEYBOARD_NAME.bytes()) {
            *dest = source as libc::c_char;
        }
        if unsafe { libc::ioctl(backend.file.as_raw_fd(), UI_DEV_SETUP, &setup) } < 0 {
            return Err(unavailable(
                "uinput UI_DEV_SETUP failed (modern uinput API required)",
                std::io::Error::last_os_error(),
            ));
        }
        if unsafe { libc::ioctl(backend.file.as_raw_fd(), UI_DEV_CREATE) } < 0 {
            return Err(unavailable(
                "uinput virtual keyboard creation failed",
                std::io::Error::last_os_error(),
            ));
        }
        backend.created = true;
        // Device creation precedes desktop/udev initialization. This startup
        // grace period emits no keys; application delivery remains unverified.
        std::thread::sleep(std::time::Duration::from_secs(1));
        backend.require_layout()?;
        Ok(backend)
    }

    fn require_layout(&mut self) -> Result<(), BackendError> {
        if self
            .last_layout_check
            .is_some_and(|checked| checked.elapsed() < Duration::from_millis(100))
        {
            return Ok(());
        }
        self.last_layout_check = None;
        let current = compositor_layout()?;
        if current != self.layout {
            return Err(BackendError::Unsupported(format!(
                "Bridge virtual keyboard layout is {current:?}, but the bridge requires {:?}",
                self.layout
            )));
        }
        self.last_layout_check = Some(Instant::now());
        Ok(())
    }

    fn set_bit(&mut self, request: libc::c_ulong, code: u16) -> Result<(), BackendError> {
        if unsafe { libc::ioctl(self.file.as_raw_fd(), request, libc::c_int::from(code)) } < 0 {
            return Err(unavailable(
                "uinput capability setup failed",
                std::io::Error::last_os_error(),
            ));
        }
        Ok(())
    }

    fn emit(&mut self, key: NativeKey, value: i32) -> Result<(), BackendError> {
        // Zeroed padding/time prevents exposing uninitialized process memory.
        let mut packet: [libc::input_event; 2] = unsafe { std::mem::zeroed() };
        packet[0].type_ = EV_KEY;
        packet[0].code = linux_keycode(key);
        packet[0].value = value;
        // packet[1] is EV_SYN / SYN_REPORT / 0.
        let bytes = unsafe {
            std::slice::from_raw_parts(packet.as_ptr().cast::<u8>(), std::mem::size_of_val(&packet))
        };
        self.file
            .write_all(bytes)
            .map_err(|e| BackendError::Injection(format!("uinput event packet failed: {e}")))
    }
}

impl KeyboardBackend for UinputBackend {
    fn capabilities(&self) -> BackendCapabilities {
        capabilities()
    }

    fn validate_layout(&mut self) -> Result<(), BackendError> {
        self.require_layout()
    }

    fn press(&mut self, key: NativeKey) -> Result<(), BackendError> {
        if self.held.contains(&key) {
            return Ok(());
        }
        self.require_layout()?;
        // Keep cleanup ownership even when a packet is only partially accepted.
        self.held.push(key);
        self.emit(key, 1)
    }

    fn release(&mut self, key: NativeKey) -> Result<(), BackendError> {
        if !self.held.contains(&key) {
            return Ok(());
        }
        self.emit(key, 0)?;
        self.held.retain(|held| *held != key);
        Ok(())
    }

    fn repeat(&mut self, _key: NativeKey) -> Result<(), BackendError> {
        Err(BackendError::Unsupported(
            "uinput uses native desktop repeat; host-generated repeats are disabled".into(),
        ))
    }
}

impl Drop for UinputBackend {
    fn drop(&mut self) {
        if self.created {
            for key in release_order(&self.held) {
                let _ = self.release(key);
            }
            unsafe {
                libc::ioctl(self.file.as_raw_fd(), UI_DEV_DESTROY);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers_match_linux_generic_uapi() {
        assert_eq!(UI_SET_EVBIT, 0x4004_5564);
        assert_eq!(UI_SET_KEYBIT, 0x4004_5565);
        assert_eq!(std::mem::size_of::<libc::uinput_setup>(), 92);
        assert_eq!(UI_DEV_SETUP, 0x405c_5503);
    }

    #[test]
    fn uk_backslash_slot_is_distinct_from_iso_backslash() {
        assert_eq!(
            resolve_keysym(NativeKey::Backslash, KeyboardLayout::Uk),
            u32::from(b'#')
        );
        assert_eq!(
            resolve_keysym(NativeKey::IntlBackslash, KeyboardLayout::Uk),
            u32::from(b'\\')
        );
        assert_eq!(
            resolve_keysym(NativeKey::Backslash, KeyboardLayout::Us),
            u32::from(b'\\')
        );
    }

    #[test]
    fn physical_x11_fingerprints_reject_rearranged_and_dead_key_maps() {
        let mut names = Vec::new();
        let mut symbols = Vec::new();
        for (row, letters) in [
            (b"AD", b"qwertyuiop".as_slice()),
            (b"AC", b"asdfghjkl".as_slice()),
            (b"AB", b"zxcvbnm".as_slice()),
        ] {
            for (index, letter) in letters.iter().copied().enumerate() {
                names.push(KeyName {
                    name: [
                        row[0],
                        row[1],
                        b'0' + ((index + 1) / 10) as u8,
                        b'0' + ((index + 1) % 10) as u8,
                    ],
                });
                symbols.extend([u32::from(letter), u32::from(letter.to_ascii_uppercase())]);
            }
        }
        for (name, pair) in [
            (b"AE02", [50, 64]),
            (b"AE03", [51, 35]),
            (b"AC11", [39, 34]),
            (b"TLDE", [96, 126]),
            (b"BKSL", [92, 124]),
            (b"LSGT", [92, 124]),
        ] {
            names.push(KeyName { name: *name });
            symbols.extend(pair);
        }
        assert_eq!(
            layout_from_x11_mapping(8, &symbols, 2, 8, &names),
            Some(KeyboardLayout::Us)
        );
        // Same punctuation alone does not establish a physical QWERTY layout.
        symbols[0] = u32::from(b'x');
        assert_eq!(layout_from_x11_mapping(8, &symbols, 2, 8, &names), None);
        symbols[0] = u32::from(b'q');
        let punctuation = 26 * 2;
        symbols[punctuation..punctuation + 10]
            .copy_from_slice(&[50, 34, 51, 163, 39, 64, 96, 172, 35, 126]);
        assert_eq!(
            layout_from_x11_mapping(8, &symbols, 2, 8, &names),
            Some(KeyboardLayout::Uk)
        );
        symbols[punctuation + 4] = 0xfe51; // dead_acute is not apostrophe.
        assert_eq!(layout_from_x11_mapping(8, &symbols, 2, 8, &names), None);
        assert_eq!(layout_from_x11_mapping(8, &symbols, 0, 8, &names), None);
    }

    #[test]
    fn compositor_guard_selects_only_its_unique_device_and_rejects_variants() {
        let inputs = serde_json::json!([
            {"name":"Other keyboard", "xkb_active_layout_name":"English (UK)"},
            {"name":"C64 Keyboard Bridge", "xkb_active_layout_name":"English (US)"}
        ]);
        assert_eq!(layout_from_compositor(&inputs).unwrap(), KeyboardLayout::Us);
        let hypr = serde_json::json!({"keyboards":[{"name":"c64-keyboard-bridge", "active_keymap":"English (UK)"}]});
        assert_eq!(layout_from_compositor(&hypr).unwrap(), KeyboardLayout::Uk);
        let variant = serde_json::json!([{"name":"C64 Keyboard Bridge", "xkb_active_layout_name":"English (US, intl., with dead keys)"}]);
        assert!(layout_from_compositor(&variant).is_err());
        let duplicate = serde_json::json!([
            {"name":"C64 Keyboard Bridge", "xkb_active_layout_name":"English (US)"},
            {"name":"C64 Keyboard Bridge", "xkb_active_layout_name":"English (US)"}
        ]);
        assert!(layout_from_compositor(&duplicate).is_err());
        assert!(layout_from_compositor(&serde_json::json!([])).is_err());
    }

    #[test]
    fn read_only_child_queries_are_bounded() {
        let mut stalled = Command::new("sh");
        stalled.args(["-c", "exec sleep 2"]);
        let started = Instant::now();
        assert!(
            bounded_compositor_query(stalled)
                .unwrap_err()
                .to_string()
                .contains("500 ms")
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        let mut oversized = Command::new("sh");
        oversized.args(["-c", "exec yes x"]);
        assert!(
            bounded_compositor_query(oversized)
                .unwrap_err()
                .to_string()
                .contains("64 KiB")
        );
        let mut valid = Command::new("sh");
        valid.args(["-c", "printf '[]'"]);
        assert_eq!(
            bounded_compositor_query(valid).unwrap(),
            serde_json::json!([])
        );
    }
}

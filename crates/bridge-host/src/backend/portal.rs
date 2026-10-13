//! Keyboard-only GNOME Wayland RemoteDesktop/EIS sender. This never opens a
//! ScreenCast or InputCapture session, requests a pointer, or reads host keys.
use super::{
    BackendProbe,
    codes::{linux_keycode, release_order},
    linux,
};
use ashpd::desktop::{
    CreateSessionOptions, PersistMode, Session,
    remote_desktop::{
        ConnectToEISOptions, DeviceType, RemoteDesktop, SelectDevicesOptions, StartOptions,
    },
};
use bridge_core::{BackendCapabilities, BackendError, KeyboardBackend, KeyboardLayout, NativeKey};
use futures_util::StreamExt;
use reis::{PendingRequestResult, ei, tokio::EiEventStream};
use std::{
    collections::HashMap,
    fs::File,
    io,
    os::{
        fd::OwnedFd,
        unix::{fs::FileExt, net::UnixStream},
    },
    ptr,
    sync::{Arc, Mutex, mpsc as sync_mpsc},
    thread,
    time::Duration,
};
use tokio::{
    sync::{mpsc, oneshot, watch},
    time::{Instant, timeout},
};

const OPERATION_TIMEOUT: Duration = Duration::from_millis(500);
const CONSENT_TIMEOUT: Duration = Duration::from_secs(120);
const SETUP_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_KEYMAP_BYTES: usize = 1024 * 1024;
const MAX_DEVICES: usize = 8;
const COMMAND_QUEUE: usize = 64;

fn capabilities() -> BackendCapabilities {
    BackendCapabilities {
        key_events: true,
        text: false,
        repeat: false,
    }
}

pub(super) fn is_gnome_wayland() -> bool {
    linux::is_wayland_session()
        && !["SWAYSOCK", "HYPRLAND_INSTANCE_SIGNATURE"]
            .iter()
            .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
        && ["XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP"]
            .iter()
            .any(|name| {
                std::env::var(name).is_ok_and(|value| {
                    value.split(':').any(|part| {
                        part.eq_ignore_ascii_case("gnome") || part.eq_ignore_ascii_case("ubuntu")
                    })
                })
            })
}

fn debug_is_disabled() -> Result<(), BackendError> {
    if std::env::var_os("REIS_DEBUG").is_some_and(|value| !value.is_empty()) {
        return Err(BackendError::Unsupported(
            "Unset REIS_DEBUG before using desktop keyboard input; raw EIS logging is disabled"
                .into(),
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    Cancelled,
    Consent,
    Unsupported,
    Layout,
    LayoutMismatch,
    LayoutMetadata,
    Revoked,
    Transport,
    Protocol,
    Timeout,
    Overflow,
}
impl Failure {
    fn error(self) -> BackendError {
        let text = match self {
            Self::Cancelled => "Desktop keyboard authorization cancelled",
            Self::Consent => {
                "Desktop keyboard permission was not granted; enable again in the posting process"
            }
            Self::Unsupported => {
                "GNOME Wayland RemoteDesktop version 2 with keyboard/EIS support is required"
            }
            Self::Layout => {
                "Active desktop keyboard map is not standard US/UK; select the configured standard layout and enable again"
            }
            Self::LayoutMismatch => {
                "Active desktop keyboard layout differs from the configured US/UK layout; select the matching input source and enable again"
            }
            Self::LayoutMetadata => {
                "The compositor did not provide usable keyboard map/group metadata; input remains disabled"
            }
            Self::Revoked => {
                "Desktop keyboard session paused or revoked; release C64 keys and explicitly enable again"
            }
            Self::Transport => "Desktop keyboard transport failed; input disabled",
            Self::Protocol => {
                "Desktop keyboard service returned an invalid or unsupported EIS response"
            }
            Self::Timeout => "Desktop keyboard operation exceeded its deadline; input disabled",
            Self::Overflow => "Desktop keyboard queue capacity exceeded; input disabled",
        };
        match self {
            Self::Consent => BackendError::PermissionDenied(text.into()),
            Self::Layout | Self::LayoutMismatch | Self::LayoutMetadata | Self::Unsupported => {
                BackendError::Unsupported(text.into())
            }
            _ => BackendError::Unavailable(text.into()),
        }
    }
}

#[derive(Default)]
struct Health {
    failure: Option<Failure>,
    ready: bool,
    neutralized: bool,
}
type SharedHealth = Arc<Mutex<Health>>;

pub(super) fn probe(_layout: KeyboardLayout) -> BackendProbe {
    let result = debug_is_disabled().and_then(|()| {
        if !is_gnome_wayland() {
            return Err(Failure::Unsupported.error());
        }
        thread::spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| Failure::Transport.error())?;
            runtime.block_on(async {
                timeout(SETUP_TIMEOUT, async {
                    let bus = zbus::Connection::session()
                        .await
                        .map_err(|_| Failure::Transport.error())?;
                    let result = async {
                        let portal = RemoteDesktop::with_connection(bus.clone())
                            .await
                            .map_err(|_| Failure::Unsupported.error())?;
                        check_portal(&portal).await.map_err(Failure::error)
                    }
                    .await;
                    let _ = timeout(OPERATION_TIMEOUT, bus.close()).await;
                    result
                })
                .await
                .map_err(|_| Failure::Timeout.error())?
            })
        })
        .join()
        .map_err(|_| Failure::Transport.error())?
    });
    match result {
        Ok(()) => BackendProbe {
            name: "portal".into(), available: true, authorized: false, layout_matches: None,
            detail: "GNOME keyboard-only RemoteDesktop/EIS is available. Enable requests desktop consent in the posting process; doctor does not prompt. Active layout is verified after authorization. No root or uinput access is needed. Desktop settings own repeat.".into(),
            capabilities: capabilities(),
        },
        Err(error) => BackendProbe::error("portal", error, capabilities()),
    }
}

async fn check_portal(portal: &RemoteDesktop) -> Result<(), Failure> {
    if portal.version() < 2
        || !portal
            .available_device_types()
            .await
            .map_err(|_| Failure::Unsupported)?
            .contains(DeviceType::Keyboard)
    {
        return Err(Failure::Unsupported);
    }
    Ok(())
}

async fn cancelled(cancellation: &mut watch::Receiver<bool>) {
    loop {
        if *cancellation.borrow() {
            return;
        }
        if cancellation.changed().await.is_err() {
            return;
        }
    }
}

async fn cancellable<T>(
    cancellation: &mut watch::Receiver<bool>,
    limit: Duration,
    operation: impl std::future::Future<Output = Result<T, Failure>>,
) -> Result<T, Failure> {
    tokio::select! {
        biased;
        _ = cancelled(cancellation) => Err(Failure::Cancelled),
        result = timeout(limit, operation) => result.map_err(|_| Failure::Timeout)?,
    }
}

struct PortalSession {
    // Dedicated connection: closing it also closes any in-flight portal request
    // if cancellation arrived before CreateSession returned its handle.
    bus: zbus::Connection,
    session: Option<Session<RemoteDesktop>>,
}
impl PortalSession {
    async fn close(&mut self) -> bool {
        let session_closed = if let Some(session) = self.session.as_ref() {
            matches!(
                timeout(OPERATION_TIMEOUT, session.close()).await,
                Ok(Ok(()))
            )
        } else {
            false
        };
        let bus_closed = matches!(
            timeout(OPERATION_TIMEOUT, self.bus.clone().close()).await,
            Ok(Ok(()))
        );
        if session_closed || bus_closed {
            self.session.take();
        }
        session_closed || bus_closed
    }
}

trait CloseAuthority {
    async fn close(&mut self) -> bool;
}
impl CloseAuthority for PortalSession {
    async fn close(&mut self) -> bool {
        PortalSession::close(self).await
    }
}

async fn authorize(
    cancellation: &mut watch::Receiver<bool>,
) -> Result<(PortalSession, UnixStream), Failure> {
    let bus = cancellable(cancellation, SETUP_TIMEOUT, async {
        zbus::Connection::session()
            .await
            .map_err(|_| Failure::Transport)
    })
    .await?;
    authorize_on_bus(cancellation, bus).await
}

async fn authorize_on_bus(
    cancellation: &mut watch::Receiver<bool>,
    bus: zbus::Connection,
) -> Result<(PortalSession, UnixStream), Failure> {
    let mut owner = PortalSession { bus, session: None };
    let result = async {
        let portal = cancellable(cancellation, SETUP_TIMEOUT, async {
            RemoteDesktop::with_connection(owner.bus.clone())
                .await
                .map_err(|_| Failure::Unsupported)
        })
        .await?;
        cancellable(cancellation, SETUP_TIMEOUT, check_portal(&portal)).await?;
        let session = cancellable(cancellation, SETUP_TIMEOUT, async {
            portal
                .create_session(CreateSessionOptions::default())
                .await
                .map_err(|_| Failure::Consent)
        })
        .await?;
        owner.session = Some(session);
        let session = owner.session.as_ref().expect("created portal session");
        cancellable(cancellation, SETUP_TIMEOUT, async {
            portal
                .select_devices(
                    session,
                    SelectDevicesOptions::default()
                        .set_devices(reis::enumflags2::BitFlags::from(DeviceType::Keyboard))
                        .set_persist_mode(PersistMode::DoNot),
                )
                .await
                .map_err(|_| Failure::Consent)?
                .response()
                .map_err(|_| Failure::Consent)
        })
        .await?;
        let selected = cancellable(cancellation, CONSENT_TIMEOUT, async {
            portal
                .start(session, None, StartOptions::default())
                .await
                .map_err(|_| Failure::Consent)?
                .response()
                .map_err(|_| Failure::Consent)
        })
        .await?;
        // Reject unexpected grants rather than acquiring any unrelated service.
        if selected.devices() != DeviceType::Keyboard
            || !selected.streams().is_empty()
            || selected.is_clipboard_enabled()
        {
            return Err(Failure::Consent);
        }
        let fd = cancellable(cancellation, SETUP_TIMEOUT, async {
            portal
                .connect_to_eis(session, ConnectToEISOptions::default())
                .await
                .map_err(|_| Failure::Transport)
        })
        .await?;
        Ok(UnixStream::from(fd))
    }
    .await;
    match result {
        Ok(stream) => Ok((owner, stream)),
        Err(error) => {
            owner.close().await;
            Err(error)
        }
    }
}

enum Operation {
    Validate,
    Press(NativeKey),
    Release(NativeKey),
}
struct Command {
    operation: Operation,
    reply: sync_mpsc::Sender<Result<(), Failure>>,
}

/// A separate owned thread keeps OS consent and EIS traffic off both the GUI
/// thread and ordered input worker. Drop closes the session and joins cleanup.
pub(super) async fn prepare(
    layout: KeyboardLayout,
    cancellation: watch::Receiver<bool>,
) -> Result<Box<dyn KeyboardBackend>, BackendError> {
    debug_is_disabled()?;
    if !is_gnome_wayland() {
        return Err(Failure::Unsupported.error());
    }
    let (commands, receiver) = mpsc::channel(COMMAND_QUEUE);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let (abort_output, failure_rx) = watch::channel(None);
    let (ready_tx, ready_rx) = oneshot::channel();
    let health = Arc::new(Mutex::new(Health::default()));
    let actor_health = health.clone();
    let thread = thread::Builder::new()
        .name("c64-keyboard-portal".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(run_actor(
                    layout,
                    cancellation,
                    ActorControls {
                        shutdown: shutdown_rx,
                        failure: failure_rx,
                    },
                    receiver,
                    ready_tx,
                    actor_health,
                )),
                Err(_) => {
                    let _ = ready_tx.send(Err(Failure::Transport));
                }
            }
        })
        .map_err(|_| Failure::Transport.error())?;
    let backend = PortalBackend {
        commands,
        shutdown,
        abort_output,
        thread: Some(thread),
        health,
        held: Vec::new(),
    };
    match ready_rx.await {
        Ok(Ok(())) => Ok(Box::new(backend)),
        Ok(Err(error)) => Err(error.error()),
        Err(_) => Err(Failure::Transport.error()),
    }
}

struct PortalBackend {
    commands: mpsc::Sender<Command>,
    shutdown: watch::Sender<bool>,
    abort_output: watch::Sender<Option<Failure>>,
    thread: Option<thread::JoinHandle<()>>,
    health: SharedHealth,
    held: Vec<NativeKey>,
}
impl PortalBackend {
    fn fail_output(&self, failure: Failure) {
        self.health.lock().unwrap().failure.get_or_insert(failure);
        let _ = self.abort_output.send(Some(failure));
    }
    fn operation(&self, operation: Operation) -> Result<(), BackendError> {
        let (reply, response) = sync_mpsc::channel();
        self.commands
            .try_send(Command { operation, reply })
            .map_err(|error| {
                let failure = match error {
                    mpsc::error::TrySendError::Full(_) => Failure::Overflow,
                    _ => Failure::Transport,
                };
                self.fail_output(failure);
                failure.error()
            })?;
        response
            .recv_timeout(OPERATION_TIMEOUT + Duration::from_millis(100))
            .map_err(|_| {
                self.fail_output(Failure::Timeout);
                Failure::Timeout.error()
            })?
            .map_err(Failure::error)
    }
}
impl KeyboardBackend for PortalBackend {
    fn capabilities(&self) -> BackendCapabilities {
        capabilities()
    }
    fn validate_layout(&mut self) -> Result<(), BackendError> {
        {
            let health = self.health.lock().unwrap();
            if let Some(error) = health.failure {
                return Err(error.error());
            }
            if !health.ready {
                return Err(Failure::Revoked.error());
            }
        }
        self.operation(Operation::Validate)
    }
    fn press(&mut self, key: NativeKey) -> Result<(), BackendError> {
        self.validate_layout()?;
        if self.held.contains(&key) {
            return Ok(());
        }
        // Retain ambiguous write ownership until release or confirmed removal.
        self.held.push(key);
        self.operation(Operation::Press(key))
    }
    fn release(&mut self, key: NativeKey) -> Result<(), BackendError> {
        if !self.held.contains(&key) {
            return Ok(());
        }
        let neutralized = self.health.lock().unwrap().neutralized;
        if !neutralized && let Err(error) = self.operation(Operation::Release(key)) {
            // A prior cleanup retry may have confirmed every output and
            // ended the actor while this release was queued. Its receipt
            // applies to this backend generation, so that evidence wins.
            if !self.health.lock().unwrap().neutralized {
                return Err(error);
            }
        }
        self.held.retain(|held| *held != key);
        Ok(())
    }
    fn repeat(&mut self, _key: NativeKey) -> Result<(), BackendError> {
        Err(BackendError::Unsupported(
            "GNOME uses native desktop repeat; host-generated repeats are disabled".into(),
        ))
    }
}
impl Drop for PortalBackend {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Device {
    proxy: ei::Device,
    keyboard: Option<ei::Keyboard>,
    layouts: Option<Vec<Option<KeyboardLayout>>>,
    group: Option<u32>,
    complete: bool,
    resumed: bool,
}
struct State {
    expected: KeyboardLayout,
    connection: ei::Connection,
    serial: u32,
    sequence: u32,
    seats: HashMap<ei::Seat, Option<u64>>,
    devices: HashMap<ei::Device, Device>,
    selected: Option<ei::Device>,
    emulating: bool,
    held: Vec<(NativeKey, u16)>,
    initialized: bool,
    revoked: bool,
}
impl State {
    fn handle(&mut self, event: ei::Event) -> Result<Option<ei::Callback>, Failure> {
        match event {
            ei::Event::Connection(_, ei::connection::Event::Seat { seat }) => {
                if self.seats.len() >= 2 {
                    return Err(Failure::Protocol);
                }
                self.seats.insert(seat, None);
            }
            ei::Event::Connection(_, ei::connection::Event::Ping { ping }) => ping.done(0),
            ei::Event::Connection(_, ei::connection::Event::Disconnected { .. }) => {
                return Err(Failure::Revoked);
            }
            ei::Event::Connection(_, ei::connection::Event::InvalidObject { last_serial, .. }) => {
                self.serial = last_serial;
                return Err(Failure::Protocol);
            }
            ei::Event::Seat(seat, event) => match event {
                ei::seat::Event::Capability { mask, interface } if interface == "ei_keyboard" => {
                    *self.seats.get_mut(&seat).ok_or(Failure::Protocol)? = Some(mask);
                }
                ei::seat::Event::Done => {
                    let mask = self
                        .seats
                        .get(&seat)
                        .copied()
                        .flatten()
                        .ok_or(Failure::Unsupported)?;
                    seat.bind(mask);
                }
                ei::seat::Event::Device { device } => {
                    if self.devices.len() >= MAX_DEVICES {
                        return Err(Failure::Protocol);
                    }
                    if self.initialized {
                        return Err(Failure::Revoked);
                    }
                    self.devices.insert(
                        device.clone(),
                        Device {
                            proxy: device,
                            keyboard: None,
                            layouts: None,
                            group: None,
                            complete: false,
                            resumed: false,
                        },
                    );
                }
                ei::seat::Event::Destroyed { serial } => {
                    self.serial = serial;
                    return Err(Failure::Revoked);
                }
                _ => {}
            },
            ei::Event::Device(device, event) => {
                let data = self.devices.get_mut(&device).ok_or(Failure::Protocol)?;
                match event {
                    ei::device::Event::Interface { object } => {
                        if data.complete || data.keyboard.is_some() {
                            return Err(Failure::Protocol);
                        }
                        data.keyboard = object.downcast::<ei::Keyboard>();
                        if data.keyboard.is_none() {
                            return Err(Failure::Protocol);
                        }
                    }
                    ei::device::Event::Done => {
                        data.complete = true;
                        if data.proxy.version() >= 3 {
                            data.proxy.ready();
                        }
                    }
                    ei::device::Event::Resumed { serial } => {
                        self.serial = serial;
                        data.resumed = true;
                    }
                    ei::device::Event::Paused { serial } => {
                        self.serial = serial;
                        data.resumed = false;
                        if self.initialized {
                            // Only the selected device's Paused event proves
                            // that our owned outputs are neutral. An unrelated
                            // device must never bypass selected-key cleanup.
                            if self.selected.as_ref() == Some(&device) {
                                self.held.clear();
                                self.emulating = false;
                                self.revoked = true;
                            }
                            return Err(Failure::Revoked);
                        }
                    }
                    ei::device::Event::Destroyed { serial } => {
                        self.serial = serial;
                        self.devices.remove(&device);
                        if self.initialized {
                            return Err(Failure::Revoked);
                        }
                    }
                    _ => {}
                }
            }
            ei::Event::Keyboard(keyboard, event) => {
                let data = self
                    .devices
                    .values_mut()
                    .find(|device| device.keyboard.as_ref() == Some(&keyboard))
                    .ok_or(Failure::Protocol)?;
                match event {
                    ei::keyboard::Event::Keymap {
                        keymap_type,
                        size,
                        keymap,
                    } => {
                        if data.complete
                            || data.layouts.is_some()
                            || keymap_type != ei::keyboard::KeymapType::Xkb
                        {
                            return Err(Failure::Protocol);
                        }
                        data.layouts = Some(read_keymap(keymap, size)?);
                    }
                    ei::keyboard::Event::Modifiers { serial, group, .. } => {
                        self.serial = serial;
                        data.group = Some(group);
                    }
                    ei::keyboard::Event::Destroyed { .. } => return Err(Failure::Revoked),
                    // A sender must never receive raw host key events.
                    ei::keyboard::Event::Key { .. } => return Err(Failure::Protocol),
                    _ => return Err(Failure::Protocol),
                }
            }
            ei::Event::Callback(callback, ei::callback::Event::Done { callback_data }) => {
                if callback_data != 0 {
                    return Err(Failure::Protocol);
                }
                return Ok(Some(callback));
            }
            _ => {}
        }
        if self.initialized {
            self.require_layout()?;
        }
        Ok(None)
    }
    fn select(&mut self) -> Result<(), Failure> {
        let mut keyboards = self
            .devices
            .values()
            .filter(|data| data.complete && data.resumed && data.keyboard.is_some());
        let keyboard = keyboards.next().ok_or(Failure::Unsupported)?;
        if keyboards.next().is_some() {
            return Err(Failure::Unsupported);
        }
        self.selected = Some(keyboard.proxy.clone());
        self.require_layout()
    }
    fn device(&self) -> Result<&Device, Failure> {
        self.selected
            .as_ref()
            .and_then(|selected| self.devices.get(selected))
            .ok_or(Failure::Revoked)
    }
    fn require_layout(&self) -> Result<(), Failure> {
        let data = self.device()?;
        if !data.resumed {
            return Err(Failure::Revoked);
        }
        let layouts = data.layouts.as_ref().ok_or(Failure::LayoutMetadata)?;
        let group = match data.group {
            Some(group) => group as usize,
            None if layouts.len() == 1 => 0,
            None => return Err(Failure::LayoutMetadata),
        };
        match layouts.get(group) {
            Some(Some(layout)) if *layout == self.expected => {}
            Some(Some(_)) => return Err(Failure::LayoutMismatch),
            Some(None) => return Err(Failure::Layout),
            None => return Err(Failure::LayoutMetadata),
        }
        Ok(())
    }
    fn emit(&mut self, key: NativeKey, down: bool) -> Result<(), Failure> {
        if down {
            self.require_layout()?;
        }
        let code = linux_keycode(key);
        if down && self.held.iter().any(|(held, _)| *held == key) {
            return Ok(());
        }
        if !down && !self.held.iter().any(|(held, _)| *held == key) {
            return Ok(());
        }
        if down && !self.emulating {
            self.sequence = self.sequence.wrapping_add(1);
            self.device()?
                .proxy
                .start_emulating(self.serial, self.sequence);
            self.emulating = true;
        }
        let data = self.device()?;
        let keyboard = data.keyboard.as_ref().ok_or(Failure::Revoked)?;
        keyboard.key(
            u32::from(code),
            if down {
                ei::keyboard::KeyState::Press
            } else {
                ei::keyboard::KeyState::Released
            },
        );
        data.proxy.frame(self.serial, monotonic_micros()?);
        if down {
            self.held.push((key, code));
        } else {
            self.held.retain(|(held, _)| *held != key);
            if self.held.is_empty() && self.emulating {
                self.device()?.proxy.stop_emulating(self.serial);
                self.emulating = false;
            }
        }
        Ok(())
    }
    fn cleanup(&mut self) -> Result<(), Failure> {
        let keys: Vec<_> = self.held.iter().map(|(key, _)| *key).collect();
        for key in release_order(&keys) {
            self.emit(key, false)?;
        }
        if self.emulating {
            self.device()?.proxy.stop_emulating(self.serial);
            self.emulating = false;
        }
        Ok(())
    }
}

fn monotonic_micros() -> Result<u64, Failure> {
    let mut time: libc::timespec = unsafe { std::mem::zeroed() };
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) } != 0 {
        return Err(Failure::Transport);
    }
    Ok((time.tv_sec as u64).saturating_mul(1_000_000) + (time.tv_nsec as u64) / 1_000)
}

fn parse_event(
    event: Option<io::Result<PendingRequestResult<ei::Event>>>,
) -> Result<ei::Event, Failure> {
    match event {
        Some(Ok(PendingRequestResult::Request(event))) => Ok(event),
        Some(Ok(_)) => Err(Failure::Protocol),
        _ => Err(Failure::Transport),
    }
}

async fn flush(context: &ei::Context) -> Result<(), Failure> {
    let deadline = Instant::now() + OPERATION_TIMEOUT;
    loop {
        match context.flush() {
            Ok(()) => return Ok(()),
            Err(error) if error.raw_os_error() == libc::EAGAIN => {
                if Instant::now() >= deadline {
                    return Err(Failure::Timeout);
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            Err(_) => return Err(Failure::Transport),
        }
    }
}
async fn sync(
    state: &mut State,
    context: &ei::Context,
    events: &mut EiEventStream,
) -> Result<(), Failure> {
    let callback = state.connection.sync(1);
    timeout(OPERATION_TIMEOUT, async {
        flush(context).await?;
        loop {
            if state.handle(parse_event(events.next().await)?)? == Some(callback.clone()) {
                return Ok(());
            }
            flush(context).await?;
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| Failure::Timeout)?
}

async fn ready_state(
    context: &ei::Context,
    layout: KeyboardLayout,
) -> Result<(State, EiEventStream), Failure> {
    let mut events = EiEventStream::new(context.clone()).map_err(|_| Failure::Transport)?;
    let handshake = timeout(
        SETUP_TIMEOUT,
        reis::tokio::ei_handshake(
            &mut events,
            "C64 Keyboard Bridge",
            ei::handshake::ContextType::Sender,
        ),
    )
    .await
    .map_err(|_| Failure::Timeout)?
    .map_err(|_| Failure::Protocol)?;
    let mut state = State {
        expected: layout,
        connection: handshake.connection,
        serial: handshake.serial,
        sequence: 0,
        seats: HashMap::new(),
        devices: HashMap::new(),
        selected: None,
        emulating: false,
        held: Vec::new(),
        initialized: false,
        revoked: false,
    };
    timeout(SETUP_TIMEOUT, async {
        loop {
            state.handle(parse_event(events.next().await)?)?;
            flush(context).await?;
            if state
                .devices
                .values()
                .any(|device| device.complete && device.resumed && device.keyboard.is_some())
            {
                break;
            }
        }
        // Initial modifier/group notifications can arrive just after Resumed.
        sync(&mut state, context, &mut events).await?;
        state.select()?;
        state.initialized = true;
        Ok::<(), Failure>(())
    })
    .await
    .map_err(|_| Failure::Timeout)??;
    Ok((state, events))
}

struct ActorControls {
    shutdown: watch::Receiver<bool>,
    failure: watch::Receiver<Option<Failure>>,
}

async fn cleanup_once(
    state: &mut State,
    context: &ei::Context,
    events: &mut EiEventStream,
    authority: &mut impl CloseAuthority,
) -> bool {
    let released = if state.revoked {
        true
    } else {
        match state.cleanup() {
            Ok(()) => sync(state, context, events).await.is_ok(),
            Err(_) => false,
        }
    };
    // A completed roundtrip proves that every preceding release frame was
    // applied. Publish that evidence before potentially slow session teardown.
    if released {
        return true;
    }
    authority.close().await
}

/// Failed output remains release-only while ownership is uncertain. Later
/// Pause/Stop cleanup can retry the same resources; no new press is admitted.
async fn recover_cleanup(
    state: &mut State,
    context: &ei::Context,
    events: &mut EiEventStream,
    authority: &mut impl CloseAuthority,
    commands: &mut mpsc::Receiver<Command>,
    shutdown: &mut watch::Receiver<bool>,
    health: &SharedHealth,
) -> bool {
    let mut neutralized = cleanup_once(state, context, events, authority).await;
    health.lock().unwrap().ready = false;
    if *shutdown.borrow() {
        // If disposal happened during the initial attempt, it already served
        // as the final bounded cleanup before resource destruction.
        health.lock().unwrap().neutralized = neutralized;
        return neutralized;
    }
    loop {
        health.lock().unwrap().neutralized = neutralized;
        if neutralized {
            return true;
        }
        tokio::select! {
            biased;
            _ = cancelled(shutdown) => {
                // Explicit backend disposal gets a final bounded attempt, then
                // owned socket/bus destructors run. Never claim uncertain keys
                // were released and never abort the actor runtime.
                neutralized = cleanup_once(state, context, events, authority).await;
                health.lock().unwrap().neutralized = neutralized;
                return neutralized;
            },
            command = commands.recv() => {
                let Some(command) = command else { return false; };
                let result = match command.operation {
                    Operation::Release(_) => {
                        neutralized = cleanup_once(state, context, events, authority).await;
                        if neutralized { Ok(()) } else { Err(Failure::Transport) }
                    },
                    Operation::Validate | Operation::Press(_) => Err(health.lock().unwrap().failure.unwrap_or(Failure::Revoked)),
                };
                health.lock().unwrap().neutralized = neutralized;
                let _ = command.reply.send(result);
            },
            event = events.next() => {
                if let Ok(event) = parse_event(event) {
                    let _ = state.handle(event);
                    if state.revoked {
                        neutralized = true;
                    }
                } else {
                    // An EOF cannot acknowledge release. Avoid a hot loop on
                    // a dead stream while leaving explicit cleanup retryable.
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            },
        }
    }
}

async fn run_actor(
    layout: KeyboardLayout,
    mut cancellation: watch::Receiver<bool>,
    mut controls: ActorControls,
    mut commands: mpsc::Receiver<Command>,
    ready: oneshot::Sender<Result<(), Failure>>,
    health: SharedHealth,
) {
    // Merge cancellation sources without dropping an in-flight authorize
    // future: it must close its Session and private bus before returning.
    let (cancel_tx, mut preparation_cancel) = watch::channel(false);
    let mut shutdown_during_setup = controls.shutdown.clone();
    tokio::spawn(async move {
        tokio::select! {
            biased;
            _ = cancelled(&mut shutdown_during_setup) => {},
            _ = cancelled(&mut cancellation) => {},
        }
        let _ = cancel_tx.send(true);
    });
    let authorization = authorize(&mut preparation_cancel).await;
    let (mut owner, stream) = match authorization {
        Ok(resources) => resources,
        Err(error) => {
            health.lock().unwrap().failure = Some(error);
            let _ = ready.send(Err(error));
            return;
        }
    };
    let context = match ei::Context::new(stream) {
        Ok(context) => context,
        Err(_) => {
            owner.close().await;
            let _ = ready.send(Err(Failure::Transport));
            return;
        }
    };
    let setup = cancellable(
        &mut preparation_cancel,
        SETUP_TIMEOUT + SETUP_TIMEOUT,
        ready_state(&context, layout),
    )
    .await;
    let (mut state, mut events) = match setup {
        Ok(ready_state) => ready_state,
        Err(error) => {
            owner.close().await;
            health.lock().unwrap().failure = Some(error);
            let _ = ready.send(Err(error));
            return;
        }
    };
    let mut closed = match timeout(
        OPERATION_TIMEOUT,
        owner
            .session
            .as_ref()
            .expect("authorized session")
            .receive_closed(),
    )
    .await
    {
        Ok(Ok(stream)) => Box::pin(stream),
        _ => {
            let _ = timeout(OPERATION_TIMEOUT, owner.bus.clone().close()).await;
            let _ = ready.send(Err(Failure::Transport));
            return;
        }
    };
    health.lock().unwrap().ready = true;
    if ready.send(Ok(())).is_err() {
        drop(closed);
        owner.close().await;
        return;
    }
    let failure = loop {
        tokio::select! {
            biased;
            _ = cancelled(&mut controls.shutdown) => break None,
            _ = controls.failure.changed() => break Some(controls.failure.borrow().unwrap_or(Failure::Transport)),
            _ = closed.next() => break Some(Failure::Revoked),
            event = events.next() => {
                let result = parse_event(event).and_then(|event| state.handle(event).map(|_| ()));
                if let Err(error) = result { break Some(error); }
                if let Err(error) = flush(&context).await { break Some(error); }
            }
            command = commands.recv() => {
                let Some(command) = command else { break None; };
                let result = match command.operation {
                    Operation::Validate => state.require_layout(),
                    Operation::Press(key) => state.emit(key, true),
                    Operation::Release(key) => state.emit(key, false),
                };
                let result = if result.is_ok() { sync(&mut state, &context, &mut events).await } else { result };
                let failed = result.as_ref().err().copied();
                let _ = command.reply.send(result);
                if let Some(error) = failed { break Some(error); }
            }
        }
    };
    drop(closed);
    if let Some(error) = failure {
        health.lock().unwrap().failure.get_or_insert(error);
    }
    let neutralized = recover_cleanup(
        &mut state,
        &context,
        &mut events,
        &mut owner,
        &mut commands,
        &mut controls.shutdown,
        &health,
    )
    .await;
    if state.connection.is_alive() {
        state.connection.disconnect();
        let _ = flush(&context).await;
    }
    // Session disposal must complete before joining, but it cannot delay a
    // release acknowledgment already established by the EIS roundtrip.
    let _ = owner.close().await;
    let mut final_health = health.lock().unwrap();
    final_health.ready = false;
    final_health.neutralized = neutralized;
}

fn read_keymap(fd: OwnedFd, size: u32) -> Result<Vec<Option<KeyboardLayout>>, Failure> {
    let size = usize::try_from(size).map_err(|_| Failure::Protocol)?;
    if size == 0 || size > MAX_KEYMAP_BYTES {
        return Err(Failure::Protocol);
    }
    let file = File::from(fd);
    let metadata = file.metadata().map_err(|_| Failure::Protocol)?;
    if !metadata.is_file() || metadata.len() < size as u64 {
        return Err(Failure::Protocol);
    }
    let mut bytes = vec![0; size];
    file.read_exact_at(&mut bytes, 0)
        .map_err(|_| Failure::Protocol)?;
    fingerprint_keymap(&bytes)
}

fn fingerprint_keymap(bytes: &[u8]) -> Result<Vec<Option<KeyboardLayout>>, Failure> {
    if bytes.is_empty()
        || bytes.len() > MAX_KEYMAP_BYTES
        || bytes[..bytes.len().saturating_sub(1)].contains(&0)
    {
        return Err(Failure::Protocol);
    }
    // EIS implementations may count the C-string terminator in their memfd
    // size. Compile the same text whether that final NUL is present or absent.
    let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
    if bytes.is_empty() {
        return Err(Failure::Protocol);
    }
    let xkb = xkbcommon_dl::xkbcommon_option().ok_or(Failure::Unsupported)?;
    let context = unsafe {
        (xkb.xkb_context_new)(xkbcommon_dl::xkb_context_flags::XKB_CONTEXT_NO_DEFAULT_INCLUDES)
    };
    if context.is_null() {
        return Err(Failure::Layout);
    }
    // Expanded compositor keymaps need no include files or environment names.
    // Suppress parser errors that could contain peer-provided keymap contents.
    // The C API accepts an integer threshold, including zero to disable every
    // severity. The binding represents named levels as a Rust enum, so call
    // through an ABI-equivalent integer signature rather than constructing an
    // invalid enum. This avoids variadic callback ABI differences on ARM64.
    let set_log_level: unsafe extern "C" fn(*mut xkbcommon_dl::xkb_context, libc::c_int) =
        unsafe { std::mem::transmute(*xkb.xkb_context_set_log_level) };
    unsafe {
        set_log_level(context, 0);
    }
    let map = unsafe {
        (xkb.xkb_keymap_new_from_buffer)(
            context,
            bytes.as_ptr().cast(),
            bytes.len(),
            xkbcommon_dl::xkb_keymap_format::XKB_KEYMAP_FORMAT_TEXT_V1,
            xkbcommon_dl::xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS,
        )
    };
    if map.is_null() {
        unsafe { (xkb.xkb_context_unref)(context) };
        return Err(Failure::Layout);
    }
    let count = unsafe { (xkb.xkb_keymap_num_layouts)(map) };
    let result = if (1..=4).contains(&count) {
        Ok((0..count)
            .map(|group| {
                let pair = |key| {
                    let symbols = |level| {
                        let mut symbols = ptr::null();
                        let count = unsafe {
                            (xkb.xkb_keymap_key_get_syms_by_level)(
                                map,
                                u32::from(linux_keycode(key)) + 8,
                                group,
                                level,
                                &mut symbols,
                            )
                        };
                        if count == 1 && !symbols.is_null() {
                            Some(unsafe { *symbols })
                        } else {
                            None
                        }
                    };
                    Some((symbols(0)?, symbols(1)?))
                };
                layout_fingerprint(pair)
            })
            .collect())
    } else {
        Err(Failure::Layout)
    };
    unsafe {
        (xkb.xkb_keymap_unref)(map);
        (xkb.xkb_context_unref)(context);
    }
    result
}

fn layout_fingerprint(pair: impl Fn(NativeKey) -> Option<(u32, u32)>) -> Option<KeyboardLayout> {
    use NativeKey::*;
    for (keys, letters) in [
        (
            [Q, W, E, R, T, Y, U, I, O, P].as_slice(),
            b"qwertyuiop".as_slice(),
        ),
        (
            [A, S, D, F, G, H, J, K, L].as_slice(),
            b"asdfghjkl".as_slice(),
        ),
        ([Z, X, C, V, B, N, M].as_slice(), b"zxcvbnm".as_slice()),
    ] {
        for (&key, &letter) in keys.iter().zip(letters) {
            if pair(key) != Some((u32::from(letter), u32::from(letter.to_ascii_uppercase()))) {
                return None;
            }
        }
    }
    match (
        pair(Num2)?,
        pair(Num3)?,
        pair(Quote)?,
        pair(Grave)?,
        pair(Backslash)?,
    ) {
        ((50, 64), (51, 35), (39, 34), (96, 126), (92, 124)) => Some(KeyboardLayout::Us),
        ((50, 34), (51, 163), (39, 64), (96, 172), (35, 126))
            if pair(IntlBackslash) == Some((92, 124)) =>
        {
            Some(KeyboardLayout::Uk)
        }
        _ => None,
    }
}

#[cfg(test)]
mod permission_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use reis::eis;
    use std::{
        ffi::{CStr, CString},
        io::Write,
        os::fd::AsFd,
        sync::atomic::{AtomicBool, Ordering},
    };

    // Builds an isolated keymap; this reads installed XKB definitions, not host
    // pressed keys, and changes no desktop layout or repeat settings.
    fn keymap(layout: &str, variant: &str) -> Vec<u8> {
        let xkb =
            xkbcommon_dl::xkbcommon_option().expect("libxkbcommon is required by Linux packages");
        let context =
            unsafe { (xkb.xkb_context_new)(xkbcommon_dl::xkb_context_flags::XKB_CONTEXT_NO_FLAGS) };
        let layout = CString::new(layout).unwrap();
        let variant = CString::new(variant).unwrap();
        let names = xkbcommon_dl::xkb_rule_names {
            rules: ptr::null(),
            model: ptr::null(),
            layout: layout.as_ptr(),
            variant: variant.as_ptr(),
            options: ptr::null(),
        };
        let map = unsafe {
            (xkb.xkb_keymap_new_from_names)(
                context,
                &names,
                xkbcommon_dl::xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS,
            )
        };
        assert!(!map.is_null());
        let text = unsafe {
            (xkb.xkb_keymap_get_as_string)(
                map,
                xkbcommon_dl::xkb_keymap_format::XKB_KEYMAP_FORMAT_TEXT_V1,
            )
        };
        let bytes = unsafe { CStr::from_ptr(text) }.to_bytes().to_vec();
        unsafe {
            libc::free(text.cast_mut().cast());
            (xkb.xkb_keymap_unref)(map);
            (xkb.xkb_context_unref)(context);
        }
        bytes
    }

    #[test]
    fn compositor_keymap_fingerprints_accept_us_uk_and_reject_variants() {
        assert_eq!(
            fingerprint_keymap(&keymap("us", "")).unwrap(),
            [Some(KeyboardLayout::Us)]
        );
        assert_eq!(
            fingerprint_keymap(&keymap("gb", "")).unwrap(),
            [Some(KeyboardLayout::Uk)]
        );
        assert_eq!(fingerprint_keymap(&keymap("us", "dvorak")).unwrap(), [None]);
        assert_eq!(fingerprint_keymap(&keymap("us", "intl")).unwrap(), [None]);
        assert_eq!(
            fingerprint_keymap(&keymap("us,gb", ",")).unwrap(),
            [Some(KeyboardLayout::Us), Some(KeyboardLayout::Uk)]
        );
        let mut nul_terminated = keymap("us,gb", ",");
        nul_terminated.push(0);
        assert_eq!(
            fingerprint_keymap(&nul_terminated).unwrap(),
            [Some(KeyboardLayout::Us), Some(KeyboardLayout::Uk)]
        );
    }

    #[test]
    fn keymap_fd_and_size_are_bounded_without_reading_streams() {
        let (stream, _peer) = UnixStream::pair().unwrap();
        assert_eq!(read_keymap(stream.into(), 12), Err(Failure::Protocol));
        let file = tempfile::tempfile().unwrap();
        assert_eq!(
            read_keymap(file.into(), MAX_KEYMAP_BYTES as u32 + 1),
            Err(Failure::Protocol)
        );
        assert_eq!(
            fingerprint_keymap(b"secret\0sentinel"),
            Err(Failure::Protocol)
        );
        assert_eq!(fingerprint_keymap(&[]), Err(Failure::Protocol));
        assert!(!Failure::Protocol.error().to_string().contains("secret"));
    }

    #[test]
    fn malformed_keymap_never_logs_peer_supplied_text() {
        const SENTINEL: &str = "PRIVATE_KEYMAP_SECRET_SENTINEL";
        const CHILD: &str = "C64_BRIDGE_KEYMAP_LOG_FIXTURE";
        if std::env::var_os(CHILD).is_some() {
            assert_eq!(
                fingerprint_keymap(format!("xkb_keymap {{ {SENTINEL}").as_bytes()),
                Err(Failure::Layout)
            );
            return;
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "backend::portal::tests::malformed_keymap_never_logs_peer_supplied_text",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(!String::from_utf8_lossy(&output.stderr).contains(SENTINEL));
        assert!(!String::from_utf8_lossy(&output.stdout).contains(SENTINEL));
    }

    #[tokio::test]
    async fn cancelled_preparation_wins_over_ready_result() {
        let (_send, mut cancellation) = watch::channel(true);
        assert_eq!(
            cancellable(&mut cancellation, OPERATION_TIMEOUT, async { Ok(()) }).await,
            Err(Failure::Cancelled)
        );
    }

    #[derive(Clone, Debug, PartialEq)]
    enum Wire {
        Bind(u64),
        Start,
        Key(u32, bool),
        Frame,
        Stop,
    }
    enum Control {
        Group(u32),
        Pause,
        Stall,
        Resume,
        PauseSibling,
        InvalidObject,
    }
    struct Fixture {
        context: ei::Context,
        control: sync_mpsc::Sender<Control>,
        recording: Arc<Mutex<Vec<Wire>>>,
        done: Arc<AtomicBool>,
        thread: Option<thread::JoinHandle<()>>,
    }
    impl Fixture {
        fn new(map: Vec<u8>, initial_group: Option<u32>) -> Self {
            Self::with_sibling(map, initial_group, false)
        }
        fn with_sibling(map: Vec<u8>, initial_group: Option<u32>, sibling: bool) -> Self {
            let (client, server) = UnixStream::pair().unwrap();
            let context = ei::Context::new(client).unwrap();
            let (control, controls) = sync_mpsc::channel();
            let recording = Arc::new(Mutex::new(Vec::new()));
            let server_recording = recording.clone();
            let done = Arc::new(AtomicBool::new(false));
            let server_done = done.clone();
            let thread = thread::spawn(move || {
                let server = eis::Context::new(server).unwrap();
                let mut handshake = reis::handshake::EisHandshaker::new(&server, 1);
                let mut keyboard: Option<eis::Keyboard> = None;
                let mut device: Option<eis::Device> = None;
                let mut sibling_device: Option<eis::Device> = None;
                let mut seat = None;
                let mut connection = None;
                let mut connected = false;
                let mut stalled = false;
                let mut file = tempfile::tempfile().unwrap();
                file.write_all(&map).unwrap();
                let deadline = std::time::Instant::now() + Duration::from_secs(10);
                while !server_done.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
                    if server.read().is_err() {
                        break;
                    }
                    while let Some(request) = server.pending_request() {
                        let PendingRequestResult::Request(request) = request else {
                            panic!("invalid fixture request");
                        };
                        if !connected {
                            if let Some(response) = handshake.handle_request(request).unwrap() {
                                assert_eq!(
                                    response.context_type,
                                    eis::handshake::ContextType::Sender
                                );
                                let new_seat = response.connection.seat(1);
                                new_seat.capability(1, "ei_keyboard");
                                new_seat.capability(2, "ei_pointer");
                                new_seat.done();
                                seat = Some(new_seat);
                                connection = Some(response.connection);
                                connected = true;
                            }
                            continue;
                        }
                        match request {
                            eis::Request::Seat(_, eis::seat::Request::Bind { capabilities }) => {
                                server_recording
                                    .lock()
                                    .unwrap()
                                    .push(Wire::Bind(capabilities));
                                let new_device = seat.as_ref().unwrap().device(1);
                                new_device.device_type(eis::device::DeviceType::Virtual);
                                let new_keyboard = new_device.interface::<eis::Keyboard>(1);
                                new_keyboard.keymap(
                                    eis::keyboard::KeymapType::Xkb,
                                    map.len() as u32,
                                    file.as_fd(),
                                );
                                new_device.done();
                                new_device.resumed(2);
                                if let Some(group) = initial_group {
                                    new_keyboard.modifiers(3, 0, 0, 0, group);
                                }
                                device = Some(new_device);
                                keyboard = Some(new_keyboard);
                                if sibling {
                                    let extra = seat.as_ref().unwrap().device(1);
                                    extra.device_type(eis::device::DeviceType::Virtual);
                                    let _ = extra.interface::<eis::Keyboard>(1);
                                    // This sibling has not completed startup
                                    // and cannot own any selected outputs.
                                    sibling_device = Some(extra);
                                }
                            }
                            eis::Request::Connection(
                                _,
                                eis::connection::Request::Sync { callback },
                            ) if !stalled => callback.done(0),
                            eis::Request::Keyboard(
                                _,
                                eis::keyboard::Request::Key { key, state },
                            ) => server_recording
                                .lock()
                                .unwrap()
                                .push(Wire::Key(key, state == eis::keyboard::KeyState::Press)),
                            eis::Request::Device(
                                _,
                                eis::device::Request::StartEmulating { .. },
                            ) => server_recording.lock().unwrap().push(Wire::Start),
                            eis::Request::Device(
                                _,
                                eis::device::Request::Frame { timestamp, .. },
                            ) => {
                                assert!(timestamp > 0);
                                server_recording.lock().unwrap().push(Wire::Frame);
                            }
                            eis::Request::Device(_, eis::device::Request::StopEmulating { .. }) => {
                                server_recording.lock().unwrap().push(Wire::Stop)
                            }
                            _ => {}
                        }
                    }
                    while let Ok(control) = controls.try_recv() {
                        match control {
                            Control::Group(group) => {
                                keyboard.as_ref().unwrap().modifiers(4, 0, 0, 0, group)
                            }
                            Control::Pause => device.as_ref().unwrap().paused(4),
                            Control::Stall => stalled = true,
                            Control::Resume => stalled = false,
                            Control::PauseSibling => sibling_device.as_ref().unwrap().paused(4),
                            Control::InvalidObject => {
                                connection.as_ref().unwrap().invalid_object(4, 999)
                            }
                        }
                    }
                    if server.flush().is_err() {
                        break;
                    }
                    thread::sleep(Duration::from_millis(1));
                }
            });
            Self {
                context,
                control,
                recording,
                done,
                thread: Some(thread),
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.done.store(true, Ordering::SeqCst);
            if let Some(thread) = self.thread.take() {
                thread.join().unwrap();
            }
        }
    }

    #[tokio::test]
    async fn eis_sender_binds_only_keyboard_and_releases_ordinary_before_modifiers() {
        let fixture = Fixture::new(keymap("us", ""), None);
        let (mut state, mut events) = ready_state(&fixture.context, KeyboardLayout::Us)
            .await
            .unwrap();
        assert_eq!(*fixture.recording.lock().unwrap(), [Wire::Bind(1)]);
        state.emit(NativeKey::LeftShift, true).unwrap();
        sync(&mut state, &fixture.context, &mut events)
            .await
            .unwrap();
        state.emit(NativeKey::A, true).unwrap();
        sync(&mut state, &fixture.context, &mut events)
            .await
            .unwrap();
        state.cleanup().unwrap();
        sync(&mut state, &fixture.context, &mut events)
            .await
            .unwrap();
        assert_eq!(
            *fixture.recording.lock().unwrap(),
            [
                Wire::Bind(1),
                Wire::Start,
                Wire::Key(42, true),
                Wire::Frame,
                Wire::Key(30, true),
                Wire::Frame,
                Wire::Key(30, false),
                Wire::Frame,
                Wire::Key(42, false),
                Wire::Frame,
                Wire::Stop,
            ]
        );
        assert!(state.held.is_empty());
    }

    #[tokio::test]
    async fn multigroup_keymap_requires_authoritative_group_and_change_releases_keys() {
        let unverified = Fixture::new(keymap("us,gb", ","), None);
        assert!(matches!(
            ready_state(&unverified.context, KeyboardLayout::Us).await,
            Err(Failure::LayoutMetadata)
        ));
        let fixture = Fixture::new(keymap("us,gb", ","), Some(0));
        let (mut state, mut events) = ready_state(&fixture.context, KeyboardLayout::Us)
            .await
            .unwrap();
        state.emit(NativeKey::A, true).unwrap();
        sync(&mut state, &fixture.context, &mut events)
            .await
            .unwrap();
        fixture.control.send(Control::Group(1)).unwrap();
        assert_eq!(
            state.handle(parse_event(events.next().await).unwrap()),
            Err(Failure::LayoutMismatch)
        );
        // Layout failure does not prevent exact held-code cleanup.
        state.cleanup().unwrap();
        sync(&mut state, &fixture.context, &mut events)
            .await
            .unwrap();
        assert!(
            fixture
                .recording
                .lock()
                .unwrap()
                .contains(&Wire::Key(30, false))
        );
    }

    #[tokio::test]
    async fn eos_pause_marks_owned_keys_neutral_without_replaying_them() {
        let fixture = Fixture::new(keymap("us", ""), Some(0));
        let (mut state, mut events) = ready_state(&fixture.context, KeyboardLayout::Us)
            .await
            .unwrap();
        state.emit(NativeKey::A, true).unwrap();
        sync(&mut state, &fixture.context, &mut events)
            .await
            .unwrap();
        fixture.control.send(Control::Pause).unwrap();
        assert_eq!(
            state.handle(parse_event(events.next().await).unwrap()),
            Err(Failure::Revoked)
        );
        assert!(state.revoked && state.held.is_empty() && !state.emulating);
        assert_eq!(state.emit(NativeKey::A, true), Err(Failure::Revoked));
    }

    #[tokio::test]
    async fn paused_sibling_cannot_neutralize_selected_keyboard_ownership() {
        let fixture = Fixture::with_sibling(keymap("us", ""), Some(0), true);
        let (mut state, mut events) = ready_state(&fixture.context, KeyboardLayout::Us)
            .await
            .unwrap();
        state.emit(NativeKey::LeftShift, true).unwrap();
        state.emit(NativeKey::A, true).unwrap();
        sync(&mut state, &fixture.context, &mut events)
            .await
            .unwrap();
        fixture.control.send(Control::PauseSibling).unwrap();
        assert_eq!(
            state.handle(parse_event(events.next().await).unwrap()),
            Err(Failure::Revoked)
        );
        assert!(!state.revoked);
        assert_eq!(state.held, [(NativeKey::LeftShift, 42), (NativeKey::A, 30)]);
        state.cleanup().unwrap();
        sync(&mut state, &fixture.context, &mut events)
            .await
            .unwrap();
        let recording = fixture.recording.lock().unwrap();
        assert!(recording.ends_with(&[
            Wire::Key(30, false),
            Wire::Frame,
            Wire::Key(42, false),
            Wire::Frame,
            Wire::Stop
        ]));
    }

    #[tokio::test]
    async fn rejected_eis_object_keeps_owned_keys_until_confirmed_cleanup() {
        let fixture = Fixture::new(keymap("us", ""), Some(0));
        let (mut state, mut events) = ready_state(&fixture.context, KeyboardLayout::Us)
            .await
            .unwrap();
        state.emit(NativeKey::A, true).unwrap();
        sync(&mut state, &fixture.context, &mut events)
            .await
            .unwrap();
        fixture.control.send(Control::InvalidObject).unwrap();
        assert_eq!(
            state.handle(parse_event(events.next().await).unwrap()),
            Err(Failure::Protocol)
        );
        assert_eq!(state.held, [(NativeKey::A, 30)]);
        state.cleanup().unwrap();
        sync(&mut state, &fixture.context, &mut events)
            .await
            .unwrap();
        assert!(
            fixture
                .recording
                .lock()
                .unwrap()
                .contains(&Wire::Key(30, false))
        );
    }

    #[tokio::test]
    async fn stalled_eis_roundtrip_has_bounded_deadline() {
        let fixture = Fixture::new(keymap("us", ""), None);
        let (mut state, mut events) = ready_state(&fixture.context, KeyboardLayout::Us)
            .await
            .unwrap();
        fixture.control.send(Control::Stall).unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        state.emit(NativeKey::A, true).unwrap();
        let start = Instant::now();
        assert_eq!(
            sync(&mut state, &fixture.context, &mut events).await,
            Err(Failure::Timeout)
        );
        assert!(start.elapsed() < Duration::from_secs(2));
        assert_eq!(state.held, [(NativeKey::A, 30)]);
    }

    #[tokio::test]
    async fn failed_cleanup_retains_release_channel_and_recovers_without_new_presses() {
        struct NeverCloses;
        impl CloseAuthority for NeverCloses {
            async fn close(&mut self) -> bool {
                false
            }
        }
        async fn response(
            receiver: sync_mpsc::Receiver<Result<(), Failure>>,
        ) -> Result<(), Failure> {
            timeout(Duration::from_secs(2), async {
                loop {
                    match receiver.try_recv() {
                        Ok(result) => return result,
                        Err(sync_mpsc::TryRecvError::Empty) => {
                            tokio::time::sleep(Duration::from_millis(1)).await
                        }
                        Err(_) => panic!("Cleanup actor reply channel disappeared"),
                    }
                }
            })
            .await
            .unwrap()
        }
        let fixture = Fixture::new(keymap("us", ""), Some(0));
        let (mut state, mut events) = ready_state(&fixture.context, KeyboardLayout::Us)
            .await
            .unwrap();
        state.emit(NativeKey::LeftShift, true).unwrap();
        state.emit(NativeKey::A, true).unwrap();
        sync(&mut state, &fixture.context, &mut events)
            .await
            .unwrap();
        fixture.control.send(Control::Stall).unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        let health = Arc::new(Mutex::new(Health {
            failure: Some(Failure::Timeout),
            ready: true,
            neutralized: false,
        }));
        let (commands, mut receiver) = mpsc::channel(COMMAND_QUEUE);
        let (_stop, mut shutdown) = watch::channel(false);
        let mut authority = NeverCloses;
        let repair = recover_cleanup(
            &mut state,
            &fixture.context,
            &mut events,
            &mut authority,
            &mut receiver,
            &mut shutdown,
            &health,
        );
        let retry = async {
            tokio::time::sleep(OPERATION_TIMEOUT + Duration::from_millis(50)).await;
            assert!(!health.lock().unwrap().neutralized);
            let (reply, answered) = sync_mpsc::channel();
            commands
                .send(Command {
                    operation: Operation::Press(NativeKey::B),
                    reply,
                })
                .await
                .unwrap();
            assert_eq!(response(answered).await, Err(Failure::Timeout));
            fixture.control.send(Control::Resume).unwrap();
            let (reply, answered) = sync_mpsc::channel();
            commands
                .send(Command {
                    operation: Operation::Release(NativeKey::A),
                    reply,
                })
                .await
                .unwrap();
            assert_eq!(response(answered).await, Ok(()));
        };
        let (neutralized, ()) = timeout(Duration::from_secs(3), async {
            tokio::join!(repair, retry)
        })
        .await
        .unwrap();
        assert!(neutralized && health.lock().unwrap().neutralized);
        assert_eq!(health.lock().unwrap().failure, Some(Failure::Timeout));
        let recording = fixture.recording.lock().unwrap();
        assert!(!recording.contains(&Wire::Key(48, true)));
        assert!(recording.ends_with(&[
            Wire::Key(30, false),
            Wire::Frame,
            Wire::Key(42, false),
            Wire::Frame,
            Wire::Stop
        ]));
    }

    #[tokio::test]
    async fn wrapper_timeout_keeps_cleanup_retryable_and_drop_joins_delayed_actor() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct DelayedFailedClose;
        impl CloseAuthority for DelayedFailedClose {
            async fn close(&mut self) -> bool {
                tokio::time::sleep(OPERATION_TIMEOUT + Duration::from_millis(150)).await;
                false
            }
        }
        let fixture = Fixture::new(keymap("us", ""), Some(0));
        let context = fixture.context.clone();
        let control = fixture.control.clone();
        let health = Arc::new(Mutex::new(Health {
            failure: Some(Failure::Timeout),
            ready: true,
            neutralized: false,
        }));
        let actor_health = health.clone();
        let (commands, mut receiver) = mpsc::channel(COMMAND_QUEUE);
        let (shutdown, mut shutdown_rx) = watch::channel(false);
        let (abort_output, _abort_rx) = watch::channel(None);
        let (ready, initialized) = oneshot::channel();
        let finished = Arc::new(AtomicBool::new(false));
        let actor_finished = finished.clone();
        let thread = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let (mut state, mut events) =
                    ready_state(&context, KeyboardLayout::Us).await.unwrap();
                state.emit(NativeKey::LeftShift, true).unwrap();
                state.emit(NativeKey::A, true).unwrap();
                sync(&mut state, &context, &mut events).await.unwrap();
                control.send(Control::Stall).unwrap();
                tokio::time::sleep(Duration::from_millis(5)).await;
                ready.send(()).unwrap();
                let mut authority = DelayedFailedClose;
                assert!(
                    recover_cleanup(
                        &mut state,
                        &context,
                        &mut events,
                        &mut authority,
                        &mut receiver,
                        &mut shutdown_rx,
                        &actor_health
                    )
                    .await
                );
                actor_finished.store(true, Ordering::SeqCst);
            });
        });
        let mut backend = PortalBackend {
            commands,
            shutdown,
            abort_output,
            thread: Some(thread),
            health: health.clone(),
            held: vec![NativeKey::LeftShift, NativeKey::A],
        };
        initialized.await.unwrap();
        assert!(backend.press(NativeKey::LeftShift).is_err());
        assert!(backend.press(NativeKey::B).is_err());
        assert!(backend.release(NativeKey::A).is_err());
        assert!(backend.held.contains(&NativeKey::A));
        assert!(!health.lock().unwrap().neutralized);
        assert!(!*backend.shutdown.borrow());
        fixture.control.send(Control::Resume).unwrap();
        backend.release(NativeKey::A).unwrap();
        backend.release(NativeKey::LeftShift).unwrap();
        assert!(backend.held.is_empty() && health.lock().unwrap().neutralized);
        drop(backend);
        assert!(finished.load(Ordering::SeqCst));
        assert_eq!(health.lock().unwrap().failure, Some(Failure::Timeout));
        assert!(
            !fixture
                .recording
                .lock()
                .unwrap()
                .contains(&Wire::Key(48, true))
        );
    }

    #[tokio::test]
    async fn drop_joins_uncertain_cleanup_without_claiming_neutral_ownership() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct DelayedFailedClose {
            attempts: Arc<AtomicUsize>,
            initial: Option<oneshot::Sender<()>>,
        }
        impl CloseAuthority for DelayedFailedClose {
            async fn close(&mut self) -> bool {
                self.attempts.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(OPERATION_TIMEOUT + Duration::from_millis(150)).await;
                if let Some(initial) = self.initial.take() {
                    let _ = initial.send(());
                }
                false
            }
        }
        let fixture = Fixture::new(keymap("us", ""), Some(0));
        let context = fixture.context.clone();
        let control = fixture.control.clone();
        let health = Arc::new(Mutex::new(Health {
            failure: Some(Failure::Timeout),
            ready: false,
            neutralized: false,
        }));
        let actor_health = health.clone();
        let (commands, mut receiver) = mpsc::channel(COMMAND_QUEUE);
        let (shutdown, mut shutdown_rx) = watch::channel(false);
        let (abort_output, _abort_rx) = watch::channel(None);
        let (initial, attempted) = oneshot::channel();
        let attempts = Arc::new(AtomicUsize::new(0));
        let actor_attempts = attempts.clone();
        let finished = Arc::new(AtomicBool::new(false));
        let actor_finished = finished.clone();
        let thread = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let (mut state, mut events) =
                    ready_state(&context, KeyboardLayout::Us).await.unwrap();
                state.emit(NativeKey::LeftShift, true).unwrap();
                state.emit(NativeKey::A, true).unwrap();
                sync(&mut state, &context, &mut events).await.unwrap();
                control.send(Control::Stall).unwrap();
                tokio::time::sleep(Duration::from_millis(5)).await;
                let mut authority = DelayedFailedClose {
                    attempts: actor_attempts,
                    initial: Some(initial),
                };
                assert!(
                    !recover_cleanup(
                        &mut state,
                        &context,
                        &mut events,
                        &mut authority,
                        &mut receiver,
                        &mut shutdown_rx,
                        &actor_health
                    )
                    .await
                );
                actor_finished.store(true, Ordering::SeqCst);
            });
        });
        let mut backend = PortalBackend {
            commands,
            shutdown,
            abort_output,
            thread: Some(thread),
            health: health.clone(),
            held: vec![NativeKey::LeftShift, NativeKey::A],
        };
        timeout(Duration::from_secs(3), attempted)
            .await
            .unwrap()
            .unwrap();
        assert!(!health.lock().unwrap().neutralized);
        // Synchronize with the release-only loop, rather than racing Drop
        // against the end of its initial failed cleanup attempt.
        assert!(backend.operation(Operation::Validate).is_err());
        assert!(backend.press(NativeKey::LeftShift).is_err());
        assert!(backend.press(NativeKey::B).is_err());
        let start = Instant::now();
        drop(backend);
        assert!(start.elapsed() < Duration::from_secs(3));
        assert!(finished.load(Ordering::SeqCst));
        assert!(attempts.load(Ordering::SeqCst) >= 2);
        assert!(!health.lock().unwrap().neutralized);
        assert_eq!(health.lock().unwrap().failure, Some(Failure::Timeout));
        let recording = fixture.recording.lock().unwrap();
        assert_eq!(
            recording
                .iter()
                .filter(|event| **event == Wire::Key(30, true))
                .count(),
            1
        );
        assert!(!recording.contains(&Wire::Key(48, true)));
    }
}

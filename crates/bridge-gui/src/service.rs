//! The desktop controller runs off the window thread. No GUI preview posts keys.
use crate::{
    config,
    model::*,
    secrets::{MemorySecretStore, OsSecretStore, SecretStore},
};
use anyhow::{Result, bail};
use bridge_core::MappingDocument;
use bridge_host::{
    backend, control, diagnostics, operations, pairing,
    rest::RestClient,
    runtime::{self, ManagedRuntime, RuntimeState, StartOptions},
};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc as async_mpsc, watch};

pub const KEYBOARD_PRG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/KEYBRIDGE.PRG"));
pub const PROBE_PRG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/NETWORKTEST.PRG"));

#[derive(Clone, Copy)]
struct SafetyState {
    focused: bool,
    tray: bool,
    auto_pause: u64,
    pause: u64,
    cancel: u64,
    stop: u64,
    tray_loss: u64,
}
impl Default for SafetyState {
    fn default() -> Self {
        Self {
            focused: true,
            tray: false,
            auto_pause: 0,
            pause: 0,
            cancel: 0,
            stop: 0,
            tray_loss: 0,
        }
    }
}
struct QueuedAction {
    action: GuiAction,
    cancel_epoch: u64,
    auto_pause_epoch: u64,
    gate_epoch: u64,
    test_focus_epoch: u64,
}
#[derive(Default)]
struct PermitEpoch {
    input: u64,
    test_focus: u64,
}
/// Regular forwarding and the deliberate test phrase have independent gates.
/// Focus can revoke only the test gate; explicit safety actions revoke both.
/// Serializes only permit changes, never network or backend operations.
#[derive(Default)]
struct OutputPermit {
    epoch: Mutex<PermitEpoch>,
    allow: Arc<AtomicBool>,
    test_allow: Arc<AtomicBool>,
    focus: Arc<AtomicBool>,
}
impl OutputPermit {
    fn close(&self) {
        let mut epoch = self.epoch.lock().unwrap();
        epoch.input = epoch.input.wrapping_add(1);
        self.allow.store(false, Ordering::Release);
        self.test_allow.store(false, Ordering::Release);
    }
    fn epoch(&self) -> u64 {
        self.epoch.lock().unwrap().input
    }
    fn test_focus_epoch(&self) -> u64 {
        self.epoch.lock().unwrap().test_focus
    }
    fn arm(&self, expected: u64) -> bool {
        let epoch = self.epoch.lock().unwrap();
        if epoch.input != expected {
            return false;
        }
        self.allow.store(true, Ordering::Release);
        true
    }
    fn arm_test(&self, expected: u64, expected_focus: u64) -> bool {
        let epoch = self.epoch.lock().unwrap();
        if epoch.input != expected
            || epoch.test_focus != expected_focus
            || !self.focus.load(Ordering::Acquire)
        {
            return false;
        }
        self.test_allow.store(true, Ordering::Release);
        true
    }
    fn focus_changed(&self, focused: bool) {
        let mut epoch = self.epoch.lock().unwrap();
        self.focus.store(!focused, Ordering::Release);
        if focused {
            epoch.test_focus = epoch.test_focus.wrapping_add(1);
            self.test_allow.store(false, Ordering::Release);
        }
    }
}

pub struct ServiceHandle {
    commands: async_mpsc::Sender<QueuedAction>,
    events: mpsc::Receiver<GuiEvent>,
    latest: Arc<Mutex<Option<GuiSnapshot>>>,
    quit: watch::Sender<bool>,
    thread: Option<thread::JoinHandle<()>>,
    safety: watch::Sender<SafetyState>,
    permit: Arc<OutputPermit>,
}
impl ServiceHandle {
    pub fn spawn(
        state_dir: PathBuf,
        env_override: Option<PathBuf>,
        smoke: bool,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        let (commands, receiver) = async_mpsc::channel(64);
        let (events, event_rx) = mpsc::sync_channel(128);
        let latest = Arc::new(Mutex::new(None));
        let snapshot = latest.clone();
        let (quit, quit_rx) = watch::channel(false);
        let reset_quit = quit.clone();
        let (safety, safety_rx) = watch::channel(SafetyState::default());
        let permit = Arc::new(OutputPermit::default());
        let gate = permit.clone();
        let handle = thread::Builder::new()
            .name("c64-keyboard-bridge-gui-service".into())
            .spawn(move || {
                let executor = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build();
                match executor {
                    Ok(executor) => executor.block_on(async move {
                        let mut actor = Actor::new(
                            state_dir,
                            env_override,
                            smoke,
                            events,
                            snapshot,
                            wake,
                            reset_quit,
                        );
                        actor.permit = gate;
                        actor.initialize();
                        actor.run(receiver, quit_rx, safety_rx).await;
                    }),
                    Err(_) => {
                        let _ = events
                            .try_send(GuiEvent::Error("Cannot start background service".into()));
                        wake();
                    }
                }
            })?;
        Ok(Self {
            commands,
            events: event_rx,
            latest,
            quit,
            thread: Some(handle),
            safety,
            permit,
        })
    }
    pub fn send(&self, action: GuiAction) -> Result<()> {
        match action {
            GuiAction::Quit => {
                self.permit.close();
                let _ = self.quit.send(true);
                return Ok(());
            }
            GuiAction::FocusChanged(focused) => {
                self.permit.focus_changed(focused);
                self.safety.send_modify(|s| {
                    s.focused = focused;
                });
                return Ok(());
            }
            GuiAction::TrayAvailable(available) => {
                if !available {
                    self.permit.close();
                }
                self.safety.send_modify(|s| {
                    if s.tray && !available {
                        s.tray_loss = s.tray_loss.wrapping_add(1);
                        s.auto_pause = s.auto_pause.wrapping_add(1);
                    }
                    s.tray = available;
                });
                return Ok(());
            }
            GuiAction::Pause => {
                self.permit.close();
                self.safety.send_modify(|s| {
                    s.pause = s.pause.wrapping_add(1);
                    s.cancel = s.cancel.wrapping_add(1);
                });
                return Ok(());
            }
            GuiAction::CancelCountdown => {
                self.permit.close();
                self.safety
                    .send_modify(|s| s.cancel = s.cancel.wrapping_add(1));
                return Ok(());
            }
            GuiAction::Stop => {
                self.permit.close();
                self.safety.send_modify(|s| {
                    s.stop = s.stop.wrapping_add(1);
                    s.cancel = s.cancel.wrapping_add(1);
                });
                return Ok(());
            }
            _ => {}
        }
        let safety = *self.safety.borrow();
        self.commands
            .try_send(QueuedAction {
                action,
                cancel_epoch: safety.cancel,
                auto_pause_epoch: safety.auto_pause,
                gate_epoch: self.permit.epoch(),
                test_focus_epoch: self.permit.test_focus_epoch(),
            })
            .map_err(|_| anyhow::anyhow!("GUI operation queue is busy; try again"))
    }
    pub fn try_recv(&self) -> Option<GuiEvent> {
        self.events.try_recv().ok().or_else(|| {
            self.latest
                .lock()
                .unwrap()
                .take()
                .map(|s| GuiEvent::Snapshot(Box::new(s)))
        })
    }
    pub fn shutdown_and_join(&mut self) -> Result<()> {
        self.permit.close();
        let _ = self.quit.send(true);
        if let Some(thread) = self.thread.take() {
            thread
                .join()
                .map_err(|_| anyhow::anyhow!("Background service panicked"))?;
        }
        Ok(())
    }
}
impl Drop for ServiceHandle {
    fn drop(&mut self) {
        self.permit.close();
        let _ = self.quit.send(true);
    }
}

struct Countdown {
    prepared: Option<Box<dyn bridge_core::KeyboardBackend>>,
    end: Instant,
    gate_epoch: u64,
    focus_epoch: u64,
}
struct PendingTestPreparation {
    cancellation: watch::Sender<bool>,
    task: tokio::task::JoinHandle<Result<Box<dyn bridge_core::KeyboardBackend>>>,
    gate_epoch: u64,
}
struct Actor {
    state_dir: PathBuf,
    env_override: Option<PathBuf>,
    smoke: bool,
    settings: GuiSettings,
    password: Option<String>,
    store: Box<dyn SecretStore>,
    owned: Option<ManagedRuntime>,
    snapshot: GuiSnapshot,
    focused: bool,
    countdown: Option<Countdown>,
    test_preparation: Option<PendingTestPreparation>,
    cancelled_test_preparations: Vec<PendingTestPreparation>,
    events: mpsc::SyncSender<GuiEvent>,
    latest: Arc<Mutex<Option<GuiSnapshot>>>,
    wake: Arc<dyn Fn() + Send + Sync>,
    history: VecDeque<Activity>,
    began: Instant,
    quit: watch::Sender<bool>,
    overflowed: bool,
    safety: SafetyState,
    permit: Arc<OutputPermit>,
    command_epoch: Option<u64>,
    command_test_focus_epoch: Option<u64>,
}
impl Actor {
    fn new(
        state_dir: PathBuf,
        env_override: Option<PathBuf>,
        smoke: bool,
        events: mpsc::SyncSender<GuiEvent>,
        latest: Arc<Mutex<Option<GuiSnapshot>>>,
        wake: Arc<dyn Fn() + Send + Sync>,
        quit: watch::Sender<bool>,
    ) -> Self {
        Self {
            state_dir,
            env_override,
            smoke,
            settings: GuiSettings::default(),
            password: None,
            store: if smoke {
                Box::<MemorySecretStore>::default()
            } else {
                Box::new(OsSecretStore)
            },
            owned: None,
            snapshot: GuiSnapshot::default(),
            focused: true,
            countdown: None,
            test_preparation: None,
            cancelled_test_preparations: Vec::new(),
            events,
            latest,
            wake,
            history: VecDeque::new(),
            began: Instant::now(),
            quit,
            overflowed: false,
            safety: SafetyState::default(),
            permit: Arc::new(OutputPermit::default()),
            command_epoch: None,
            command_test_focus_epoch: None,
        }
    }
    fn initialize(&mut self) {
        let settings = if self.smoke {
            Ok(GuiSettings {
                env_file: String::new(),
                device_host: "127.0.0.1".into(),
                backend: "recording".into(),
                bind_address: "127.0.0.1".into(),
                advertise_ip: "127.0.0.1".into(),
                credential_id: config::random_id().unwrap_or_default(),
                ..Default::default()
            })
        } else {
            config::load_settings(&self.state_dir, self.env_override.as_deref())
        };
        match settings {
            Ok(settings)=>{
                self.settings=settings;
                self.restore_password();
                self.event(GuiEvent::SettingsLoaded(self.settings.clone())); self.probe_backend();
                self.configuration_warning();
                if !self.settings.mappings_file.is_empty() {
                    let path=PathBuf::from(&self.settings.mappings_file);
                    match config::read_regular(&path,false) {
                        Ok(text)=>self.event(GuiEvent::ProfileDraftLoaded{text,path}),
                        Err(_)=>self.error("Selected mapping file could not be read; settings and the original file are preserved"),
                    }
                }
                self.note("Desktop opened; hosting and input remain off");
            },
            Err(error)=>self.error(&format!("Configuration could not be loaded: {}. Original file preserved; review Settings before starting", display_error(&error))),
        }
        self.publish();
    }
    fn restore_password(&mut self) {
        if self.settings.remember_password {
            match self.store.get(&self.settings.credential_id) {
                Ok(value) => self.password = value,
                Err(_) => {
                    self.error("Saved password is locked or unavailable; enter a session password")
                }
            }
        }
    }
    fn event(&mut self, event: GuiEvent) {
        if self.events.try_send(event).is_err() {
            self.overflowed = true;
            self.snapshot.message = "GUI notifications overflowed; input will be paused".into();
        }
        (self.wake)();
    }
    fn note(&mut self, message: &str) {
        self.history.push_back(Activity {
            elapsed_ms: self.began.elapsed().as_millis().min(u64::MAX as u128) as u64,
            summary: message.chars().take(256).collect(),
        });
        while self.history.len() > 128 {
            self.history.pop_front();
        }
        self.snapshot.message = message.into();
    }
    fn error(&mut self, message: &str) {
        self.note(message);
        self.event(GuiEvent::Error(message.into()));
    }
    fn cancel_test_for_configuration_change(&mut self) {
        if self.test_preparation.is_some() || self.countdown.is_some() {
            self.permit.close();
            self.countdown = None;
            self.cancel_test_preparation();
        }
    }
    fn cancel_test_preparation(&mut self) {
        if let Some(pending) = self.test_preparation.take() {
            pending.cancellation.send_replace(true);
            self.cancelled_test_preparations.push(pending);
        }
        self.snapshot.test_authorization_pending = false;
    }
    async fn join_cancelled_test_preparations(&mut self) {
        for pending in std::mem::take(&mut self.cancelled_test_preparations) {
            drop(pending.task.await);
        }
    }
    async fn poll_test_preparation(&mut self) {
        let mut index = 0;
        while index < self.cancelled_test_preparations.len() {
            if self.cancelled_test_preparations[index].task.is_finished() {
                let pending = self.cancelled_test_preparations.swap_remove(index);
                drop(pending.task.await);
            } else {
                index += 1;
            }
        }
        if self
            .test_preparation
            .as_ref()
            .is_some_and(|pending| pending.gate_epoch != self.permit.epoch())
        {
            self.cancel_test_preparation();
        }
        if !self
            .test_preparation
            .as_ref()
            .is_some_and(|pending| pending.task.is_finished())
        {
            return;
        }
        let pending = self.test_preparation.take().unwrap();
        self.snapshot.test_authorization_pending = false;
        match pending.task.await {
            Ok(Ok(prepared)) if pending.gate_epoch == self.permit.epoch() => {
                self.countdown = Some(Countdown {
                    prepared: Some(prepared),
                    end: Instant::now() + Duration::from_secs(5),
                    gate_epoch: pending.gate_epoch,
                    // Consent dialogs may return focus to this window. Posting
                    // remains impossible until the following guarded countdown.
                    focus_epoch: self.permit.test_focus_epoch(),
                });
                self.note("Desktop keyboard approval ready. Five-second input test countdown: focus a scratch application");
            }
            Ok(Ok(prepared)) => {
                drop(prepared);
                self.note("Obsolete desktop test approval closed; no input was posted");
            }
            _ => self.error(
                "Desktop keyboard test authorization failed or was denied; no input was posted",
            ),
        }
    }
    fn acknowledge_tray_command(&mut self, command: TrayCommand, outcome: CommandOutcome) {
        let sequence = self
            .snapshot
            .tray_command_ack
            .map_or(1, |ack| ack.sequence.saturating_add(1));
        self.snapshot.tray_command_ack = Some(TrayCommandAck {
            sequence,
            command,
            outcome,
        });
    }
    fn publish(&mut self) {
        self.snapshot.history = self.history.iter().cloned().collect();
        self.snapshot.countdown = self.countdown.as_ref().map(|c| {
            c.end
                .saturating_duration_since(Instant::now())
                .as_secs()
                .saturating_add(1)
        });
        *self.latest.lock().unwrap() = Some(self.snapshot.clone());
        (self.wake)();
    }
    fn effective_config(&self) -> Result<bridge_host::config::Config> {
        let mut config = if self.smoke {
            self.settings.to_config_with_environment(
                &self.state_dir,
                Some(""),
                Default::default(),
            )?
        } else {
            self.settings
                .to_config(&self.state_dir, self.password.as_deref())?
        };
        if config.backend == "recording" {
            config.device_host = Some("127.0.0.1".into());
            config.bind_address = Some(std::net::Ipv4Addr::LOCALHOST);
            config.advertise_ip = Some(std::net::Ipv4Addr::LOCALHOST);
            config.allowed_ips = vec![std::net::Ipv4Addr::LOCALHOST];
        }
        Ok(config)
    }
    fn configuration_warning(&mut self) {
        self.snapshot.configuration_warning = if !self.settings.env_file.is_empty()
            && !Path::new(&self.settings.env_file).is_file()
        {
            "The selected .env file was not found. Enter the C64 network password on Connection, or choose an existing .env file in Settings. Desktop launchers do not inherit your terminal configuration.".into()
        } else {
            String::new()
        };
    }
    fn probe_backend(&mut self) {
        if let Ok(config) = self.effective_config() {
            let mut p = backend::probe_backend(&config.backend, runtime::layout(&config));
            if p.name == "portal"
                && self.owned.is_some()
                && self.snapshot.status.as_ref().is_some_and(|status| {
                    status.authorization.phase == control::AuthorizationPhase::Ready
                })
            {
                p.authorized = true;
                p.layout_matches = Some(true);
                p.detail="The GUI posting process has an approved keyboard session; layout and permission remain monitored".into();
            }
            self.snapshot.backend = Some(GuiBackendInfo {
                name: p.name,
                available: p.available,
                authorized: p.authorized,
                layout_matches: p.layout_matches,
                capabilities: p.capabilities,
                detail: p.detail,
            });
        }
    }
    async fn run(
        &mut self,
        mut commands: async_mpsc::Receiver<QueuedAction>,
        mut quit: watch::Receiver<bool>,
        mut safety: watch::Receiver<SafetyState>,
    ) {
        let mut ticker = tokio::time::interval(Duration::from_millis(500));
        loop {
            tokio::select! {
                biased;
                changed=quit.changed()=>{
                    if (changed.is_err() || *quit.borrow()) && self.try_quit().await{break;}
                },
                _=safety.changed()=>{let next=*safety.borrow_and_update();self.apply_safety(next).await;self.publish();},
                queued=commands.recv()=>{
                    let Some(queued)=queued else {let _=self.quit.send(true);continue;};
                    let next=*safety.borrow_and_update();self.apply_safety(next).await;
                    if matches!(queued.action,GuiAction::Enable|GuiAction::TestInput|GuiAction::Configured{operation:ConnectionOperation::TestInput|ConnectionOperation::RequestPermission,..}) && (queued.cancel_epoch!=next.cancel || queued.auto_pause_epoch!=next.auto_pause){if matches!(queued.action,GuiAction::Enable){self.acknowledge_tray_command(TrayCommand::Enable,CommandOutcome::Cancelled);}self.note("Obsolete input request cancelled by a newer safety action");self.publish();continue;}
                    self.command_epoch=Some(queued.gate_epoch);self.command_test_focus_epoch=Some(queued.test_focus_epoch);self.snapshot.busy=true;self.publish();
                    let tray_command=match &queued.action {GuiAction::Enable=>Some(TrayCommand::Enable),GuiAction::Stop=>Some(TrayCommand::Stop),_=>None};
                    let operation=operation_name(&queued.action);
                    let cancelable=matches!(queued.action,GuiAction::Doctor|GuiAction::DeviceInfo|GuiAction::Configured{operation:ConnectionOperation::Doctor|ConnectionOperation::DeviceInfo,..});
                    let result=if cancelable {
                        tokio::select!{biased;_=quit.changed()=>None,result=self.action(queued.action)=>Some(result)}
                    }else{Some(self.action(queued.action).await)};
                    self.command_epoch=None;
                    self.command_test_focus_epoch=None;
                    if result.is_none(){self.snapshot.busy=false;if self.try_quit().await{break;}continue;}
                    let result=result.unwrap();
                    if let Some(command)=tray_command {self.acknowledge_tray_command(command,if result.is_ok(){CommandOutcome::Completed}else{CommandOutcome::Rejected});}
                    if let Err(error)=result {self.error(&format!("{operation}: {}. No automatic retry was made",display_error(&error)));}
                    self.snapshot.busy=false;self.publish();
                },
                _=ticker.tick()=>{let next=*safety.borrow_and_update();self.apply_safety(next).await;self.tick().await;self.publish();},
            }
        }
    }
    async fn try_quit(&mut self) -> bool {
        self.permit.close();
        self.cancel_test_preparation();
        self.join_cancelled_test_preparations().await;
        if self.stop_owned().await.is_ok() {
            self.snapshot.quit_ready = true;
            self.note("Desktop stopped; external CLI sessions are unchanged");
            self.publish();
            return true;
        }
        self.snapshot.phase = GuiPhase::Running;
        self.error("Could not release bridge-owned keys. Input is disabled; retry Pause or Quit after restoring backend access");
        self.event(GuiEvent::ShowWindow);
        let _ = self.quit.send(false);
        self.publish();
        false
    }
    async fn apply_safety(&mut self, next: SafetyState) {
        let previous = self.safety;
        self.safety = next;
        self.focused = next.focused;
        self.snapshot.tray_available = next.tray;
        if previous.cancel != next.cancel || previous.auto_pause != next.auto_pause {
            self.countdown = None;
            self.cancel_test_preparation();
        }
        if self
            .countdown
            .as_ref()
            .is_some_and(|countdown| countdown.focus_epoch != self.permit.test_focus_epoch())
        {
            self.countdown = None;
            self.note("Input test countdown cancelled by returning to the GUI");
        }
        if (previous.pause != next.pause
            || (previous.auto_pause != next.auto_pause && self.owned.is_some()))
            && self.pause().await.is_err()
        {
            self.error("Input disabled; owned-key cleanup failed and remains retryable");
        }
        if previous.stop != next.stop {
            let result = self.action(GuiAction::Stop).await;
            self.acknowledge_tray_command(
                TrayCommand::Stop,
                if result.is_ok() {
                    CommandOutcome::Completed
                } else {
                    CommandOutcome::Rejected
                },
            );
            if result.is_err() {
                self.error("Stop failed during cleanup; input remains disabled for retry");
            }
        }
        if previous.tray_loss != next.tray_loss {
            self.event(GuiEvent::ShowWindow);
            self.note("Tray unavailable; window restored and GUI-owned input paused");
        }
    }
    async fn refresh_status(&mut self) {
        if self.owned.as_ref().is_some_and(ManagedRuntime::is_finished) {
            let mut runtime = self.owned.take().unwrap();
            let completion = runtime.wait().await;
            self.snapshot.owned = false;
            self.snapshot.phase = GuiPhase::Idle;
            if let Some(status) = self.snapshot.status.as_mut() {
                status.running = false;
                status.connected = false;
                status.enabled = false;
            }
            self.countdown = None;
            if let Err(error) = completion {
                self.error(&format!(
                    "Bridge could not continue: {}. Hosting and input are off",
                    display_error(&error)
                ));
            }
        }
        if self.state_dir.join("runtime.json").exists() {
            if let Ok(response) = control::send(&self.state_dir, "status").await {
                self.snapshot.status = Some(response.status);
                self.snapshot.phase = if self
                    .owned
                    .as_ref()
                    .is_some_and(|r| matches!(*r.subscribe().borrow(), RuntimeState::Starting))
                {
                    GuiPhase::Starting
                } else {
                    GuiPhase::Running
                };
                self.snapshot.owned = self.owned.is_some();
            }
        } else if self.owned.is_none() {
            self.snapshot.phase = GuiPhase::Idle;
            if let Some(status) = self.snapshot.status.as_mut() {
                status.running = false;
                status.connected = false;
                status.enabled = false;
            }
        }
    }
    async fn tick(&mut self) {
        self.poll_test_preparation().await;
        if self.overflowed {
            let _ = self.pause().await;
            self.overflowed = false;
        }
        self.refresh_status().await;
        if self
            .countdown
            .as_ref()
            .is_some_and(|c| Instant::now() >= c.end)
        {
            let countdown = self.countdown.take().unwrap();
            self.probe_backend();
            let allowed = !self.focused
                && self.snapshot.backend.as_ref().is_some_and(|p| {
                    p.available
                        && (countdown.prepared.is_some()
                            || (p.authorized && p.layout_matches == Some(true)))
                });
            if !allowed {
                self.error("Input test countdown cancelled: switch away from the GUI and verify layout/permission");
                return;
            }
            if self.snapshot.status.as_ref().is_some_and(|s| s.enabled) {
                let cleanup = self.pause().await;
                self.error(if cleanup.is_ok() {
                    "Input test cancelled because hosting was enabled during its countdown; input is paused"
                } else {
                    "Input test cancelled because key cleanup failed; retry Pause"
                });
                return;
            }
            if !self
                .permit
                .arm_test(countdown.gate_epoch, countdown.focus_epoch)
            {
                self.note("Input test countdown cancelled by a newer focus or safety action");
                return;
            }
            // Hosting stays paused during the independent test phrase.
            // Focus changes close only the test gate; explicit safety actions
            // close both gates, including during blocking native checks.
            let result = if let Some(prepared) = countdown.prepared {
                operations::test_input_prepared(
                    prepared,
                    Some(self.permit.test_allow.clone()),
                    Some(self.permit.focus.clone()),
                )
                .await
            } else {
                match self.effective_config() {
                    Ok(config) => {
                        operations::test_input_guarded_with_focus(
                            &config,
                            self.permit.test_allow.clone(),
                            self.permit.focus.clone(),
                        )
                        .await
                    }
                    Err(error) => Err(error),
                }
            };
            self.permit.close();
            match result {
                Ok(_) => {
                    self.note("Input test accepted 11 pairs; visual delivery remains user-observed")
                }
                Err(error) => {
                    self.error(&format!("Input stayed disabled: {}", display_error(&error)))
                }
            }
            self.refresh_status().await;
        }
    }
    async fn enable_input(&mut self, expected: u64) -> Result<()> {
        self.refresh_status().await;
        if !self
            .snapshot
            .status
            .as_ref()
            .is_some_and(|s| s.running && s.connected)
        {
            bail!("No authenticated client; input stayed disabled");
        }
        // An external CLI session performs its own posting-process checks.
        if self.owned.is_some() {
            self.probe_backend();
            let probe = self
                .snapshot
                .backend
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Input backend could not be checked"))?;
            if !probe.available {
                bail!("Input backend is unavailable; inspect Diagnostics");
            }
            let pending_approval = backend::requires_authorization(&self.settings.backend)?;
            if !probe.authorized && !pending_approval {
                bail!(
                    "GUI posting permission is missing; inspect Diagnostics and enable the application in system settings"
                );
            }
            if probe.layout_matches != Some(true) && !pending_approval {
                bail!(
                    "Active keyboard layout does not match the configured layout; inspect Diagnostics"
                );
            }
        }
        if !self.permit.arm(expected) {
            self.note("Input request cancelled by a newer safety action");
            return Ok(());
        }
        let result = control::send(&self.state_dir, "resume").await;
        match result {
            Ok(response) => {
                self.snapshot.status = Some(response.status);
                if !response.ok {
                    self.permit.close();
                    bail!("{}", response.error.unwrap_or_else(|| "Input was not enabled; inspect the runtime permission and layout checks".into()));
                }
            }
            Err(error) => {
                self.permit.close();
                return Err(error);
            }
        }
        if self.permit.epoch() != expected {
            // A priority Pause/Stop/Quit may arrive while resume is in flight.
            // Its gate already prevents GUI output; also pause external CLI
            // input before acknowledging the obsolete enable operation.
            self.pause().await?;
            self.note("Input request cancelled by a newer safety action");
        } else if self.snapshot.status.as_ref().is_some_and(|status| {
            status.authorization.phase == control::AuthorizationPhase::Pending
        }) {
            self.note("Desktop keyboard authorization pending; input remains disabled. Pause cancels approval");
        } else {
            self.note("Input enabled");
        }
        Ok(())
    }
    async fn pause(&mut self) -> Result<()> {
        self.permit.close();
        self.countdown = None;
        self.cancel_test_preparation();
        if !self.state_dir.join("runtime.json").exists() {
            return Ok(());
        }
        let response = control::send(&self.state_dir, "pause").await?;
        self.snapshot.status = Some(response.status);
        if !response.ok {
            bail!(
                "{}",
                response
                    .error
                    .unwrap_or_else(|| "Key cleanup failed; ownership retained for retry".into())
            );
        }
        self.note("Input paused; owned keys released");
        Ok(())
    }
    async fn stop_owned(&mut self) -> Result<()> {
        self.permit.close();
        self.countdown = None;
        self.cancel_test_preparation();
        self.join_cancelled_test_preparations().await;
        if self
            .owned
            .as_ref()
            .is_some_and(|r| matches!(*r.subscribe().borrow(), RuntimeState::Ready))
        {
            self.pause().await?;
        }
        if let Some(runtime) = self.owned.as_mut() {
            self.snapshot.phase = GuiPhase::Stopping;
            // Managed stop preserves a failed pause for retry before stopping its worker.
            runtime.stop().await?;
            self.owned = None;
            self.snapshot.owned = false;
            if let Some(status) = self.snapshot.status.as_mut() {
                status.running = false;
                status.connected = false;
                status.enabled = false;
            }
        }
        self.snapshot.phase = GuiPhase::Idle;
        Ok(())
    }
    async fn start(&mut self, deploy: bool, allow_reset: bool) -> Result<()> {
        validate_platform_backend(&self.settings)?;
        if self.owned.is_some() || self.state_dir.join("runtime.json").exists() {
            bail!("A bridge is already running");
        }
        if deploy && !allow_reset {
            bail!("Reset confirmation required");
        }
        let config = self.effective_config()?;
        runtime::mapping_profile(&config)?;
        let fake_device = config.backend == "recording";
        if self.smoke && deploy {
            bail!("Smoke cannot deploy");
        }
        self.owned = Some(ManagedRuntime::spawn_guarded(
            config,
            StartOptions {
                enable_input: false,
                deploy,
                allow_reset,
                template: KEYBOARD_PRG.to_vec(),
                fake_device,
            },
            self.permit.allow.clone(),
        ));
        self.snapshot.owned = true;
        self.snapshot.phase = GuiPhase::Starting;
        self.snapshot.settings_pending = false;
        self.note("Starting bridge with input disabled");
        Ok(())
    }
    fn save_form(&mut self, mut settings: GuiSettings, password: String) -> Result<()> {
        validate_platform_backend(&settings)?;
        if settings.credential_id.is_empty() {
            settings.credential_id = if self.settings.credential_id.is_empty() {
                config::random_id()?
            } else {
                self.settings.credential_id.clone()
            };
        }
        if !settings.credential_id.is_empty()
            && !self.settings.credential_id.is_empty()
            && settings.credential_id != self.settings.credential_id
        {
            bail!("Obsolete settings draft after local data reset");
        }
        settings.resolve_paths(&std::env::current_dir()?);
        let next_password = if password.is_empty() {
            self.password.clone()
        } else {
            Some(password)
        };
        settings.to_config(&self.state_dir, next_password.as_deref())?;
        let mut store_failure = false;
        if settings.remember_password {
            let password = next_password.clone().unwrap_or_else(|| {
                settings
                    .to_config(&self.state_dir, None)
                    .map(|c| c.password.expose().to_owned())
                    .unwrap_or_default()
            });
            if self.store.set(&settings.credential_id, &password).is_err() {
                settings.remember_password = self.settings.remember_password;
                store_failure = true;
            }
        } else if self.settings.remember_password
            && self.store.delete(&settings.credential_id).is_err()
        {
            bail!("Stored password deletion failed");
        }
        let mut previous = self.settings.clone();
        previous.remember_password = settings.remember_password;
        let changed = previous != settings || self.password != next_password;
        config::save_settings(&self.state_dir, &settings)?;
        if changed {
            self.cancel_test_for_configuration_change();
        }
        self.settings = settings;
        self.password = next_password;
        self.configuration_warning();
        self.snapshot.settings_pending |= changed
            && (self.owned.is_some() || self.snapshot.status.as_ref().is_some_and(|s| s.running));
        self.event(GuiEvent::SettingsLoaded(self.settings.clone()));
        self.probe_backend();
        if store_failure {
            self.error(
                "Password could not be saved in the OS store; it remains in this session only",
            );
        } else {
            self.note("Settings saved; passwords are excluded from the settings file");
        }
        Ok(())
    }
    /// Buttons act on one validated copy of the current form. This does not
    /// write settings or touch the credential store; Save/Apply remain explicit.
    fn use_form(&mut self, mut settings: GuiSettings, password: String) -> Result<bool> {
        validate_platform_backend(&settings)?;
        if settings.credential_id.is_empty() {
            settings.credential_id = self.settings.credential_id.clone();
        }
        if settings.credential_id != self.settings.credential_id {
            bail!("Obsolete settings draft after local data reset");
        }
        settings.resolve_paths(&std::env::current_dir()?);
        let next_password = if password.is_empty() {
            self.password.clone()
        } else {
            Some(password)
        };
        settings.to_config(&self.state_dir, next_password.as_deref())?;
        // Remember describes the actual saved credential, not an unsaved checkbox.
        let accepted_form = settings.clone();
        settings.remember_password = self.settings.remember_password;
        let changed = settings != self.settings || next_password != self.password;
        if changed {
            self.cancel_test_for_configuration_change();
        }
        self.snapshot.settings_pending |= changed
            && (self.owned.is_some()
                || self
                    .snapshot
                    .status
                    .as_ref()
                    .is_some_and(|status| status.running));
        self.settings = settings;
        self.password = next_password;
        self.configuration_warning();
        self.event(GuiEvent::FormAccepted(accepted_form));
        Ok(changed)
    }
    fn save_profile(&mut self, document: &MappingDocument, path: Option<&Path>) -> Result<PathBuf> {
        document.validate()?;
        let path = if let Some(path) = path {
            path.to_owned()
        } else {
            let directory = self.state_dir.join("profiles");
            pairing::prepare_directory(&directory)?;
            directory.join(format!("{}.toml", config::random_id()?))
        };
        config::atomic_write(&path, document.to_toml()?.as_bytes())?;
        Ok(path)
    }
    async fn action(&mut self, action: GuiAction) -> Result<()> {
        let action = match action {
            GuiAction::Configured {
                settings,
                password,
                operation,
            } => {
                let before = self.permit.epoch();
                if self.use_form(settings, password)? && self.owned.is_some() {
                    self.pause().await?;
                    // Account only for this operation's own pause. A newer
                    // cancel action still invalidates the queued operation.
                    let after = before.wrapping_add(1);
                    if self.command_epoch == Some(before) && self.permit.epoch() == after {
                        self.command_epoch = Some(after);
                    }
                }
                operation.into_action()
            }
            action => action,
        };
        match action {
            GuiAction::Start {
                deploy,
                allow_reset,
            } => self.start(deploy, allow_reset).await?,
            GuiAction::Stop => {
                self.cancel_test_preparation();
                self.join_cancelled_test_preparations().await;
                if self.owned.is_some() {
                    self.stop_owned().await?;
                } else if self.state_dir.join("runtime.json").exists() {
                    let response = control::send(&self.state_dir, "stop").await?;
                    if !response.ok {
                        bail!("External CLI cleanup failed");
                    }
                    self.snapshot.status = Some(response.status);
                }
                self.note("Stop requested; keys released before shutdown");
            }
            GuiAction::Pause => self.pause().await?,
            GuiAction::Enable | GuiAction::TestInput => {
                validate_platform_backend(&self.settings)?;
                let test = matches!(action, GuiAction::TestInput);
                if !test && self.snapshot.settings_pending {
                    bail!(
                        "Apply the changed settings before enabling input; hosting restarts with input disabled"
                    );
                }
                if self.smoke && self.settings.backend != "recording" {
                    bail!("Smoke cannot post native input");
                }
                let before = self.permit.epoch();
                let focus_epoch = self
                    .command_test_focus_epoch
                    .unwrap_or_else(|| self.permit.test_focus_epoch());
                self.pause().await?;
                let expected = before.wrapping_add(1);
                if self.command_epoch.is_some_and(|epoch| epoch != before)
                    || self.permit.epoch() != expected
                {
                    self.note("Input request cancelled by a newer safety action");
                    return Ok(());
                }
                if test {
                    if self.permit.test_focus_epoch() != focus_epoch {
                        self.note("Input test request cancelled by returning to the GUI");
                        return Ok(());
                    }
                    let config = self.effective_config()?;
                    if backend::requires_authorization(&config.backend)? {
                        if self.cancelled_test_preparations.len() >= 4 {
                            bail!(
                                "Previous desktop test authorization is still closing; retry after cleanup"
                            );
                        }
                        let (cancellation, cancel_rx) = watch::channel(false);
                        let task = tokio::spawn(async move {
                            operations::prepare_input_test(&config, cancel_rx).await
                        });
                        self.test_preparation = Some(PendingTestPreparation {
                            cancellation,
                            task,
                            gate_epoch: expected,
                        });
                        self.snapshot.test_authorization_pending = true;
                        self.note("Approve desktop keyboard control for the GUI first; the input test countdown starts after approval. Pause or Cancel closes the request");
                    } else {
                        self.countdown = Some(Countdown {
                            prepared: None,
                            end: Instant::now() + Duration::from_secs(5),
                            gate_epoch: expected,
                            focus_epoch,
                        });
                        self.note("Five-second input test countdown: focus a scratch application");
                    }
                } else {
                    self.enable_input(expected).await?;
                }
            }
            GuiAction::CancelCountdown => {
                self.permit.close();
                self.countdown = None;
                self.cancel_test_preparation();
                self.note("Input countdown cancelled");
            }
            GuiAction::FocusChanged(focused) => {
                self.focused = focused;
                self.permit.focus_changed(focused);
                if focused && self.countdown.take().is_some() {
                    self.note("Input test countdown cancelled by returning to the GUI");
                }
            }
            GuiAction::TrayAvailable(available) => {
                if !available {
                    self.permit.close();
                }
                let lost = self.snapshot.tray_available && !available;
                self.snapshot.tray_available = available;
                if lost {
                    if self.owned.is_some() {
                        self.pause().await?;
                    }
                    self.event(GuiEvent::ShowWindow);
                    self.note("Tray unavailable; window restored and GUI-owned input paused");
                }
            }
            GuiAction::SaveSettings { settings, password } => self.save_form(settings, password)?,
            GuiAction::ApplySettings { settings, password } => {
                let restart = self.owned.is_some();
                self.save_form(settings, password)?;
                if restart {
                    self.stop_owned().await?;
                    self.start(false, false).await?;
                }
            }
            GuiAction::ForgetPassword => {
                self.store.delete(&self.settings.credential_id)?;
                self.settings.remember_password = false;
                config::save_settings(&self.state_dir, &self.settings)?;
                self.password = None;
                self.event(GuiEvent::SettingsLoaded(self.settings.clone()));
                self.note("Stored password removed");
            }
            GuiAction::ImportProfile(path) => {
                let text = config::read_regular(&path, false)?;
                self.event(GuiEvent::ProfileDraftLoaded { text, path });
                self.note("Mapping draft imported; validation is required before applying");
            }
            GuiAction::SaveProfile { document, path } => {
                let path = self.save_profile(&document, path.as_deref())?;
                self.event(GuiEvent::ProfileSaved { path });
                self.note("Validated mapping profile saved");
            }
            GuiAction::ExportProfile { document, path } => {
                self.save_profile(&document, Some(&path))?;
                self.note("Validated mapping exported; editor draft is unchanged");
            }
            GuiAction::ApplyProfile { document } => {
                let path = self.save_profile(&document, None)?;
                self.cancel_test_for_configuration_change();
                let restart = self.owned.is_some();
                self.settings.layout = document.layout.clone();
                self.settings.profile = document.profile.clone();
                self.settings.mappings_file = path.to_string_lossy().into_owned();
                self.snapshot.settings_pending =
                    restart || self.snapshot.status.as_ref().is_some_and(|s| s.running);
                config::save_settings(&self.state_dir, &self.settings)?;
                self.event(GuiEvent::SettingsLoaded(self.settings.clone()));
                self.event(GuiEvent::ProfileSaved { path });
                if restart {
                    self.stop_owned().await?;
                    self.start(false, false).await?;
                } else {
                    self.snapshot.settings_pending =
                        self.snapshot.status.as_ref().is_some_and(|s| s.running);
                }
                self.note("Mapping selected; applying to GUI hosting keeps input off and performs no deployment");
            }
            GuiAction::ExportDiagnostics(path) => {
                let data = diagnostic_export(&self.snapshot)?;
                config::atomic_write(&path, &data)?;
                self.note("Redacted diagnostics exported");
            }
            GuiAction::DeviceInfo => {
                if self.smoke {
                    bail!("Smoke cannot contact devices");
                }
                self.snapshot.device = None;
                let info = RestClient::new(&self.effective_config()?)?.info().await?;
                self.snapshot.device = Some(GuiDeviceInfo {
                    product: info.product,
                    firmware: info.firmware_version,
                    fpga: info.fpga_version,
                    core: info.core_version.unwrap_or_default(),
                });
                self.note("REST information retrieved; this alone does not verify UCI networking");
            }
            GuiAction::Deploy { run, allow_reset } => {
                if self.snapshot.settings_pending {
                    bail!(
                        "Apply the changed settings before deployment so the C64 program matches the running host"
                    );
                }
                if self
                    .owned
                    .as_ref()
                    .is_some_and(|r| matches!(*r.subscribe().borrow(), RuntimeState::Starting))
                {
                    bail!("Wait for startup to complete or cancel it before another deployment");
                }
                if self.smoke || !allow_reset {
                    bail!("Fresh reset confirmation required");
                }
                self.pause().await?;
                operations::deploy(&self.effective_config()?, KEYBOARD_PRG, run, allow_reset)
                    .await?;
                self.note("PRG request completed; input remains disabled");
            }
            GuiAction::Probe { allow_reset } => {
                if self.smoke
                    || !allow_reset
                    || self.owned.is_some()
                    || self.state_dir.join("runtime.json").exists()
                {
                    bail!("Stop hosting and confirm the reset before probing");
                }
                operations::probe(&self.effective_config()?, PROBE_PRG, allow_reset).await?;
                self.note(
                    "Physical networking probe completed; evidence retained in private state",
                );
            }
            GuiAction::Doctor => {
                let config = self.effective_config()?;
                if self.smoke {
                    self.snapshot.checks = vec![GuiCheck {
                        name: "Simulation".into(),
                        state: "isolated".into(),
                        detail: "Recording only; no device or native input".into(),
                    }];
                } else {
                    self.snapshot.checks = diagnostics::doctor(&config)
                        .await
                        .checks
                        .into_iter()
                        .map(|c| GuiCheck {
                            name: c.name,
                            state: c.state,
                            detail: c.detail,
                        })
                        .collect();
                }
                self.probe_backend();
                self.note("Diagnostics refreshed; permission probe applies to this GUI process");
            }
            GuiAction::RequestPermission => {
                if self.smoke {
                    bail!("Smoke cannot request native authorization");
                }
                self.probe_backend();
                let config = self.effective_config()?;
                if backend::requires_authorization(&config.backend)? {
                    self.refresh_status().await;
                    if !self
                        .snapshot
                        .status
                        .as_ref()
                        .is_some_and(|status| status.running)
                    {
                        bail!(
                            "Start hosting with input disabled first; desktop authorization must belong to the running posting process"
                        );
                    }
                    let response = control::send(&self.state_dir, "authorize").await?;
                    self.snapshot.status = Some(response.status);
                    if !response.ok {
                        bail!(
                            "{}",
                            response.error.unwrap_or_else(|| {
                                "Desktop authorization could not be requested".into()
                            })
                        );
                    }
                    self.note(if self.snapshot.owned {"Desktop keyboard approval requested for the GUI posting process; input stays paused"}else{"Desktop keyboard approval requested for the separately running CLI process; input stays paused"});
                } else if self
                    .snapshot
                    .backend
                    .as_ref()
                    .is_some_and(|probe| probe.name == "macos")
                {
                    let result = backend::request_permission();
                    self.probe_backend();
                    result?;
                    self.note("Native authorization checked for the GUI application");
                } else {
                    self.note("GUI input access refreshed; consult the platform instructions in Diagnostics");
                }
            }
            GuiAction::OpenPermissionSettings => {
                if self.open_external(ExternalTarget::MacInputSettings).await? {
                    self.note("Permission settings opened; enable C64 Keyboard Bridge under Device Control and Data Access (Accessibility on older macOS), then refresh Diagnostics");
                }
            }
            GuiAction::OpenProjectWebsite => {
                if self.open_external(ExternalTarget::ProjectWebsite).await? {
                    self.note("Project website opened in the default browser");
                }
            }
            GuiAction::RotatePairing => {
                if self.owned.is_some() {
                    bail!("Stop GUI hosting before rotating pairing");
                }
                pairing::rotate_token(&self.state_dir)?;
                self.note("Pairing token rotated; explicit C64 redeployment is required");
            }
            GuiAction::RemoveLocalData { confirmed } => {
                if !confirmed
                    || self.owned.is_some()
                    || self.state_dir.join("runtime.json").exists()
                {
                    bail!("Stop all hosting and confirm local-data removal first");
                }
                self.store.delete(&self.settings.credential_id)?;
                self.password = None;
                self.countdown = None;
                for name in [
                    "gui-settings.toml",
                    "pairing.key",
                    "pairing.new",
                    "hardware-evidence.json",
                    "hardware-failure.json",
                ] {
                    let path = self.state_dir.join(name);
                    if path.exists() {
                        std::fs::remove_file(path)?;
                    }
                }
                let profiles = self.state_dir.join("profiles");
                if profiles.exists() {
                    pairing::prepare_directory(&profiles)?;
                    for file in std::fs::read_dir(&profiles)? {
                        let path = file?.path();
                        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                        if path.extension().is_some_and(|v| v == "toml")
                            && name.len() == 32
                            && name.bytes().all(|b| b.is_ascii_hexdigit())
                        {
                            std::fs::remove_file(path)?;
                        }
                    }
                }
                self.settings = GuiSettings {
                    env_file: String::new(),
                    credential_id: config::random_id()?,
                    ..Default::default()
                };
                config::save_settings(&self.state_dir, &self.settings)?;
                self.snapshot = GuiSnapshot {
                    tray_available: self.safety.tray,
                    ..Default::default()
                };
                self.history.clear();
                self.probe_backend();
                self.event(GuiEvent::LocalDataReset(self.settings.clone()));
                self.note("Local bridge data reset; imported .env files and exported profiles were preserved");
            }
            GuiAction::Quit => {
                let _ = self.quit.send(true);
            }
            GuiAction::Configured { .. } => {
                unreachable!("Configured operations are resolved above")
            }
        }
        Ok(())
    }
    async fn open_external(&mut self, target: ExternalTarget) -> Result<bool> {
        if self.smoke {
            self.note("Isolated fixture: external navigation suppressed");
            return Ok(false);
        }
        let mut command = external_command(GuiPlatform::current(), target)?;
        run_external(&mut command, target, Duration::from_secs(5)).await?;
        Ok(true)
    }
}

/// Show the controlled outer context, never chained transport errors, response
/// bodies or credential-store diagnostics. The export uses a separate whitelist.
fn display_error(error: &anyhow::Error) -> String {
    error.to_string().chars().take(512).collect()
}

fn operation_name(action: &GuiAction) -> &'static str {
    match action {
        GuiAction::Configured { operation, .. } => operation_name(&(*operation).into_action()),
        GuiAction::Start { .. } => "Start hosting failed",
        GuiAction::Stop => "Stop failed",
        GuiAction::Pause => "Pause failed",
        GuiAction::Enable => "Enable input failed",
        GuiAction::TestInput => "Input test failed",
        GuiAction::SaveSettings { .. } | GuiAction::ApplySettings { .. } => {
            "Settings were not applied"
        }
        GuiAction::DeviceInfo => "Device information failed",
        GuiAction::Deploy { .. } => "Keyboard deployment failed",
        GuiAction::Probe { .. } => "Networking probe failed",
        GuiAction::Doctor => "Diagnostics failed",
        GuiAction::RequestPermission => "Permission request needs attention",
        GuiAction::OpenPermissionSettings => "Accessibility settings could not be opened",
        GuiAction::OpenProjectWebsite => "Website could not be opened",
        GuiAction::RotatePairing => "Pairing rotation failed",
        GuiAction::ImportProfile(_) => "Mapping import failed",
        GuiAction::SaveProfile { .. } | GuiAction::ExportProfile { .. } => "Mapping save failed",
        GuiAction::ApplyProfile { .. } => "Mapping apply failed",
        GuiAction::ExportDiagnostics(_) => "Diagnostic export failed",
        GuiAction::ForgetPassword => "Password could not be forgotten",
        GuiAction::RemoveLocalData { .. } => "Local data could not be reset",
        _ => "Operation failed",
    }
}

impl ConnectionOperation {
    fn into_action(self) -> GuiAction {
        match self {
            Self::Start {
                deploy,
                allow_reset,
            } => GuiAction::Start {
                deploy,
                allow_reset,
            },
            Self::DeviceInfo => GuiAction::DeviceInfo,
            Self::Deploy { run, allow_reset } => GuiAction::Deploy { run, allow_reset },
            Self::Probe { allow_reset } => GuiAction::Probe { allow_reset },
            Self::Doctor => GuiAction::Doctor,
            Self::RequestPermission => GuiAction::RequestPermission,
            Self::TestInput => GuiAction::TestInput,
        }
    }
}

fn validate_platform_backend(settings: &GuiSettings) -> Result<()> {
    if !GuiPlatform::current().supports_backend(&settings.backend) {
        bail!(
            "The configured backend is unavailable on this platform. Select a supported backend in Settings; the imported choice has been preserved"
        );
    }
    Ok(())
}

fn external_command(
    platform: GuiPlatform,
    target: ExternalTarget,
) -> Result<std::process::Command> {
    if target == ExternalTarget::MacInputSettings && platform != GuiPlatform::MacOs {
        bail!("This settings shortcut is available on macOS only");
    }
    let mut command = match platform {
        GuiPlatform::MacOs => std::process::Command::new("/usr/bin/open"),
        GuiPlatform::Windows => {
            let mut command = std::process::Command::new("rundll32.exe");
            command.arg("url.dll,FileProtocolHandler");
            command
        }
        GuiPlatform::Linux => std::process::Command::new("xdg-open"),
    };
    command.arg(target.url());
    Ok(command)
}

/// Poll a bounded child on the service thread without blocking the GUI or Tokio.
/// Never surface desktop-opener output: it can contain local paths or credentials.
async fn run_external(
    command: &mut std::process::Command,
    target: ExternalTarget,
    timeout: Duration,
) -> Result<()> {
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| anyhow::anyhow!(target.failure()))?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => bail!("{}", target.failure()),
            Ok(None) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                bail!("{}", target.failure());
            }
        }
    }
}

pub fn diagnostic_export(snapshot: &GuiSnapshot) -> Result<Vec<u8>> {
    // Only typed counters/capabilities are exported: never arbitrary error strings or UI fields.
    Ok(serde_json::to_vec_pretty(&serde_json::json!({
        "application":"C64 Keyboard Bridge","version":env!("CARGO_PKG_VERSION"),"phase":snapshot.phase,
        "gui_owned":snapshot.owned,
        "backend":snapshot.backend.as_ref().map(|b|serde_json::json!({"available":b.available,"authorized":b.authorized,"layout_matches":b.layout_matches,"capabilities":b.capabilities})),
        "runtime":snapshot.status.as_ref().map(|s|serde_json::json!({"running":s.running,"connected":s.connected,"enabled":s.enabled,"events":s.events,"blocked_combinations":s.blocked_combinations,"diagnostics":s.diagnostics,"authorization":s.authorization})),
    }))?)
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;

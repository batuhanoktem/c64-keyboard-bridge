#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
mod macos;

use anyhow::{Context, Result, bail};
use bridge_gui::{
    app::BridgeApp,
    config,
    instance::{self, Instance, InstanceGuard},
    model::{
        CommandOutcome, GuiAction, GuiEvent, GuiPhase, GuiSettings, GuiSnapshot, TrayCommand,
        TrayCommandAck,
    },
    service::ServiceHandle,
    tray::{Tray, TrayEvent},
};
use clap::Parser;
use eframe::egui;
use serde::Serialize;
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(
    name = "c64-keyboard-bridge-gui",
    version,
    about = "C64 Keyboard Bridge desktop companion"
)]
struct Args {
    /// Private bridge state directory, shared with the CLI.
    #[arg(long)]
    state_dir: Option<PathBuf>,
    #[arg(long)]
    env_file: Option<PathBuf>,
    /// Render the real window using isolated software-only services.
    #[arg(long, requires_all = ["state_dir", "smoke_report", "screenshot"])]
    smoke: bool,
    #[arg(long, requires = "smoke")]
    smoke_report: Option<PathBuf>,
    #[arg(long, requires = "smoke")]
    screenshot: Option<PathBuf>,
    /// Force a theme only for isolated documentation/smoke captures.
    #[arg(long, requires = "smoke", value_parser = ["light", "dark"])]
    smoke_theme: Option<String>,
    #[arg(long, requires = "smoke", value_parser = clap::value_parser!(u16).range(640..=1440))]
    smoke_height: Option<u16>,
    /// Require the harness-owned Linux tray fixture before smoke completion.
    #[arg(long, requires = "smoke")]
    smoke_require_tray: bool,
}

#[derive(Default, Serialize)]
struct SmokeEvidence {
    platform: String,
    theme: String,
    ok: bool,
    frames: u64,
    pages_rendered: Vec<String>,
    screenshots_saved: Vec<String>,
    hosting: bool,
    input_enabled: bool,
    native_input: bool,
    device_contacted: bool,
    tray_seen: bool,
    tray_loss_observed: bool,
    activation_requests: u64,
    quit_ready: bool,
    native_menu_quit_observed: bool,
    native_menu_dirty_preserved: bool,
    native_termination_requests: u64,
    native_termination_dirty_preserved: bool,
    native_termination_hook_restored: bool,
    hidden_window_exercised: bool,
    hidden_tray_commands: Vec<String>,
    rejected_tray_enable: bool,
    tray_stop_completed: bool,
    tray_command_sequence: u64,
    tray_command_ack: Option<TrayCommandAck>,
    #[serde(skip)]
    pending_stop_after: Option<u64>,
    #[serde(skip)]
    pending_enable_after: Option<u64>,
    error: Option<String>,
}
impl SmokeEvidence {
    fn dispatched_hidden_command(&mut self, command: TrayCommand, after_sequence: u64) {
        match command {
            TrayCommand::Stop => {
                self.hidden_tray_commands.push("stop".into());
                self.pending_stop_after = Some(after_sequence);
            }
            TrayCommand::Enable => {
                self.hidden_tray_commands.push("enable".into());
                self.pending_enable_after = Some(after_sequence);
            }
        }
    }
    fn observe_tray_ack(&mut self, ack: TrayCommandAck) {
        let pending = match ack.command {
            TrayCommand::Stop => &mut self.pending_stop_after,
            TrayCommand::Enable => &mut self.pending_enable_after,
        };
        if !pending.is_some_and(|sequence| ack.sequence > sequence) {
            return;
        }
        *pending = None;
        self.tray_command_sequence = ack.sequence;
        self.tray_command_ack = Some(ack);
        match ack.command {
            TrayCommand::Stop => {
                self.tray_stop_completed = ack.outcome == CommandOutcome::Completed
            }
            TrayCommand::Enable => {
                self.rejected_tray_enable = ack.outcome == CommandOutcome::Rejected;
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
struct SmokeProgress {
    hidden_window_exercised: bool,
    screenshots_saved: usize,
    tray_stop_completed: bool,
    rejected_tray_enable: bool,
    tray_command_sequence: u64,
    tray_command_ack: Option<TrayCommandAck>,
}

enum NativeEvent {
    Tray(TrayEvent),
    Show,
    #[cfg(target_os = "macos")]
    Quit,
    #[cfg(target_os = "macos")]
    ApplicationQuit,
    ScreenshotSaved {
        page: String,
        result: std::result::Result<(), String>,
    },
    CaptureProgressFailed,
}
struct ActivationServer {
    stop: tokio::sync::watch::Sender<bool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl ActivationServer {
    fn spawn(guard: &mut InstanceGuard, show: Arc<dyn Fn() + Send + Sync>) -> Result<Self> {
        let listener = guard.take_listener();
        let token = guard.token();
        let (stop, stopping) = tokio::sync::watch::channel(false);
        let thread = thread::Builder::new()
            .name("c64-keyboard-bridge-gui-activation".into())
            .spawn(move || {
                if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    let _ = runtime.block_on(instance::serve(listener, token, show, stopping));
                }
            })?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}
impl Drop for ActivationServer {
    fn drop(&mut self) {
        self.stop.send_replace(true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct NativeApp {
    app: BridgeApp,
    service: ServiceHandle,
    snapshot: GuiSnapshot,
    events: mpsc::Receiver<NativeEvent>,
    sender: mpsc::Sender<NativeEvent>,
    tray: Option<Tray>,
    #[cfg(target_os = "macos")]
    mac_main_thread: Option<bridge_host::backend::MacOsMainThreadContext>,
    #[cfg(target_os = "macos")]
    quit_menu: Option<macos::QuitMenu>,
    #[cfg(target_os = "macos")]
    menu_smoke_stage: u8,
    #[cfg(target_os = "macos")]
    menu_dirty_frame: u64,
    activation: Option<ActivationServer>,
    _instance: InstanceGuard,
    focused: Option<bool>,
    tray_available: bool,
    tray_enabled: bool,
    exiting: bool,
    service_joined: bool,
    smoke: bool,
    #[cfg(target_os = "linux")]
    smoke_require_tray: bool,
    smoke_hidden: bool,
    screenshot: Option<PathBuf>,
    smoke_progress: Option<PathBuf>,
    smoke_progress_sent: Option<SmokeProgress>,
    smoke_progress_revision: u64,
    smoke_progress_written: Arc<Mutex<u64>>,
    captured: BTreeSet<String>,
    requested: BTreeSet<String>,
    received: BTreeSet<String>,
    capture_threads: Vec<thread::JoinHandle<()>>,
    began: Instant,
    evidence: Arc<Mutex<SmokeEvidence>>,
}
impl NativeApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        args: &Args,
        state_dir: PathBuf,
        mut guard: InstanceGuard,
        evidence: Arc<Mutex<SmokeEvidence>>,
    ) -> Result<Self> {
        let (sender, events) = mpsc::channel();
        #[cfg(target_os = "macos")]
        let mac_main_thread = bridge_host::backend::MacOsMainThreadContext::install()?;
        let context = cc.egui_ctx.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || context.request_repaint());
        let mut app = BridgeApp::new_for_state(GuiSettings::default(), &state_dir);
        if args.smoke {
            app.prepare_documentation_capture();
            evidence.lock().unwrap().platform = std::env::consts::OS.into();
            evidence.lock().unwrap().theme =
                args.smoke_theme.clone().unwrap_or_else(|| "system".into());
        }
        if let Some(theme) = &args.smoke_theme {
            cc.egui_ctx.set_theme(if theme == "dark" {
                egui::ThemePreference::Dark
            } else {
                egui::ThemePreference::Light
            });
        }
        let service =
            ServiceHandle::spawn(state_dir, args.env_file.clone(), args.smoke, wake.clone())?;
        let show_tx = sender.clone();
        let show_wake = wake.clone();
        let activation = ActivationServer::spawn(
            &mut guard,
            Arc::new(move || {
                let _ = show_tx.send(NativeEvent::Show);
                show_wake();
            }),
        )?;
        let tray_tx = sender.clone();
        let tray = Tray::new(Arc::new(move |event| {
            let _ = tray_tx.send(NativeEvent::Tray(event));
            wake();
        }))
        .ok();
        #[cfg(target_os = "macos")]
        let quit_menu = {
            let quit_tx = sender.clone();
            let context = cc.egui_ctx.clone();
            let terminate_tx = sender.clone();
            let terminate_context = cc.egui_ctx.clone();
            macos::QuitMenu::install(
                Arc::new(move || {
                    let _ = quit_tx.send(NativeEvent::Quit);
                    context.request_repaint();
                }),
                Arc::new(move || {
                    let _ = terminate_tx.send(NativeEvent::ApplicationQuit);
                    terminate_context.request_repaint();
                }),
            )?
        };
        cc.egui_ctx.all_styles_mut(|style| {
            style.spacing.item_spacing = egui::vec2(10.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 8.0);
        });
        Ok(Self {
            app,
            service,
            snapshot: GuiSnapshot::default(),
            events,
            sender,
            tray,
            #[cfg(target_os = "macos")]
            mac_main_thread: Some(mac_main_thread),
            #[cfg(target_os = "macos")]
            quit_menu: Some(quit_menu),
            #[cfg(target_os = "macos")]
            menu_smoke_stage: 0,
            #[cfg(target_os = "macos")]
            menu_dirty_frame: 0,
            activation: Some(activation),
            _instance: guard,
            focused: None,
            tray_available: false,
            tray_enabled: false,
            exiting: false,
            service_joined: false,
            smoke: args.smoke,
            #[cfg(target_os = "linux")]
            smoke_require_tray: args.smoke_require_tray,
            smoke_hidden: false,
            screenshot: args.screenshot.clone(),
            smoke_progress: args
                .smoke_report
                .as_ref()
                .map(|path| path.with_extension("progress.json")),
            smoke_progress_sent: None,
            smoke_progress_revision: 0,
            smoke_progress_written: Arc::new(Mutex::new(0)),
            captured: BTreeSet::new(),
            requested: BTreeSet::new(),
            received: BTreeSet::new(),
            capture_threads: Vec::new(),
            began: Instant::now(),
            evidence,
        })
    }
    fn show(&self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.request_repaint();
    }
    fn dispatch(&mut self, action: GuiAction, ctx: &egui::Context) {
        if let Err(error) = self.service.send(action) {
            self.app.on_event(&GuiEvent::Error(error.to_string()));
            self.show(ctx);
        }
    }
    fn quit(&mut self, ctx: &egui::Context) {
        self.dispatch(GuiAction::CancelCountdown, ctx);
        if let Some(action) = self.app.request_quit() {
            self.dispatch(action, ctx);
        } else {
            if self.smoke {
                self.evidence.lock().unwrap().native_menu_dirty_preserved =
                    self.app.editor.is_dirty();
            }
            self.show(ctx);
        }
    }
    fn on_tray(&mut self, event: TrayEvent, ctx: &egui::Context) {
        if self.smoke && self.smoke_hidden {
            let command = match event {
                TrayEvent::Stop => Some(TrayCommand::Stop),
                TrayEvent::Enable => Some(TrayCommand::Enable),
                _ => None,
            };
            if let Some(command) = command {
                let after = self.snapshot.tray_command_ack.map_or(0, |ack| ack.sequence);
                self.evidence
                    .lock()
                    .unwrap()
                    .dispatched_hidden_command(command, after);
            }
        }
        match event {
            TrayEvent::Open => self.show(ctx),
            TrayEvent::Pause => self.dispatch(GuiAction::Pause, ctx),
            TrayEvent::Enable => {
                if self.app.form_has_changes() {
                    self.dispatch(GuiAction::CancelCountdown, ctx);
                    self.app.on_event(&GuiEvent::Error(
                        "Apply settings before enabling input".into(),
                    ));
                    self.show(ctx);
                } else {
                    self.dispatch(GuiAction::Enable, ctx);
                }
            }
            TrayEvent::Stop => self.dispatch(GuiAction::Stop, ctx),
            TrayEvent::Quit => self.quit(ctx),
            TrayEvent::Available(available) => {
                let was_available = self.tray_available;
                self.tray_available = available;
                let mut evidence = self.evidence.lock().unwrap();
                evidence.tray_seen |= available;
                evidence.tray_loss_observed |= was_available && !available;
                drop(evidence);
                self.dispatch(GuiAction::TrayAvailable(available), ctx);
                if !available {
                    self.show(ctx);
                }
            }
        }
    }
    fn capture(&mut self, ctx: &egui::Context) {
        let captures = ctx.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| {
                    if let egui::Event::Screenshot {
                        user_data, image, ..
                    } = event
                    {
                        let page = user_data.data.as_ref()?.downcast_ref::<String>()?.clone();
                        Some((page, image.clone()))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        });
        for (page, image) in captures {
            // Hidden-window logic can retain the last visible input batch.
            if !self.received.insert(page.clone()) {
                continue;
            }
            let Some(base) = self.screenshot.clone() else {
                continue;
            };
            let path = if page == "keyboard" {
                base
            } else {
                let stem = base.file_stem().unwrap_or_default().to_string_lossy();
                base.with_file_name(format!("{stem}-{page}.png"))
            };
            let tx = self.sender.clone();
            let wake = ctx.clone();
            self.capture_threads.push(thread::spawn(move || {
                let result = (|| {
                    let width = u32::try_from(image.width())?;
                    let height = u32::try_from(image.height())?;
                    let bytes = image
                        .pixels
                        .iter()
                        .flat_map(egui::Color32::to_srgba_unmultiplied)
                        .collect::<Vec<_>>();
                    image::save_buffer_with_format(
                        path,
                        &bytes,
                        width,
                        height,
                        image::ColorType::Rgba8,
                        image::ImageFormat::Png,
                    )?;
                    Ok::<_, anyhow::Error>(())
                })()
                .map_err(|_| "Could not save the software-only GUI screenshot".into());
                let _ = tx.send(NativeEvent::ScreenshotSaved { page, result });
                wake.request_repaint();
            }));
        }
    }
    fn join_services(&mut self) {
        if !self.service_joined {
            if self.service.shutdown_and_join().is_err() {
                self.evidence.lock().unwrap().error =
                    Some("Desktop service shutdown failed".into());
            }
            self.service_joined = true;
        }
        self.activation.take();
        #[cfg(target_os = "macos")]
        self.mac_main_thread.take();
        #[cfg(target_os = "macos")]
        if self.quit_menu.take().is_some() && self.smoke {
            let restored = macos::termination_hook_restored();
            let mut evidence = self.evidence.lock().unwrap();
            evidence.native_termination_hook_restored = restored;
            evidence.ok &= restored;
        }
        self.tray.take();
        for thread in self.capture_threads.drain(..) {
            let _ = thread.join();
        }
    }
}
impl eframe::App for NativeApp {
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                NativeEvent::Tray(event) => self.on_tray(event, ctx),
                #[cfg(target_os = "macos")]
                NativeEvent::Quit => {
                    self.evidence.lock().unwrap().native_menu_quit_observed = true;
                    self.quit(ctx);
                    self.menu_dirty_frame = self.evidence.lock().unwrap().frames;
                }
                #[cfg(target_os = "macos")]
                NativeEvent::ApplicationQuit => {
                    let mut evidence = self.evidence.lock().unwrap();
                    evidence.native_termination_requests += 1;
                    evidence.native_termination_dirty_preserved |= self.app.editor.is_dirty();
                    drop(evidence);
                    self.quit(ctx);
                    self.menu_dirty_frame = self.evidence.lock().unwrap().frames;
                }
                NativeEvent::Show => {
                    self.evidence.lock().unwrap().activation_requests += 1;
                    self.show(ctx);
                }
                NativeEvent::ScreenshotSaved { page, result } => {
                    if let Err(error) = result {
                        self.evidence.lock().unwrap().error = Some(error);
                    } else {
                        self.captured.insert(page.clone());
                        self.evidence.lock().unwrap().screenshots_saved.push(page);
                    }
                }
                NativeEvent::CaptureProgressFailed => {
                    self.evidence.lock().unwrap().error =
                        Some("Could not save isolated GUI readiness evidence".into());
                    self.quit(ctx);
                }
            }
        }
        while let Some(event) = self.service.try_recv() {
            self.app.on_event(&event);
            match event {
                GuiEvent::Snapshot(snapshot) => {
                    if self.smoke
                        && let Some(ack) = snapshot.tray_command_ack
                    {
                        self.evidence.lock().unwrap().observe_tray_ack(ack);
                    }
                    self.snapshot = *snapshot;
                }
                GuiEvent::Error(_) => self.show(ctx),
                GuiEvent::ShowWindow => self.show(ctx),
                _ => {}
            }
        }
        for action in self.app.take_queued_actions() {
            self.dispatch(action, ctx);
        }
        self.capture(ctx);
        // Each persisted progress change acknowledges work already observed by
        // the GUI. D-Bus dispatch alone is not a background-service result.
        if self.smoke
            && self.smoke_hidden
            && self.tray_available
            && let Some(path) = self.smoke_progress.clone()
        {
            let evidence = self.evidence.lock().unwrap();
            let progress = SmokeProgress {
                hidden_window_exercised: evidence.hidden_window_exercised,
                screenshots_saved: self.captured.len(),
                tray_stop_completed: evidence.tray_stop_completed,
                rejected_tray_enable: evidence.rejected_tray_enable,
                tray_command_sequence: evidence.tray_command_sequence,
                tray_command_ack: evidence.tray_command_ack,
            };
            drop(evidence);
            if self.smoke_progress_sent != Some(progress) {
                self.smoke_progress_sent = Some(progress);
                self.smoke_progress_revision += 1;
                let revision = self.smoke_progress_revision;
                let written = self.smoke_progress_written.clone();
                let sender = self.sender.clone();
                let wake = ctx.clone();
                self.capture_threads.push(thread::spawn(move || {
                    // Save threads may run out of order. Hold this small writer
                    // lock so an older acknowledgment cannot overwrite a newer one.
                    let mut last = written.lock().unwrap();
                    if revision <= *last {
                        return;
                    }
                    let result = serde_json::to_vec(&progress)
                        .map_err(anyhow::Error::from)
                        .and_then(|bytes| config::atomic_write(&path, &bytes));
                    if result.is_err() {
                        let _ = sender.send(NativeEvent::CaptureProgressFailed);
                        wake.request_repaint();
                    } else {
                        *last = revision;
                    }
                }));
            }
        }
        if self.smoke && !self.smoke_hidden && self.tray_available && self.captured.len() == 4 {
            self.smoke_hidden = true;
            self.dispatch(GuiAction::CancelCountdown, ctx);
            self.dispatch(GuiAction::FocusChanged(false), ctx);
            self.focused = Some(false);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.evidence.lock().unwrap().hidden_window_exercised = true;
            ctx.request_repaint();
        }
        let focused = ctx.input(|input| input.viewport().focused.unwrap_or(true));
        if self.focused != Some(focused) {
            self.focused = Some(focused);
            self.dispatch(GuiAction::FocusChanged(focused), ctx);
        }
        let enabled = self
            .snapshot
            .status
            .as_ref()
            .is_some_and(|status| status.enabled);
        if enabled != self.tray_enabled {
            self.tray_enabled = enabled;
            if let Some(tray) = &self.tray {
                tray.set_enabled(enabled);
            }
        }
        if self.snapshot.quit_ready {
            self.exiting = true;
            let mut evidence = self.evidence.lock().unwrap();
            evidence.quit_ready = true;
            evidence.hosting = self
                .snapshot
                .status
                .as_ref()
                .is_some_and(|status| status.running)
                || self.snapshot.phase != GuiPhase::Idle;
            evidence.input_enabled = enabled;
            evidence.ok = evidence.error.is_none()
                && evidence.frames > 0
                && evidence.pages_rendered.len() == 4
                && self.captured.len() == 4
                && !evidence.hosting
                && !enabled;
            #[cfg(target_os = "macos")]
            {
                evidence.ok &= evidence.native_menu_quit_observed
                    && evidence.native_menu_dirty_preserved
                    && evidence.native_termination_requests >= 2
                    && evidence.native_termination_dirty_preserved;
            }
            drop(evidence);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if ctx.input(|input| input.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.tray_available {
                self.dispatch(GuiAction::CancelCountdown, ctx);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            } else {
                // Without a visible tray, closing cannot hide the only control
                // surface. The explicit Quit action still performs cleanup.
                self.show(ctx);
            }
        }
        if self.smoke && !self.exiting {
            if self.began.elapsed() > Duration::from_secs(25) {
                self.evidence.lock().unwrap().error =
                    Some("GUI smoke did not finish rendering within its bound".into());
                self.quit(ctx);
            } else if self.captured.len() == 4 && self.began.elapsed() >= Duration::from_secs(8) {
                #[cfg(target_os = "macos")]
                {
                    let (dirty_preserved, termination_dirty_preserved, frames) = {
                        let evidence = self.evidence.lock().unwrap();
                        (
                            evidence.native_menu_dirty_preserved,
                            evidence.native_termination_dirty_preserved,
                            evidence.frames,
                        )
                    };
                    if self.menu_smoke_stage == 0 {
                        self.app
                            .editor
                            .document
                            .name
                            .push_str("-unsaved-software-smoke");
                        self.menu_smoke_stage = 1;
                        if let Some(menu) = &self.quit_menu {
                            menu.perform_quit();
                        }
                    } else if self.menu_smoke_stage == 1
                        && dirty_preserved
                        && frames >= self.menu_dirty_frame + 2
                    {
                        self.menu_smoke_stage = 2;
                        if let Some(menu) = &self.quit_menu {
                            menu.perform_application_quit();
                        }
                    } else if self.menu_smoke_stage == 2
                        && termination_dirty_preserved
                        && frames >= self.menu_dirty_frame + 2
                    {
                        self.app.editor.discard();
                        self.menu_smoke_stage = 3;
                        if let Some(menu) = &self.quit_menu {
                            menu.perform_application_quit();
                        }
                    }
                }
                #[cfg(target_os = "windows")]
                self.quit(ctx);
                #[cfg(target_os = "linux")]
                if !self.smoke_require_tray || self.evidence.lock().unwrap().tray_loss_observed {
                    self.quit(ctx);
                }
            }
            ctx.request_repaint_after(Duration::from_millis(33));
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let mut page = None;
        if self.smoke {
            let frame = self.evidence.lock().unwrap().frames;
            let index = ((frame / 8) as usize).min(3);
            page = self.app.smoke_page(index);
        }
        egui::CentralPanel::default().show(ui, |ui| {
            let actions = self.app.render(ui, &self.snapshot);
            let context = ui.ctx().clone();
            for action in actions {
                self.dispatch(action, &context);
            }
        });
        let mut evidence = self.evidence.lock().unwrap();
        evidence.frames += 1;
        if let Some(page) = page {
            if !evidence
                .pages_rendered
                .iter()
                .any(|existing| existing == page)
            {
                evidence.pages_rendered.push(page.into());
            }
            if evidence.frames % 8 == 6 && self.requested.insert(page.into()) {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                        page.to_owned(),
                    )));
            }
        }
    }
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        self.join_services();
    }
}
impl Drop for NativeApp {
    fn drop(&mut self) {
        self.join_services();
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("C64 Keyboard Bridge: {error:#}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let args = Args::parse();
    let state_dir = args
        .state_dir
        .clone()
        .or_else(|| {
            if args.smoke {
                None
            } else {
                std::env::var_os("BRIDGE_STATE_DIR").map(PathBuf::from)
            }
        })
        .unwrap_or_else(config::default_state_dir);
    let state_dir =
        std::path::absolute(state_dir).context("Cannot resolve the bridge state directory")?;
    let Instance::Primary(guard) = instance::acquire(&state_dir)? else {
        return Ok(());
    };
    let evidence = Arc::new(Mutex::new(SmokeEvidence::default()));
    let app_evidence = evidence.clone();
    let image = image::load_from_memory_with_format(
        include_bytes!("../../../assets/c64-keyboard-bridge.png"),
        image::ImageFormat::Png,
    )?
    .into_rgba8();
    let viewport = egui::ViewportBuilder::default()
        .with_title("C64 Keyboard Bridge")
        .with_app_id("com.flameliongames.c64keyboardbridge")
        .with_inner_size([
            1200.0,
            if args.smoke {
                f32::from(args.smoke_height.unwrap_or(1100))
            } else {
                840.0
            },
        ])
        .with_min_inner_size([900.0, 640.0])
        .with_icon(egui::IconData {
            rgba: image.as_raw().clone(),
            width: image.width(),
            height: image.height(),
        });
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        persist_window: false,
        ..Default::default()
    };
    let screenshot_args = Args {
        state_dir: args.state_dir.clone(),
        env_file: args.env_file.clone(),
        smoke: args.smoke,
        smoke_report: args.smoke_report.clone(),
        screenshot: args.screenshot.clone(),
        smoke_theme: args.smoke_theme.clone(),
        smoke_height: args.smoke_height,
        smoke_require_tray: args.smoke_require_tray,
    };
    eframe::run_native(
        "C64 Keyboard Bridge",
        options,
        Box::new(move |cc| {
            NativeApp::new(cc, &screenshot_args, state_dir, guard, app_evidence)
                .map(|app| Box::new(app) as Box<dyn eframe::App>)
                .map_err(Into::into)
        }),
    )
    .map_err(|error| anyhow::anyhow!("Desktop window failed: {error}"))?;
    if args.smoke {
        let report = evidence.lock().unwrap();
        if let Some(path) = args.smoke_report {
            config::atomic_write(&path, &serde_json::to_vec_pretty(&*report)?)
                .context("Cannot write GUI smoke report")?;
        }
        if !report.ok {
            bail!("GUI smoke did not confirm complete rendering and graceful shutdown");
        }
    }
    Ok(())
}

#[cfg(test)]
mod smoke_tests {
    use super::*;

    #[test]
    fn tray_progress_requires_a_fresh_matching_service_acknowledgment() {
        let mut evidence = SmokeEvidence::default();
        evidence.dispatched_hidden_command(TrayCommand::Stop, 7);
        evidence.observe_tray_ack(TrayCommandAck {
            sequence: 7,
            command: TrayCommand::Stop,
            outcome: CommandOutcome::Completed,
        });
        assert!(!evidence.tray_stop_completed);
        evidence.observe_tray_ack(TrayCommandAck {
            sequence: 8,
            command: TrayCommand::Enable,
            outcome: CommandOutcome::Rejected,
        });
        assert!(!evidence.rejected_tray_enable);
        assert_eq!(evidence.tray_command_sequence, 0);
        evidence.observe_tray_ack(TrayCommandAck {
            sequence: 8,
            command: TrayCommand::Stop,
            outcome: CommandOutcome::Completed,
        });
        assert!(evidence.tray_stop_completed);
        evidence.dispatched_hidden_command(TrayCommand::Enable, 8);
        evidence.observe_tray_ack(TrayCommandAck {
            sequence: 9,
            command: TrayCommand::Enable,
            outcome: CommandOutcome::Cancelled,
        });
        assert!(!evidence.rejected_tray_enable);
        evidence.dispatched_hidden_command(TrayCommand::Enable, 9);
        evidence.observe_tray_ack(TrayCommandAck {
            sequence: 10,
            command: TrayCommand::Enable,
            outcome: CommandOutcome::Rejected,
        });
        assert!(evidence.rejected_tray_enable);
        assert_eq!(evidence.tray_command_sequence, 10);
    }
}

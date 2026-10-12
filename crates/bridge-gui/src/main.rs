#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
mod macos;

use anyhow::{Context, Result, bail};
use bridge_gui::{
    app::BridgeApp,
    config,
    instance::{self, Instance, InstanceGuard},
    model::{GuiAction, GuiEvent, GuiPhase, GuiSettings, GuiSnapshot},
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
    name = "c64bridge-gui",
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
}

#[derive(Default, Serialize)]
struct SmokeEvidence {
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
    error: Option<String>,
}

enum NativeEvent {
    Tray(TrayEvent),
    Show,
    #[cfg(target_os = "macos")]
    Quit,
    ScreenshotSaved {
        page: String,
        result: std::result::Result<(), String>,
    },
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
            .name("c64bridge-gui-activation".into())
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
    screenshot: Option<PathBuf>,
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
        let context = cc.egui_ctx.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || context.request_repaint());
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
            macos::QuitMenu::install(Arc::new(move || {
                let _ = quit_tx.send(NativeEvent::Quit);
                context.request_repaint();
            }))?
        };
        cc.egui_ctx.all_styles_mut(|style| {
            style.spacing.item_spacing = egui::vec2(10.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 8.0);
        });
        Ok(Self {
            app: BridgeApp::new(GuiSettings::default()),
            service,
            snapshot: GuiSnapshot::default(),
            events,
            sender,
            tray,
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
            screenshot: args.screenshot.clone(),
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
        match event {
            TrayEvent::Open => self.show(ctx),
            TrayEvent::Pause => self.dispatch(GuiAction::Pause, ctx),
            TrayEvent::Enable => self.dispatch(GuiAction::Enable, ctx),
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
        self.quit_menu.take();
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
            }
        }
        while let Some(event) = self.service.try_recv() {
            self.app.on_event(&event);
            match event {
                GuiEvent::Snapshot(snapshot) => self.snapshot = *snapshot,
                GuiEvent::ShowWindow | GuiEvent::Error(_) => self.show(ctx),
                _ => {}
            }
        }
        for action in self.app.take_queued_actions() {
            self.dispatch(action, ctx);
        }
        self.capture(ctx);
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
                evidence.ok &=
                    evidence.native_menu_quit_observed && evidence.native_menu_dirty_preserved;
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
                    let (dirty_preserved, frames) = {
                        let evidence = self.evidence.lock().unwrap();
                        (evidence.native_menu_dirty_preserved, evidence.frames)
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
                        self.app.editor.discard();
                        self.menu_smoke_stage = 2;
                        if let Some(menu) = &self.quit_menu {
                            menu.perform_quit();
                        }
                    }
                }
                #[cfg(not(target_os = "macos"))]
                self.quit(ctx);
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
    let Instance::Primary(guard) = instance::acquire(&state_dir)? else {
        return Ok(());
    };
    let evidence = Arc::new(Mutex::new(SmokeEvidence::default()));
    let app_evidence = evidence.clone();
    let image = image::load_from_memory_with_format(
        include_bytes!("../../../assets/c64bridge.png"),
        image::ImageFormat::Png,
    )?
    .into_rgba8();
    let viewport = egui::ViewportBuilder::default()
        .with_title("C64 Keyboard Bridge")
        .with_app_id("com.flameliongames.c64bridge")
        .with_inner_size([1200.0, 840.0])
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

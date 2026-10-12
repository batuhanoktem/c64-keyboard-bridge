//! Desktop presentation. All I/O and native input belong to the ordered service.
use crate::{
    editor::{EditorIntent, MappingEditor},
    model::{GuiAction, GuiEvent, GuiPhase, GuiSettings, GuiSnapshot},
    presentation,
};
use eframe::egui;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Connection,
    Keyboard,
    Diagnostics,
    Settings,
}
#[derive(Clone)]
enum Confirm {
    StartDeploy,
    RunClient,
    LoadClient,
    Probe,
    RotatePairing,
    InputTest,
    WriteProfile {
        document: bridge_core::MappingDocument,
        path: PathBuf,
        export: bool,
    },
    WriteDiagnostics(PathBuf),
    RemoveLocalData,
}
impl Confirm {
    fn title(&self) -> &'static str {
        match self {
            Self::StartDeploy => "Start and deploy keyboard client?",
            Self::RunClient => "Deploy and run keyboard client?",
            Self::LoadClient => "Load keyboard client?",
            Self::Probe => "Run networking probe?",
            Self::RotatePairing => "Rotate pairing token?",
            Self::InputTest => "Run native input test?",
            Self::WriteProfile { export, .. } => {
                if *export {
                    "Export mapping profile?"
                } else {
                    "Save mapping copy?"
                }
            }
            Self::WriteDiagnostics(_) => "Export redacted diagnostics?",
            Self::RemoveLocalData => "Reset local bridge data?",
        }
    }
    fn detail(&self) -> &'static str {
        match self {
            Self::StartDeploy | Self::RunClient | Self::LoadClient | Self::Probe => {
                "This operation resets the C64 and replaces its current BASIC program. Save your C64 work first. Host keyboard input stays disabled. A timed-out deployment is not automatically retried."
            }
            Self::RotatePairing => {
                "Hosting must be stopped before rotation. Previously configured C64 programs will no longer authenticate. Deploy a newly paired program deliberately when ready."
            }
            Self::InputTest => {
                "After a five-second countdown the bridge will type ‘c64 bridge’ and Enter into the focused application. Open an empty scratch document and switch to it during the countdown."
            }
            Self::WriteProfile { .. } | Self::WriteDiagnostics(_) => {
                "This writes the selected file and replaces its contents if it already exists. Choose a different path to keep an existing file."
            }
            Self::RemoveLocalData => {
                "This deletes saved GUI settings, pairing state, GUI-owned mapping profiles and the saved OS password. Your mapping draft is cleared. Imported .env files and external or exported profiles are left untouched. Hosting must be stopped. C64 redeployment is required to pair again."
            }
        }
    }
    fn action(&self) -> GuiAction {
        match self {
            Self::StartDeploy => GuiAction::Start {
                deploy: true,
                allow_reset: true,
            },
            Self::RunClient => GuiAction::Deploy {
                run: true,
                allow_reset: true,
            },
            Self::LoadClient => GuiAction::Deploy {
                run: false,
                allow_reset: true,
            },
            Self::Probe => GuiAction::Probe { allow_reset: true },
            Self::RotatePairing => GuiAction::RotatePairing,
            Self::InputTest => GuiAction::TestInput,
            Self::WriteProfile {
                document,
                path,
                export,
            } => {
                if *export {
                    GuiAction::ExportProfile {
                        document: document.clone(),
                        path: path.clone(),
                    }
                } else {
                    GuiAction::SaveProfile {
                        document: document.clone(),
                        path: Some(path.clone()),
                    }
                }
            }
            Self::WriteDiagnostics(path) => GuiAction::ExportDiagnostics(path.clone()),
            Self::RemoveLocalData => GuiAction::RemoveLocalData { confirmed: true },
        }
    }
}

pub struct BridgeApp {
    pub settings: GuiSettings,
    pub editor: MappingEditor,
    page: Page,
    password: String,
    confirm: Option<Confirm>,
    quit_dialog: bool,
    quit_after_save: bool,
    pending_save: Option<bridge_core::MappingDocument>,
    queued: Vec<GuiAction>,
    diagnostics_path: String,
    local_error: Option<String>,
}
impl BridgeApp {
    pub fn new(settings: GuiSettings) -> Self {
        Self {
            editor: MappingEditor::new(&settings.profile, &settings.layout),
            settings,
            page: Page::Connection,
            password: String::new(),
            confirm: None,
            quit_dialog: false,
            quit_after_save: false,
            pending_save: None,
            queued: Vec::new(),
            diagnostics_path: "c64bridge-diagnostics.json".into(),
            local_error: None,
        }
    }
    /// Presentation-only selector used by the isolated GUI launch fixture.
    pub fn smoke_page(&mut self, index: usize) -> Option<&'static str> {
        let (page, label) = match index {
            0 => (Page::Connection, "connection"),
            1 => (Page::Keyboard, "keyboard"),
            2 => (Page::Diagnostics, "diagnostics"),
            3 => (Page::Settings, "settings"),
            _ => return None,
        };
        self.page = page;
        Some(label)
    }
    pub fn on_event(&mut self, event: &GuiEvent) {
        match event {
            GuiEvent::SettingsLoaded(settings) => {
                if !self.editor.is_dirty()
                    && self.pending_save.is_none()
                    && settings.mappings_file.is_empty()
                    && (self.editor.document.profile != settings.profile
                        || self.editor.document.layout != settings.layout)
                {
                    self.editor = MappingEditor::new(&settings.profile, &settings.layout);
                }
                self.settings = settings.clone();
            }
            GuiEvent::ProfileLoaded { document, path } => {
                self.editor.load(document.clone(), path.clone())
            }
            GuiEvent::ProfileDraftLoaded { text, path } => {
                self.editor.load_raw(text.clone(), path.clone());
            }
            GuiEvent::LocalDataReset(settings) => {
                self.settings = settings.clone();
                self.password.clear();
                self.editor = MappingEditor::new(&settings.profile, &settings.layout);
                self.confirm = None;
                self.quit_dialog = false;
                self.quit_after_save = false;
                self.pending_save = None;
                self.local_error = None;
            }
            GuiEvent::ProfileSaved { path } => {
                if let Some(document) = self.pending_save.take() {
                    self.editor.acknowledge_saved(document, path);
                }
                if self.quit_after_save {
                    self.quit_after_save = false;
                    if self.editor.is_dirty() {
                        self.quit_dialog = true;
                    } else {
                        self.queued.push(GuiAction::Quit);
                    }
                }
            }
            GuiEvent::Error(error) => {
                self.local_error = Some(error.clone());
                self.quit_after_save = false;
                self.pending_save = None;
            }
            _ => {}
        }
    }
    /// Drain asynchronous decisions during `App::logic`, including while hidden.
    pub fn take_queued_actions(&mut self) -> Vec<GuiAction> {
        std::mem::take(&mut self.queued)
    }
    /// Quit from a tray command follows the same unsaved-draft decision as the window.
    pub fn request_quit(&mut self) -> Option<GuiAction> {
        self.confirm = None;
        self.queued.push(GuiAction::CancelCountdown);
        if self.editor.is_dirty() {
            self.quit_dialog = true;
            None
        } else {
            Some(GuiAction::Quit)
        }
    }
    pub fn render(&mut self, ui: &mut egui::Ui, snapshot: &GuiSnapshot) -> Vec<GuiAction> {
        let mut actions = self.take_queued_actions();
        let context = ui.ctx().clone();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("C64")
                    .size(24.0)
                    .strong()
                    .color(presentation::accent(ui)),
            );
            ui.label(egui::RichText::new("Keyboard Bridge").size(23.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let enabled = snapshot
                    .status
                    .as_ref()
                    .is_some_and(|status| status.enabled);
                let connected = snapshot
                    .status
                    .as_ref()
                    .is_some_and(|status| status.connected);
                presentation::status(
                    ui,
                    if enabled {
                        "Input enabled"
                    } else {
                        "Input paused"
                    },
                    enabled,
                );
                presentation::status(
                    ui,
                    if connected {
                        "C64 connected"
                    } else {
                        "No C64 session"
                    },
                    connected,
                );
            });
        });
        ui.separator();
        if let Some(seconds) = snapshot.countdown {
            presentation::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "Switch to your scratch application · {seconds}s"
                        ))
                        .strong(),
                    );
                    if ui.button("Cancel countdown").clicked() {
                        actions.push(GuiAction::CancelCountdown);
                    }
                });
                ui.small("Input remains disabled if this window is focused or the connection/layout checks fail.");
            });
        }
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(164.0);
                ui.add_space(10.0);
                for (page, label) in [
                    (Page::Connection, "Connection"),
                    (Page::Keyboard, "Keyboard / Mappings"),
                    (Page::Diagnostics, "Diagnostics"),
                    (Page::Settings, "Settings / About"),
                ] {
                    if ui
                        .add_sized(
                            [164.0, 38.0],
                            egui::Button::new(label).selected(self.page == page),
                        )
                        .clicked()
                    {
                        self.page = page;
                    }
                }
                ui.add_space(20.0);
                ui.small("Version 0.1.0");
                ui.small("GPL-3.0-only");
                if !snapshot.tray_available {
                    ui.small("Tray unavailable; keep this window open.");
                }
                ui.add_space(10.0);
                if ui.button("Quit").clicked()
                    && let Some(action) = self.request_quit()
                {
                    actions.push(action);
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("page-content")
                .show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_width(ui.available_width());
                        ui.add_space(12.0);
                        match self.page {
                            Page::Connection => self.connection(ui, snapshot, &mut actions),
                            Page::Keyboard => {
                                for intent in self.editor.render(
                                    ui,
                                    snapshot
                                        .backend
                                        .as_ref()
                                        .map(|backend| backend.capabilities),
                                ) {
                                    match intent {
                                        EditorIntent::Import(path) => {
                                            actions.push(GuiAction::ImportProfile(path))
                                        }
                                        EditorIntent::Save {
                                            document,
                                            path: None,
                                        } => actions.push(GuiAction::SaveProfile {
                                            document,
                                            path: None,
                                        }),
                                        EditorIntent::Save {
                                            document,
                                            path: Some(path),
                                        } => {
                                            self.confirm = Some(Confirm::WriteProfile {
                                                document,
                                                path,
                                                export: false,
                                            })
                                        }
                                        EditorIntent::Apply(document) => {
                                            actions.push(GuiAction::ApplyProfile { document })
                                        }
                                        EditorIntent::Export { document, path } => {
                                            self.confirm = Some(Confirm::WriteProfile {
                                                document,
                                                path,
                                                export: true,
                                            })
                                        }
                                    }
                                }
                            }
                            Page::Diagnostics => self.diagnostics(ui, snapshot, &mut actions),
                            Page::Settings => self.settings_page(ui, snapshot, &mut actions),
                        }
                        if !snapshot.message.is_empty() {
                            ui.add_space(12.0);
                            ui.label(&snapshot.message);
                        }
                        if let Some(error) = &self.local_error {
                            ui.add_space(8.0);
                            ui.colored_label(ui.visuals().error_fg_color, error);
                            if ui.small_button("Dismiss").clicked() {
                                self.local_error = None;
                            }
                        }
                    });
                });
        });
        if let Some(confirmation) = self.confirm.clone() {
            let mut dismiss = false;
            egui::Modal::new(egui::Id::new("deliberate-operation")).show(&context, |ui| {
                ui.set_max_width(450.0);
                ui.heading(confirmation.title());
                if let Confirm::WriteProfile { path, .. } | Confirm::WriteDiagnostics(path) =
                    &confirmation
                {
                    ui.label(format!("File: {}", path.display()));
                }
                ui.label(confirmation.detail());
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        dismiss = true;
                    }
                    if ui
                        .add_enabled(!snapshot.busy, egui::Button::new("Confirm"))
                        .clicked()
                    {
                        actions.push(confirmation.action());
                        dismiss = true;
                    }
                });
            });
            if dismiss {
                self.confirm = None;
            }
        }
        if self.quit_dialog {
            let mut dismiss = false;
            egui::Modal::new(egui::Id::new("dirty-quit"))
                .show(&context, |ui| {
                    ui.set_max_width(460.0); ui.heading("Save mapping changes before quitting?"); ui.label("Your mapping draft has unsaved changes. Hosting is stopped gracefully after this decision.");
                    let valid = self.editor.validated_document();
                    if let Err(error) = &valid { ui.colored_label(ui.visuals().error_fg_color, error); }
                    ui.horizontal(|ui| {
                        if ui.add_enabled(valid.is_ok() && !self.quit_after_save, egui::Button::new("Save and quit")).clicked() {
                            self.quit_after_save = true;
                            actions.push(GuiAction::SaveProfile { document: self.editor.document.clone(), path: None }); dismiss = true;
                        }
                        if ui.button("Discard and quit").clicked() { self.editor.discard(); actions.push(GuiAction::Quit); dismiss = true; }
                        if ui.button("Cancel").clicked() { dismiss = true; }
                    });
                });
            if dismiss {
                self.quit_dialog = false;
            }
        }
        actions.extend(self.take_queued_actions());
        for action in &actions {
            if let GuiAction::SaveProfile { document, .. } | GuiAction::ApplyProfile { document } =
                action
            {
                self.pending_save = Some(document.clone());
            }
        }
        actions
    }
    fn connection(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &GuiSnapshot,
        actions: &mut Vec<GuiAction>,
    ) {
        presentation::heading(
            ui,
            "Connection",
            "Host the C64 connection, then deliberately enable input when a scratch application is ready.",
        );
        presentation::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(format!("{:?}", snapshot.phase));
                if snapshot.phase != GuiPhase::Idle {
                    ui.label(if snapshot.owned {
                        "GUI-owned session"
                    } else {
                        "Existing CLI session"
                    });
                }
            });
            if !snapshot.owned && snapshot.phase != GuiPhase::Idle {
                ui.label("These controls can manage the CLI session. Quitting this GUI leaves that separately started host running.");
            }
            if let Some(status) = &snapshot.status {
                ui.label(&status.detail);
            }
            ui.horizontal_wrapped(|ui| {
                let running = snapshot.phase == GuiPhase::Running;
                if ui
                    .add_enabled(
                        snapshot.phase == GuiPhase::Idle && !snapshot.busy,
                        egui::Button::new("Start host"),
                    )
                    .clicked()
                {
                    actions.push(GuiAction::Start {
                        deploy: false,
                        allow_reset: false,
                    });
                }
                if ui
                    .add_enabled(
                        snapshot.phase == GuiPhase::Idle && !snapshot.busy,
                        egui::Button::new("Start + deploy…"),
                    )
                    .clicked()
                {
                    self.confirm = Some(Confirm::StartDeploy);
                }
                if ui
                    .add_enabled(running, egui::Button::new("Pause input"))
                    .clicked()
                {
                    actions.push(GuiAction::Pause);
                }
                if ui
                    .add_enabled(
                        running && snapshot.countdown.is_none() && !snapshot.busy,
                        egui::Button::new("Enable input in 5s"),
                    )
                    .clicked()
                {
                    actions.push(GuiAction::Enable);
                }
                if ui
                    .add_enabled(
                        snapshot.phase != GuiPhase::Idle && snapshot.phase != GuiPhase::Stopping,
                        egui::Button::new("Stop host"),
                    )
                    .clicked()
                {
                    actions.push(GuiAction::Stop);
                }
            });
        });
        ui.add_space(12.0);
        presentation::card(ui, |ui| {
            ui.heading("C64 Ultimate");
            egui::Grid::new("connection-settings")
                .spacing([18.0, 9.0])
                .show(ui, |ui| {
                    presentation::field(
                        ui,
                        "Device hostname / IPv4",
                        &mut self.settings.device_host,
                        "Enter the device hostname or IPv4 address",
                    );
                    ui.label("REST port");
                    ui.add(egui::DragValue::new(&mut self.settings.device_port).range(1..=65535));
                    ui.end_row();
                    ui.label("Bridge TCP port");
                    ui.add(egui::DragValue::new(&mut self.settings.bridge_port).range(1..=65535));
                    ui.end_row();
                });
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!snapshot.busy, egui::Button::new("Save settings"))
                    .clicked()
                {
                    actions.push(self.save_settings(false));
                }
                if ui
                    .add_enabled(!snapshot.busy, egui::Button::new("Apply settings"))
                    .clicked()
                {
                    actions.push(self.save_settings(true));
                }
                if ui
                    .add_enabled(!snapshot.busy, egui::Button::new("Device information"))
                    .clicked()
                {
                    actions.push(GuiAction::DeviceInfo);
                }
            });
            if snapshot.settings_pending {
                ui.colored_label(ui.visuals().warn_fg_color, if snapshot.owned {"Settings are saved. Apply them to restart GUI-owned hosting with input disabled."} else {"Settings are pending for the CLI-owned session. Stop it explicitly before restarting with these settings."});
            }
            ui.small("Connection changes may require redeploying the C64 program. Applying settings never deploys automatically.");
            if let Some(device) = &snapshot.device {
                ui.add_space(8.0);
                ui.label(format!(
                    "{} · firmware {} · FPGA {} · core {}",
                    device.product, device.firmware, device.fpga, device.core
                ));
            }
        });
        ui.add_space(12.0);
        ui.collapsing("Deployment and networking probe", |ui| {
            ui.label(
                "Each load/run resets the machine and replaces the program. Input stays disabled.",
            );
            ui.horizontal_wrapped(|ui| {
                for (label, confirmation) in [
                    ("Deploy and run client…", Confirm::RunClient),
                    ("Load client…", Confirm::LoadClient),
                    ("Run networking probe…", Confirm::Probe),
                ] {
                    if ui
                        .add_enabled(!snapshot.busy, egui::Button::new(label))
                        .clicked()
                    {
                        self.confirm = Some(confirmation);
                    }
                }
            });
        });
    }
    fn save_settings(&self, apply: bool) -> GuiAction {
        if apply {
            GuiAction::ApplySettings {
                settings: self.settings.clone(),
                password: self.password.clone(),
            }
        } else {
            GuiAction::SaveSettings {
                settings: self.settings.clone(),
                password: self.password.clone(),
            }
        }
    }
    fn diagnostics(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &GuiSnapshot,
        actions: &mut Vec<GuiAction>,
    ) {
        presentation::heading(
            ui,
            "Diagnostics",
            "Retained session evidence and permission checks without capturing your host keyboard.",
        );
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!snapshot.busy, egui::Button::new("Run doctor"))
                .clicked()
            {
                actions.push(GuiAction::Doctor);
            }
            if ui
                .add_enabled(
                    !snapshot.busy,
                    egui::Button::new("Check / request permission"),
                )
                .clicked()
            {
                actions.push(GuiAction::RequestPermission);
            }
            if ui
                .add_enabled(!snapshot.busy, egui::Button::new("Native input test…"))
                .clicked()
            {
                self.confirm = Some(Confirm::InputTest);
            }
        });
        if let Some(backend) = &snapshot.backend {
            presentation::card(ui, |ui| {
                ui.label(format!(
                    "Permission probe: GUI executable · backend {}",
                    backend.name
                ));
                if !snapshot.owned && snapshot.phase == GuiPhase::Running {
                    ui.label("Session input is posted by the CLI executable. Its authenticated controls validate that process before enabling input.");
                }
                ui.label(format!(
                    "Available: {} · authorized: {} · layout: {}",
                    backend.available,
                    backend.authorized,
                    match backend.layout_matches {
                        Some(true) => "matches",
                        Some(false) => "mismatch",
                        None => "unknown",
                    }
                ));
                ui.label(format!(
                    "Key events: {} · text: {} · repeat: {}",
                    backend.capabilities.key_events,
                    backend.capabilities.text,
                    backend.capabilities.repeat
                ));
                ui.label(&backend.detail);
            });
        }
        if let Some(status) = &snapshot.status {
            ui.add_space(10.0);
            presentation::card(ui, |ui| {
                let diagnostics = &status.diagnostics;
                ui.heading("Session evidence");
                ui.label(format!(
                    "{} attempts · {} authenticated · {} reconnects",
                    diagnostics.attempts,
                    diagnostics.authenticated_sessions,
                    diagnostics.reconnects
                ));
                ui.label(format!(
                    "Phase {} · valid frame age {} · {} validated frames",
                    diagnostics.current.phase.as_str(),
                    diagnostics
                        .current
                        .valid_frame_age_ms
                        .map_or("unavailable".into(), |age| format!("{age} ms")),
                    diagnostics.current.validated_frames
                ));
                if let Some(failure) = &diagnostics.last_failure {
                    ui.label(format!(
                        "Last failure: {} during {} · {} ms elapsed · cleanup {:?}",
                        failure.reason.as_str(),
                        failure.phase.as_str(),
                        failure.elapsed_ms,
                        failure.cleanup
                    ));
                }
                if let Ok(counters) = serde_json::to_string_pretty(&diagnostics.failures) {
                    ui.monospace(counters);
                }
            });
        }
        for check in &snapshot.checks {
            ui.label(format!(
                "{} · {} · {}",
                check.name, check.state, check.detail
            ));
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label("Export path");
            ui.text_edit_singleline(&mut self.diagnostics_path);
            if ui
                .add_enabled(
                    !self.diagnostics_path.trim().is_empty() && !snapshot.busy,
                    egui::Button::new("Export diagnostics"),
                )
                .clicked()
            {
                self.confirm = Some(Confirm::WriteDiagnostics(PathBuf::from(
                    self.diagnostics_path.trim(),
                )));
            }
        });
        ui.small("Exports exclude credentials, addresses, key identities, matrices and device-supplied text.");
        ui.add_space(12.0);
        ui.heading("Activity");
        for activity in snapshot.history.iter().rev().take(128) {
            ui.label(format!(
                "+{:.1}s  {}",
                activity.elapsed_ms as f64 / 1000.0,
                activity.summary
            ));
        }
    }
    fn settings_page(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &GuiSnapshot,
        actions: &mut Vec<GuiAction>,
    ) {
        presentation::heading(
            ui,
            "Settings & about",
            "Explicit GUI values override process environment and .env. Your imported .env file is left unchanged.",
        );
        presentation::card(ui, |ui| {
            ui.heading("Credentials");
            ui.label("Network password");
            ui.add(
                egui::TextEdit::singleline(&mut self.password)
                    .password(true)
                    .hint_text("Leave empty to use the existing environment / saved credential")
                    .desired_width(350.0),
            );
            ui.checkbox(
                &mut self.settings.remember_password,
                "Remember in the operating system credential store",
            );
            ui.small("macOS Keychain · Windows Credential Manager · Linux Secret Service. No plaintext fallback.");
            if ui.button("Forget saved password").clicked() {
                self.password.clear();
                actions.push(GuiAction::ForgetPassword);
            }
        });
        ui.add_space(12.0);
        egui::Grid::new("advanced-settings")
            .spacing([18.0, 9.0])
            .show(ui, |ui| {
                presentation::field(ui, ".env import path", &mut self.settings.env_file, ".env");
                presentation::field(
                    ui,
                    "Bind IPv4 override",
                    &mut self.settings.bind_address,
                    "Automatic route when empty",
                );
                presentation::field(
                    ui,
                    "Advertised IPv4 override",
                    &mut self.settings.advertise_ip,
                    "Automatic route when empty",
                );
                presentation::field(
                    ui,
                    "Allowed device IPv4 addresses",
                    &mut self.settings.allowed_ips,
                    "Comma-separated; configured device when empty",
                );
                ui.label("Host keyboard layout");
                egui::ComboBox::from_id_salt("host-layout")
                    .selected_text(&self.settings.layout)
                    .show_ui(ui, |ui| {
                        for value in ["US", "UK"] {
                            ui.selectable_value(&mut self.settings.layout, value.into(), value);
                        }
                    });
                ui.end_row();
                ui.label("Base mapping");
                egui::ComboBox::from_id_salt("host-profile")
                    .selected_text(&self.settings.profile)
                    .show_ui(ui, |ui| {
                        for value in ["modern", "authentic"] {
                            ui.selectable_value(&mut self.settings.profile, value.into(), value);
                        }
                    });
                ui.end_row();
                ui.label("Input backend");
                egui::ComboBox::from_id_salt("host-backend")
                    .selected_text(&self.settings.backend)
                    .show_ui(ui, |ui| {
                        for value in ["auto", "macos", "windows", "x11", "uinput", "recording"] {
                            ui.selectable_value(&mut self.settings.backend, value.into(), value);
                        }
                    });
                ui.end_row();
                presentation::field(
                    ui,
                    "Mapping file",
                    &mut self.settings.mappings_file,
                    "Built-in profile when empty",
                );
                ui.label("Repeat delay (ms)");
                ui.add(egui::DragValue::new(&mut self.settings.repeat_delay_ms).range(100..=5000));
                ui.end_row();
                ui.label("Repeat interval (ms)");
                ui.add(
                    egui::DragValue::new(&mut self.settings.repeat_interval_ms).range(10..=1000),
                );
                ui.end_row();
                ui.label("Log level");
                egui::ComboBox::from_id_salt("log-level")
                    .selected_text(&self.settings.log_level)
                    .show_ui(ui, |ui| {
                        for value in ["error", "warn", "info", "debug", "trace"] {
                            ui.selectable_value(&mut self.settings.log_level, value.into(), value);
                        }
                    });
                ui.end_row();
            });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!snapshot.busy, egui::Button::new("Save settings"))
                .clicked()
            {
                actions.push(self.save_settings(false));
            }
            if ui
                .add_enabled(!snapshot.busy, egui::Button::new("Apply settings"))
                .clicked()
            {
                actions.push(self.save_settings(true));
            }
            if ui
                .add_enabled(
                    !snapshot.busy && snapshot.phase == GuiPhase::Idle,
                    egui::Button::new("Rotate pairing token…"),
                )
                .clicked()
            {
                self.confirm = Some(Confirm::RotatePairing);
            }
        });
        if ui
            .add_enabled(
                !snapshot.busy && snapshot.phase == GuiPhase::Idle,
                egui::Button::new("Reset local bridge data…"),
            )
            .clicked()
        {
            self.confirm = Some(Confirm::RemoveLocalData);
        }
        ui.small("Linux uses desktop repeat. These settings never change global Linux repeat or grant uinput access.");
        ui.add_space(20.0);
        ui.separator();
        ui.label(egui::RichText::new("C64 Keyboard Bridge 0.1.0").strong());
        ui.label(
            "GPL-3.0-only · Rust host with genuine cc65 C64 programs · wire protocol version 1",
        );
        ui.hyperlink_to(
            "Source, installation and hardware acceptance notes",
            "https://github.com/batuhanoktem/c64-keyboard-bridge",
        );
        ui.label("Closing hides to the tray when available. Quit releases GUI-owned keys and waits for graceful cleanup.");
        ui.label("Pairing tokens are transmitted in plaintext on the LAN. Use a trusted network.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_core::{BindingDocument, NativeKey};
    use bridge_protocol::PhysicalKey;
    #[test]
    fn quitting_dirty_draft_requires_decision_and_failed_save_does_not_quit() {
        let mut app = BridgeApp::new(GuiSettings::default());
        app.editor.document.bindings.push(BindingDocument {
            key: PhysicalKey::A,
            modifiers: vec![],
            keys: vec![NativeKey::B],
            text: None,
            shift: None,
        });
        assert!(app.request_quit().is_none());
        assert!(app.quit_dialog);
        app.quit_after_save = true;
        app.on_event(&GuiEvent::Error("save failed".into()));
        assert!(!app.quit_after_save);
        assert!(matches!(
            app.queued.as_slice(),
            [GuiAction::CancelCountdown]
        ));
        assert!(app.editor.is_dirty());
    }
    #[test]
    fn quit_after_successful_save_waits_for_acknowledgment() {
        let mut app = BridgeApp::new(GuiSettings::default());
        app.quit_after_save = true;
        assert!(app.queued.is_empty());
        app.on_event(&GuiEvent::ProfileSaved {
            path: PathBuf::from("profile.toml"),
        });
        assert!(matches!(app.queued.as_slice(), [GuiAction::Quit]));
    }
    #[test]
    fn deployment_reset_opt_in_is_only_emitted_by_fresh_confirmed_action() {
        assert!(matches!(
            Confirm::RunClient.action(),
            GuiAction::Deploy {
                run: true,
                allow_reset: true
            }
        ));
        let mut app = BridgeApp::new(GuiSettings::default());
        app.confirm = Some(Confirm::Probe);
        assert!(matches!(app.request_quit(), Some(GuiAction::Quit)));
        assert!(app.confirm.is_none());
    }
    #[test]
    fn all_pages_render_without_dispatching_side_effects() {
        let context = egui::Context::default();
        let mut app = BridgeApp::new(GuiSettings::default());
        let snapshot = GuiSnapshot::default();
        for page in [
            Page::Connection,
            Page::Keyboard,
            Page::Diagnostics,
            Page::Settings,
        ] {
            app.page = page;
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200.0, 900.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        assert!(app.render(ui, &snapshot).is_empty());
                    });
                },
            );
            assert!(!output.shapes.is_empty());
            let required: &[&str] = match page {
                Page::Connection => &["Start host", "Device information"],
                Page::Keyboard => &["SPACE", "F7 / F8", "Add rule", "Apply mapping"],
                Page::Diagnostics => &["Run doctor", "Native input test…"],
                Page::Settings => &["Network password", "Forget saved password"],
            };
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0));
            for label in required {
                assert!(
                    output.shapes.iter().any(|clipped| {
                        if let egui::epaint::Shape::Text(text) = &clipped.shape {
                            let center = text.visual_bounding_rect().center();
                            text.galley.job.text == *label
                                && screen.contains(center)
                                && clipped.clip_rect.contains(center)
                        } else {
                            false
                        }
                    }),
                    "Required control is outside the page viewport: {label}"
                );
            }
            // This headless test intentionally has no renderer for texture uploads.
            output.textures_delta.clear();
        }
    }
    #[test]
    fn delayed_save_acknowledgment_keeps_newer_mapping_edits_dirty() {
        let mut app = BridgeApp::new(GuiSettings::default());
        let submitted = app.editor.document.clone();
        app.pending_save = Some(submitted);
        app.quit_after_save = true;
        app.editor.document.name = "Newer draft".into();
        app.on_event(&GuiEvent::ProfileSaved {
            path: PathBuf::from("saved.toml"),
        });
        assert!(app.editor.is_dirty());
        assert!(app.quit_dialog);
        assert!(app.queued.is_empty());
    }
    #[test]
    fn loaded_builtin_settings_refresh_pristine_editor_but_preserve_unsaved_draft() {
        let mut app = BridgeApp::new(GuiSettings::default());
        let settings = GuiSettings {
            profile: "authentic".into(),
            layout: "UK".into(),
            ..Default::default()
        };
        app.on_event(&GuiEvent::SettingsLoaded(settings));
        assert_eq!(app.editor.document.profile, "authentic");
        assert_eq!(app.editor.document.layout, "UK");
        app.editor.document.name = "Unsaved draft".into();
        app.on_event(&GuiEvent::SettingsLoaded(GuiSettings::default()));
        assert_eq!(app.editor.document.name, "Unsaved draft");
        assert_eq!(app.editor.document.layout, "UK");
    }
    #[test]
    fn acknowledged_local_data_reset_clears_secret_and_invalid_mapping_draft() {
        let mut app = BridgeApp::new(GuiSettings::default());
        app.password = "SENTINEL_DO_NOT_PERSIST".into();
        app.editor
            .load_raw("[invalid".into(), PathBuf::from("broken.toml"));
        let settings = GuiSettings {
            env_file: String::new(),
            ..Default::default()
        };
        app.on_event(&GuiEvent::LocalDataReset(settings));
        assert!(app.password.is_empty());
        assert!(!app.editor.is_dirty());
        assert!(app.editor.validated_document().is_ok());
    }
}

//! Desktop presentation. All I/O and native input belong to the ordered service.
use crate::{
    editor::{EditorIntent, MappingEditor},
    model::{
        ConnectionOperation, GuiAction, GuiEvent, GuiPhase, GuiPlatform, GuiSettings, GuiSnapshot,
    },
    presentation,
};
use eframe::egui;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
};

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

struct FormSubmission {
    settings: GuiSettings,
    password: String,
    configured: bool,
}
pub struct BridgeApp {
    pub settings: GuiSettings,
    pub editor: MappingEditor,
    page: Page,
    platform: GuiPlatform,
    password: String,
    accepted_settings: GuiSettings,
    accepted_password: String,
    form_submissions: VecDeque<FormSubmission>,
    settings_loaded: bool,
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
        Self::new_for_state(settings, &crate::config::default_state_dir())
    }
    /// Use the same absolute state directory as the background service, rather
    /// than the desktop launcher's working directory, for the export default.
    pub fn new_for_state(settings: GuiSettings, state_dir: &Path) -> Self {
        Self {
            editor: MappingEditor::new(&settings.profile, &settings.layout),
            accepted_settings: settings.clone(),
            settings,
            page: Page::Connection,
            platform: GuiPlatform::current(),
            password: String::new(),
            accepted_password: String::new(),
            form_submissions: VecDeque::new(),
            settings_loaded: false,
            confirm: None,
            quit_dialog: false,
            quit_after_save: false,
            pending_save: None,
            queued: Vec::new(),
            diagnostics_path: state_dir
                .join("c64bridge-diagnostics.json")
                .to_string_lossy()
                .into_owned(),
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
    /// Only documentation fixtures use a display-safe path. Real exports keep
    /// their absolute private-state destination, independent of launcher cwd.
    pub fn prepare_documentation_capture(&mut self) {
        self.diagnostics_path = "demo-diagnostics.json".into();
    }
    pub fn on_event(&mut self, event: &GuiEvent) {
        match event {
            GuiEvent::SettingsLoaded(settings) | GuiEvent::FormAccepted(settings) => {
                let configured = matches!(event, GuiEvent::FormAccepted(_));
                let initial_load = !configured && !self.settings_loaded;
                if !configured {
                    self.settings_loaded = true;
                }
                let submission = if !initial_load
                    && self.form_submissions.front().is_some_and(|pending| {
                        pending.configured == configured
                            && acknowledgment_matches(&pending.settings, settings)
                    }) {
                    self.form_submissions.pop_front()
                } else {
                    None
                };
                let baseline = submission
                    .as_ref()
                    .map_or(&self.accepted_settings, |pending| &pending.settings)
                    .clone();
                self.merge_loaded_settings(settings, &baseline);
                self.accepted_settings = settings.clone();
                if let Some(submission) = submission {
                    self.accepted_password = submission.password;
                }
                if !self.editor.is_dirty()
                    && self.pending_save.is_none()
                    && self.settings.mappings_file.is_empty()
                    && (self.editor.document.profile != self.settings.profile
                        || self.editor.document.layout != self.settings.layout)
                {
                    self.editor = MappingEditor::new(&self.settings.profile, &self.settings.layout);
                }
            }
            GuiEvent::ProfileLoaded { document, path } => {
                self.editor.load(document.clone(), path.clone())
            }
            GuiEvent::ProfileDraftLoaded { text, path } => {
                self.editor.load_raw(text.clone(), path.clone());
            }
            GuiEvent::LocalDataReset(settings) => {
                self.settings = settings.clone();
                self.accepted_settings = settings.clone();
                self.password.clear();
                self.accepted_password.clear();
                self.form_submissions.clear();
                self.settings_loaded = true;
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
                self.form_submissions.clear();
            }
            _ => {}
        }
    }
    fn merge_loaded_settings(&mut self, incoming: &GuiSettings, baseline: &GuiSettings) {
        // Apply service normalization only to fields that have not been edited
        // since the submitted/loaded form. Late acknowledgments keep newer drafts.
        macro_rules! merge {
            ($($field:ident),+ $(,)?) => {$(
                if self.settings.$field == baseline.$field {
                    self.settings.$field.clone_from(&incoming.$field);
                }
            )+};
        }
        merge!(
            schema_version,
            env_file,
            device_host,
            device_port,
            bind_address,
            advertise_ip,
            bridge_port,
            allowed_ips,
            layout,
            profile,
            backend,
            mappings_file,
            repeat_delay_ms,
            repeat_interval_ms,
            log_level,
            remember_password
        );
        self.settings.credential_id = incoming.credential_id.clone();
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
                    .is_some_and(|status| status.running && status.enabled)
                    && snapshot.phase == GuiPhase::Running;
                let connected = snapshot
                    .status
                    .as_ref()
                    .is_some_and(|status| status.running && status.connected)
                    && snapshot.phase == GuiPhase::Running;
                presentation::status(
                    ui,
                    if enabled {
                        "Input enabled"
                    } else if snapshot.phase == GuiPhase::Running {
                        "Input paused"
                    } else {
                        "Input disabled"
                    },
                    enabled,
                );
                presentation::status(
                    ui,
                    if connected && snapshot.phase == GuiPhase::Running {
                        "C64 connected"
                    } else {
                        match snapshot.phase {
                            GuiPhase::Idle => "Host stopped",
                            GuiPhase::Starting => "Host starting",
                            GuiPhase::Running => "Waiting for C64",
                            GuiPhase::Stopping => "Host stopping",
                        }
                    },
                    connected,
                );
            });
        });
        ui.separator();
        if let Some(error) = self.local_error.clone() {
            presentation::card(ui, |ui| {
                ui.colored_label(ui.visuals().error_fg_color, error);
                if ui.small_button("Dismiss error").clicked() {
                    self.local_error = None;
                }
            });
            ui.add_space(6.0);
        }
        if !snapshot.message.is_empty() {
            ui.label(&snapshot.message);
            ui.add_space(6.0);
        }
        if !self.platform.supports_backend(&self.settings.backend) {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "The configured backend is unavailable on this platform. Select a supported backend in Settings before saving, applying or starting. Your imported choice is preserved.",
            );
        }
        if let Some(seconds) = snapshot.countdown {
            presentation::card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "Native input test · switch to your scratch application · {seconds}s"
                        ))
                        .strong(),
                    );
                    if ui.button("Cancel countdown").clicked() {
                        actions.push(GuiAction::CancelCountdown);
                    }
                });
                ui.small("The input test is cancelled if this window is focused or permission/layout checks fail.");
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
                        actions.push(self.with_form(confirmation.action()));
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
            if let GuiAction::SaveSettings { settings, password }
            | GuiAction::ApplySettings { settings, password }
            | GuiAction::Configured {
                settings, password, ..
            } = action
            {
                self.form_submissions.push_back(FormSubmission {
                    settings: settings.clone(),
                    password: password.clone(),
                    configured: matches!(action, GuiAction::Configured { .. }),
                });
            }
            if let GuiAction::ForgetPassword = action {
                let mut settings = self.settings.clone();
                settings.remember_password = false;
                self.form_submissions.push_back(FormSubmission {
                    settings,
                    password: String::new(),
                    configured: false,
                });
            }
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
            "Enter the C64 address and password, check Device information, then start hosting. Enable input when the keyboard client is connected.",
        );
        if !snapshot.configuration_warning.is_empty() {
            ui.colored_label(ui.visuals().warn_fg_color, &snapshot.configuration_warning);
            ui.add_space(8.0);
        }
        presentation::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(match snapshot.phase {
                    GuiPhase::Idle => "Hosting stopped",
                    GuiPhase::Starting => "Starting host",
                    GuiPhase::Running => "Host listening",
                    GuiPhase::Stopping => "Stopping host",
                });
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
                if status.running && !status.connected {
                    ui.label("Waiting for an authenticated keyboard client. Start host listens for the C64; it does not load or run its program.");
                }
            }
            ui.small("Start host uses this form and keeps input disabled. Start + deploy also resets the C64 after a fresh confirmation.");
            ui.horizontal_wrapped(|ui| {
                let running = snapshot.phase == GuiPhase::Running;
                if ui
                    .add_enabled(
                        snapshot.phase == GuiPhase::Idle
                            && !snapshot.busy
                            && self.platform.supports_backend(&self.settings.backend),
                        egui::Button::new("Start host"),
                    )
                    .clicked()
                {
                    actions.push(self.with_form(GuiAction::Start {
                        deploy: false,
                        allow_reset: false,
                    }));
                }
                if ui
                    .add_enabled(
                        snapshot.phase == GuiPhase::Idle
                            && !snapshot.busy
                            && self.platform.supports_backend(&self.settings.backend),
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
                        can_enable_input(snapshot)
                            && !self.form_has_changes()
                            && self.platform.supports_backend(&self.settings.backend),
                        egui::Button::new("Enable input"),
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
            if let Some(status) = &snapshot.status {
                match status.authorization.phase {
                    bridge_host::control::AuthorizationPhase::Pending => {
                        ui.label("Desktop keyboard approval is pending. Input is disabled; Pause cancels the request.");
                    }
                    bridge_host::control::AuthorizationPhase::Denied => {
                        ui.label("Desktop keyboard approval was denied. Enable input or Authorize desktop keyboard can request it again.");
                    }
                    bridge_host::control::AuthorizationPhase::Cancelled => {
                        ui.label("Desktop keyboard approval was cancelled. Input stays disabled.");
                    }
                    bridge_host::control::AuthorizationPhase::Failed => {
                        ui.label("Desktop keyboard authorization or layout was lost. Input is disabled; inspect Diagnostics before enabling again.");
                    }
                    _ => {}
                }
            }
            ui.small("Enable input activates immediately. C64 keystrokes go to the focused app, including this window. Use Pause to stop input.");
            if self.form_has_changes() {
                ui.small("This form has unapplied changes. Apply settings before enabling input; a running GUI-owned host restarts with input disabled.");
            }
            if snapshot.phase == GuiPhase::Running
                && !snapshot
                    .status
                    .as_ref()
                    .is_some_and(|status| status.connected)
            {
                ui.small("Input can be enabled after the C64 keyboard client authenticates. Device information only checks the REST connection.");
            }
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
            self.credentials(ui, actions);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !snapshot.busy && self.platform.supports_backend(&self.settings.backend),
                        egui::Button::new("Save settings"),
                    )
                    .clicked()
                {
                    actions.push(self.save_settings(false));
                }
                if ui
                    .add_enabled(
                        !snapshot.busy && self.platform.supports_backend(&self.settings.backend),
                        egui::Button::new("Apply settings"),
                    )
                    .clicked()
                {
                    actions.push(self.save_settings(true));
                }
                if ui
                    .add_enabled(!snapshot.busy, egui::Button::new("Device information"))
                    .clicked()
                {
                    actions.push(self.with_form(GuiAction::DeviceInfo));
                }
            });
            if snapshot.settings_pending {
                ui.colored_label(ui.visuals().warn_fg_color, if snapshot.owned {"Settings have changed. Apply them to restart GUI-owned hosting with input disabled."} else {"Settings are pending for the CLI-owned session. Stop it explicitly before restarting with these settings."});
            }
            ui.small("Connection changes may require redeploying the C64 program. Applying settings never deploys automatically.");
            ui.small("Device information and Start use the form above immediately. Save settings remembers non-secret settings; Apply also restarts GUI-owned hosting with input disabled.");
            if let Some(device) = &snapshot.device {
                ui.add_space(8.0);
                ui.label(egui::RichText::new("Last successful REST device check").strong());
                ui.label(format!(
                    "{} · firmware {} · FPGA {} · core {}",
                    device.product, device.firmware, device.fpga, device.core
                ));
            }
        });
        ui.add_space(12.0);
        self.permission_controls(ui, snapshot, actions);
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
                        .add_enabled(
                            !snapshot.busy
                                && !matches!(
                                    snapshot.phase,
                                    GuiPhase::Starting | GuiPhase::Stopping
                                ),
                            egui::Button::new(label),
                        )
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
    fn with_form(&self, action: GuiAction) -> GuiAction {
        let operation = match action {
            GuiAction::Start {
                deploy,
                allow_reset,
            } => ConnectionOperation::Start {
                deploy,
                allow_reset,
            },
            GuiAction::DeviceInfo => ConnectionOperation::DeviceInfo,
            GuiAction::Deploy { run, allow_reset } => {
                ConnectionOperation::Deploy { run, allow_reset }
            }
            GuiAction::Probe { allow_reset } => ConnectionOperation::Probe { allow_reset },
            GuiAction::Doctor => ConnectionOperation::Doctor,
            GuiAction::RequestPermission => ConnectionOperation::RequestPermission,
            GuiAction::TestInput => ConnectionOperation::TestInput,
            other => return other,
        };
        GuiAction::Configured {
            settings: self.settings.clone(),
            password: self.password.clone(),
            operation,
        }
    }
    /// Window and tray controls must not enable an older configuration while
    /// the user has a different connection/password draft on screen.
    pub fn form_has_changes(&self) -> bool {
        self.settings != self.accepted_settings || self.password != self.accepted_password
    }
    fn credentials(&mut self, ui: &mut egui::Ui, actions: &mut Vec<GuiAction>) {
        ui.label("Network password");
        ui.add_sized(
            [350.0, ui.spacing().interact_size.y],
            egui::TextEdit::singleline(&mut self.password)
                .password(true)
                .hint_text("Empty: use saved credential or imported environment"),
        );
        ui.checkbox(
            &mut self.settings.remember_password,
            "Remember in the operating system credential store",
        );
        ui.small("The password stays in memory unless Remember is selected and settings are explicitly saved or applied. No plaintext fallback.");
        ui.small("An imported .env path must exist. Desktop launchers do not inherit your terminal's environment.");
        if ui.button("Forget saved password").clicked() {
            self.password.clear();
            actions.push(GuiAction::ForgetPassword);
        }
    }
    fn permission_controls(
        &self,
        ui: &mut egui::Ui,
        snapshot: &GuiSnapshot,
        actions: &mut Vec<GuiAction>,
    ) {
        let portal = self.platform == GuiPlatform::Linux
            && (self.settings.backend == "portal"
                || snapshot
                    .backend
                    .as_ref()
                    .is_some_and(|backend| backend.name == "portal"));
        let label = if portal {
            "Authorize desktop keyboard"
        } else {
            self.platform.permission_button()
        };
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !snapshot.busy
                        && !snapshot.status.as_ref().is_some_and(|status| {
                            status.authorization.phase
                                == bridge_host::control::AuthorizationPhase::Pending
                        }),
                    egui::Button::new(label),
                )
                .clicked()
            {
                actions.push(self.with_form(GuiAction::RequestPermission));
            }
            if self.platform == GuiPlatform::MacOs
                && ui.button("Open input permission settings").clicked()
            {
                actions.push(GuiAction::OpenPermissionSettings);
            }
        });
        ui.small(self.platform.permission_help());
        if portal {
            ui.small("Start hosting first. Authorization requests keyboard control only for the running posting process and keeps input paused. Pause cancels a pending request.");
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
                actions.push(self.with_form(GuiAction::Doctor));
            }
            if ui
                .add_enabled(!snapshot.busy, egui::Button::new("Native input test…"))
                .clicked()
            {
                self.confirm = Some(Confirm::InputTest);
            }
        });
        self.permission_controls(ui, snapshot, actions);
        if snapshot.test_authorization_pending {
            ui.label("Desktop keyboard test approval pending. The five-second countdown has not started; no input is posted.");
            if ui.button("Cancel input test authorization").clicked() {
                actions.push(GuiAction::CancelCountdown);
            }
        }
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
                if status.authorization.phase
                    != bridge_host::control::AuthorizationPhase::NotRequested
                {
                    ui.label(format!(
                        "Desktop keyboard authorization: {}",
                        status.authorization.phase.as_str()
                    ));
                }
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
            self.credentials(ui, actions);
            ui.small(self.platform.credential_store());
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
                        for &value in self.platform.backends() {
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
                if self.platform != GuiPlatform::Linux {
                    ui.label("Repeat delay (ms)");
                    ui.add(
                        egui::DragValue::new(&mut self.settings.repeat_delay_ms).range(100..=5000),
                    );
                    ui.end_row();
                    ui.label("Repeat interval (ms)");
                    ui.add(
                        egui::DragValue::new(&mut self.settings.repeat_interval_ms)
                            .range(10..=1000),
                    );
                    ui.end_row();
                }
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
                .add_enabled(
                    !snapshot.busy && self.platform.supports_backend(&self.settings.backend),
                    egui::Button::new("Save settings"),
                )
                .clicked()
            {
                actions.push(self.save_settings(false));
            }
            if ui
                .add_enabled(
                    !snapshot.busy && self.platform.supports_backend(&self.settings.backend),
                    egui::Button::new("Apply settings"),
                )
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
        if self.platform == GuiPlatform::Linux {
            ui.small("Linux uses desktop repeat. The bridge never changes global repeat settings or grants uinput access. Saved repeat settings are preserved for use on other platforms.");
        }
        ui.add_space(20.0);
        ui.separator();
        ui.label(egui::RichText::new("C64 Keyboard Bridge 0.1.0").strong());
        ui.label(
            "GPL-3.0-only · Rust host with genuine cc65 C64 programs · wire protocol version 1",
        );
        if ui
            .link("Source, installation and hardware acceptance notes")
            .clicked()
        {
            actions.push(GuiAction::OpenProjectWebsite);
        }
        ui.label("Closing hides to the tray when available. Quit releases GUI-owned keys and waits for graceful cleanup.");
        ui.label("Pairing tokens are transmitted in plaintext on the LAN. Use a trusted network.");
    }
}

fn acknowledgment_matches(submitted: &GuiSettings, incoming: &GuiSettings) -> bool {
    fn path_matches(submitted: &str, incoming: &str) -> bool {
        submitted == incoming
            || (!submitted.is_empty()
                && Path::new(submitted).is_relative()
                && Path::new(incoming).is_absolute()
                && Path::new(incoming).ends_with(submitted))
    }
    if !path_matches(&submitted.env_file, &incoming.env_file)
        || !path_matches(&submitted.mappings_file, &incoming.mappings_file)
    {
        return false;
    }
    let mut normalized = submitted.clone();
    normalized.env_file.clone_from(&incoming.env_file);
    normalized.mappings_file.clone_from(&incoming.mappings_file);
    // A vault failure can preserve the previous Remember state; it does not
    // change the accepted in-memory password. Empty references are initialized.
    normalized.remember_password = incoming.remember_password;
    if normalized.credential_id.is_empty() {
        normalized.credential_id.clone_from(&incoming.credential_id);
    }
    normalized == *incoming
}

fn can_enable_input(snapshot: &GuiSnapshot) -> bool {
    snapshot.phase == GuiPhase::Running
        && snapshot.status.as_ref().is_some_and(|status| {
            status.running
                && status.connected
                && !status.enabled
                && status.authorization.phase != bridge_host::control::AuthorizationPhase::Pending
        })
        && snapshot.countdown.is_none()
        && !snapshot.test_authorization_pending
        && !snapshot.busy
        && !snapshot.settings_pending
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_fixture(
        context: &egui::Context,
        app: &mut BridgeApp,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, Vec<GuiAction>) {
        let mut actions = Vec::new();
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 1600.0),
                )),
                events,
                ..Default::default()
            },
            |ui| actions = app.render(ui, &GuiSnapshot::default()),
        );
        output.textures_delta.clear();
        (output, actions)
    }

    fn text_center(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::epaint::Shape::Text(text) = &shape.shape {
                    (text.galley.job.text == label).then(|| text.visual_bounding_rect().center())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| panic!("Widget not rendered: {label}"))
    }

    fn pointer_click(pos: egui::Pos2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    fn texts(output: &egui::FullOutput) -> Vec<&str> {
        output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::epaint::Shape::Text(text) = &shape.shape {
                    Some(text.galley.job.text.as_str())
                } else {
                    None
                }
            })
            .collect()
    }

    #[test]
    fn about_link_pointer_click_dispatches_trusted_background_navigation() {
        let context = egui::Context::default();
        let mut app = BridgeApp::new(GuiSettings::default());
        app.page = Page::Settings;
        for _ in 0..2 {
            render_fixture(&context, &mut app, vec![]);
        }
        let (output, _) = render_fixture(&context, &mut app, vec![]);
        let pos = text_center(
            &output,
            "Source, installation and hardware acceptance notes",
        );
        let (_, actions) = render_fixture(&context, &mut app, pointer_click(pos));
        assert!(matches!(
            actions.as_slice(),
            [GuiAction::OpenProjectWebsite]
        ));
        assert!(app.form_submissions.is_empty());
    }

    #[test]
    fn sidebar_buttons_navigate_without_dispatching_io_or_input() {
        let context = egui::Context::default();
        let mut app = BridgeApp::new(GuiSettings::default());
        for (page, label) in [
            (Page::Keyboard, "Keyboard / Mappings"),
            (Page::Diagnostics, "Diagnostics"),
            (Page::Settings, "Settings / About"),
            (Page::Connection, "Connection"),
        ] {
            for _ in 0..2 {
                render_fixture(&context, &mut app, vec![]);
            }
            let (output, _) = render_fixture(&context, &mut app, vec![]);
            let pos = text_center(&output, label);
            let (_, actions) = render_fixture(&context, &mut app, pointer_click(pos));
            assert!(actions.is_empty());
            assert!(app.page == page);
        }
    }

    #[test]
    fn sidebar_supports_tab_and_space_navigation() {
        let context = egui::Context::default();
        let mut app = BridgeApp::new(GuiSettings::default());
        for _ in 0..3 {
            render_fixture(&context, &mut app, vec![]);
        }
        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        render_fixture(&context, &mut app, vec![key(egui::Key::Tab)]);
        let first = context.memory(|memory| memory.focused());
        render_fixture(&context, &mut app, vec![key(egui::Key::Tab)]);
        let second = context.memory(|memory| memory.focused());
        assert!(first.is_some());
        assert_ne!(first, second);
        let (_, actions) = render_fixture(&context, &mut app, vec![key(egui::Key::Space)]);
        assert!(actions.is_empty());
        assert!(app.page == Page::Keyboard);
    }

    #[test]
    fn platform_controls_and_backend_menu_use_the_current_posting_platform() {
        for platform in [GuiPlatform::MacOs, GuiPlatform::Windows, GuiPlatform::Linux] {
            let context = egui::Context::default();
            let mut app = BridgeApp::new(GuiSettings::default());
            app.platform = platform;
            for _ in 0..2 {
                render_fixture(&context, &mut app, vec![]);
            }
            let (output, _) = render_fixture(&context, &mut app, vec![]);
            let labels = texts(&output);
            assert!(labels.contains(&platform.permission_button()));
            assert_eq!(
                labels.contains(&"Open input permission settings"),
                platform == GuiPlatform::MacOs
            );
            assert!(labels.contains(&platform.permission_help()));

            app.page = Page::Settings;
            for _ in 0..2 {
                render_fixture(&context, &mut app, vec![]);
            }
            let (output, _) = render_fixture(&context, &mut app, vec![]);
            let labels = texts(&output);
            assert!(labels.contains(&platform.credential_store()));
            for other in [GuiPlatform::MacOs, GuiPlatform::Windows, GuiPlatform::Linux] {
                if other != platform {
                    assert!(!labels.contains(&other.credential_store()));
                }
            }
            assert_eq!(
                labels.contains(&"Repeat delay (ms)"),
                platform != GuiPlatform::Linux
            );
            assert_eq!(
                labels.contains(&"Repeat interval (ms)"),
                platform != GuiPlatform::Linux
            );
            let pos = text_center(&output, "auto");
            render_fixture(&context, &mut app, pointer_click(pos));
            let (output, _) = render_fixture(&context, &mut app, vec![]);
            let labels = texts(&output);
            for candidate in ["macos", "windows", "x11", "uinput", "recording"] {
                assert_eq!(
                    labels.contains(&candidate),
                    platform.supports_backend(candidate),
                    "{platform:?}: {candidate}"
                );
            }
        }
    }

    #[test]
    fn foreign_backend_draft_stays_visible_and_cannot_be_saved_by_a_pointer_click() {
        let context = egui::Context::default();
        let mut app = BridgeApp::new(GuiSettings {
            backend: "windows".into(),
            repeat_delay_ms: 777,
            repeat_interval_ms: 66,
            ..Default::default()
        });
        app.platform = GuiPlatform::Linux;
        app.page = Page::Settings;
        let original = app.settings.clone();
        for _ in 0..2 {
            render_fixture(&context, &mut app, vec![]);
        }
        let (output, _) = render_fixture(&context, &mut app, vec![]);
        assert!(
            texts(&output)
                .iter()
                .any(|text| text.contains("Your imported choice is preserved"))
        );
        let pos = text_center(&output, "Save settings");
        let (_, actions) = render_fixture(&context, &mut app, pointer_click(pos));
        assert!(actions.is_empty());
        assert_eq!(app.settings, original);
        assert!(app.form_submissions.is_empty());
    }
    use bridge_core::{BindingDocument, NativeKey};
    use bridge_protocol::PhysicalKey;
    #[test]
    fn selected_state_directory_sets_the_absolute_diagnostic_export_default() {
        let state = std::env::temp_dir().join("c64bridge custom state");
        let app = BridgeApp::new_for_state(GuiSettings::default(), &state);
        let path = Path::new(&app.diagnostics_path);
        assert!(path.is_absolute());
        assert_eq!(path, state.join("c64bridge-diagnostics.json"));
    }
    #[test]
    fn diagnostic_export_default_is_independent_of_desktop_launch_directory() {
        let fixture = std::env::temp_dir().join(format!(
            "c64bridge-export-default-{}",
            crate::config::random_id().unwrap()
        ));
        let state = fixture.join("selected-state");
        std::fs::create_dir_all(&state).unwrap();
        for name in ["finder-launch", "terminal-launch"] {
            let cwd = fixture.join(name);
            std::fs::create_dir(&cwd).unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "app::tests::diagnostic_export_constructor_fixture_child",
                ])
                .env("C64BRIDGE_GUI_EXPORT_TEST_STATE", &state)
                .current_dir(cwd)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "Constructor fixture failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::fs::remove_dir_all(fixture).unwrap();
    }
    #[test]
    fn diagnostic_export_constructor_fixture_child() {
        let Some(state) = std::env::var_os("C64BRIDGE_GUI_EXPORT_TEST_STATE") else {
            return;
        };
        let state = PathBuf::from(state);
        let app = BridgeApp::new_for_state(GuiSettings::default(), &state);
        assert!(Path::new(&app.diagnostics_path).is_absolute());
        assert_eq!(
            Path::new(&app.diagnostics_path),
            state.join("c64bridge-diagnostics.json")
        );
        assert_ne!(
            Path::new(&app.diagnostics_path).parent(),
            Some(std::env::current_dir().unwrap().as_path())
        );
    }
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
    #[test]
    fn connection_operations_use_the_current_form_without_implicitly_saving() {
        let mut app = BridgeApp::new(GuiSettings::default());
        app.settings.device_host = "new-device.invalid".into();
        app.settings.backend = "recording".into();
        app.password = "IN_MEMORY_SENTINEL".into();
        let operations = [
            (
                GuiAction::Start {
                    deploy: false,
                    allow_reset: false,
                },
                ConnectionOperation::Start {
                    deploy: false,
                    allow_reset: false,
                },
            ),
            (GuiAction::DeviceInfo, ConnectionOperation::DeviceInfo),
            (GuiAction::Doctor, ConnectionOperation::Doctor),
            (
                GuiAction::RequestPermission,
                ConnectionOperation::RequestPermission,
            ),
            (GuiAction::TestInput, ConnectionOperation::TestInput),
            (
                Confirm::RunClient.action(),
                ConnectionOperation::Deploy {
                    run: true,
                    allow_reset: true,
                },
            ),
            (
                Confirm::Probe.action(),
                ConnectionOperation::Probe { allow_reset: true },
            ),
        ];
        for (action, expected) in operations {
            let GuiAction::Configured {
                settings,
                password,
                operation,
            } = app.with_form(action)
            else {
                panic!("Operation did not carry the current form")
            };
            assert_eq!(settings.device_host, "new-device.invalid");
            assert_eq!(settings.backend, "recording");
            assert_eq!(password, "IN_MEMORY_SENTINEL");
            assert_eq!(operation, expected);
        }
        assert!(app.queued.is_empty());
        assert!(matches!(
            app.with_form(GuiAction::OpenPermissionSettings),
            GuiAction::OpenPermissionSettings
        ));
    }
    #[test]
    fn connection_buttons_dispatch_current_draft_and_fresh_confirmed_reset() {
        for (label, confirmation, expected) in [
            (
                "Start host",
                None,
                ConnectionOperation::Start {
                    deploy: false,
                    allow_reset: false,
                },
            ),
            ("Device information", None, ConnectionOperation::DeviceInfo),
            (
                GuiPlatform::current().permission_button(),
                None,
                ConnectionOperation::RequestPermission,
            ),
            (
                "Confirm",
                Some(Confirm::RunClient),
                ConnectionOperation::Deploy {
                    run: true,
                    allow_reset: true,
                },
            ),
        ] {
            let context = egui::Context::default();
            let mut app = BridgeApp::new(GuiSettings::default());
            app.settings.device_host = "edited-device.invalid".into();
            app.password = "CURRENT_MEMORY_PASSWORD".into();
            app.confirm = confirmation;
            let mut center = None;
            for _ in 0..3 {
                let mut output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1200.0, 1100.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        assert!(app.render(ui, &GuiSnapshot::default()).is_empty());
                    },
                );
                center = output.shapes.iter().find_map(|shape| {
                    if let egui::epaint::Shape::Text(text) = &shape.shape {
                        (text.galley.job.text == label)
                            .then(|| text.visual_bounding_rect().center())
                    } else {
                        None
                    }
                });
                output.textures_delta.clear();
            }
            let pos = center.unwrap_or_else(|| panic!("Button not rendered: {label}"));
            let mut dispatched = Vec::new();
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200.0, 1100.0),
                    )),
                    events: vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        },
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed: false,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ui| {
                    dispatched = app.render(ui, &GuiSnapshot::default());
                },
            );
            output.textures_delta.clear();
            let [
                GuiAction::Configured {
                    settings,
                    password,
                    operation,
                },
            ] = dispatched.as_slice()
            else {
                panic!("Expected one configured operation from {label}")
            };
            assert_eq!(settings.device_host, "edited-device.invalid");
            assert_eq!(password, "CURRENT_MEMORY_PASSWORD");
            assert_eq!(*operation, expected);
        }
    }
    #[test]
    fn delayed_settings_acknowledgment_preserves_newer_form_edits() {
        let mut app = BridgeApp::new(GuiSettings::default());
        app.on_event(&GuiEvent::SettingsLoaded(GuiSettings::default()));
        app.settings.device_host = "submitted.invalid".into();
        app.form_submissions.push_back(FormSubmission {
            settings: app.settings.clone(),
            password: String::new(),
            configured: false,
        });
        let mut submitted = app.settings.clone();
        submitted.env_file = "/private/project/.env".into();
        submitted.credential_id = "private-id".into();
        app.settings.device_host = "newer.invalid".into();
        app.settings.layout = "UK".into();
        app.on_event(&GuiEvent::SettingsLoaded(submitted));
        assert_eq!(app.settings.device_host, "newer.invalid");
        assert_eq!(app.settings.layout, "UK");
        assert_eq!(app.settings.env_file, "/private/project/.env");
        assert_eq!(app.settings.credential_id, "private-id");
    }
    #[test]
    fn ordered_acceptance_keeps_two_quick_submissions_and_newer_drafts_separate() {
        let mut app = BridgeApp::new(GuiSettings::default());
        app.on_event(&GuiEvent::SettingsLoaded(GuiSettings::default()));
        let first = GuiSettings {
            device_host: "first.invalid".into(),
            ..Default::default()
        };
        let second = GuiSettings {
            device_host: "second.invalid".into(),
            ..Default::default()
        };
        for (settings, password) in [(&first, "FIRST_PASSWORD"), (&second, "SECOND_PASSWORD")] {
            app.form_submissions.push_back(FormSubmission {
                settings: settings.clone(),
                password: password.into(),
                configured: true,
            });
        }
        app.settings = second.clone();
        app.password = "SECOND_PASSWORD".into();
        app.on_event(&GuiEvent::FormAccepted(first));
        assert_eq!(app.settings.device_host, "second.invalid");
        assert_eq!(app.accepted_password, "FIRST_PASSWORD");
        assert!(app.form_has_changes());
        app.on_event(&GuiEvent::FormAccepted(second));
        assert_eq!(app.accepted_password, "SECOND_PASSWORD");
        assert!(!app.form_has_changes());
        assert!(app.form_submissions.is_empty());
    }
    #[test]
    fn startup_and_failed_settings_cannot_accept_a_queued_password() {
        let mut app = BridgeApp::new(GuiSettings::default());
        app.password = "PENDING_PASSWORD".into();
        app.form_submissions.push_back(FormSubmission {
            settings: app.settings.clone(),
            password: app.password.clone(),
            configured: true,
        });
        app.on_event(&GuiEvent::SettingsLoaded(GuiSettings::default()));
        assert!(app.accepted_password.is_empty());
        assert!(app.form_has_changes());
        assert_eq!(app.form_submissions.len(), 1);
        app.on_event(&GuiEvent::Error("Configuration validation failed".into()));
        assert!(app.accepted_password.is_empty());
        assert!(app.form_submissions.is_empty());
        assert!(app.form_has_changes());
        // An unassociated later acknowledgment fails closed for the password.
        app.on_event(&GuiEvent::FormAccepted(app.settings.clone()));
        assert!(app.form_has_changes());
    }
    #[test]
    fn password_only_submissions_are_accepted_in_order() {
        let mut app = BridgeApp::new(GuiSettings::default());
        app.on_event(&GuiEvent::SettingsLoaded(GuiSettings::default()));
        for password in ["FIRST_PASSWORD", "SECOND_PASSWORD"] {
            app.form_submissions.push_back(FormSubmission {
                settings: app.settings.clone(),
                password: password.into(),
                configured: true,
            });
        }
        app.password = "SECOND_PASSWORD".into();
        app.on_event(&GuiEvent::FormAccepted(app.settings.clone()));
        assert_eq!(app.accepted_password, "FIRST_PASSWORD");
        assert!(app.form_has_changes());
        app.on_event(&GuiEvent::FormAccepted(app.settings.clone()));
        assert_eq!(app.accepted_password, "SECOND_PASSWORD");
        assert!(!app.form_has_changes());
    }
    #[test]
    fn enabling_requires_an_authenticated_active_session_and_no_pending_change() {
        let mut snapshot = GuiSnapshot {
            phase: GuiPhase::Running,
            status: Some(bridge_host::control::Status::default()),
            ..Default::default()
        };
        assert!(!can_enable_input(&snapshot));
        snapshot.status.as_mut().unwrap().connected = true;
        assert!(can_enable_input(&snapshot));
        snapshot.settings_pending = true;
        assert!(!can_enable_input(&snapshot));
        snapshot.settings_pending = false;
        snapshot.status.as_mut().unwrap().enabled = true;
        assert!(!can_enable_input(&snapshot));
        snapshot.status.as_mut().unwrap().enabled = false;
        snapshot.countdown = Some(5);
        assert!(!can_enable_input(&snapshot));
    }
    #[test]
    fn asynchronous_errors_stay_visible_above_the_page_and_password_is_masked() {
        let context = egui::Context::default();
        let mut app = BridgeApp::new(GuiSettings::default());
        app.password = "SENTINEL_NEVER_RENDER".into();
        app.on_event(&GuiEvent::Error(
            "Device information failed: check the address and password".into(),
        ));
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 700.0),
                )),
                ..Default::default()
            },
            |ui| {
                assert!(app.render(ui, &GuiSnapshot::default()).is_empty());
            },
        );
        let texts: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::epaint::Shape::Text(text) = &shape.shape {
                    Some((
                        text.galley.job.text.as_str(),
                        text.visual_bounding_rect().center(),
                    ))
                } else {
                    None
                }
            })
            .collect();
        let error = texts
            .iter()
            .find(|(text, _)| text.starts_with("Device information failed"))
            .unwrap();
        assert!(error.1.y < 150.0);
        assert!(texts.iter().any(|(text, _)| *text == "Host stopped"));
        assert!(texts.iter().any(|(text, _)| *text == "Input disabled"));
        assert!(
            !texts
                .iter()
                .any(|(text, _)| text.contains("SENTINEL_NEVER_RENDER"))
        );
        output.textures_delta.clear();
    }
}

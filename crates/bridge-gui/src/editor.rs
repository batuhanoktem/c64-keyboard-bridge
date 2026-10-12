//! Editable drafts and a physical keyboard editor. Preview never posts input.
use bridge_core::{
    BackendCapabilities, BindingDocument, HostPlatform, MappingDocument, NativeKey, ResolvedAction,
    ShiftPolicy, SourceModifier,
};
use bridge_protocol::PhysicalKey;
use eframe::egui;
use std::path::PathBuf;

pub enum EditorIntent {
    Import(PathBuf),
    Save {
        document: MappingDocument,
        path: Option<PathBuf>,
    },
    Apply(MappingDocument),
    Export {
        document: MappingDocument,
        path: PathBuf,
    },
}

pub struct MappingEditor {
    pub document: MappingDocument,
    saved: MappingDocument,
    selected: PhysicalKey,
    selected_rule: Option<usize>,
    source_mask: u8,
    pub file_path: String,
    pub error: Option<String>,
    raw_draft: String,
    raw_dirty: bool,
    show_raw: bool,
}
impl MappingEditor {
    pub fn new(profile: &str, layout: &str) -> Self {
        let document = MappingDocument::builtin(profile, layout)
            .unwrap_or_else(|_| MappingDocument::builtin("modern", "US").expect("builtin profile"));
        let raw_draft = document.to_toml().expect("mapping document serializes");
        Self {
            saved: document.clone(),
            document,
            selected: PhysicalKey::A,
            selected_rule: None,
            source_mask: 0,
            file_path: String::new(),
            error: None,
            raw_draft,
            raw_dirty: false,
            show_raw: false,
        }
    }
    pub fn is_dirty(&self) -> bool {
        self.document != self.saved || self.raw_dirty
    }
    pub fn validated_document(&self) -> Result<MappingDocument, String> {
        if self.raw_dirty {
            return Err("Validate the TOML draft before saving or applying it.".into());
        }
        self.document
            .validate()
            .map_err(|error| error.to_string())?;
        Ok(self.document.clone())
    }
    pub fn load(&mut self, document: MappingDocument, path: PathBuf) {
        self.document = document.clone();
        self.saved = document;
        self.file_path = path.display().to_string();
        self.selected_rule = None;
        self.raw_dirty = false;
        self.refresh_raw();
    }
    pub fn load_raw(&mut self, text: String, path: PathBuf) {
        match MappingDocument::from_toml(&text) {
            Ok(document) => {
                self.load(document, path);
                self.error = self
                    .document
                    .validate()
                    .err()
                    .map(|error| error.to_string());
            }
            Err(error) => {
                self.file_path = path.display().to_string();
                self.raw_draft = text;
                self.raw_dirty = true;
                self.show_raw = true;
                self.error = Some(error.to_string());
            }
        }
    }
    pub fn acknowledge_saved(&mut self, document: MappingDocument, path: &std::path::Path) {
        self.saved = document;
        self.file_path = path.display().to_string();
    }
    pub fn mark_saved(&mut self, path: &std::path::Path) {
        self.saved = self.document.clone();
        self.file_path = path.display().to_string();
        self.raw_dirty = false;
    }
    pub fn discard(&mut self) {
        self.document = self.saved.clone();
        self.raw_dirty = false;
        self.selected_rule = None;
        self.error = None;
        self.refresh_raw();
    }
    fn refresh_raw(&mut self) {
        if let Ok(text) = self.document.to_toml() {
            self.raw_draft = text;
        }
    }
    pub fn parse_raw(&mut self) -> Result<(), String> {
        let document =
            MappingDocument::from_toml(&self.raw_draft).map_err(|error| error.to_string())?;
        // Keep structurally valid but semantically invalid documents editable.
        self.document = document;
        self.raw_dirty = false;
        self.selected_rule = None;
        self.document
            .validate()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
    fn add_rule(&mut self) {
        let modifiers = [
            SourceModifier::Shift,
            SourceModifier::Control,
            SourceModifier::Commodore,
        ]
        .into_iter()
        .filter(|modifier| self.source_mask & modifier.mask() != 0)
        .collect();
        let action = self.document.validate().ok().and_then(|profile| {
            profile
                .preview(self.selected, self.source_mask, HostPlatform::current())
                .ok()
        });
        let (keys, text, shift) = match action {
            Some(ResolvedAction::Keys { keys, shift }) => (keys, None, Some(shift)),
            Some(ResolvedAction::Text(text)) => (Vec::new(), Some(text), None),
            _ => (vec![NativeKey::A], None, Some(ShiftPolicy::Physical)),
        };
        self.document.bindings.push(BindingDocument {
            key: self.selected,
            modifiers,
            keys,
            text,
            shift,
        });
        self.selected_rule = Some(self.document.bindings.len() - 1);
    }
    pub fn render(
        &mut self,
        ui: &mut egui::Ui,
        capabilities: Option<BackendCapabilities>,
    ) -> Vec<EditorIntent> {
        let mut intents = Vec::new();
        super::presentation::heading(
            ui,
            "Keyboard & mappings",
            "Choose a physical C64 key, inspect its inherited output, or add a custom rule.",
        );
        if self.raw_dirty {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Validate or discard the TOML draft before editing the visual mapping.",
            );
        }
        ui.add_enabled_ui(!self.raw_dirty, |ui| {
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut self.document.name);
            egui::ComboBox::from_id_salt("base-profile")
                .selected_text(&self.document.profile)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.document.profile, "modern".into(), "Modern");
                    ui.selectable_value(
                        &mut self.document.profile,
                        "authentic".into(),
                        "Authentic C64",
                    );
                });
            egui::ComboBox::from_id_salt("mapping-layout")
                .selected_text(&self.document.layout)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.document.layout, "US".into(), "US");
                    ui.selectable_value(&mut self.document.layout, "UK".into(), "UK");
                });
            if self.is_dirty() {
                ui.label(egui::RichText::new("Unsaved changes").italics());
            }
        });
        ui.add_space(10.0);
        super::keyboard::render(ui, &mut self.selected, |key| {
            self.document.bindings.iter().any(|rule| rule.key == key)
        });
        ui.add_space(12.0);
        let rules: Vec<_> = self
            .document
            .bindings
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule.key == self.selected)
            .map(|(index, rule)| (index, rule.modifiers.clone()))
            .collect();
        if self
            .selected_rule
            .is_some_and(|index| !rules.iter().any(|(candidate, _)| *candidate == index))
        {
            self.selected_rule = None;
        }
        super::presentation::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(format!("{:?}", self.selected));
                ui.label(format!(
                    "Matrix {}:{}",
                    self.selected.id() / 8,
                    self.selected.id() % 8
                ));
            });
            ui.horizontal(|ui| {
                ui.label("Preview with");
                for (modifier, label) in [
                    (SourceModifier::Shift, "Shift"),
                    (SourceModifier::Control, "CTRL"),
                    (SourceModifier::Commodore, "Commodore"),
                ] {
                    let mut selected = self.source_mask & modifier.mask() != 0;
                    if ui.checkbox(&mut selected, label).changed() {
                        if selected {
                            self.source_mask |= modifier.mask();
                        } else {
                            self.source_mask &= !modifier.mask();
                        }
                    }
                }
            });
            match self.document.validate().and_then(|profile| {
                profile.preview(self.selected, self.source_mask, HostPlatform::current())
            }) {
                Ok(action) => {
                    ui.label(format!("Resolved output: {}", action_label(&action)));
                    if matches!(action, ResolvedAction::Text(_))
                        && capabilities.is_some_and(|caps| !caps.text)
                    {
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            "This backend does not support this text action.",
                        );
                    }
                }
                Err(error) => {
                    ui.colored_label(ui.visuals().error_fg_color, error.to_string());
                }
            }
            if self.selected.is_modifier() {
                ui.label("Modifier identities use the global roles below. Shift remains physical.");
            } else {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("selected-rule")
                        .selected_text(
                            self.selected_rule
                                .map_or("Inherited mapping".to_owned(), |i| {
                                    format!("Custom rule {}", i + 1)
                                }),
                        )
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.selected_rule, None, "Inherited mapping");
                            for (index, modifiers) in &rules {
                                ui.selectable_value(
                                    &mut self.selected_rule,
                                    Some(*index),
                                    format!("Rule {} · {:?}", index + 1, modifiers),
                                );
                            }
                        });
                    if ui.button("Add rule").clicked() {
                        self.add_rule();
                    }
                    if ui
                        .add_enabled(
                            self.selected_rule.is_some(),
                            egui::Button::new("Remove rule"),
                        )
                        .clicked()
                        && let Some(index) = self.selected_rule.take()
                    {
                        self.document.bindings.remove(index);
                    }
                });
                if let Some(index) = self.selected_rule {
                    rule_editor(ui, index, &mut self.document.bindings[index]);
                } else {
                    ui.small("This output comes from the base profile. Adding a rule overrides matching source modifiers.");
                }
            }
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            role_combo(ui, "CTRL role", &mut self.document.control_modifier, false);
            role_combo(
                ui,
                "Commodore role",
                &mut self.document.commodore_modifier,
                true,
            );
            if ui.button("Reset to base").clicked() {
                self.document.bindings.clear();
                self.document.control_modifier = None;
                self.document.commodore_modifier = None;
                self.selected_rule = None;
                self.raw_dirty = false;
                self.refresh_raw();
            }
        });
        ui.add_space(10.0);
        });
        if ui.checkbox(&mut self.show_raw, "Edit TOML draft").changed()
            && self.show_raw
            && !self.raw_dirty
        {
            self.refresh_raw();
        }
        if self.show_raw {
            if ui
                .add(
                    egui::TextEdit::multiline(&mut self.raw_draft)
                        .code_editor()
                        .desired_rows(10)
                        .desired_width(f32::INFINITY),
                )
                .changed()
            {
                self.raw_dirty = true;
            }
            ui.horizontal(|ui| {
                if ui.button("Validate TOML draft").clicked() {
                    self.error = self.parse_raw().err();
                }
                if ui
                    .add_enabled(self.raw_dirty, egui::Button::new("Discard TOML draft"))
                    .clicked()
                {
                    self.raw_dirty = false;
                    self.error = None;
                    self.refresh_raw();
                }
            });
        }
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        let validation = self.validated_document();
        if let Err(error) = &validation {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.horizontal(|ui| {
            ui.label("Profile file");
            ui.add(
                egui::TextEdit::singleline(&mut self.file_path)
                    .hint_text("Path for import, save copy or export")
                    .desired_width(400.0),
            );
            if ui
                .add_enabled(
                    !self.file_path.trim().is_empty(),
                    egui::Button::new("Import"),
                )
                .clicked()
            {
                intents.push(EditorIntent::Import(PathBuf::from(self.file_path.trim())));
            }
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(validation.is_ok(), egui::Button::new("Save profile"))
                .clicked()
            {
                intents.push(EditorIntent::Save {
                    document: self.document.clone(),
                    path: None,
                });
            }
            for label in ["Save copy", "Export"] {
                if ui
                    .add_enabled(
                        validation.is_ok() && !self.file_path.trim().is_empty(),
                        egui::Button::new(label),
                    )
                    .clicked()
                {
                    let path = PathBuf::from(self.file_path.trim());
                    intents.push(if label == "Export" {
                        EditorIntent::Export {
                            document: self.document.clone(),
                            path,
                        }
                    } else {
                        EditorIntent::Save {
                            document: self.document.clone(),
                            path: Some(path),
                        }
                    });
                }
            }
            if ui
                .add_enabled(validation.is_ok(), egui::Button::new("Apply mapping"))
                .clicked()
            {
                intents.push(EditorIntent::Apply(self.document.clone()));
            }
        });
        ui.small("Applying restarts GUI-owned hosting with input disabled. It never deploys a C64 program.");
        intents
    }
}
fn role_combo(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut Option<NativeKey>,
    platform_default: bool,
) {
    ui.label(label)
        .on_hover_text("Meta means Command on macOS and the Windows/Super key on Windows/Linux.");
    egui::ComboBox::from_id_salt(label)
        .selected_text(value.map_or(
            if platform_default {
                "Platform default".into()
            } else {
                "Control (default)".into()
            },
            |key| format!("{key:?}"),
        ))
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, "Default");
            for key in [NativeKey::Control, NativeKey::Alt, NativeKey::Meta] {
                ui.selectable_value(value, Some(key), format!("{key:?}"));
            }
        });
}
fn rule_editor(ui: &mut egui::Ui, index: usize, rule: &mut BindingDocument) {
    ui.horizontal(|ui| {
        ui.label("Match source modifiers");
        for modifier in [
            SourceModifier::Shift,
            SourceModifier::Control,
            SourceModifier::Commodore,
        ] {
            let mut present = rule.modifiers.contains(&modifier);
            if ui.checkbox(&mut present, format!("{modifier:?}")).changed() {
                if present {
                    rule.modifiers.push(modifier);
                } else {
                    rule.modifiers.retain(|candidate| *candidate != modifier);
                }
            }
        }
    });
    let mut text_action = rule.text.is_some();
    ui.horizontal(|ui| {
        ui.label("Action");
        if ui
            .radio_value(&mut text_action, false, "Native chord")
            .changed()
        {
            rule.text = None;
            if rule.keys.is_empty() {
                rule.keys.push(NativeKey::A);
            }
        }
        if ui.radio_value(&mut text_action, true, "Text").changed() {
            rule.keys.clear();
            rule.text = Some(String::new());
            rule.shift = None;
        }
    });
    if let Some(text) = &mut rule.text {
        ui.add(
            egui::TextEdit::singleline(text)
                .hint_text("Up to 16 characters, no control characters"),
        );
        ui.small("Text support depends on the backend and is separate from native shortcuts.");
    } else {
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            for (position, key) in rule.keys.iter_mut().enumerate() {
                egui::ComboBox::from_id_salt(("output-key", index, position))
                    .selected_text(format!("{key:?}"))
                    .show_ui(ui, |ui| {
                        for choice in NativeKey::ALL {
                            ui.selectable_value(key, *choice, format!("{choice:?}"));
                        }
                    });
                if ui
                    .small_button("×")
                    .on_hover_text("Remove this chord key")
                    .clicked()
                {
                    remove = Some(position);
                }
            }
            if ui
                .add_enabled(rule.keys.len() < 8, egui::Button::new("+ Key"))
                .clicked()
            {
                rule.keys.push(NativeKey::A);
            }
        });
        if let Some(position) = remove {
            rule.keys.remove(position);
        }
        ui.horizontal(|ui| {
            ui.label("Shift policy");
            egui::ComboBox::from_id_salt(("shift-policy", index))
                .selected_text(format!("{:?}", rule.shift.unwrap_or(ShiftPolicy::Physical)))
                .show_ui(ui, |ui| {
                    for policy in [
                        ShiftPolicy::Physical,
                        ShiftPolicy::Required,
                        ShiftPolicy::Suppress,
                    ] {
                        ui.selectable_value(&mut rule.shift, Some(policy), format!("{policy:?}"));
                    }
                });
        });
    }
}
pub fn action_label(action: &ResolvedAction) -> String {
    match action {
        ResolvedAction::Keys { keys, shift } => format!(
            "{} · Shift {shift:?}",
            keys.iter()
                .map(|key| format!("{key:?}"))
                .collect::<Vec<_>>()
                .join(" + ")
        ),
        ResolvedAction::Text(text) => format!("Text {text:?}"),
        ResolvedAction::Modifier(key) => format!("Modifier {key:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_toml_draft_survives_failed_validation_and_discard_restores_saved_document() {
        let mut editor = MappingEditor::new("modern", "US");
        editor.raw_draft = "name = [broken".into();
        editor.raw_dirty = true;
        assert!(editor.parse_raw().is_err());
        assert!(editor.is_dirty());
        assert_eq!(editor.raw_draft, "name = [broken");
        assert!(editor.validated_document().is_err());
        editor.discard();
        assert!(!editor.is_dirty());
        assert!(editor.validated_document().is_ok());
    }
    #[test]
    fn semantically_invalid_rule_is_retained_and_cannot_be_applied() {
        let mut editor = MappingEditor::new("modern", "US");
        editor.add_rule();
        editor.document.bindings[0].keys = vec![NativeKey::A, NativeKey::A];
        editor.refresh_raw();
        editor.raw_dirty = true;
        assert!(editor.parse_raw().is_err());
        assert_eq!(editor.document.bindings[0].keys.len(), 2);
        assert!(editor.is_dirty());
        assert!(editor.validated_document().is_err());
    }
    #[test]
    fn saved_copy_round_trip_keeps_roles_and_modifiers() {
        let mut editor = MappingEditor::new("authentic", "UK");
        editor.document.commodore_modifier = Some(NativeKey::Alt);
        editor.source_mask = 1;
        editor.add_rule();
        editor.document.bindings[0].keys = vec![NativeKey::Control, NativeKey::B];
        let copy = MappingDocument::from_toml(&editor.document.to_toml().unwrap()).unwrap();
        let resolved = copy
            .validate()
            .unwrap()
            .preview(PhysicalKey::A, 1, HostPlatform::MacOS)
            .unwrap();
        assert!(
            matches!(resolved, ResolvedAction::Keys { keys, .. } if keys == vec![NativeKey::Control, NativeKey::B])
        );
        editor.load(copy, PathBuf::from("copy.toml"));
        assert!(!editor.is_dirty());
    }
    #[test]
    fn importing_invalid_text_retains_it_and_keeps_the_saved_profile() {
        let mut editor = MappingEditor::new("modern", "US");
        let prior = editor.document.clone();
        editor.load_raw("[invalid".into(), PathBuf::from("draft.toml"));
        assert_eq!(editor.document, prior);
        assert_eq!(editor.raw_draft, "[invalid");
        assert!(editor.show_raw && editor.is_dirty());
        assert!(editor.validated_document().is_err());
    }
    #[test]
    fn new_rule_starts_with_inherited_shifted_cursor_output() {
        let mut editor = MappingEditor::new("modern", "US");
        editor.selected = PhysicalKey::CursorRight;
        editor.source_mask = SourceModifier::Shift.mask();
        editor.add_rule();
        let rule = &editor.document.bindings[0];
        assert_eq!(rule.keys, vec![NativeKey::ArrowLeft]);
        assert_eq!(rule.shift, Some(ShiftPolicy::Suppress));
        assert!(editor.validated_document().is_ok());
    }
}

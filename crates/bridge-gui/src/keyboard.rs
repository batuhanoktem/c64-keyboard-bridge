//! Physical C64 keyboard geometry. A click selects a matrix identity only.
use bridge_protocol::PhysicalKey;
use eframe::egui;

#[derive(Clone, Copy)]
struct Cap {
    label: &'static str,
    key: Option<PhysicalKey>,
    width: f32,
}
const fn key(label: &'static str, key: PhysicalKey, width: f32) -> Cap {
    Cap {
        label,
        key: Some(key),
        width,
    }
}
const fn annotation(label: &'static str, width: f32) -> Cap {
    Cap {
        label,
        key: None,
        width,
    }
}
use PhysicalKey as P;
const ROWS: &[&[Cap]] = &[
    &[
        key("LEFT\nARROW", P::LeftArrow, 1.0),
        key("1", P::Num1, 1.0),
        key("2", P::Num2, 1.0),
        key("3", P::Num3, 1.0),
        key("4", P::Num4, 1.0),
        key("5", P::Num5, 1.0),
        key("6", P::Num6, 1.0),
        key("7", P::Num7, 1.0),
        key("8", P::Num8, 1.0),
        key("9", P::Num9, 1.0),
        key("0", P::Num0, 1.0),
        key("+", P::Plus, 1.0),
        key("−", P::Minus, 1.0),
        key("£", P::Pound, 1.0),
        key("HOME", P::Home, 1.0),
        key("INST\nDEL", P::Delete, 1.0),
        key("F1 / F2", P::F1, 1.7),
    ],
    &[
        key("CTRL", P::Control, 1.5),
        key("Q", P::Q, 1.0),
        key("W", P::W, 1.0),
        key("E", P::E, 1.0),
        key("R", P::R, 1.0),
        key("T", P::T, 1.0),
        key("Y", P::Y, 1.0),
        key("U", P::U, 1.0),
        key("I", P::I, 1.0),
        key("O", P::O, 1.0),
        key("P", P::P, 1.0),
        key("@", P::At, 1.0),
        key("*", P::Asterisk, 1.0),
        key("UP\nARROW", P::UpArrow, 1.0),
        annotation("RESTORE", 1.5),
        key("F3 / F4", P::F3, 1.7),
    ],
    &[
        key("RUN\nSTOP", P::RunStop, 1.5),
        annotation("SHIFT\nLOCK", 1.0),
        key("A", P::A, 1.0),
        key("S", P::S, 1.0),
        key("D", P::D, 1.0),
        key("F", P::F, 1.0),
        key("G", P::G, 1.0),
        key("H", P::H, 1.0),
        key("J", P::J, 1.0),
        key("K", P::K, 1.0),
        key("L", P::L, 1.0),
        key(":", P::Colon, 1.0),
        key(";", P::Semicolon, 1.0),
        key("=", P::Equal, 1.0),
        key("RETURN", P::Return, 1.5),
        key("F5 / F6", P::F5, 1.7),
    ],
    &[
        key("C=", P::Commodore, 1.0),
        key("SHIFT", P::LeftShift, 1.5),
        key("Z", P::Z, 1.0),
        key("X", P::X, 1.0),
        key("C", P::C, 1.0),
        key("V", P::V, 1.0),
        key("B", P::B, 1.0),
        key("N", P::N, 1.0),
        key("M", P::M, 1.0),
        key(",", P::Comma, 1.0),
        key(".", P::Period, 1.0),
        key("/", P::Slash, 1.0),
        key("SHIFT", P::RightShift, 1.5),
        key("UP /\nDOWN", P::CursorDown, 1.0),
        key("LEFT /\nRIGHT", P::CursorRight, 1.0),
        key("F7 / F8", P::F7, 1.7),
    ],
    &[key("SPACE", P::Space, 10.0)],
];

pub fn render(
    ui: &mut egui::Ui,
    selected: &mut PhysicalKey,
    overridden: impl Fn(PhysicalKey) -> bool,
) {
    egui::ScrollArea::horizontal().id_salt("physical-keyboard").show(ui, |ui| {
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
        for (index, row) in ROWS.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if index == 4 { ui.add_space(118.0); }
                for cap in *row {
                    if cap.key.is_some_and(|key| matches!(key, P::F1 | P::F3 | P::F5 | P::F7)) {
                        ui.add_space(12.0);
                    }
                    let text = if cap.key.is_some_and(&overridden) {
                        format!("{} •", cap.label)
                    } else { cap.label.to_owned() };
                    let mut button = egui::Button::new(egui::RichText::new(text).size(12.0))
                        .min_size(egui::vec2(46.0 * cap.width - 4.0, 48.0));
                    if cap.key == Some(*selected) {
                        button = button.fill(super::presentation::accent(ui));
                    }
                    let response = ui.add_enabled(cap.key.is_some(), button);
                    if let Some(key) = cap.key {
                        if response.clicked() { *selected = key; }
                        response.on_hover_text(format!("{key:?} · matrix {}:{}", key.id() / 8, key.id() % 8));
                    } else {
                        response.on_disabled_hover_text(if cap.label == "RESTORE" {
                            "RESTORE is outside the scanned matrix; independent events are unverified."
                        } else {
                            "SHIFT LOCK shares the observed left Shift state; it has no independent identity."
                        });
                    }
                }
            });
        }
        });
    });
    ui.small("• Custom rule    SHIFT LOCK shares Shift state    RESTORE events are unavailable");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unavailable_keys_show_hover_explanations_without_changing_selection() {
        for (label, explanation) in [
            ("RESTORE", "RESTORE is outside the scanned matrix"),
            (
                "SHIFT\nLOCK",
                "SHIFT LOCK shares the observed left Shift state",
            ),
        ] {
            let context = egui::Context::default();
            context.global_style_mut(|style| {
                style.interaction.tooltip_delay = 0.0;
                style.interaction.show_tooltips_only_when_still = false;
            });
            let mut selected = P::A;
            let mut draw = |events| {
                let mut output = context.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1200.0, 500.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| render(ui, &mut selected, |_| false),
                );
                output.textures_delta.clear();
                output
            };
            for _ in 0..2 {
                draw(vec![]);
            }
            let output = draw(vec![]);
            let pos = output
                .shapes
                .iter()
                .find_map(|shape| {
                    if let egui::epaint::Shape::Text(text) = &shape.shape {
                        (text.galley.job.text == label)
                            .then(|| text.visual_bounding_rect().center())
                    } else {
                        None
                    }
                })
                .unwrap();
            draw(vec![egui::Event::PointerMoved(pos)]);
            let output = draw(vec![]);
            assert!(output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::epaint::Shape::Text(text) if text.galley.job.text.starts_with(explanation))
            }), "Tooltip missing for {label}");
            draw(vec![
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
            ]);
            assert_eq!(selected, P::A);
        }
    }
    #[test]
    fn keyboard_contains_every_matrix_key_once_without_inventing_restore() {
        let keys: Vec<_> = ROWS
            .iter()
            .flat_map(|row| row.iter())
            .filter_map(|cap| cap.key)
            .collect();
        assert_eq!(keys.len(), 64);
        let set: std::collections::BTreeSet<_> = keys.into_iter().collect();
        assert_eq!(set.len(), 64);
        assert_eq!(set, bridge_protocol::ALL_KEYS.into_iter().collect());
        assert_eq!(
            ROWS.iter()
                .flat_map(|row| row.iter())
                .filter(|cap| cap.key.is_none())
                .count(),
            2
        );
    }
    #[test]
    fn function_column_is_aligned_after_sixteen_main_key_units() {
        for row in &ROWS[..4] {
            let width: f32 = row
                .iter()
                .filter(|cap| {
                    !cap.key
                        .is_some_and(|key| matches!(key, P::F1 | P::F3 | P::F5 | P::F7))
                })
                .map(|cap| cap.width)
                .sum();
            assert!((width - 16.0).abs() < 0.001, "{width}");
        }
    }
}

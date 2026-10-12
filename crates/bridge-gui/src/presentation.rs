use eframe::egui;

pub fn accent(ui: &egui::Ui) -> egui::Color32 {
    if ui.visuals().dark_mode {
        egui::Color32::from_rgb(92, 101, 181)
    } else {
        egui::Color32::from_rgb(161, 170, 229)
    }
}
pub fn heading(ui: &mut egui::Ui, title: &str, detail: &str) {
    ui.heading(title);
    ui.add_space(4.0);
    ui.label(egui::RichText::new(detail).color(ui.visuals().weak_text_color()));
    ui.add_space(14.0);
}
pub fn status(ui: &mut egui::Ui, label: &str, active: bool) {
    let color = if active {
        egui::Color32::from_rgb(76, 170, 119)
    } else {
        ui.visuals().weak_text_color()
    };
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 3.0, color);
        ui.colored_label(color, label);
    });
}
pub fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style())
        .inner_margin(14.0)
        .show(ui, add);
}
pub fn field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) -> egui::Response {
    ui.label(label);
    let response = ui.add_sized(
        [300.0, ui.spacing().interact_size.y],
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(300.0),
    );
    ui.end_row();
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn grid_text_fields_keep_readable_width_next_to_short_numeric_controls() {
        let context = egui::Context::default();
        let mut address = "127.0.0.1".to_owned();
        let mut port = 80u16;
        for _ in 0..3 {
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 700.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    egui::Grid::new("field-width-regression").show(ui, |ui| {
                        let text = field(
                            ui,
                            "Device hostname / IPv4",
                            &mut address,
                            "Enter a hostname",
                        );
                        assert!(text.rect.width() >= 299.0, "{}", text.rect.width());
                        ui.label("REST port");
                        ui.add(egui::DragValue::new(&mut port));
                        ui.end_row();
                    });
                },
            );
            output.textures_delta.clear();
        }
    }
}

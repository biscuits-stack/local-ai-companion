use eframe::egui::{self, Pos2, Rect, Vec2};
use crate::theme;

#[derive(Debug, Default)]
pub struct ToolbarActions {
    pub toggle_mic: bool,
    pub toggle_voice: bool,
    pub toggle_chat: bool,
    pub toggle_roam: bool,
    pub capture_screen: bool,
    pub toggle_always_on_top: bool,
    pub exit_app: bool,
}

pub struct Toolbar {
    pub pos: Pos2,
    pub size: Vec2,
    pub is_dragging: bool,
    pub drag_offset: Vec2,
}

impl Toolbar {
    pub fn new(screen_size: Vec2) -> Self {
        let initial_w = 400.0_f32;
        let default_x = ((screen_size.x - initial_w) * 0.5).max(20.0);
        let default_y = (screen_size.y - 75.0).max(20.0);

        Self {
            pos: Pos2::new(default_x, default_y),
            size: Vec2::new(initial_w, 44.0),
            is_dragging: false,
            drag_offset: Vec2::ZERO,
        }
    }

    pub fn get_rect(&self) -> Rect {
        Rect::from_min_size(self.pos, self.size)
    }

    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        is_mic_muted: bool,
        is_voice_muted: bool,
        is_chat_open: bool,
        is_roaming: bool,
        is_always_on_top: bool,
    ) -> (ToolbarActions, Rect) {
        let t = theme::tokens();
        let mut actions = ToolbarActions::default();

        // egui::Area auto-sizes to fit all buttons without any hardcoded fixed width,
        // preventing the rightmost button (exit '✕') from ever being clipped.
        let area_resp = egui::Area::new(egui::Id::new("companion_toolbar"))
            .fixed_pos(self.pos)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::none()
                    .fill(t.bg_surface_elevated)
                    .stroke(t.hairline_border)
                    .rounding(t.rounding_control)
                    .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(2.0);

                            // Drag handle grip
                            let grip_rect = Rect::from_min_size(ui.cursor().min, Vec2::new(12.0, 26.0));
                            let grip_resp = ui.allocate_rect(grip_rect, egui::Sense::drag());
                            if grip_resp.hovered() || self.is_dragging {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                            }
                            if grip_resp.drag_started() {
                                self.is_dragging = true;
                                if let Some(mouse_pos) = ui.ctx().pointer_latest_pos() {
                                    self.drag_offset = self.pos - mouse_pos;
                                }
                            }
                            if grip_resp.dragged() {
                                if let Some(mouse_pos) = ui.ctx().pointer_latest_pos() {
                                    self.pos = mouse_pos + self.drag_offset;
                                }
                            }
                            if grip_resp.drag_stopped() {
                                self.is_dragging = false;
                            }

                            // Draw subtle grip dots using theme text_muted
                            let grip_painter = ui.painter();
                            for row in 0..3 {
                                let dot_y = grip_rect.min.y + 6.0 + row as f32 * 5.5;
                                grip_painter.circle_filled(Pos2::new(grip_rect.min.x + 3.0, dot_y), 1.2, t.text_muted);
                                grip_painter.circle_filled(Pos2::new(grip_rect.min.x + 8.0, dot_y), 1.2, t.text_muted);
                            }

                            ui.add_space(2.0);

                            // 1. Mic Button (STT)
                            let (mic_color, mic_text) = if is_mic_muted {
                                (t.status_muted, "Mic Off")
                            } else {
                                (t.status_live, "Mic Live")
                            };
                            let mic_btn = egui::Button::new(
                                egui::RichText::new(mic_text)
                                    .color(mic_color)
                                    .size(11.5)
                                    .strong(),
                            )
                            .rounding(t.rounding_control);

                            if ui.add(mic_btn)
                                .on_hover_text(if is_mic_muted { "Turn ON Mic (Whisper STT)" } else { "Mute Mic (Whisper STT)" })
                                .clicked()
                            {
                                actions.toggle_mic = true;
                            }

                            // 2. Voice Button
                            let (voice_color, voice_text) = if is_voice_muted {
                                (t.text_muted, "Voice Off")
                            } else {
                                (t.text_primary, "Voice")
                            };
                            let voice_btn = egui::Button::new(
                                egui::RichText::new(voice_text)
                                    .color(voice_color)
                                    .size(11.5)
                                    .strong(),
                            )
                            .rounding(t.rounding_control);

                            if ui.add(voice_btn)
                                .on_hover_text(if is_voice_muted { "Unmute AI Voice (Kokoro TTS)" } else { "Mute AI Voice (Kokoro TTS)" })
                                .clicked()
                            {
                                actions.toggle_voice = true;
                            }

                            // 3. Chat Button
                            let chat_btn = if is_chat_open {
                                egui::Button::new(
                                    egui::RichText::new("Chat")
                                        .color(t.text_active_control)
                                        .size(11.5)
                                        .strong(),
                                )
                                .fill(t.bg_active_control)
                                .stroke(t.border_active_control)
                                .rounding(t.rounding_control)
                            } else {
                                egui::Button::new(
                                    egui::RichText::new("Chat")
                                        .color(t.text_primary)
                                        .size(11.5)
                                        .strong(),
                                )
                                .rounding(t.rounding_control)
                            };

                            if ui.add(chat_btn)
                                .on_hover_text(if is_chat_open { "Close Chat" } else { "Open Chat" })
                                .clicked()
                            {
                                actions.toggle_chat = true;
                            }

                            // 4. Roaming Button
                            let (roam_text, roam_color) = if is_roaming {
                                ("Roam", t.text_primary)
                            } else {
                                ("Parked", t.text_muted)
                            };
                            let roam_btn = egui::Button::new(
                                egui::RichText::new(roam_text)
                                    .color(roam_color)
                                    .size(11.5),
                            )
                            .rounding(t.rounding_control);

                            if ui.add(roam_btn)
                                .on_hover_text(if is_roaming { "Park Companion (Stop roaming)" } else { "Free Companion (Resume roaming)" })
                                .clicked()
                            {
                                actions.toggle_roam = true;
                            }

                            // 5. Vision / Screen Button
                            let vision_btn = egui::Button::new(
                                egui::RichText::new("Vision")
                                    .color(t.text_primary)
                                    .size(11.5),
                            )
                            .rounding(t.rounding_control);

                            if ui.add(vision_btn)
                                .on_hover_text("Take Screenshot & Ask Companion to Observe Screen")
                                .clicked()
                            {
                                actions.capture_screen = true;
                            }

                            // 6. Always-on-top Pin Button
                            let pin_color = if is_always_on_top {
                                t.accent
                            } else {
                                t.text_muted
                            };
                            let pin_btn = egui::Button::new(
                                egui::RichText::new("Pin")
                                    .color(pin_color)
                                    .size(11.5),
                            )
                            .rounding(t.rounding_control);

                            if ui.add(pin_btn)
                                .on_hover_text(if is_always_on_top { "Unpin Window" } else { "Pin Always On Top" })
                                .clicked()
                            {
                                actions.toggle_always_on_top = true;
                            }

                            // 7. Exit Button
                            let exit_btn = egui::Button::new(
                                egui::RichText::new("✕")
                                    .color(t.status_muted)
                                    .size(11.5),
                            )
                            .rounding(t.rounding_control);

                            if ui.add(exit_btn)
                                .on_hover_text("Exit Companion")
                                .clicked()
                            {
                                actions.exit_app = true;
                            }
                        });
                    });
            });

        let toolbar_rect = area_resp.response.rect;
        self.size = toolbar_rect.size();

        // Keep inside screen boundaries
        let screen = ui.ctx().screen_rect();
        self.pos.x = self.pos.x.clamp(10.0, (screen.width() - self.size.x - 10.0).max(10.0));
        self.pos.y = self.pos.y.clamp(10.0, (screen.height() - self.size.y - 10.0).max(10.0));

        (actions, toolbar_rect)
    }
}

use crate::markdown;
use crate::theme;
use crate::types::{ChatMessage, MessageRole};
use eframe::egui::{self, Pos2, Rect, Vec2};
use std::time::{Duration, Instant};

pub struct ChatWindow {
    pub pos: Pos2,
    pub size: Vec2,
    pub is_dragging: bool,
    pub drag_offset: Vec2,
    pub is_resizing: bool,
    pub is_enlarged: bool,
    pub normal_size: Vec2,
    pub enlarged_size: Vec2,
    pub input_text: String,
    pub should_scroll_to_bottom: bool,
    pub copied_feedback: Option<(String, Instant)>,
}

impl ChatWindow {
    pub fn new(screen_size: Vec2) -> Self {
        let normal_w = 380.0_f32;
        let normal_h = 520.0_f32;
        let initial_x = (screen_size.x - normal_w - 40.0).max(20.0);
        let initial_y = (screen_size.y - normal_h - 90.0).max(40.0);

        Self {
            pos: Pos2::new(initial_x, initial_y),
            size: Vec2::new(normal_w, normal_h),
            is_dragging: false,
            drag_offset: Vec2::ZERO,
            is_resizing: false,
            is_enlarged: false,
            normal_size: Vec2::new(normal_w, normal_h),
            enlarged_size: Vec2::new(
                (screen_size.x * 0.42).clamp(520.0, 720.0),
                (screen_size.y * 0.72).clamp(620.0, 840.0),
            ),
            input_text: String::new(),
            should_scroll_to_bottom: true,
            copied_feedback: None,
        }
    }

    pub fn get_rect(&self) -> Rect {
        Rect::from_min_size(self.pos, self.size)
    }

    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        messages: &[ChatMessage],
        is_thinking: bool,
        thinking_tokens: usize,
        is_mic_muted: bool,
        on_send: &mut Option<String>,
        on_toggle_mic: &mut bool,
        on_capture_screen: &mut bool,
        on_close: &mut bool,
    ) -> Rect {
        let t = theme::tokens();
        let screen = ui.ctx().screen_rect();
        let chat_rect = Rect::from_min_size(self.pos, self.size);

        let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(chat_rect));

        // 1. Outer Container Frame
        egui::Frame::none()
            .fill(t.bg_surface)
            .stroke(t.hairline_border)
            .rounding(t.rounding_card)
            .inner_margin(t.window_margin)
            .show(&mut child_ui, |ui| {
                ui.set_width(self.size.x - t.window_margin.sum().x);
                ui.set_height(self.size.y - t.window_margin.sum().y);

                ui.vertical(|ui| {
                    // A. HEADER BAR
                    ui.horizontal(|ui| {
                        ui.add_space(4.0);

                        // Drag region title
                        let title_w = (self.size.x - 120.0).max(100.0);
                        let title_rect =
                            Rect::from_min_size(ui.cursor().min, Vec2::new(title_w, 24.0));
                        let drag_resp = ui.allocate_rect(title_rect, egui::Sense::drag());

                        if drag_resp.hovered() || self.is_dragging {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        }
                        if drag_resp.drag_started() {
                            self.is_dragging = true;
                            if let Some(mouse_pos) = ui.ctx().pointer_latest_pos() {
                                self.drag_offset = self.pos - mouse_pos;
                            }
                        }
                        if drag_resp.dragged() {
                            if let Some(mouse_pos) = ui.ctx().pointer_latest_pos() {
                                self.pos = mouse_pos + self.drag_offset;
                            }
                        }
                        if drag_resp.drag_stopped() {
                            self.is_dragging = false;
                        }

                        // Clean title text
                        ui.painter().text(
                            title_rect.min + Vec2::new(0.0, 3.0),
                            egui::Align2::LEFT_TOP,
                            "AI Companion",
                            egui::FontId::proportional(14.0),
                            t.text_primary,
                        );

                        // Header buttons
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Close button
                            let close_btn = egui::Button::new(
                                egui::RichText::new("✕").color(t.text_secondary).size(11.0),
                            )
                            .rounding(t.rounding_control);

                            if ui.add(close_btn).on_hover_text("Close chat").clicked() {
                                *on_close = true;
                            }

                            // Enlarge / Compact toggle button
                            let size_label = if self.is_enlarged { "–" } else { "+" };
                            let size_hover = if self.is_enlarged {
                                "Compact chatbox"
                            } else {
                                "Enlarge chatbox"
                            };

                            let size_btn = egui::Button::new(
                                egui::RichText::new(size_label)
                                    .color(t.text_primary)
                                    .size(13.0)
                                    .strong(),
                            )
                            .rounding(t.rounding_control);

                            if ui.add(size_btn).on_hover_text(size_hover).clicked() {
                                self.is_enlarged = !self.is_enlarged;
                                self.size = if self.is_enlarged {
                                    self.enlarged_size
                                } else {
                                    self.normal_size
                                };
                                self.pos.x = self
                                    .pos
                                    .x
                                    .clamp(10.0, (screen.width() - self.size.x - 10.0).max(10.0));
                                self.pos.y = self
                                    .pos
                                    .y
                                    .clamp(10.0, (screen.height() - self.size.y - 10.0).max(10.0));
                            }
                        });
                    });

                    // Hairline separator
                    ui.add_space(2.0);
                    ui.separator();
                    ui.add_space(4.0);

                    // B. SCROLLABLE MESSAGES AREA
                    let scroll_height = (self.size.y - 110.0).max(140.0);
                    let available_w = (self.size.x - t.window_margin.sum().x).max(200.0);
                    let max_bubble_w = (available_w - 28.0).max(160.0);

                    egui::ScrollArea::vertical()
                        .max_height(scroll_height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_width(available_w);
                            ui.add_space(2.0);

                            for msg in messages {
                                match msg.role {
                                    MessageRole::User => {
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::TOP),
                                            |ui| {
                                                ui.add_space(4.0);
                                                egui::Frame::none()
                                                    .fill(t.bg_user_bubble)
                                                    .rounding(t.rounding_card)
                                                    .inner_margin(egui::Margin::symmetric(
                                                        10.0, 7.0,
                                                    ))
                                                    .stroke(t.hairline_subtle)
                                                    .show(ui, |ui| {
                                                        ui.set_max_width(max_bubble_w);
                                                        ui.add(
                                                            egui::Label::new(
                                                                egui::RichText::new(&msg.text)
                                                                    .color(t.text_primary)
                                                                    .size(13.0),
                                                            )
                                                            .selectable(true)
                                                            .wrap_mode(egui::TextWrapMode::Wrap),
                                                        );
                                                    });
                                            },
                                        );
                                    }
                                    MessageRole::Assistant => {
                                        ui.with_layout(
                                            egui::Layout::left_to_right(egui::Align::TOP),
                                            |ui| {
                                                ui.add_space(4.0);
                                                egui::Frame::none()
                                                    .fill(t.bg_surface_elevated)
                                                    .rounding(t.rounding_card)
                                                    .inner_margin(egui::Margin::symmetric(
                                                        10.0, 8.0,
                                                    ))
                                                    .stroke(t.hairline_subtle)
                                                    .show(ui, |ui| {
                                                        ui.set_max_width(max_bubble_w);
                                                        ui.vertical(|ui| {
                                                            ui.horizontal(|ui| {
                                                                ui.label(
                                                                    egui::RichText::new("AI")
                                                                        .color(t.text_primary)
                                                                        .size(11.0)
                                                                        .strong(),
                                                                );
                                                                if let Some(emo) = msg.emotion {
                                                                    ui.label(
                                                                        egui::RichText::new(
                                                                            format!(
                                                                                "· {}",
                                                                                emo.display_name()
                                                                            ),
                                                                        )
                                                                        .color(t.text_secondary)
                                                                        .size(11.0),
                                                                    );
                                                                }
                                                                ui.with_layout(
                                                                    egui::Layout::right_to_left(
                                                                        egui::Align::Center,
                                                                    ),
                                                                    |ui| {
                                                                        let is_copied = self
                                                                            .copied_feedback
                                                                            .as_ref()
                                                                            .map(|(k, ts)| {
                                                                                k == &msg.id
                                                                                    && ts.elapsed()
                                                                                        < Duration::from_secs(2)
                                                                            })
                                                                            .unwrap_or(false);
                                                                        let (label, color) =
                                                                            if is_copied {
                                                                                (
                                                                                    "Copied ✓",
                                                                                    t.status_live,
                                                                                )
                                                                            } else {
                                                                                (
                                                                                    "Copy",
                                                                                    t.text_muted,
                                                                                )
                                                                            };
                                                                        let copy_btn =
                                                                            egui::Button::new(
                                                                                egui::RichText::new(
                                                                                    label,
                                                                                )
                                                                                .color(color)
                                                                                .size(10.5),
                                                                            )
                                                                            .fill(t.bg_surface_subtle)
                                                                            .rounding(
                                                                                t.rounding_badge,
                                                                            );
                                                                        if ui
                                                                            .add(copy_btn)
                                                                            .on_hover_text(
                                                                                "Copy entire reply",
                                                                            )
                                                                            .clicked()
                                                                        {
                                                                            ui.ctx().copy_text(
                                                                                msg.text.clone(),
                                                                            );
                                                                            self.copied_feedback =
                                                                                Some((
                                                                                    msg.id.clone(),
                                                                                    Instant::now(),
                                                                                ));
                                                                        }
                                                                    },
                                                                );
                                                            });
                                                            ui.add_space(4.0);
                                                            markdown::render_assistant_blocks(
                                                                ui,
                                                                &msg.id,
                                                                &msg.text,
                                                                max_bubble_w,
                                                                t,
                                                                &mut self.copied_feedback,
                                                            );
                                                        });
                                                    });
                                            },
                                        );
                                    }
                                    MessageRole::System => {
                                        ui.horizontal(|ui| {
                                            ui.add_space(8.0);
                                            ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(format!("• {}", msg.text))
                                                        .color(t.text_muted)
                                                        .size(11.5)
                                                        .italics(),
                                                )
                                                .wrap_mode(egui::TextWrapMode::Wrap),
                                            );
                                        });
                                    }
                                }
                                ui.add_space(6.0);
                            }

                            if is_thinking {
                                ui.horizontal(|ui| {
                                    ui.add_space(8.0);
                                    let thinking_label = if thinking_tokens > 0 {
                                        format!("⋯ Thinking... ({thinking_tokens} tok)")
                                    } else {
                                        "⋯ Thinking...".to_string()
                                    };
                                    ui.label(
                                        egui::RichText::new(thinking_label)
                                            .color(t.text_secondary)
                                            .size(12.0)
                                            .italics(),
                                    );
                                });
                            }

                            if self.should_scroll_to_bottom {
                                ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                                self.should_scroll_to_bottom = false;
                            }
                        });

                    // Hairline separator before input bar
                    ui.separator();
                    ui.add_space(6.0);

                    // C. INPUT BAR
                    ui.horizontal(|ui| {
                        ui.add_space(4.0);

                        // 1. Vision Button
                        let vision_btn = egui::Button::new(
                            egui::RichText::new("Cam").color(t.text_primary).size(11.5),
                        )
                        .rounding(t.rounding_control);

                        if ui
                            .add(vision_btn)
                            .on_hover_text("Send Screen Capture to Companion")
                            .clicked()
                        {
                            *on_capture_screen = true;
                        }

                        // 2. Mic Button
                        let mic_color = if is_mic_muted {
                            t.status_muted
                        } else {
                            t.status_live
                        };

                        let mic_btn = egui::Button::new(
                            egui::RichText::new("Mic")
                                .color(mic_color)
                                .size(11.5)
                                .strong(),
                        )
                        .rounding(t.rounding_control);

                        if ui
                            .add(mic_btn)
                            .on_hover_text(if is_mic_muted {
                                "Turn ON Mic"
                            } else {
                                "Mute Mic"
                            })
                            .clicked()
                        {
                            *on_toggle_mic = true;
                        }

                        // 3. Text Input Box
                        let input_w = (self.size.x - 165.0).max(120.0);
                        let text_edit = egui::TextEdit::singleline(&mut self.input_text)
                            .hint_text("Type a message...")
                            .text_color(t.text_primary)
                            .desired_width(input_w);

                        let text_resp = ui.add(text_edit);

                        // 4. Send Button
                        let send_btn = egui::Button::new(
                            egui::RichText::new("Send")
                                .color(t.text_active_control)
                                .size(12.0)
                                .strong(),
                        )
                        .fill(t.bg_active_control)
                        .stroke(t.border_active_control)
                        .rounding(t.rounding_control);

                        let enter_pressed =
                            text_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        let send_clicked = ui.add(send_btn).clicked();

                        if (enter_pressed || send_clicked) && !self.input_text.trim().is_empty() {
                            let to_send = self.input_text.trim().to_string();
                            self.input_text.clear();
                            self.should_scroll_to_bottom = true;
                            *on_send = Some(to_send);
                            text_resp.request_focus();
                        }
                    });
                });
            });

        // 2. Interactive Resize Drag Handle (Bottom-Right Corner)
        let handle_size = Vec2::new(16.0, 16.0);
        let handle_rect = Rect::from_min_size(chat_rect.max - handle_size, handle_size);
        let resize_resp = ui.allocate_rect(handle_rect, egui::Sense::drag());

        if resize_resp.hovered() || self.is_resizing {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
        }

        if resize_resp.drag_started() {
            self.is_resizing = true;
        }

        if resize_resp.dragged() {
            let delta = resize_resp.drag_delta();
            self.size.x = (self.size.x + delta.x).clamp(300.0, (screen.width() - 40.0).max(300.0));
            self.size.y = (self.size.y + delta.y).clamp(360.0, (screen.height() - 60.0).max(360.0));
        }

        if resize_resp.drag_stopped() {
            self.is_resizing = false;
        }

        // Corner resize grip indication
        let grip_stroke = if resize_resp.hovered() || self.is_resizing {
            t.hairline_sheen
        } else {
            t.hairline_subtle
        };
        let br = chat_rect.max - Vec2::new(4.0, 4.0);
        ui.painter().line_segment(
            [br - Vec2::new(8.0, 0.0), br - Vec2::new(0.0, 8.0)],
            grip_stroke,
        );
        ui.painter().line_segment(
            [br - Vec2::new(4.0, 0.0), br - Vec2::new(0.0, 4.0)],
            grip_stroke,
        );

        // Keep inside screen boundaries
        self.pos.x = self
            .pos
            .x
            .clamp(10.0, (screen.width() - self.size.x - 10.0).max(10.0));
        self.pos.y = self
            .pos
            .y
            .clamp(10.0, (screen.height() - self.size.y - 10.0).max(10.0));

        chat_rect
    }
}

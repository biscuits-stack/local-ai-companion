use std::time::Instant;
use eframe::egui::{self, Vec2};

use crate::backend::{BackendEvent, BackendHandle};
use crate::chat::ChatWindow;
use crate::companion::Companion;
use crate::toolbar::Toolbar;
use crate::types::{ChatMessage, CompanionEmotion, LlmStatus, MessageRole, Toast};
use crate::window_helper::WindowHitTestManager;

pub struct DesktopCompanionApp {
    pub companion: Companion,
    pub toolbar: Toolbar,
    pub chat_window: ChatWindow,
    pub backend: BackendHandle,
    pub llm_status: LlmStatus,
    pub messages: Vec<ChatMessage>,
    pub is_chat_open: bool,
    pub is_mic_muted: bool,
    pub is_voice_muted: bool,
    pub is_always_on_top: bool,
    pub toast: Option<Toast>,
    pub hit_test_mgr: WindowHitTestManager,
    pub start_time: Instant,
    pub last_frame_time: Instant,
}

impl DesktopCompanionApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let screen_size = cc.egui_ctx.screen_rect().size();
        let companion = Companion::new(screen_size);
        let toolbar = Toolbar::new(screen_size);
        let chat_window = ChatWindow::new(screen_size);
        let backend = BackendHandle::new();

        let initial_messages = vec![ChatMessage {
            id: "sys_init".to_string(),
            role: MessageRole::System,
            text: "AI Companion Online (Kokoro TTS, Whisper STT & Screen Vision ready).".to_string(),
            emotion: Some(CompanionEmotion::Idle),
            timestamp: "Now".to_string(),
        }];

        Self {
            companion,
            toolbar,
            chat_window,
            backend,
            llm_status: LlmStatus::Starting,
            messages: initial_messages,
            is_chat_open: false,
            is_mic_muted: true,
            is_voice_muted: false,
            is_always_on_top: true,
            toast: Some(Toast {
                message: "🟢 Desktop Companion Active".to_string(),
                expires_at: 3.0,
            }),
            hit_test_mgr: WindowHitTestManager::new(),
            start_time: Instant::now(),
            last_frame_time: Instant::now(),
        }
    }

    pub fn show_toast(&mut self, msg: String, duration_secs: f64) {
        let current_time = self.start_time.elapsed().as_secs_f64();
        self.toast = Some(Toast {
            message: msg,
            expires_at: current_time + duration_secs,
        });
    }

    pub fn handle_send_message(&mut self, text: String, force_screenshot: bool) {
        let current_time = self.start_time.elapsed().as_secs_f64();
        self.messages.push(ChatMessage {
            id: format!("user_{current_time}"),
            role: MessageRole::User,
            text: text.clone(),
            emotion: None,
            timestamp: "Now".to_string(),
        });

        self.companion.is_thinking = true;
        self.companion.emotion = CompanionEmotion::Thinking;
        self.backend.send_message(text, force_screenshot);
    }
}

impl eframe::App for DesktopCompanionApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = (now - self.last_frame_time).as_secs_f32().min(0.1);
        self.last_frame_time = now;
        let current_time = self.start_time.elapsed().as_secs_f64();

        let screen_size = ctx.screen_rect().size();

        // 1. Process backend events
        while let Ok(event) = self.backend.event_receiver.try_recv() {
            match event {
                BackendEvent::ServerStatus { connected: _, mic_active, bot_is_talking } => {
                    self.is_mic_muted = !mic_active;
                    self.companion.is_listening = mic_active;
                    self.companion.is_speaking = bot_is_talking && !self.is_voice_muted;
                }
                BackendEvent::LlmStatusChanged(status) => {
                    self.llm_status = status.clone();
                    match status {
                        LlmStatus::LoadingModel => {
                            self.show_toast("Bonsai 27B: Loading model into VRAM...".to_string(), 4.0);
                        }
                        LlmStatus::Ready => {
                            self.show_toast("Bonsai 27B: Ready".to_string(), 3.0);
                        }
                        LlmStatus::Error(err) => {
                            self.show_toast(format!("Bonsai Error: {err}"), 5.0);
                        }
                        LlmStatus::Starting => {}
                    }
                }
                BackendEvent::AssistantMessage { text, emotion } => {
                    self.companion.is_thinking = false;
                    self.companion.emotion = emotion;
                    self.companion.set_subtitle(text.clone(), current_time, 7.0);

                    self.messages.push(ChatMessage {
                        id: format!("ai_{current_time}"),
                        role: MessageRole::Assistant,
                        text,
                        emotion: Some(emotion),
                        timestamp: "Now".to_string(),
                    });
                    self.chat_window.should_scroll_to_bottom = true;
                }
                BackendEvent::AssistantState { phase, emotion } => {
                    self.companion.emotion = emotion;
                    self.companion.is_thinking = phase == "thinking";
                    if phase == "speaking" {
                        self.companion.is_speaking = !self.is_voice_muted;
                    } else if phase == "idle" {
                        self.companion.is_speaking = false;
                    }
                }
                BackendEvent::Error(err) => {
                    self.show_toast(format!("⚠️ {err}"), 4.0);
                    self.companion.is_thinking = false;
                }
            }
        }

        // 2. Global hotkeys
        if ctx.input(|i| i.modifiers.ctrl && i.modifiers.alt && i.key_pressed(egui::Key::M)) {
            self.backend.toggle_mic();
        }

        // 3. Update companion roaming physics
        self.companion.update(screen_size, dt, current_time);

        // 4. Render main overlay with full transparent frame
        let frame = egui::Frame::none().fill(crate::theme::tokens().bg_transparent);
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            // A. Draw Companion Pet
            if self.companion.draw(ui, current_time) {
                self.is_chat_open = !self.is_chat_open;
            }

            // B. Draw Chat Box if open
            if self.is_chat_open {
                let mut on_send = None;
                let mut on_toggle_mic = false;
                let mut on_capture_screen = false;
                let mut on_close = false;

                self.chat_window.draw(
                    ui,
                    &self.messages,
                    self.companion.is_thinking,
                    self.is_mic_muted,
                    &mut on_send,
                    &mut on_toggle_mic,
                    &mut on_capture_screen,
                    &mut on_close,
                );

                if let Some(text) = on_send {
                    self.handle_send_message(text, false);
                }
                if on_toggle_mic {
                    self.backend.toggle_mic();
                }
                if on_capture_screen {
                    self.show_toast("Capturing screen for AI companion...".to_string(), 3.0);
                    self.handle_send_message("What is on my screen right now?".to_string(), true);
                }
                if on_close {
                    self.is_chat_open = false;
                }
            }

            // C. Draw Floating Pill Toolbar
            let (actions, _toolbar_rect) = self.toolbar.draw(
                ui,
                self.is_mic_muted,
                self.is_voice_muted,
                self.is_chat_open,
                self.companion.is_roaming,
                self.is_always_on_top,
            );

            if actions.toggle_mic {
                self.backend.toggle_mic();
                self.show_toast(
                    if self.is_mic_muted { "Mic: LISTENING" } else { "Mic: MUTED" }.to_string(),
                    2.5,
                );
            }

            if actions.toggle_voice {
                self.is_voice_muted = !self.is_voice_muted;
                self.backend.set_voice_muted(self.is_voice_muted);
                self.show_toast(
                    if self.is_voice_muted { "AI Voice: MUTED" } else { "AI Voice: ACTIVE" }.to_string(),
                    2.5,
                );
            }

            if actions.toggle_chat {
                self.is_chat_open = !self.is_chat_open;
            }

            if actions.toggle_roam {
                self.companion.is_roaming = !self.companion.is_roaming;
                self.show_toast(
                    if self.companion.is_roaming { "Roaming: ON" } else { "Companion Parked" }.to_string(),
                    2.5,
                );
            }

            if actions.capture_screen {
                self.show_toast("Capturing screen for AI companion...".to_string(), 3.0);
                self.handle_send_message("What is on my screen right now?".to_string(), true);
            }

            if actions.toggle_always_on_top {
                self.is_always_on_top = !self.is_always_on_top;
                let level = if self.is_always_on_top {
                    egui::viewport::WindowLevel::AlwaysOnTop
                } else {
                    egui::viewport::WindowLevel::Normal
                };
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
                self.show_toast(
                    if self.is_always_on_top { "Always on Top: ON" } else { "Always on Top: OFF" }.to_string(),
                    2.5,
                );
            }

            if actions.exit_app {
                self.backend.shutdown();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }

            // D. Minimalist Toast Notification
            if let Some(toast) = &self.toast {
                if current_time < toast.expires_at {
                    let t = crate::theme::tokens();
                    let screen = ctx.screen_rect();
                    let toast_w = 260.0;
                    let toast_h = 36.0;
                    let toast_rect = egui::Rect::from_min_size(
                        egui::Pos2::new((screen.width() - toast_w) * 0.5, 24.0),
                        Vec2::new(toast_w, toast_h),
                    );

                    let painter = ui.painter();
                    painter.rect(
                        toast_rect,
                        t.rounding_control,
                        t.bg_surface_elevated,
                        t.hairline_border,
                    );

                    painter.text(
                        toast_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        &toast.message,
                        egui::FontId::proportional(12.0),
                        t.text_primary,
                    );
                } else {
                    self.toast = None;
                }
            }
        });

        // Update OS-level interactive hit regions so background is 100% click-through
        let mut interactive_rects = Vec::with_capacity(5);
        interactive_rects.push(self.companion.get_interactive_rect());
        if let Some(bubble_r) = self.companion.get_bubble_rect(screen_size.x) {
            interactive_rects.push(bubble_r);
        }
        interactive_rects.push(self.toolbar.get_rect());
        if self.is_chat_open {
            interactive_rects.push(self.chat_window.get_rect());
        }
        if self.toast.is_some() {
            let toast_w = 260.0;
            let toast_h = 36.0;
            let toast_rect = egui::Rect::from_min_size(
                egui::Pos2::new((screen_size.x - toast_w) * 0.5, 24.0),
                Vec2::new(toast_w, toast_h),
            );
            interactive_rects.push(toast_rect);
        }

        let scale_factor = ctx.pixels_per_point();
        self.hit_test_mgr.update_interactive_regions(
            "AI Desktop Companion",
            &interactive_rects,
            scale_factor,
        );

        // Continuous redraw for fluid swimming animation
        ctx.request_repaint();
    }
}

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use std::f32::consts::PI;
use crate::types::CompanionEmotion;

pub struct Companion {
    pub pos: Pos2,
    pub target_pos: Option<Pos2>,
    pub is_facing_left: bool,
    pub swim_tick: f32,
    pub is_roaming: bool,
    pub is_hovered: bool,
    pub is_dragging: bool,
    pub drag_offset: Vec2,
    pub emotion: CompanionEmotion,
    pub subtitle_text: String,
    pub subtitle_expires: f64,
    pub is_thinking: bool,
    pub is_speaking: bool,
    pub is_listening: bool,
    pub bounce_timer: f32,
    pub pause_until: f64,
}

impl Companion {
    pub fn new(screen_size: Vec2) -> Self {
        let initial_x = (screen_size.x - 240.0).max(100.0);
        let initial_y = (screen_size.y - 260.0).max(150.0);

        Self {
            pos: Pos2::new(initial_x, initial_y),
            target_pos: None,
            is_facing_left: false,
            swim_tick: 0.0,
            is_roaming: true,
            is_hovered: false,
            is_dragging: false,
            drag_offset: Vec2::ZERO,
            emotion: CompanionEmotion::Idle,
            subtitle_text: String::new(),
            subtitle_expires: 0.0,
            is_thinking: false,
            is_speaking: false,
            is_listening: false,
            bounce_timer: 0.0,
            pause_until: 0.0,
        }
    }

    pub fn set_subtitle(&mut self, text: String, current_time: f64, duration_secs: f64) {
        self.subtitle_text = text;
        self.subtitle_expires = current_time + duration_secs;
    }

    pub fn get_interactive_rect(&self) -> Rect {
        Rect::from_center_size(self.pos, Vec2::new(140.0, 120.0))
    }

    pub fn get_bubble_rect(&self, screen_w: f32) -> Option<Rect> {
        let has_content = self.is_thinking || (!self.subtitle_text.is_empty());
        if !has_content {
            return None;
        }

        let is_near_top = self.pos.y < 180.0;
        let bubble_w = 270.0;
        let bubble_h = 80.0;
        let bubble_x = (self.pos.x - bubble_w * 0.5).clamp(20.0, (screen_w - bubble_w - 20.0).max(20.0));
        let bubble_y = if is_near_top {
            self.pos.y + 65.0
        } else {
            self.pos.y - bubble_h - 65.0
        };

        Some(Rect::from_min_size(Pos2::new(bubble_x, bubble_y), Vec2::new(bubble_w, bubble_h)))
    }

    pub fn update(&mut self, screen_size: Vec2, dt: f32, current_time: f64) {
        self.swim_tick += dt * 2.8;

        if self.bounce_timer > 0.0 {
            self.bounce_timer = (self.bounce_timer - dt * 4.0).max(0.0);
        }

        // Roaming logic
        if self.is_roaming && !self.is_dragging && !self.is_hovered {
            if current_time >= self.pause_until {
                if let Some(target) = self.target_pos {
                    let dir = target - self.pos;
                    let dist = dir.length();

                    if dist < 18.0 {
                        self.target_pos = None;
                        // Pause for 2 to 4 seconds to nibble / drift
                        let pause_dur = 2.0 + (self.swim_tick.sin().abs() * 2.5) as f64;
                        self.pause_until = current_time + pause_dur;
                    } else {
                        let speed = 80.0 * dt;
                        let norm_dir = dir / dist;
                        self.pos += norm_dir * speed;
                        self.is_facing_left = norm_dir.x < 0.0;
                    }
                } else {
                    let margin = 120.0;
                    let max_w = (screen_size.x - margin).max(margin + 50.0);
                    let max_h = (screen_size.y - margin).max(margin + 50.0);

                    let rx = margin + ((self.swim_tick * 1.37).sin().abs() * (max_w - margin));
                    let ry = margin + ((self.swim_tick * 0.89).cos().abs() * (max_h - margin));
                    self.target_pos = Some(Pos2::new(rx, ry));
                }
            } else {
                // Natural water drift while paused
                let drift_y = (self.swim_tick * 1.4).sin() * 0.45;
                self.pos.y += drift_y;
            }
        }

        // Clamp inside screen
        self.pos.x = self.pos.x.clamp(70.0, (screen_size.x - 140.0).max(70.0));
        self.pos.y = self.pos.y.clamp(70.0, (screen_size.y - 140.0).max(70.0));
    }

    pub fn draw(&mut self, ui: &mut egui::Ui, current_time: f64) -> bool {
        let mut toggle_chat_requested = false;
        let shrimp_rect = self.get_interactive_rect();
        let response = ui.allocate_rect(shrimp_rect, egui::Sense::click_and_drag());

        self.is_hovered = response.hovered();

        if response.drag_started() {
            self.is_dragging = true;
            if let Some(mouse_pos) = ui.ctx().pointer_latest_pos() {
                self.drag_offset = self.pos - mouse_pos;
            }
        }

        if response.dragged() {
            if let Some(mouse_pos) = ui.ctx().pointer_latest_pos() {
                self.pos = mouse_pos + self.drag_offset;
            }
        }

        if response.drag_stopped() {
            self.is_dragging = false;
        }

        if response.clicked() {
            self.bounce_timer = 1.0;
            toggle_chat_requested = true;
        }

        let painter = ui.painter();
        let center = self.pos;

        let t = crate::theme::tokens();

        // 1. Restrained State Indicator Ring (1.0px hairline, zero neon glow)
        if self.is_listening {
            let glow_radius = 56.0 + (self.swim_tick * 4.0).sin() * 2.0;
            painter.circle_stroke(
                center,
                glow_radius,
                Stroke::new(1.0_f32, t.status_live),
            );
        } else if self.is_thinking {
            let glow_radius = 54.0 + (self.swim_tick * 3.5).cos() * 2.0;
            painter.circle_stroke(
                center,
                glow_radius,
                t.hairline_focus,
            );
        } else if self.is_speaking {
            let glow_radius = 54.0 + (self.swim_tick * 6.0).sin().abs() * 3.0;
            painter.circle_stroke(
                center,
                glow_radius,
                t.hairline_sheen,
            );
        }

        // 2. High-Fidelity Kawaii Shrimp Vector Art
        self.draw_kawaii_shrimp(painter, center);

        // 3. Attached Speech Bubble
        let show_bubble = self.is_thinking
            || (!self.subtitle_text.is_empty() && current_time < self.subtitle_expires);

        if show_bubble {
            if self.draw_speech_bubble(ui, center) {
                toggle_chat_requested = true;
            }
        }

        toggle_chat_requested
    }

    /// Draws the exact high-detail kawaii shrimp (Ebi) matching the original SVG artwork
    fn draw_kawaii_shrimp(&self, painter: &egui::Painter, center: Pos2) {
        let flip = if self.is_facing_left { -1.0 } else { 1.0 };
        let bounce = (self.bounce_timer * PI).sin() * 14.0;
        let swim_undulation = (self.swim_tick * 1.5).sin() * 3.5;
        let swim_angle = if self.is_roaming { (self.swim_tick * 1.2).sin() * 0.12 } else { 0.0 };

        let c = Pos2::new(center.x, center.y - bounce + swim_undulation);

        // Transform helper: maps SVG coordinates (0..130, 0..110) to screen space
        let origin = Pos2::new(52.0, 56.0);
        let pt = |sx: f32, sy: f32| -> Pos2 {
            let dx = (sx - origin.x) * flip;
            let dy = sy - origin.y;
            // Apply slight swim rotation
            let cos_a = swim_angle.cos();
            let sin_a = swim_angle.sin() * flip;
            let rx = dx * cos_a - dy * sin_a;
            let ry = dx * sin_a + dy * cos_a;
            Pos2::new(c.x + rx, c.y + ry)
        };

        // Palette matching SVG gradients:
        let rose = Color32::from_rgb(251, 113, 133);        // #fb7185
        let crimson = Color32::from_rgb(244, 63, 94);       // #f43f5e
        let coral = Color32::from_rgb(251, 146, 60);        // #fb923c
        let dark_orange = Color32::from_rgb(234, 88, 12);   // #ea580c
        let ruby = Color32::from_rgb(225, 29, 72);          // #e11d48
        let soft_rose = Color32::from_rgb(253, 164, 175);   // #fda4af
        let belly_cream = Color32::from_rgb(254, 205, 211); // #fecdd3
        let white_stroke = Stroke::new(0.9_f32, Color32::from_rgba_premultiplied(255, 255, 255, 220));

        // 1. ANTENNAE / FEELERS (Arching back gracefully over head)
        let antenna_wave1 = (self.swim_tick * 1.6).sin() * 4.0;
        let antenna_wave2 = (self.swim_tick * 1.9).cos() * 5.0;

        // Primary feeler
        let ant1_points = [
            pt(46.0, 36.0),
            pt(32.0, 16.0 + antenna_wave1 * 0.5),
            pt(10.0, 8.0 + antenna_wave1),
            pt(-12.0, 18.0 + antenna_wave1),
        ];
        for i in 0..ant1_points.len() - 1 {
            painter.line_segment([ant1_points[i], ant1_points[i + 1]], Stroke::new(1.3_f32, soft_rose));
        }

        // Secondary feeler
        let ant2_points = [
            pt(48.0, 38.0),
            pt(36.0, 22.0 + antenna_wave2 * 0.5),
            pt(16.0, 18.0 + antenna_wave2),
            pt(0.0, 28.0 + antenna_wave2),
        ];
        for i in 0..ant2_points.len() - 1 {
            painter.line_segment([ant2_points[i], ant2_points[i + 1]], Stroke::new(1.0_f32, soft_rose));
        }

        // 2. SWIMMERETS (Fluttering beneath belly)
        let flutter = (self.swim_tick * 3.5).sin() * 3.5;
        painter.circle_filled(pt(60.0, 74.0 + flutter), 5.5, Color32::from_rgba_premultiplied(253, 164, 175, 210));
        painter.circle_filled(pt(72.0, 73.0 - flutter * 0.8), 5.0, Color32::from_rgba_premultiplied(253, 164, 175, 210));
        painter.circle_filled(pt(83.0, 70.0 + flutter * 0.6), 4.5, Color32::from_rgba_premultiplied(253, 164, 175, 210));

        // 3. TAIL FAN (Uropods & Telson: 3 curved fins)
        let tail_fan_points_upper = vec![pt(98.0, 56.0), pt(115.0, 42.0), pt(124.0, 45.0), pt(116.0, 58.0), pt(100.0, 60.0)];
        painter.add(egui::Shape::convex_polygon(tail_fan_points_upper, ruby, white_stroke));

        let tail_fan_points_mid = vec![pt(100.0, 60.0), pt(120.0, 54.0), pt(128.0, 62.0), pt(118.0, 69.0), pt(100.0, 65.0)];
        painter.add(egui::Shape::convex_polygon(tail_fan_points_mid, crimson, white_stroke));

        let tail_fan_points_lower = vec![pt(98.0, 65.0), pt(115.0, 72.0), pt(122.0, 79.0), pt(112.0, 76.0), pt(97.0, 68.0)];
        painter.add(egui::Shape::convex_polygon(tail_fan_points_lower, ruby, white_stroke));

        // 4. CURVED SEGMENTED SHRIMP BODY (Ebi Abdomen)
        // Segment 4 (Rear plate)
        let seg4 = vec![pt(85.0, 52.0), pt(95.0, 54.0), pt(102.0, 62.0), pt(94.0, 70.0), pt(85.0, 68.0), pt(80.0, 58.0)];
        painter.add(egui::Shape::convex_polygon(seg4, dark_orange, white_stroke));

        // Segment 3 (Mid-Rear plate)
        let seg3 = vec![pt(72.0, 45.0), pt(86.0, 48.0), pt(90.0, 62.0), pt(80.0, 72.0), pt(72.0, 71.0), pt(68.0, 56.0)];
        painter.add(egui::Shape::convex_polygon(seg3, coral, white_stroke));

        // Segment 2 (Mid plate)
        let seg2 = vec![pt(58.0, 40.0), pt(74.0, 42.0), pt(77.0, 62.0), pt(68.0, 73.0), pt(57.0, 71.0), pt(54.0, 52.0)];
        painter.add(egui::Shape::convex_polygon(seg2, crimson, white_stroke));

        // Segment 1 (Carapace / Chubby Head)
        let head = vec![
            pt(28.0, 42.0),
            pt(44.0, 32.0),
            pt(62.0, 38.0),
            pt(64.0, 64.0),
            pt(54.0, 72.0),
            pt(32.0, 74.0),
            pt(24.0, 60.0),
            pt(20.0, 48.0),
        ];
        painter.add(egui::Shape::convex_polygon(head, rose, Stroke::new(1.1_f32, Color32::WHITE)));

        // Soft Cream Belly Underside
        let belly = vec![pt(28.0, 64.0), pt(45.0, 74.0), pt(74.0, 72.0), pt(62.0, 67.0), pt(45.0, 66.0), pt(32.0, 64.0)];
        painter.add(egui::Shape::convex_polygon(belly, belly_cream, Stroke::NONE));

        // White Pearlescent Highlights and Shell Stripes
        let stripe_stroke = Stroke::new(1.8_f32, Color32::from_rgba_premultiplied(255, 255, 255, 120));
        painter.line_segment([pt(44.0, 38.0), pt(46.0, 66.0)], stripe_stroke);
        painter.line_segment([pt(58.0, 41.0), pt(60.0, 65.0)], stripe_stroke);
        painter.line_segment([pt(73.0, 47.0), pt(74.0, 65.0)], stripe_stroke);
        painter.line_segment([pt(87.0, 53.0), pt(87.0, 64.0)], stripe_stroke);

        // Pearlescent curve on head
        painter.line_segment([pt(28.0, 44.0), pt(45.0, 36.0)], Stroke::new(2.2_f32, Color32::from_rgba_premultiplied(255, 255, 255, 180)));
        painter.line_segment([pt(45.0, 36.0), pt(60.0, 41.0)], Stroke::new(2.2_f32, Color32::from_rgba_premultiplied(255, 255, 255, 180)));

        // Front paws/claws
        painter.circle_filled(pt(26.0, 66.0), 4.5, soft_rose);
        painter.circle_filled(pt(34.0, 69.0), 4.0, soft_rose);

        // Rosy Cheek Blush
        painter.circle_filled(pt(27.0, 59.0), 5.5, Color32::from_rgba_premultiplied(244, 63, 94, 150));

        // 5. DYNAMIC EMOTION EXPRESSIONS
        let dark_navy = Color32::from_rgb(30, 27, 75);      // #1e1b4b
        let eye_stroke = Stroke::new(2.4_f32, dark_navy);
        let mouth_crimson = Color32::from_rgb(190, 18, 60); // #be123c

        match self.emotion {
            CompanionEmotion::Happy => {
                // Happy curved eyes (^ ‿ ^)
                painter.line_segment([pt(26.0, 49.0), pt(31.0, 44.0)], eye_stroke);
                painter.line_segment([pt(31.0, 44.0), pt(36.0, 49.0)], eye_stroke);

                painter.line_segment([pt(39.0, 48.0), pt(44.0, 43.0)], eye_stroke);
                painter.line_segment([pt(44.0, 43.0), pt(49.0, 48.0)], eye_stroke);

                // Smile mouth
                painter.line_segment([pt(32.0, 58.0), pt(36.0, 62.0)], Stroke::new(2.0_f32, mouth_crimson));
                painter.line_segment([pt(36.0, 62.0), pt(41.0, 58.0)], Stroke::new(2.0_f32, mouth_crimson));

                // Yellow water star sparkle ✨
                let star_c = pt(18.0, 36.0);
                painter.circle_filled(star_c, 3.5, Color32::from_rgb(253, 224, 71));
                painter.line_segment([Pos2::new(star_c.x - 6.0, star_c.y), Pos2::new(star_c.x + 6.0, star_c.y)], Stroke::new(1.8_f32, Color32::from_rgb(253, 224, 71)));
                painter.line_segment([Pos2::new(star_c.x, star_c.y - 6.0), Pos2::new(star_c.x, star_c.y + 6.0)], Stroke::new(1.8_f32, Color32::from_rgb(253, 224, 71)));
            }
            CompanionEmotion::Thinking => {
                // Thinking eyes (? ‿ •)
                painter.circle_filled(pt(32.0, 48.0), 4.2, dark_navy);
                painter.circle_filled(pt(33.5, 46.5), 1.5, Color32::WHITE);

                painter.line_segment([pt(40.0, 49.0), pt(44.0, 46.0)], eye_stroke);
                painter.line_segment([pt(44.0, 46.0), pt(48.0, 49.0)], eye_stroke);

                // Puzzled mouth
                painter.line_segment([pt(33.0, 59.0), pt(40.0, 59.0)], Stroke::new(2.0_f32, mouth_crimson));

                // Thought bubbles rising 💭
                let b1 = pt(16.0, 36.0 - (self.swim_tick * 2.0).sin() * 2.0);
                let b2 = pt(10.0, 26.0 - (self.swim_tick * 2.0).cos() * 2.0);
                painter.circle_filled(b1, 3.0, Color32::from_rgba_premultiplied(56, 189, 248, 200));
                painter.circle_filled(b2, 4.5, Color32::from_rgba_premultiplied(56, 189, 248, 230));
            }
            CompanionEmotion::Attentive => {
                // Attentive emerald listening eyes (◉ ‿ ◉)
                painter.circle_filled(pt(31.0, 49.0), 5.0, Color32::from_rgb(16, 185, 129));
                painter.circle_filled(pt(31.0, 49.0), 2.5, dark_navy);
                painter.circle_filled(pt(32.5, 47.5), 1.3, Color32::WHITE);

                painter.circle_filled(pt(44.0, 48.0), 5.0, Color32::from_rgb(16, 185, 129));
                painter.circle_filled(pt(44.0, 48.0), 2.5, dark_navy);
                painter.circle_filled(pt(45.5, 46.5), 1.3, Color32::WHITE);

                // Attentive smile
                painter.line_segment([pt(34.0, 58.0), pt(38.0, 61.0)], Stroke::new(2.2_f32, Color32::from_rgb(6, 95, 70)));
                painter.line_segment([pt(38.0, 61.0), pt(42.0, 58.0)], Stroke::new(2.2_f32, Color32::from_rgb(6, 95, 70)));
            }
            CompanionEmotion::Vibing => {
                // Vibing wavy eyes (~ ‿ ~)
                painter.line_segment([pt(27.0, 50.0), pt(32.0, 53.0)], eye_stroke);
                painter.line_segment([pt(32.0, 53.0), pt(37.0, 50.0)], eye_stroke);

                painter.line_segment([pt(40.0, 49.0), pt(45.0, 52.0)], eye_stroke);
                painter.line_segment([pt(45.0, 52.0), pt(50.0, 49.0)], eye_stroke);

                painter.circle_filled(pt(38.0, 58.0), 2.0, mouth_crimson);

                // Musical note 🎵
                let note_y = (self.swim_tick * 2.0).sin() * 3.0;
                let note_p = pt(16.0, 32.0 + note_y);
                painter.circle_filled(note_p, 2.5, Color32::from_rgb(236, 72, 153));
                painter.line_segment([note_p, Pos2::new(note_p.x, note_p.y - 8.0)], Stroke::new(1.5_f32, Color32::from_rgb(236, 72, 153)));
            }
            CompanionEmotion::Surprise => {
                // Wide surprise eyes (⊙ 0 ⊙)
                painter.circle_filled(pt(30.0, 48.0), 5.5, dark_navy);
                painter.circle_filled(pt(30.0, 48.0), 2.0, Color32::WHITE);

                painter.circle_filled(pt(44.0, 47.0), 5.5, dark_navy);
                painter.circle_filled(pt(44.0, 47.0), 2.0, Color32::WHITE);

                painter.circle_stroke(pt(37.0, 58.0), 3.2, Stroke::new(2.0_f32, mouth_crimson));
            }
            CompanionEmotion::Guilty => {
                // Shy/Guilty eyes (> <)
                painter.line_segment([pt(27.0, 46.0), pt(35.0, 50.0)], eye_stroke);
                painter.line_segment([pt(35.0, 50.0), pt(27.0, 54.0)], eye_stroke);

                painter.line_segment([pt(48.0, 45.0), pt(40.0, 49.0)], eye_stroke);
                painter.line_segment([pt(40.0, 49.0), pt(48.0, 53.0)], eye_stroke);

                painter.line_segment([pt(34.0, 59.0), pt(37.0, 57.0)], Stroke::new(2.0_f32, mouth_crimson));
                painter.line_segment([pt(37.0, 57.0), pt(41.0, 59.0)], Stroke::new(2.0_f32, mouth_crimson));
            }
            CompanionEmotion::Sad => {
                // Sad eyes (╥ ﹏ ╥) with blue teardrop
                painter.line_segment([pt(27.0, 52.0), pt(31.0, 47.0)], eye_stroke);
                painter.line_segment([pt(31.0, 47.0), pt(35.0, 52.0)], eye_stroke);

                painter.line_segment([pt(40.0, 51.0), pt(44.0, 46.0)], eye_stroke);
                painter.line_segment([pt(44.0, 46.0), pt(48.0, 51.0)], eye_stroke);

                // Teardrop
                painter.circle_filled(pt(36.0, 57.0), 2.5, Color32::from_rgb(56, 189, 248));
                painter.line_segment([pt(33.0, 60.0), pt(37.0, 57.0)], Stroke::new(2.0_f32, mouth_crimson));
                painter.line_segment([pt(37.0, 57.0), pt(41.0, 60.0)], Stroke::new(2.0_f32, mouth_crimson));
            }
            _ => {
                // Normal Idle/Speaking kawaii anime eyes (• ‿ •)
                painter.circle_filled(pt(31.0, 48.0), 4.5, dark_navy);
                painter.circle_filled(pt(32.5, 46.5), 1.6, Color32::WHITE);

                painter.circle_filled(pt(44.0, 47.0), 4.5, dark_navy);
                painter.circle_filled(pt(45.5, 45.5), 1.6, Color32::WHITE);

                if self.is_speaking {
                    // Mouth animated with sound wave
                    let mouth_open = (self.swim_tick * 8.0).sin().abs() * 3.5;
                    painter.circle_filled(pt(37.0, 58.0), mouth_open + 2.0, mouth_crimson);
                } else {
                    painter.line_segment([pt(33.0, 57.0), pt(37.0, 61.0)], Stroke::new(2.2_f32, mouth_crimson));
                    painter.line_segment([pt(37.0, 61.0), pt(41.0, 57.0)], Stroke::new(2.2_f32, mouth_crimson));
                }
            }
        }
    }

    fn draw_speech_bubble(&self, ui: &mut egui::Ui, center: Pos2) -> bool {
        let t = crate::theme::tokens();
        let is_near_top = center.y < 180.0;
        let bubble_w = 270.0;
        let bubble_h = 80.0;

        let bubble_x = (center.x - bubble_w * 0.5).clamp(20.0, (ui.ctx().screen_rect().width() - bubble_w - 20.0).max(20.0));
        let bubble_y = if is_near_top {
            center.y + 65.0
        } else {
            center.y - bubble_h - 65.0
        };

        let bubble_rect = Rect::from_min_size(Pos2::new(bubble_x, bubble_y), Vec2::new(bubble_w, bubble_h));
        let response = ui.allocate_rect(bubble_rect, egui::Sense::click());
        let painter = ui.painter();

        painter.rect(
            bubble_rect,
            t.rounding_card,
            t.bg_surface,
            t.hairline_border,
        );

        // Speech bubble pointer arrow
        let pointer_tip = Pos2::new(center.x, if is_near_top { bubble_y } else { bubble_y + bubble_h });
        let pt_left = Pos2::new(center.x - 8.0, if is_near_top { bubble_y + 6.0 } else { bubble_y + bubble_h - 6.0 });
        let pt_right = Pos2::new(center.x + 8.0, if is_near_top { bubble_y + 6.0 } else { bubble_y + bubble_h - 6.0 });

        painter.add(egui::Shape::convex_polygon(
            vec![pointer_tip, pt_left, pt_right],
            t.bg_surface,
            t.hairline_border,
        ));

        // Header info
        let header_text = if self.is_thinking {
            "⋯ Thinking...".to_string()
        } else {
            format!("✦ {}", self.emotion.display_name())
        };

        painter.text(
            Pos2::new(bubble_rect.min.x + 12.0, bubble_rect.min.y + 10.0),
            egui::Align2::LEFT_TOP,
            header_text,
            egui::FontId::proportional(11.0),
            t.text_primary,
        );

        painter.text(
            Pos2::new(bubble_rect.max.x - 12.0, bubble_rect.min.y + 10.0),
            egui::Align2::RIGHT_TOP,
            "Tap to chat",
            egui::FontId::proportional(10.0),
            t.text_muted,
        );

        // Subtitle text
        let display_text = if self.is_thinking {
            "Bubbling thoughts...".to_string()
        } else {
            self.subtitle_text.clone()
        };

        let short_text = if display_text.len() > 72 {
            format!("{}...", &display_text[..69])
        } else {
            display_text
        };

        painter.text(
            Pos2::new(bubble_rect.min.x + 12.0, bubble_rect.min.y + 32.0),
            egui::Align2::LEFT_TOP,
            short_text,
            egui::FontId::proportional(13.0),
            t.text_primary,
        );

        response.clicked()
    }
}

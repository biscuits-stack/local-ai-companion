//! Centralized Design Theme for AI Desktop Companion
//!
//! Aesthetic Direction: "Smoked Mica"
//! A quiet, layered Windows 11 translucent utility aesthetic tailored for a compact,
//! frameless desktop companion. It pairs deep obsidian/mica neutral surface tones with
//! 1.0px specular hairline borders at low opacity, restrained corner roundings, and
//! a single muted warm-ivory accent.
//!
//! Guaranteed zero neon/cyan glows, zero >1.0px colored borders, zero window shadows,
//! and 0.0 expansion across all widget states so hover effects never expand or jitter
//! outside the Win32 `SetWindowRgn` GDI hit-test boundaries.

use std::sync::{Arc, OnceLock};
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Vec2};

static THEME_TOKENS: OnceLock<ThemeTokens> = OnceLock::new();

/// Centralized design tokens for surface colors, strokes, typography, and metrics.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ThemeTokens {
    // -------------------------------------------------------------------------
    // Layered Neutral Surface Tones
    // -------------------------------------------------------------------------
    /// Fully transparent window background for click-through frameless desktop overlay.
    pub bg_transparent: Color32,
    /// Primary card background (Chat card, speech bubble): deep smoked obsidian mica.
    pub bg_surface: Color32,
    /// Elevated element fill (Pill toolbar, input containers, frame panels).
    pub bg_surface_elevated: Color32,
    /// Subtle button / badge fill.
    pub bg_surface_subtle: Color32,
    /// Hovered interactive element fill.
    pub bg_surface_hover: Color32,
    /// Active / pressed element fill.
    pub bg_surface_active: Color32,
    /// High-contrast monochrome light glass fill for user chat bubbles.
    pub bg_user_bubble: Color32,
    /// Muted slate fill for active toggle controls (Chat pill, Send button).
    pub bg_active_control: Color32,
    /// Crisp off-white text for active toggle controls.
    pub text_active_control: Color32,
    /// Subtle 1.0px hairline border for active toggle controls.
    pub border_active_control: Stroke,

    // -------------------------------------------------------------------------
    // Hairline Specular Borders (1.0px, low opacity - no colored glows)
    // -------------------------------------------------------------------------
    /// Faintest divider / subtle separator hairline.
    pub hairline_subtle: Stroke,
    /// Standard card / pill container specular glass rim.
    pub hairline_border: Stroke,
    /// Top specular sheen highlight segment.
    pub hairline_sheen: Stroke,
    /// Interactive focus / active hairline stroke.
    pub hairline_focus: Stroke,

    // -------------------------------------------------------------------------
    // 1 Muted Accent Color
    // -------------------------------------------------------------------------
    /// Soft warm ivory / muted sand accent for key states and tags.
    pub accent: Color32,
    /// Subtle tinted fill of the accent color.
    pub accent_subtle: Color32,

    // -------------------------------------------------------------------------
    // Neutral Typography
    // -------------------------------------------------------------------------
    /// Crisp primary text for dark surfaces.
    pub text_primary: Color32,
    /// Secondary / muted label text.
    pub text_secondary: Color32,
    /// De-emphasized placeholder / timestamp text.
    pub text_muted: Color32,
    /// High-contrast dark charcoal text for light user bubbles.
    pub text_on_light: Color32,

    // -------------------------------------------------------------------------
    // Status Indicators (Muted, non-neon)
    // -------------------------------------------------------------------------
    /// Subdued sage green for active/listening microphone.
    pub status_live: Color32,
    /// Subdued terracotta red for muted state.
    pub status_muted: Color32,

    // -------------------------------------------------------------------------
    // Restrained Roundings
    // -------------------------------------------------------------------------
    /// 10.0px for primary cards, speech bubbles, and window panels.
    pub rounding_card: egui::Rounding,
    /// 6.0px for inputs, buttons, and control pills.
    pub rounding_control: egui::Rounding,
    /// 4.0px for small badges and tags.
    pub rounding_badge: egui::Rounding,

    // -------------------------------------------------------------------------
    // Compact Desktop Metrics
    // -------------------------------------------------------------------------
    pub item_spacing: Vec2,
    pub window_margin: egui::Margin,
    pub button_padding: Vec2,
}

impl Default for ThemeTokens {
    fn default() -> Self {
        Self::smoked_mica()
    }
}

impl ThemeTokens {
    /// Factory for the "Smoked Mica" aesthetic.
    pub fn smoked_mica() -> Self {
        Self {
            bg_transparent: Color32::from_rgba_premultiplied(0, 0, 0, 0),
            bg_surface: Color32::from_rgb(24, 24, 27),          // #18181B
            bg_surface_elevated: Color32::from_rgb(24, 24, 27), // #18181B
            bg_surface_subtle: Color32::from_rgb(35, 35, 41),   // #232329
            bg_surface_hover: Color32::from_rgb(44, 44, 52),    // #2C2C34
            bg_surface_active: Color32::from_rgba_premultiplied(58, 58, 70, 220),
            bg_user_bubble: Color32::from_rgb(52, 56, 64),
            bg_active_control: Color32::from_rgb(56, 60, 68),
            text_active_control: Color32::from_rgb(235, 237, 240),
            border_active_control: Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 30)),

            hairline_subtle: Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 16)),
            hairline_border: Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 18)),
            hairline_sheen: Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 25)),
            hairline_focus: Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 35)),

            accent: Color32::from_rgb(215, 205, 190),
            accent_subtle: Color32::from_rgba_premultiplied(215, 205, 190, 35),

            text_primary: Color32::from_rgb(240, 240, 245),
            text_secondary: Color32::from_rgb(160, 160, 172),
            text_muted: Color32::from_rgb(115, 115, 125),
            text_on_light: Color32::from_rgb(18, 18, 21), // #121215

            status_live: Color32::from_rgb(82, 188, 122),
            status_muted: Color32::from_rgb(210, 85, 85),

            rounding_card: egui::Rounding::same(10.0),
            rounding_control: egui::Rounding::same(6.0),
            rounding_badge: egui::Rounding::same(4.0),

            item_spacing: Vec2::new(6.0, 6.0),
            window_margin: egui::Margin::symmetric(10.0, 8.0),
            button_padding: Vec2::new(8.0, 4.0),
        }
    }
}

/// Access the active design tokens.
pub fn tokens() -> &'static ThemeTokens {
    THEME_TOKENS.get_or_init(ThemeTokens::smoked_mica)
}

/// Applies native Windows font definitions with safe fallback and explicit `TextStyle` scale.
pub fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    // 1. Try loading Segoe UI from native Windows font directory
    let segoe_loaded = if let Ok(data) = std::fs::read("C:\\Windows\\Fonts\\segoeui.ttf") {
        fonts.font_data.insert(
            "segoe_ui".to_owned(),
            Arc::new(FontData::from_owned(data)),
        );
        true
    } else {
        false
    };

    // 2. Try loading native monospace font (Cascadia Mono, then Consolas)
    let mono_path = if std::path::Path::new("C:\\Windows\\Fonts\\CascadiaMono.ttf").exists() {
        Some("C:\\Windows\\Fonts\\CascadiaMono.ttf")
    } else if std::path::Path::new("C:\\Windows\\Fonts\\consola.ttf").exists() {
        Some("C:\\Windows\\Fonts\\consola.ttf")
    } else {
        None
    };

    let mono_loaded = if let Some(path) = mono_path {
        if let Ok(data) = std::fs::read(path) {
            fonts.font_data.insert(
                "windows_mono".to_owned(),
                Arc::new(FontData::from_owned(data)),
            );
            true
        } else {
            false
        }
    } else {
        false
    };

    // 3. Prepend native fonts to families with fallback to embedded egui glyphs
    if segoe_loaded {
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "segoe_ui".to_owned());
    }

    if mono_loaded {
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "windows_mono".to_owned());
    }

    ctx.set_fonts(fonts);
}

/// Configures `egui::Style`, `egui::Visuals`, and typography sizes once on startup.
pub fn apply_theme(ctx: &egui::Context) {
    let t = tokens();

    // 1. Native fonts & TextStyles
    configure_fonts(ctx);

    let mut style = (*ctx.style()).clone();

    // Explicit TextStyle hierarchy
    style.text_styles = [
        (TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(13.0, FontFamily::Proportional)),
        (TextStyle::Heading, FontId::new(16.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace)),
    ]
    .into();

    // 2. Compact desktop spacing
    style.spacing.item_spacing = t.item_spacing;
    style.spacing.button_padding = t.button_padding;
    style.spacing.window_margin = t.window_margin;
    style.spacing.interact_size = Vec2::new(20.0, 20.0);

    // 3. Visuals configuration
    let mut visuals = egui::Visuals::dark();

    // Frameless transparent base
    visuals.panel_fill = t.bg_transparent;
    visuals.window_fill = t.bg_surface;
    visuals.extreme_bg_color = t.bg_surface_elevated;
    visuals.faint_bg_color = t.bg_surface_subtle;

    // Zero window / popup shadows so visuals never bleed outside hit-test bounds
    visuals.window_shadow = egui::Shadow::NONE;
    visuals.popup_shadow = egui::Shadow::NONE;

    // Roundings
    visuals.window_rounding = t.rounding_card;
    visuals.menu_rounding = t.rounding_card;

    // Window stroke: subtle 7% hairline border instead of bright white
    visuals.window_stroke = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 18));

    // Widget visuals:
    // CRITICAL: expansion = 0.0 across all states prevents bounds jitter or hit-test region mismatch.
    visuals.widgets.noninteractive.expansion = 0.0;
    visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(24, 24, 27); // #18181B
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 16));
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, t.text_secondary);
    visuals.widgets.noninteractive.rounding = t.rounding_control;

    visuals.widgets.inactive.expansion = 0.0;
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(35, 35, 41); // #232329
    visuals.widgets.inactive.bg_stroke = Stroke::NONE; // no white outlines around idle buttons
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, t.text_primary);
    visuals.widgets.inactive.rounding = t.rounding_control;

    visuals.widgets.hovered.expansion = 0.0;
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(44, 44, 52); // #2C2C34
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 28));
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, t.text_primary);
    visuals.widgets.hovered.rounding = t.rounding_control;

    visuals.widgets.active.expansion = 0.0;
    visuals.widgets.active.bg_fill = t.bg_surface_active;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, t.accent);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, t.text_primary);
    visuals.widgets.active.rounding = t.rounding_control;

    visuals.widgets.open.expansion = 0.0;
    visuals.widgets.open.bg_fill = Color32::from_rgb(44, 44, 52);
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 28));
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, t.text_primary);
    visuals.widgets.open.rounding = t.rounding_control;

    // Subtle selection fill without colored glow
    visuals.selection.bg_fill = Color32::from_rgba_premultiplied(255, 255, 255, 45);
    visuals.selection.stroke = Stroke::new(1.0_f32, t.accent);

    style.visuals = visuals;

    ctx.set_style(style);
}

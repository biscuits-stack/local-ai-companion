---
name: egui-companion-design
description: Designs distinctive, native-feeling desktop companion UIs in Rust using egui/eframe (v0.30) with Win32 GDI hit-test regions, avoiding default programmer-gray egui looks and neon AI clichés.
---

This skill builds immediate-mode Rust (`egui` 0.30) UIs for a transparent, frameless desktop companion (`local_ai_companion.exe`) with a cohesive, native Windows aesthetic instead of default `egui` dark-mode gray or glowing neon AI clichés.

## When this skill activates
- Designing or refactoring any `egui::Ui`, `egui::Frame`, `egui::Window`, `egui::Area`, or `egui::Style` / `Visuals` code for the desktop companion (speech bubble, chat card, floating toolbar, status badges, or settings popover).
- Replacing hardcoded `Color32`, `Rounding`, `Stroke`, or `Margin` magic numbers with a centralized theme module.

## Stack & Architectural Constraints (Strictly Enforced)
- **Framework:** `egui` / `eframe` v0.30 (`egui_glow` backend). Pure Rust only—zero HTML, CSS, JS, or web metaphors.
- **Transparent Overlay Context:** The viewport runs with `with_transparent(true)`, `with_decorations(false)`, and `with_always_on_top(true)`. Never draw a full-screen background fill; keep `CentralPanel` transparent (`egui::Frame::none()`).
- **Win32 Hit-Test Alignment (`SetWindowRgn`):** Every interactive container (speech bubble, chat card, toolbar) must have crisp, deterministic rectangular or cleanly rounded bounding boxes (`Response::rect`) so `window_helper.rs` can register accurate GDI hit-test regions. Never rely on outer blurred shadows or painter-drawn elements extending outside container layout bounds.

## Workflow
1. **Commit to a Desktop Aesthetic Direction:** Before writing Rust code, state a clear 2-word direction (e.g., *"Tactile Paper & Ink"*, *"Warm Slate & Brass"*, or *"Frosted Mica Utility"*) tailored to a small floating desktop companion—never a full-screen SaaS dashboard.
2. **Centralize Tokens in `theme.rs`:** Never scatter raw `Color32::from_rgb(...)` or `Rounding::same(...)` inside widget rendering code. Define a single `ThemeTokens` struct and an `apply_theme(ctx: &egui::Context)` function configuring `egui::Style` and `egui::Visuals`:
   - **Palette:** 2–3 layered neutral surface tones (e.g., base card fill, elevated input fill, subtle hover fill), 1 hairline border color (`1.0` px `Stroke` at 8%–14% opacity), and 1 muted accent color.
   - **No Neon / No Glows:** Never use bright cyan/purple/magenta strokes, `2.0px+` colored borders, or multi-ring `painter.rect_stroke` glow effects.
   - **No Default `egui` Shadow:** Set `visuals.window_shadow = egui::Shadow::NONE` and `visuals.popup_shadow = egui::Shadow::NONE` (alpha shadows render poorly on transparent Win32 layered windows). Use crisp `1.0` px hairline `Stroke` separation instead.
3. **Load Native Windows Typography (`FontDefinitions`):** Replace `egui`'s default programmer fonts by loading native system fonts from `C:\Windows\Fonts\` at startup (with fallback to embedded bytes if missing):
   - **Proportional UI:** `segoeui.ttf` / `seguisb.ttf` (Segoe UI / SemiBold) for speech bubbles, chat messages, and labels.
   - **Monospace:** `consola.ttf` or `CascadiaMono.ttf` for status indicators, latencies, or logs.
   - Establish a tight desktop type hierarchy via `TextStyle` (`Small`: `11.0`, `Body`: `13.5`, `Button`: `13.0`, `Heading`: `16.0`).
4. **Calibrate Shapes & Spacing (`Style::spacing`):**
   - **Corner Radii:** Consistent, restrained `Rounding` (`8.0`–`12.0` for speech bubbles and chat cards; `6.0` for buttons and text inputs; `4.0` for small badges). Never use pill-shaped `30.0` rounding on rectangular containers.
   - **Compact Desktop Density:** Keep padding tight (`Margin::symmetric(12.0, 10.0)` for cards, `Vec2::new(8.0, 5.0)` for `button_padding`, `item_spacing = Vec2::new(6.0, 6.0)`). Set `WidgetVisuals::expansion = 0.0` across `inactive`, `hovered`, and `active` states so hovering buttons does not jitter layout alignment.
5. **Custom Widget Polish:**
   - Style `TextEdit` inside an explicit `egui::Frame` with subtle inset fill and a muted 1px focus stroke rather than `egui`'s harsh default blue focus ring.
   - For chat history, use `egui::ScrollArea::vertical().auto_shrink([false, true])` with compact message bubbles distinguishing user vs. companion turns by subtle surface contrast and alignment—not loud colored boxes.
6. **Self-Audit Before Output:** Check that `cargo check` compatibility is maintained (using `egui` 0.30 APIs: `Rounding`, `Stroke`, `Margin`, `Shadow`, `Color32`), zero hardcoded colors exist outside `theme.rs`, and every visible panel exposes its layout `Rect` cleanly for Win32 region clipping.

## Output Format
- Preceded by 3–4 lines stating the chosen aesthetic direction, palette values, and font configuration.
- Clean, modular Rust code (`theme.rs` + updated UI component functions) adhering strictly to `egui` 0.30 types without placeholder `todo!()` blocks.
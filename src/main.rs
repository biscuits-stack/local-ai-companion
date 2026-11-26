mod app;
mod backend;
mod chat;
mod companion;
mod config;
mod hardware;
mod screen;
mod theme;
mod toolbar;
mod types;
mod window_helper;

use app::DesktopCompanionApp;
use eframe::egui;

fn main() -> eframe::Result<()> {
    println!("========================================================");
    println!("      Desktop Companion (Rust Native Desktop Pet)       ");
    println!("      Mode: Transparent Overlay | Always-On-Top         ");
    println!("      Engine: Kokoro TTS + Faster-Whisper STT           ");
    println!("========================================================");

    let viewport = egui::ViewportBuilder::default()
        .with_title("AI Desktop Companion")
        .with_transparent(true)
        .with_decorations(false)
        .with_always_on_top()
        .with_maximized(true);

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "AI Desktop Companion",
        options,
        Box::new(|cc| {
            theme::apply_theme(&cc.egui_ctx);
            Ok(Box::new(DesktopCompanionApp::new(cc)))
        }),
    )
}

# AI Desktop Companion (Rust) 🦐

A native, high-performance **Desktop AI Companion** written entirely in **Rust** using **eframe/egui** with true background transparency, responsive UI controls, and direct integration with **Kokoro Neural Voice TTS** and **Faster-Whisper STT**.

---

## 🌟 Key Features

1. **100% Rust Native Desktop Companion**:
   - Replaces the multi-runtime stack (Python `pywebview` + .NET WinForms + Node.js + Vite) with a unified, lightweight, blazing-fast native Rust application.
   - Zero DPI scaling mismatch, zero event loop freezing.

2. **True Background Transparency**:
   - Transparent frameless window floating over desktop icons and other applications.
   - Clears background to `Color32::TRANSPARENT` via GPU hardware acceleration.
   - Always-on-top toggleable pin control.

3. **Rock-Solid, Responsive UI Buttons**:
   - In previous iterations, Win32 `WS_EX_TRANSPARENT` polling loops caused button clicks to miss or pass through into background windows.
   - In this Rust architecture, all UI hit-testing is handled directly in the native application loop. Every button click (Mic, TTS Voice Mute, Chat Toggle, Roaming Toggle, Screen Vision, Always-on-Top, Close) is immediately detected with 100% reliability.

4. **Preserved Speech-to-Text (STT) & Text-to-Speech (TTS)**:
   - **TTS**: **Kokoro Neural Voice TTS** (`kokoro-v1.0.onnx` + `voices-v1.0.bin`, voice: `af_heart`, speed: `1.06`).
   - **STT**: **Faster-Whisper STT** (local CTranslate2 Whisper model with continuous microphone listening and silence detection).
   - Real-time synchronization of speaking states, audio visualization aura, and emotion brackets (`[happy]`, `[attentive]`, `[vibing]`, `[surprise]`, `[guilty]`, `[sad]`).

5. **Lively Animated Shrimp Companion**:
   - Fluid autonomous swimming locomotion across the screen with gentle water drift.
   - 6 dynamic emotional expressions with eyes, blush, antennae, mouth movement, and audio waves.
   - Attached speech bubble following the pet with live subtitle and thought streaming.
   - Interactive: click to pop chat, or drag anywhere on your monitor.

6. **Floating Pill Toolbar & Frameless Chatbox**:
   - **Floating Toolbar**: Quick actions for Microphone, TTS Mute, Chat window toggle, Roam/Park mode, Screen Observation, Always-on-top, and App Exit.
   - **Floating Chatbox**: Draggable glassmorphism chat with message history, user & AI speech bubbles, thinking indicator, text input with Enter-to-send, and screen snapshot button.

7. **Native Screen Vision**:
   - Built-in Win32 GDI screen capture in pure Rust.
   - Encodes monitor frames into base64 JPEG and feeds them to vision-capable local LLMs (LM Studio on `http://127.0.0.1:5555`).

---

## 🏗️ Project Architecture

```text
Local_AI(Rust)/
├── Cargo.toml               # Rust dependencies (eframe, tungstenite, ureq, image, etc.)
├── run.bat                  # Instant 1-click launcher
└── src/
    ├── main.rs              # Application entrypoint & native viewport configuration
    ├── app.rs               # Main application coordinator, event loop, toast manager
    ├── companion.rs         # Procedural shrimp pet rendering, swimming physics & speech bubble
    ├── toolbar.rs           # Floating pill toolbar with responsive buttons
    ├── chat.rs              # Draggable chatbox with history & input
    ├── backend.rs           # Kokoro TTS + Whisper STT supervisor & WebSocket client
    ├── screen.rs            # Windows GDI screen capture to base64 JPEG
    └── types.rs             # Core data structures, emotion mapping & message types
```

---

## 🚀 How to Run

### Quick Start (Double Click):
Simply run `run.bat` inside `Local_AI(Rust)`.

### Via Cargo:
```powershell
cd "Local_AI(Rust)"
cargo run
```

### Keyboard Shortcuts:
- `Ctrl + Alt + M`: Toggle Microphone (Whisper STT Listening / Muted)
- `Enter` in Chat: Send message to companion
- `Escape`: Close Chat window

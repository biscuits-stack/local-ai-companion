use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tungstenite::{connect, Message};

use crate::types::CompanionEmotion;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum BackendCommand {
    SendMessage { text: String, force_screenshot: bool },
    ToggleMic,
    CaptureScreen { prompt: Option<String> },
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum BackendEvent {
    ServerStatus {
        connected: bool,
        mic_active: bool,
        bot_is_talking: bool,
    },
    AssistantMessage {
        text: String,
        emotion: CompanionEmotion,
    },
    AssistantState {
        phase: String, // "thinking", "speaking", "idle"
        emotion: CompanionEmotion,
    },
    Error(String),
}

pub struct BackendHandle {
    pub cmd_sender: Sender<BackendCommand>,
    pub event_receiver: Receiver<BackendEvent>,
    #[allow(dead_code)]
    pub is_connected: Arc<AtomicBool>,
    child_process: Arc<Mutex<Option<Child>>>,
    we_spawned_backend: Arc<AtomicBool>,
}

impl BackendHandle {
    pub fn new() -> Self {
        let (cmd_tx, cmd_rx) = channel::<BackendCommand>();
        let (event_tx, event_rx) = channel::<BackendEvent>();
        let is_connected = Arc::new(AtomicBool::new(false));
        let child_process = Arc::new(Mutex::new(None));
        let we_spawned_backend = Arc::new(AtomicBool::new(false));

        let is_conn_worker = is_connected.clone();
        let is_conn_ws = is_connected.clone();
        let event_tx_ws = event_tx.clone();
        let child_for_worker = child_process.clone();
        let spawned_for_worker = we_spawned_backend.clone();

        // 1. HTTP Command Worker thread
        thread::spawn(move || {
            run_backend_worker(cmd_rx, event_tx, is_conn_worker, child_for_worker, spawned_for_worker);
        });

        // 2. Real-time WebSocket Listener thread (Event-driven)
        thread::spawn(move || {
            run_websocket_listener(event_tx_ws, is_conn_ws);
        });

        Self {
            cmd_sender: cmd_tx,
            event_receiver: event_rx,
            is_connected,
            child_process,
            we_spawned_backend,
        }
    }

    /// Cleanly kills the spawned child process tree if we spawned it.
    /// Preserves existing external backend instances if we connected to an already running server.
    pub fn shutdown(&self) {
        if self.we_spawned_backend.load(Ordering::SeqCst) {
            if let Ok(mut lock) = self.child_process.lock() {
                if let Some(mut child) = lock.take() {
                    let pid = child.id();
                    println!("[Backend Supervisor] Shutting down spawned backend process tree (PID {pid})...");

                    #[cfg(windows)]
                    {
                        // Cleanly terminate the child process tree on Windows (/T = tree, /F = force)
                        let _ = Command::new("taskkill")
                            .args(["/F", "/T", "/PID", &pid.to_string()])
                            .output();
                    }

                    let _ = child.kill();
                    let _ = child.wait();
                    println!("[Backend Supervisor] Backend process tree terminated.");
                }
            }
        } else {
            println!("[Backend Supervisor] Preserving external backend on exit (not spawned by companion).");
        }
    }

    pub fn send_message(&self, text: String, force_screenshot: bool) {
        let _ = self.cmd_sender.send(BackendCommand::SendMessage {
            text,
            force_screenshot,
        });
    }

    pub fn toggle_mic(&self) {
        let _ = self.cmd_sender.send(BackendCommand::ToggleMic);
    }

    #[allow(dead_code)]
    pub fn capture_screen(&self, prompt: Option<String>) {
        let _ = self.cmd_sender.send(BackendCommand::CaptureScreen { prompt });
    }
}

impl Drop for BackendHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Returns the configured HTTP base URL for the backend server
pub fn get_server_base_url() -> String {
    std::env::var("LOCAL_AI_SERVER_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8000".to_string())
        .trim_end_matches('/')
        .to_string()
}

/// Returns the configured WebSocket URL for the backend stream
pub fn get_server_ws_url() -> String {
    std::env::var("LOCAL_AI_WS_URL")
        .unwrap_or_else(|_| {
            let base = get_server_base_url();
            let ws_base = if base.starts_with("https://") {
                base.replacen("https://", "wss://", 1)
            } else if base.starts_with("http://") {
                base.replacen("http://", "ws://", 1)
            } else {
                format!("ws://{base}")
            };
            format!("{ws_base}/ws")
        })
}

/// Finds the Backend root directory portably without hardcoded absolute machine paths
pub fn find_backend_dir() -> Option<PathBuf> {
    // 1. Explicit environment variable
    if let Ok(env_dir) = std::env::var("LOCAL_AI_BACKEND_DIR").or_else(|_| std::env::var("BACKEND_DIR")) {
        let p = PathBuf::from(env_dir);
        if p.join("convene_server.py").exists() {
            return Some(p);
        }
    }

    // 2. Relative to current executable
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let candidates = [
                exe_dir.join("Backend"),
                exe_dir.join("backend"),
                exe_dir.join("..").join("Backend"),
                exe_dir.join("..").join("backend"),
                exe_dir.join("..").join("Local-AI").join("Backend"),
                exe_dir.join("..").join("Local-AI").join("backend"),
                exe_dir.join("..").join("..").join("Local-AI").join("Backend"),
                exe_dir.join("..").join("..").join("Local-AI").join("backend"),
            ];

            for cand in &candidates {
                if cand.join("convene_server.py").exists() {
                    if let Ok(canonical) = cand.canonicalize() {
                        return Some(canonical);
                    }
                    return Some(cand.clone());
                }
            }

            // Upward directory traversal
            let mut cur = exe_dir.parent();
            while let Some(dir) = cur {
                for name in &["Backend", "backend"] {
                    let cand1 = dir.join("Local-AI").join(name);
                    if cand1.join("convene_server.py").exists() {
                        return Some(cand1);
                    }
                    let cand2 = dir.join(name);
                    if cand2.join("convene_server.py").exists() {
                        return Some(cand2);
                    }
                }
                cur = dir.parent();
            }
        }
    }

    // 3. Relative to current working directory
    if let Ok(cwd) = std::env::current_dir() {
        let cwd_candidates = [
            cwd.join("Backend"),
            cwd.join("backend"),
            cwd.join("..").join("Backend"),
            cwd.join("..").join("backend"),
            cwd.join("..").join("Local-AI").join("Backend"),
            cwd.join("..").join("Local-AI").join("backend"),
        ];

        for cand in &cwd_candidates {
            if cand.join("convene_server.py").exists() {
                if let Ok(canonical) = cand.canonicalize() {
                    return Some(canonical);
                }
                return Some(cand.clone());
            }
        }

        let mut cur: Option<&std::path::Path> = Some(&cwd);
        while let Some(dir) = cur {
            for name in &["Backend", "backend"] {
                let cand1 = dir.join("Local-AI").join(name);
                if cand1.join("convene_server.py").exists() {
                    return Some(cand1);
                }
                let cand2 = dir.join(name);
                if cand2.join("convene_server.py").exists() {
                    return Some(cand2);
                }
            }
            cur = dir.parent();
        }
    }

    None
}

/// Probes whether the backend server is active and responding
pub fn is_backend_port_alive() -> bool {
    let base_url = get_server_base_url();

    // 1. Fast TCP connect probe to target host:port
    let stripped = base_url
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let host_port = stripped.split('/').next().unwrap_or("127.0.0.1:8000");
    let addr_str = if host_port.contains(':') {
        host_port.to_string()
    } else {
        format!("{host_port}:8000")
    };

    if let Ok(socket_addrs) = std::net::ToSocketAddrs::to_socket_addrs(&addr_str) {
        for addr in socket_addrs {
            if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
                return true;
            }
        }
    }

    // 2. HTTP probe to /status
    let status_url = format!("{base_url}/status");
    if ureq::get(&status_url)
        .timeout(Duration::from_millis(300))
        .call()
        .is_ok()
    {
        return true;
    }

    false
}

fn try_start_local_backend_if_needed(
    child_holder: &Arc<Mutex<Option<Child>>>,
    we_spawned: &Arc<AtomicBool>,
) {
    // If backend is already running, reuse it without spawning a duplicate process!
    if is_backend_port_alive() {
        println!("[Backend Supervisor] Existing backend instance detected. Reusing active instance.");
        we_spawned.store(false, Ordering::SeqCst);
        return;
    }

    let Some(base) = find_backend_dir() else {
        println!("[Backend Supervisor] Backend directory not found; running in standalone client mode.");
        return;
    };

    let script = base.join("convene_server.py");
    if !script.exists() {
        return;
    }

    // Find Python executable: virtual environments first, then system python
    let python_candidates = [
        base.join("python_env").join("python.exe"),
        base.join("python_env").join("Scripts").join("python.exe"),
        base.join(".venv").join("Scripts").join("python.exe"),
        base.join("venv").join("Scripts").join("python.exe"),
        base.join("python_env").join("bin").join("python"),
        PathBuf::from("python.exe"),
        PathBuf::from("python"),
    ];

    let mut python_cmd: Option<PathBuf> = None;
    for cand in &python_candidates {
        if cand.is_absolute() {
            if cand.exists() {
                python_cmd = Some(cand.clone());
                break;
            }
        } else if Command::new(cand).arg("--version").output().is_ok() {
            python_cmd = Some(cand.clone());
            break;
        }
    }

    let Some(python_exe) = python_cmd else {
        println!("[Backend Supervisor] Python executable not found; running in standalone client mode.");
        return;
    };

    // Double check right before spawn
    if is_backend_port_alive() {
        println!("[Backend Supervisor] Backend port already active. Skipping duplicate spawn.");
        we_spawned.store(false, Ordering::SeqCst);
        return;
    }

    println!("[Backend Supervisor] Spawning Kokoro TTS + Faster-Whisper backend from: {:?}", base);

    let spawn_res = Command::new(&python_exe)
        .arg(&script)
        .current_dir(&base)
        .spawn();

    match spawn_res {
        Ok(child) => {
            let pid = child.id();
            println!("[Backend Supervisor] Backend process spawned (PID {pid}). Waiting for initialization...");
            we_spawned.store(true, Ordering::SeqCst);
            if let Ok(mut lock) = child_holder.lock() {
                *lock = Some(child);
            }

            let base_url = get_server_base_url();
            let status_url = format!("{base_url}/status");

            // Poll for up to 15 seconds until status endpoint is ready
            for _ in 0..30 {
                thread::sleep(Duration::from_millis(500));
                if ureq::get(&status_url)
                    .timeout(Duration::from_millis(500))
                    .call()
                    .is_ok()
                {
                    println!("[Backend Supervisor] Kokoro TTS & Whisper STT online and ready!");
                    return;
                }
            }
            println!("[Backend Supervisor] Backend process running; initialization in progress.");
        }
        Err(e) => {
            println!("[Backend Supervisor] Failed to spawn backend process: {e}");
        }
    }
}

fn run_websocket_listener(event_tx: Sender<BackendEvent>, is_connected: Arc<AtomicBool>) {
    loop {
        let ws_url = get_server_ws_url();
        match connect(&ws_url) {
            Ok((mut socket, _response)) => {
                println!("[WebSocket] Connected to Kokoro TTS + Whisper STT real-time stream!");
                is_connected.store(true, Ordering::Relaxed);

                if let tungstenite::stream::MaybeTlsStream::Plain(s) = socket.get_ref() {
                    let _ = s.set_read_timeout(Some(Duration::from_millis(300)));
                }

                loop {
                    match socket.read() {
                        Ok(Message::Text(txt)) => {
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&txt) {
                                if let Some(msg_type) = json.get("type").and_then(|t| t.as_str()) {
                                    match msg_type {
                                        "server.hello" => {
                                            if let Some(mic_active) = json.get("mic_active").and_then(|v| v.as_bool()) {
                                                let _ = event_tx.send(BackendEvent::ServerStatus {
                                                    connected: true,
                                                    mic_active,
                                                    bot_is_talking: false,
                                                });
                                            }
                                        }
                                        "assistant.state" => {
                                            let phase = json.get("phase").and_then(|p| p.as_str()).unwrap_or("idle");
                                            let emotion_tag = json.get("emotion").and_then(|e| e.as_str()).unwrap_or("Idle");
                                            let emotion = CompanionEmotion::from_tag(emotion_tag);
                                            let _ = event_tx.send(BackendEvent::AssistantState {
                                                phase: phase.to_string(),
                                                emotion,
                                            });
                                        }
                                        "assistant.message" => {
                                            let text = json.get("text").and_then(|t| t.as_str()).unwrap_or("");
                                            let emotion_tag = json.get("emotion").and_then(|e| e.as_str()).unwrap_or("Idle");
                                            let emotion = CompanionEmotion::from_tag(emotion_tag);
                                            if !text.is_empty() {
                                                let _ = event_tx.send(BackendEvent::AssistantMessage {
                                                    text: text.to_string(),
                                                    emotion,
                                                });
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        Ok(Message::Ping(p)) => {
                            let _ = socket.send(Message::Pong(p));
                        }
                        Ok(Message::Close(_)) => break,
                        Err(tungstenite::Error::Io(ref e))
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                || e.kind() == std::io::ErrorKind::TimedOut =>
                        {
                            thread::sleep(Duration::from_millis(20));
                        }
                        Err(_) => break,
                        _ => {}
                    }
                }

                // WebSocket dropped: mark disconnected and alert UI
                is_connected.store(false, Ordering::Relaxed);
                let _ = event_tx.send(BackendEvent::ServerStatus {
                    connected: false,
                    mic_active: false,
                    bot_is_talking: false,
                });
            }
            Err(_) => {
                is_connected.store(false, Ordering::Relaxed);
                // Wait 5 seconds before retrying WebSocket connection
                thread::sleep(Duration::from_secs(5));
            }
        }
    }
}

fn run_backend_worker(
    cmd_rx: Receiver<BackendCommand>,
    event_tx: Sender<BackendEvent>,
    is_connected: Arc<AtomicBool>,
    child_holder: Arc<Mutex<Option<Child>>>,
    we_spawned: Arc<AtomicBool>,
) {
    try_start_local_backend_if_needed(&child_holder, &we_spawned);

    // Fallback timer: only used when WebSocket is disconnected (every 60 seconds / 1 minute)
    let mut last_fallback_check = std::time::Instant::now();
    let mut prev_mic = false;

    loop {
        let base_url = get_server_base_url();

        // 1. Process pending commands from UI (wait up to 500ms so thread doesn't busy-wait)
        match cmd_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(cmd) => {
                match cmd {
                    BackendCommand::SendMessage { text, force_screenshot } => {
                        let _ = event_tx.send(BackendEvent::AssistantState {
                            phase: "thinking".to_string(),
                            emotion: CompanionEmotion::Thinking,
                        });

                        let payload = serde_json::json!({
                            "message": text,
                            "screenshot": force_screenshot
                        });

                        let chat_url = format!("{base_url}/chat");

                        // Retry up to 2 times if server is warming up
                        let mut sent = false;
                        for attempt in 0..2 {
                            match ureq::post(&chat_url)
                                .timeout(Duration::from_secs(35))
                                .send_json(&payload)
                            {
                                Ok(_) => {
                                    sent = true;
                                    break;
                                }
                                Err(e) => {
                                    if attempt == 0 {
                                        try_start_local_backend_if_needed(&child_holder, &we_spawned);
                                        thread::sleep(Duration::from_millis(1000));
                                    } else {
                                        let _ = event_tx.send(BackendEvent::Error(format!(
                                            "Could not reach Kokoro TTS & Whisper backend ({e})"
                                        )));
                                        let _ = event_tx.send(BackendEvent::AssistantState {
                                            phase: "idle".to_string(),
                                            emotion: CompanionEmotion::Sad,
                                        });
                                    }
                                }
                            }
                        }
                        if sent {
                            is_connected.store(true, Ordering::Relaxed);
                        }
                    }
                    BackendCommand::ToggleMic => {
                        let mic_url = format!("{base_url}/toggle_mic");
                        match ureq::post(&mic_url)
                            .timeout(Duration::from_secs(3))
                            .call()
                        {
                            Ok(res) => {
                                is_connected.store(true, Ordering::Relaxed);
                                if let Ok(json) = res.into_json::<serde_json::Value>() {
                                    if let Some(active) = json.get("mic_active").and_then(|v| v.as_bool()) {
                                        prev_mic = active;
                                        let _ = event_tx.send(BackendEvent::ServerStatus {
                                            connected: true,
                                            mic_active: active,
                                            bot_is_talking: false,
                                        });
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = event_tx.send(BackendEvent::Error(format!("Toggle mic failed: {e}")));
                            }
                        }
                    }
                    BackendCommand::CaptureScreen { prompt } => {
                        let _ = event_tx.send(BackendEvent::AssistantState {
                            phase: "thinking".to_string(),
                            emotion: CompanionEmotion::Thinking,
                        });

                        let payload = serde_json::json!({
                            "prompt": prompt.unwrap_or_else(|| "What is on my screen right now?".to_string())
                        });

                        let capture_url = format!("{base_url}/capture_screen");
                        match ureq::post(&capture_url)
                            .timeout(Duration::from_secs(35))
                            .send_json(&payload)
                        {
                            Ok(_) => {
                                is_connected.store(true, Ordering::Relaxed);
                            }
                            Err(e) => {
                                let _ = event_tx.send(BackendEvent::Error(format!(
                                    "Screen observation failed: {e}"
                                )));
                            }
                        }
                    }
                }
            }
            Err(_) => {
                // Timeout without command - move to condition check
            }
        }

        // 2. CONDITION-BASED HEALTH CHECK:
        // When WebSocket is connected, NEVER poll /status (WebSocket pushes all updates).
        // Only if disconnected, perform a slow fallback check once per minute (60 seconds).
        if !is_connected.load(Ordering::Relaxed) && last_fallback_check.elapsed() >= Duration::from_secs(60) {
            last_fallback_check = std::time::Instant::now();

            let status_url = format!("{base_url}/status");
            if let Ok(res) = ureq::get(&status_url)
                .timeout(Duration::from_millis(1000))
                .call()
            {
                is_connected.store(true, Ordering::Relaxed);
                if let Ok(json) = res.into_json::<serde_json::Value>() {
                    let mic_active = json
                        .get("mic_active")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(prev_mic);
                    let bot_talking = json
                        .get("bot_is_talking")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    let _ = event_tx.send(BackendEvent::ServerStatus {
                        connected: true,
                        mic_active,
                        bot_is_talking: bot_talking,
                    });
                }
            }
        }
    }
}

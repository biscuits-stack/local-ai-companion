use std::fs;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::{connect, Message};

use crate::config::RuntimeConfig;
use crate::hardware::{self, HardwareProfile, ReleaseAsset, SelectedAssets};
use crate::screen::capture_screen_base64;
use crate::types::{CompanionEmotion, LlmStatus};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const SYSTEM_PROMPT: &str = "You are Convene, an intelligent, charming, and lively desktop AI companion. \
You have three core capabilities: you can HEAR the user through their microphone, \
you can TALK back naturally with expressive emotion and neural voice, and you can SEE the user's screen \
whenever they ask or send a screenshot.\n\n\
CRITICAL RULES:\n\
1. Every sentence you speak MUST begin with an emotion bracket reflecting your tone, \
such as [happy], [attentive], [curious], [vibing], [surprise], [surprisepout], [guilty], or [sad].\n\
2. Keep your responses natural, concise, engaging, and conversational.\n\
3. When viewing a screenshot of the user's screen, observe it directly and give clear, accurate assistance based on what is visible.";

const SCREENSHOT_TRIGGERS: &[&str] = &[
    "look at my screen",
    "look at the screen",
    "see my screen",
    "on my screen",
    "check my screen",
    "what's on screen",
    "what is on screen",
    "screenshot",
    "capture screen",
];

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
    LlmStatusChanged(LlmStatus),
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
    #[allow(dead_code)]
    pub llm_status: Arc<Mutex<LlmStatus>>,
    voice_muted: Arc<AtomicBool>,
    python_child: Arc<Mutex<Option<Child>>>,
    we_spawned_python: Arc<AtomicBool>,
    llama_child: Arc<Mutex<Option<Child>>>,
    we_spawned_llama: Arc<AtomicBool>,
}

impl BackendHandle {
    pub fn new() -> Self {
        let cfg = RuntimeConfig::load();
        let (cmd_tx, cmd_rx) = channel::<BackendCommand>();
        let (event_tx, event_rx) = channel::<BackendEvent>();
        let is_connected = Arc::new(AtomicBool::new(false));
        let llm_status = Arc::new(Mutex::new(LlmStatus::DetectingHardware));
        let voice_muted = Arc::new(AtomicBool::new(false));
        let python_child = Arc::new(Mutex::new(None));
        let we_spawned_python = Arc::new(AtomicBool::new(false));
        let llama_child = Arc::new(Mutex::new(None));
        let we_spawned_llama = Arc::new(AtomicBool::new(false));

        // 1. PrismML llama-server.exe Supervisor & Health Polling thread (127.0.0.1:8080)
        {
            let cfg_llama = cfg.clone();
            let event_tx_llama = event_tx.clone();
            let llama_child_ref = llama_child.clone();
            let we_spawned_llama_ref = we_spawned_llama.clone();
            let llm_status_ref = llm_status.clone();
            thread::spawn(move || {
                run_llama_supervisor(
                    cfg_llama,
                    event_tx_llama,
                    llama_child_ref,
                    we_spawned_llama_ref,
                    llm_status_ref,
                );
            });
        }

        // 2. Chat / Vision / Audio Command Worker thread
        {
            let cfg_worker = cfg.clone();
            let is_conn_worker = is_connected.clone();
            let event_tx_worker = event_tx.clone();
            let py_child_ref = python_child.clone();
            let we_spawned_py_ref = we_spawned_python.clone();
            let voice_muted_ref = voice_muted.clone();
            thread::spawn(move || {
                run_backend_worker(
                    cfg_worker,
                    cmd_rx,
                    event_tx_worker,
                    is_conn_worker,
                    py_child_ref,
                    we_spawned_py_ref,
                    voice_muted_ref,
                );
            });
        }

        // 3. Real-time WebSocket Listener thread for Kokoro TTS + Faster-Whisper STT (127.0.0.1:8000)
        {
            let is_conn_ws = is_connected.clone();
            let event_tx_ws = event_tx.clone();
            thread::spawn(move || {
                run_websocket_listener(event_tx_ws, is_conn_ws);
            });
        }

        Self {
            cmd_sender: cmd_tx,
            event_receiver: event_rx,
            is_connected,
            llm_status,
            voice_muted,
            python_child,
            we_spawned_python,
            llama_child,
            we_spawned_llama,
        }
    }

    pub fn set_voice_muted(&self, muted: bool) {
        self.voice_muted.store(muted, Ordering::Relaxed);
    }

    /// Cleanly kills ONLY the child process trees spawned by this app.
    /// External processes already listening before launch are preserved.
    pub fn shutdown(&self) {
        terminate_owned_child(
            &self.llama_child,
            &self.we_spawned_llama,
            "PrismML llama-server",
        );
        terminate_owned_child(
            &self.python_child,
            &self.we_spawned_python,
            "Kokoro/Whisper backend",
        );
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

fn terminate_owned_child(
    child_holder: &Arc<Mutex<Option<Child>>>,
    we_spawned: &Arc<AtomicBool>,
    label: &str,
) {
    if we_spawned.swap(false, Ordering::SeqCst) {
        if let Ok(mut lock) = child_holder.lock() {
            if let Some(mut child) = lock.take() {
                let pid = child.id();
                println!("[Supervisor] Shutting down spawned {label} process tree (PID {pid})...");

                #[cfg(windows)]
                {
                    let mut tk = Command::new("taskkill");
                    tk.args(["/F", "/T", "/PID", &pid.to_string()]);
                    tk.creation_flags(CREATE_NO_WINDOW);
                    let _ = tk.output();
                }

                let _ = child.kill();
                let _ = child.wait();
                println!("[Supervisor] {label} process tree terminated.");
            }
        }
    } else {
        println!("[Supervisor] Preserving external {label} on exit (not spawned by companion).");
    }
}

/// Returns the configured HTTP base URL for the Kokoro/Whisper audio server
pub fn get_server_base_url() -> String {
    std::env::var("LOCAL_AI_SERVER_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8000".to_string())
        .trim_end_matches('/')
        .to_string()
}

/// Returns the configured WebSocket URL for the Kokoro/Whisper audio stream
pub fn get_server_ws_url() -> String {
    std::env::var("LOCAL_AI_WS_URL").unwrap_or_else(|_| {
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

/// Fast TCP probe to check whether an address (e.g. `127.0.0.1:8080`) is currently accepting connections.
fn is_tcp_listening(addr_str: &str) -> bool {
    if let Ok(socket_addrs) = addr_str.to_socket_addrs() {
        for addr in socket_addrs {
            if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
                return true;
            }
        }
    }
    false
}

fn update_llm_status(
    status_ref: &Arc<Mutex<LlmStatus>>,
    event_tx: &Sender<BackendEvent>,
    new_status: LlmStatus,
) {
    let changed = if let Ok(mut guard) = status_ref.lock() {
        if *guard != new_status {
            *guard = new_status.clone();
            true
        } else {
            false
        }
    } else {
        true
    };

    if changed {
        println!("[LLM Supervisor] Status -> {}", new_status.label());
        let _ = event_tx.send(BackendEvent::LlmStatusChanged(new_status));
    }
}

fn download_file_with_progress(
    url: &str,
    dest: &Path,
    label: &str,
    is_model: bool,
    llm_status: &Arc<Mutex<LlmStatus>>,
    event_tx: &Sender<BackendEvent>,
) -> Result<(), String> {
    if dest.exists() {
        if let Ok(meta) = fs::metadata(dest) {
            if meta.len() > 1024 {
                return Ok(());
            }
        }
    }

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir failed: {e}"))?;
    }

    let part_path = PathBuf::from(format!("{}.part", dest.display()));
    let resp = ureq::get(url)
        .set("User-Agent", "LocalAI-Companion")
        .timeout(Duration::from_secs(3600))
        .call()
        .map_err(|e| format!("GET {label} failed: {e}"))?;

    let total_bytes: Option<u64> = resp
        .header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok());

    let mut reader = resp.into_reader();
    let mut file = fs::File::create(&part_path)
        .map_err(|e| format!("Failed creating {:?}: {e}", part_path))?;

    let mut buf = [0u8; 65_536];
    let mut downloaded: u64 = 0;
    let mut last_pct: u64 = 101;
    let mut last_emit = Instant::now();

    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Stream read error on {label}: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("Disk write error on {label}: {e}"))?;
        downloaded += n as u64;

        let should_emit = if let Some(total) = total_bytes.filter(|t| *t > 0) {
            let pct = (downloaded * 100) / total;
            if pct != last_pct && (pct % 5 == 0 || last_emit.elapsed() >= Duration::from_secs(2)) {
                last_pct = pct;
                last_emit = Instant::now();
                let msg = format!("{label} ({pct}%)");
                let status = if is_model {
                    LlmStatus::DownloadingModel(msg)
                } else {
                    LlmStatus::DownloadingRuntime(msg)
                };
                update_llm_status(llm_status, event_tx, status);
                false
            } else {
                false
            }
        } else {
            last_emit.elapsed() >= Duration::from_secs(2)
        };

        if should_emit {
            last_emit = Instant::now();
            let mb = downloaded / (1024 * 1024);
            let msg = format!("{label} ({mb} MB)");
            let status = if is_model {
                LlmStatus::DownloadingModel(msg)
            } else {
                LlmStatus::DownloadingRuntime(msg)
            };
            update_llm_status(llm_status, event_tx, status);
        }
    }

    file.flush().map_err(|e| format!("Flush failed: {e}"))?;
    drop(file);
    if dest.exists() {
        let _ = fs::remove_file(dest);
    }
    fs::rename(&part_path, dest).map_err(|e| format!("Rename failed: {e}"))?;
    Ok(())
}

fn collect_runtime_files_recursive(dir: &Path, out_bin_dir: &Path) -> Result<usize, String> {
    let mut count = 0;
    let entries = fs::read_dir(dir).map_err(|e| format!("read_dir {:?} failed: {e}", dir))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            count += collect_runtime_files_recursive(&path, out_bin_dir)?;
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let low = name.to_ascii_lowercase();
        let is_target_bin = if cfg!(windows) {
            low == "llama-server.exe" || low.ends_with(".dll")
        } else {
            low == "llama-server" || low.contains(".so") || low.ends_with(".dylib")
        };

        if is_target_bin {
            let dest = out_bin_dir.join(name);
            fs::copy(&path, &dest)
                .map_err(|e| format!("Copy {:?} -> {:?} failed: {e}", path, dest))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = fs::metadata(&dest) {
                    let mut perms = meta.permissions();
                    perms.set_mode(0o755);
                    let _ = fs::set_permissions(&dest, perms);
                }
            }
            count += 1;
        }
    }
    Ok(count)
}

fn extract_archive_to_bin(
    archive_path: &Path,
    cache_dir: &Path,
    bin_dir: &Path,
) -> Result<(), String> {
    fs::create_dir_all(bin_dir).map_err(|e| format!("Failed creating bin_dir: {e}"))?;
    let tmp_dir = cache_dir.join("extract_tmp");
    if tmp_dir.exists() {
        let _ = fs::remove_dir_all(&tmp_dir);
    }
    fs::create_dir_all(&tmp_dir).map_err(|e| format!("Failed creating extract_tmp: {e}"))?;

    let low = archive_path
        .to_string_lossy()
        .to_ascii_lowercase();

    let extracted = if cfg!(windows) {
        let mut tar_cmd = Command::new("tar.exe");
        tar_cmd.arg("-xf").arg(archive_path).arg("-C").arg(&tmp_dir);
        #[cfg(windows)]
        tar_cmd.creation_flags(CREATE_NO_WINDOW);

        if tar_cmd.status().map(|s| s.success()).unwrap_or(false) {
            true
        } else {
            let script = format!(
                "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
                archive_path.display().to_string().replace('\'', "''"),
                tmp_dir.display().to_string().replace('\'', "''")
            );
            let mut ps_cmd = Command::new("powershell");
            ps_cmd.args(["-NoProfile", "-Command", &script]);
            #[cfg(windows)]
            ps_cmd.creation_flags(CREATE_NO_WINDOW);
            ps_cmd.status().map(|s| s.success()).unwrap_or(false)
        }
    } else if low.ends_with(".zip") {
        Command::new("unzip")
            .arg("-o")
            .arg(archive_path)
            .arg("-d")
            .arg(&tmp_dir)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        Command::new("tar")
            .arg("-xzf")
            .arg(archive_path)
            .arg("-C")
            .arg(&tmp_dir)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    if !extracted {
        let _ = fs::remove_dir_all(&tmp_dir);
        return Err(format!("Failed extracting {:?}", archive_path));
    }

    let copied = collect_runtime_files_recursive(&tmp_dir, bin_dir)?;
    let _ = fs::remove_dir_all(&tmp_dir);
    if copied == 0 {
        return Err(format!("No binaries found in {:?}", archive_path));
    }

    #[cfg(target_os = "macos")]
    {
        // Strip macOS Gatekeeper quarantine flag so downloaded Metal runtime & dylibs execute cleanly
        let _ = Command::new("xattr")
            .args(["-dr", "com.apple.quarantine"])
            .arg(bin_dir)
            .status();
    }

    println!("[Provisioner] Extracted {copied} binary/library file(s) to {:?}", bin_dir);
    Ok(())
}

fn parse_release_assets(rel: &serde_json::Value) -> Option<(String, Vec<ReleaseAsset>)> {
    let tag = rel.get("tag_name")?.as_str()?.to_string();
    let arr = rel.get("assets")?.as_array()?;
    let mut assets = Vec::with_capacity(arr.len());
    for item in arr {
        let Some(name) = item.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(url) = item.get("browser_download_url").and_then(|v| v.as_str()) else {
            continue;
        };
        let size = item.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
        assets.push(ReleaseAsset {
            name: name.to_string(),
            download_url: url.to_string(),
            size,
        });
    }
    Some((tag, assets))
}

fn fetch_matching_prism_assets(profile: &HardwareProfile) -> Result<SelectedAssets, String> {
    // 1. Try /releases/latest first
    if let Ok(res) = ureq::get("https://api.github.com/repos/PrismML-Eng/llama.cpp/releases/latest")
        .set("User-Agent", "LocalAI-Companion")
        .timeout(Duration::from_secs(15))
        .call()
    {
        if let Ok(rel) = res.into_json::<serde_json::Value>() {
            if let Some((tag, assets)) = parse_release_assets(&rel) {
                if let Some(sel) = hardware::select_release_assets(&tag, &assets, profile) {
                    return Ok(sel);
                }
            }
        }
    }

    // 2. Fallback to recent releases list in case latest tag hasn't finished uploading platform assets
    let res = ureq::get("https://api.github.com/repos/PrismML-Eng/llama.cpp/releases?per_page=6")
        .set("User-Agent", "LocalAI-Companion")
        .timeout(Duration::from_secs(20))
        .call()
        .map_err(|e| format!("GitHub releases query failed: {e}"))?;

    let list: serde_json::Value = res
        .into_json()
        .map_err(|e| format!("GitHub releases JSON invalid: {e}"))?;

    if let Some(arr) = list.as_array() {
        for rel in arr {
            if let Some((tag, assets)) = parse_release_assets(rel) {
                if let Some(sel) = hardware::select_release_assets(&tag, &assets, profile) {
                    return Ok(sel);
                }
            }
        }
    }

    Err(format!(
        "No matching PrismML release asset for {}",
        profile.backend.display_name()
    ))
}

fn ensure_runtime_provisioned(
    cfg: &RuntimeConfig,
    profile: &HardwareProfile,
    llm_status: &Arc<Mutex<LlmStatus>>,
    event_tx: &Sender<BackendEvent>,
) -> Result<(), String> {
    if cfg.llama_server_bin.exists() {
        return Ok(());
    }

    update_llm_status(
        llm_status,
        event_tx,
        LlmStatus::DownloadingRuntime(format!(
            "Finding {} release...",
            profile.backend.display_name()
        )),
    );

    let selected = fetch_matching_prism_assets(profile)?;
    println!(
        "[Provisioner] Selected PrismML release {} -> {}",
        selected.tag, selected.main.name
    );

    let main_dest = cfg.cache_dir.join(&selected.main.name);
    download_file_with_progress(
        &selected.main.download_url,
        &main_dest,
        &selected.main.name,
        false,
        llm_status,
        event_tx,
    )?;
    extract_archive_to_bin(&main_dest, &cfg.cache_dir, &cfg.bin_dir)?;

    if let Some(ref cudart) = selected.companion_cudart {
        println!("[Provisioner] Downloading CUDA runtime DLLs: {}", cudart.name);
        let cudart_dest = cfg.cache_dir.join(&cudart.name);
        download_file_with_progress(
            &cudart.download_url,
            &cudart_dest,
            &cudart.name,
            false,
            llm_status,
            event_tx,
        )?;
        extract_archive_to_bin(&cudart_dest, &cfg.cache_dir, &cfg.bin_dir)?;
    }

    if !cfg.llama_server_bin.exists() {
        return Err(format!(
            "Provisioning finished but {:?} was not found.",
            cfg.llama_server_bin
        ));
    }
    Ok(())
}

fn ensure_models_provisioned(
    cfg: &mut RuntimeConfig,
    llm_status: &Arc<Mutex<LlmStatus>>,
    event_tx: &Sender<BackendEvent>,
) -> Result<(), String> {
    let model_ok = cfg.model_path.exists()
        && fs::metadata(&cfg.model_path)
            .map(|m| m.len() > 1_048_576)
            .unwrap_or(false);
    let mmproj_ok = cfg
        .mmproj_path
        .as_ref()
        .map(|p| {
            p.exists()
                && fs::metadata(p)
                    .map(|m| m.len() > 1_048_576)
                    .unwrap_or(false)
        })
        .unwrap_or(false);

    if model_ok && mmproj_ok {
        return Ok(());
    }

    const HF_REPO: &str = "prism-ml/Ternary-Bonsai-2.5-27B-GGUF";
    update_llm_status(
        llm_status,
        event_tx,
        LlmStatus::DownloadingModel("Querying Hugging Face...".to_string()),
    );

    let api_url = format!("https://huggingface.co/api/models/{HF_REPO}");
    let meta: serde_json::Value = ureq::get(&api_url)
        .set("User-Agent", "LocalAI-Companion")
        .timeout(Duration::from_secs(20))
        .call()
        .map_err(|e| format!("Hugging Face query failed: {e}"))?
        .into_json()
        .map_err(|e| format!("Hugging Face JSON invalid: {e}"))?;

    let siblings = meta
        .get("siblings")
        .and_then(|s| s.as_array())
        .ok_or_else(|| "Missing siblings list in Hugging Face metadata".to_string())?;

    let mut q2_name: Option<String> = None;
    let mut mmproj_name: Option<String> = None;

    for s in siblings {
        let Some(fname) = s.get("rfilename").and_then(|v| v.as_str()) else {
            continue;
        };
        let low = fname.to_ascii_lowercase();
        if fname == "Ternary-Bonsai-2.5-27B-Q2_0.gguf"
            || (q2_name.is_none() && low.contains("q2_0") && low.ends_with(".gguf"))
        {
            q2_name = Some(fname.to_string());
        }
        if low.contains("mmproj") && low.ends_with(".gguf") {
            mmproj_name = Some(fname.to_string());
        }
    }

    if !model_ok {
        let model_file = q2_name.ok_or_else(|| "Q2_0 GGUF file not found on Hugging Face".to_string())?;
        let leaf = Path::new(&model_file)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Ternary-Bonsai-2.5-27B-Q2_0.gguf");
        let dest = cfg.models_dir.join(leaf);
        let url = format!("https://huggingface.co/{HF_REPO}/resolve/main/{model_file}");
        download_file_with_progress(&url, &dest, leaf, true, llm_status, event_tx)?;
        cfg.model_path = dest;
    }

    if !mmproj_ok {
        if let Some(mm_file) = mmproj_name {
            let leaf = Path::new(&mm_file)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("mmproj.gguf");
            let dest = cfg.models_dir.join(leaf);
            let url = format!("https://huggingface.co/{HF_REPO}/resolve/main/{mm_file}");
            download_file_with_progress(&url, &dest, leaf, true, llm_status, event_tx)?;
            cfg.mmproj_path = Some(dest);
        }
    }

    cfg.resolve_mmproj();
    Ok(())
}

fn run_llama_supervisor(
    mut cfg: RuntimeConfig,
    event_tx: Sender<BackendEvent>,
    llama_child: Arc<Mutex<Option<Child>>>,
    we_spawned_llama: Arc<AtomicBool>,
    llm_status: Arc<Mutex<LlmStatus>>,
) {
    let llm_addr = cfg.llm_addr();
    let health_url = format!("{}/health", cfg.llm_base_url());

    // 1. Detect hardware & compute VRAM-aware defaults
    update_llm_status(&llm_status, &event_tx, LlmStatus::DetectingHardware);
    let profile = hardware::detect_hardware(cfg.backend_override.as_deref());
    let (effective_ngl, effective_ctx) = if cfg.auto_vram_sizing {
        (profile.recommended_ngl, profile.recommended_ctx)
    } else {
        (cfg.n_gpu_layers, cfg.ctx_size)
    };
    println!(
        "[Hardware] OS={:?} Arch={:?} Backend={} GPU={:?} VRAM={:?}MiB -> ngl={}, ctx={}",
        profile.os,
        profile.arch,
        profile.backend.display_name(),
        profile.gpu_name,
        profile.vram_mb,
        effective_ngl,
        effective_ctx
    );

    // 2. Reuse existing instance if port is already open, else provision + spawn
    if is_tcp_listening(&llm_addr) {
        println!(
            "[LLM Supervisor] Existing LLM server detected on {}. Reusing active instance.",
            llm_addr
        );
        we_spawned_llama.store(false, Ordering::SeqCst);
    } else {
        if let Err(err) = ensure_runtime_provisioned(&cfg, &profile, &llm_status, &event_tx) {
            update_llm_status(&llm_status, &event_tx, LlmStatus::Error(err));
        } else if let Err(err) = ensure_models_provisioned(&mut cfg, &llm_status, &event_tx) {
            update_llm_status(&llm_status, &event_tx, LlmStatus::Error(err));
        } else {
            println!(
                "[LLM Supervisor] Spawning PrismML llama-server on {} (model: {:?}, ngl={}, ctx={})",
                llm_addr, cfg.model_path, effective_ngl, effective_ctx
            );

            let mut cmd = Command::new(&cfg.llama_server_bin);
            cmd.arg("-m").arg(&cfg.model_path);

            if let Some(ref mmproj) = cfg.mmproj_path {
                if mmproj.exists() {
                    println!("[LLM Supervisor] Attaching vision projector: {:?}", mmproj);
                    cmd.arg("--mmproj").arg(mmproj);
                }
            }

            cmd.args([
                "--host",
                &cfg.host,
                "--port",
                &cfg.port.to_string(),
                "-ngl",
                &effective_ngl.to_string(),
                "-fa",
                "on",
                "-ctk",
                "f16",
                "-ctv",
                "f16",
                "-c",
                &effective_ctx.to_string(),
                "-b",
                &cfg.batch_size.to_string(),
                "-ub",
                &cfg.ubatch_size.to_string(),
                "--parallel",
                &cfg.parallel.to_string(),
            ]);

            if let Some(bin_dir) = cfg.llama_server_bin.parent() {
                cmd.current_dir(bin_dir);
            }

            #[cfg(windows)]
            cmd.creation_flags(CREATE_NO_WINDOW);

            match cmd.spawn() {
                Ok(child) => {
                    let pid = child.id();
                    println!("[LLM Supervisor] llama-server spawned (PID {pid}).");
                    we_spawned_llama.store(true, Ordering::SeqCst);
                    if let Ok(mut lock) = llama_child.lock() {
                        *lock = Some(child);
                    }
                    update_llm_status(&llm_status, &event_tx, LlmStatus::Starting);
                }
                Err(e) => {
                    let msg = format!("Failed to launch llama-server: {e}");
                    update_llm_status(&llm_status, &event_tx, LlmStatus::Error(msg));
                }
            }
        }
    }

    // 3. Non-blocking /health polling loop
    loop {
        let child_exited = if let Ok(mut lock) = llama_child.lock() {
            if let Some(ref mut child) = *lock {
                match child.try_wait() {
                    Ok(Some(code)) => Some(format!("llama-server exited early ({code})")),
                    _ => None,
                }
            } else {
                None
            }
        } else {
            None
        };

        if let Some(err_msg) = child_exited {
            update_llm_status(&llm_status, &event_tx, LlmStatus::Error(err_msg));
            thread::sleep(Duration::from_secs(10));
            continue;
        }

        let next_status = match ureq::get(&health_url)
            .timeout(Duration::from_millis(800))
            .call()
        {
            Ok(_) => LlmStatus::Ready,
            Err(ureq::Error::Status(503, _)) => LlmStatus::LoadingModel,
            Err(ureq::Error::Status(code, _)) => {
                LlmStatus::Error(format!("Unexpected health status HTTP {code}"))
            }
            Err(_) => {
                let has_child = llama_child.lock().map(|l| l.is_some()).unwrap_or(false);
                if has_child {
                    LlmStatus::Starting
                } else if !cfg.llama_server_bin.exists() || !cfg.model_path.exists() {
                    if let Ok(guard) = llm_status.lock() {
                        guard.clone()
                    } else {
                        LlmStatus::Starting
                    }
                } else {
                    LlmStatus::Starting
                }
            }
        };

        let is_ready = matches!(next_status, LlmStatus::Ready);
        update_llm_status(&llm_status, &event_tx, next_status);

        thread::sleep(if is_ready {
            Duration::from_secs(15)
        } else {
            Duration::from_secs(1)
        });
    }
}

/// Finds the optional audio backend directory inside app search roots (including macOS .app Resources)
pub fn find_backend_dir() -> Option<PathBuf> {
    if let Ok(env_dir) =
        std::env::var("LOCAL_AI_BACKEND_DIR").or_else(|_| std::env::var("BACKEND_DIR"))
    {
        let p = PathBuf::from(env_dir);
        if p.join("convene_server.py").exists() {
            return Some(p);
        }
    }

    for root in crate::config::app_search_roots() {
        let local_candidates = [
            root.join("backend"),
            root.join("Backend"),
            root.join("runtime").join("backend"),
            root.join("runtime").join("Backend"),
        ];

        for cand in &local_candidates {
            if cand.join("convene_server.py").exists() {
                return Some(cand.clone());
            }
        }
    }

    None
}

/// Probes whether the Kokoro/Whisper audio server port is active and responding
pub fn is_backend_port_alive() -> bool {
    let base_url = get_server_base_url();

    let stripped = base_url
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let host_port = stripped.split('/').next().unwrap_or("127.0.0.1:8000");
    let addr_str = if host_port.contains(':') {
        host_port.to_string()
    } else {
        format!("{host_port}:8000")
    };

    if is_tcp_listening(&addr_str) {
        return true;
    }

    let status_url = format!("{base_url}/status");
    ureq::get(&status_url)
        .timeout(Duration::from_millis(300))
        .call()
        .is_ok()
}

fn venv_python_candidates(base: &Path) -> Vec<PathBuf> {
    if cfg!(windows) {
        vec![
            base.join("python_env").join("python.exe"),
            base.join("python_env").join("Scripts").join("python.exe"),
            base.join(".venv").join("Scripts").join("python.exe"),
            base.join("venv").join("Scripts").join("python.exe"),
        ]
    } else {
        vec![
            base.join(".venv").join("bin").join("python3"),
            base.join(".venv").join("bin").join("python"),
            base.join("venv").join("bin").join("python3"),
            base.join("venv").join("bin").join("python"),
            base.join("python_env").join("bin").join("python3"),
            base.join("python_env").join("bin").join("python"),
        ]
    }
}

fn find_system_python() -> Option<PathBuf> {
    let candidates: &[&str] = if cfg!(windows) {
        &["python.exe", "python", "py"]
    } else {
        &[
            "python3",
            "/opt/homebrew/bin/python3",
            "/usr/local/bin/python3",
            "python",
        ]
    };

    for cand in candidates {
        let mut cmd = Command::new(cand);
        cmd.arg("--version");
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        if cmd.output().map(|o| o.status.success()).unwrap_or(false) {
            return Some(PathBuf::from(cand));
        }
    }
    None
}

fn bootstrap_python_venv(base: &Path) -> Option<PathBuf> {
    let req_file = base.join("requirements.txt");
    if !req_file.exists() {
        return None;
    }

    let sys_py = find_system_python()?;
    let venv_dir = base.join(".venv");
    println!(
        "[Backend Supervisor] Bootstrapping Python virtual environment at {:?} using {:?}...",
        venv_dir, sys_py
    );

    let mut mk_venv = Command::new(&sys_py);
    mk_venv.args(["-m", "venv"]).arg(&venv_dir).current_dir(base);
    #[cfg(windows)]
    mk_venv.creation_flags(CREATE_NO_WINDOW);

    if !mk_venv.status().map(|s| s.success()).unwrap_or(false) {
        println!("[Backend Supervisor] Failed to create .venv; falling back to system Python.");
        return None;
    }

    let venv_py = venv_python_candidates(base).into_iter().find(|p| p.exists())?;
    println!("[Backend Supervisor] Installing Backend/requirements.txt into .venv...");

    let mut pip_cmd = Command::new(&venv_py);
    pip_cmd
        .args(["-m", "pip", "install", "--disable-pip-version-check", "-r"])
        .arg(&req_file)
        .current_dir(base);
    #[cfg(windows)]
    pip_cmd.creation_flags(CREATE_NO_WINDOW);

    if pip_cmd.status().map(|s| s.success()).unwrap_or(false) {
        println!("[Backend Supervisor] Backend/.venv ready!");
        Some(venv_py)
    } else {
        println!("[Backend Supervisor] pip install failed; attempting fallback.");
        Some(venv_py)
    }
}

fn try_start_local_backend_if_needed(
    child_holder: &Arc<Mutex<Option<Child>>>,
    we_spawned: &Arc<AtomicBool>,
) {
    if is_backend_port_alive() {
        println!("[Backend Supervisor] Existing Kokoro/Whisper backend detected. Reusing active instance.");
        we_spawned.store(false, Ordering::SeqCst);
        return;
    }

    let Some(base) = find_backend_dir() else {
        println!("[Backend Supervisor] Kokoro/Whisper directory not found; running without Python TTS/STT.");
        return;
    };

    let script = base.join("convene_server.py");
    if !script.exists() {
        return;
    }

    let python_exe = venv_python_candidates(&base)
        .into_iter()
        .find(|p| p.exists())
        .or_else(|| bootstrap_python_venv(&base))
        .or_else(find_system_python);

    let Some(python_exe) = python_exe else {
        println!("[Backend Supervisor] Python executable not found; skipping Python TTS/STT spawn.");
        return;
    };

    if is_backend_port_alive() {
        println!("[Backend Supervisor] Backend port already active. Skipping duplicate spawn.");
        we_spawned.store(false, Ordering::SeqCst);
        return;
    }

    println!("[Backend Supervisor] Spawning Kokoro TTS + Faster-Whisper backend from: {:?}", base);

    let mut cmd = Command::new(&python_exe);
    cmd.arg(&script).current_dir(&base);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    match cmd.spawn() {
        Ok(child) => {
            let pid = child.id();
            println!("[Backend Supervisor] Audio backend spawned (PID {pid}). Waiting for initialization...");
            we_spawned.store(true, Ordering::SeqCst);
            if let Ok(mut lock) = child_holder.lock() {
                *lock = Some(child);
            }

            let base_url = get_server_base_url();
            let status_url = format!("{base_url}/status");

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
            println!("[Backend Supervisor] Audio backend running; initialization in progress.");
        }
        Err(e) => {
            println!("[Backend Supervisor] Failed to spawn audio backend: {e}");
        }
    }
}

fn should_take_screenshot(user_text: &str) -> bool {
    let low = user_text.to_lowercase();
    SCREENSHOT_TRIGGERS.iter().any(|t| low.contains(t))
}

fn strip_emotion_for_display(text: &str) -> String {
    text.trim().to_string()
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
                                            if let Some(mic_active) =
                                                json.get("mic_active").and_then(|v| v.as_bool())
                                            {
                                                let _ = event_tx.send(BackendEvent::ServerStatus {
                                                    connected: true,
                                                    mic_active,
                                                    bot_is_talking: false,
                                                });
                                            }
                                        }
                                        "assistant.state" => {
                                            let phase = json
                                                .get("phase")
                                                .and_then(|p| p.as_str())
                                                .unwrap_or("idle");
                                            let emotion_tag = json
                                                .get("emotion")
                                                .and_then(|e| e.as_str())
                                                .unwrap_or("Idle");
                                            let emotion = CompanionEmotion::from_tag(emotion_tag);
                                            let _ = event_tx.send(BackendEvent::AssistantState {
                                                phase: phase.to_string(),
                                                emotion,
                                            });
                                        }
                                        "assistant.message" => {
                                            let text = json
                                                .get("text")
                                                .and_then(|t| t.as_str())
                                                .unwrap_or("");
                                            let emotion_tag = json
                                                .get("emotion")
                                                .and_then(|e| e.as_str())
                                                .unwrap_or("Idle");
                                            let emotion = CompanionEmotion::from_tag(emotion_tag);
                                            if !text.is_empty() {
                                                let _ = event_tx.send(
                                                    BackendEvent::AssistantMessage {
                                                        text: text.to_string(),
                                                        emotion,
                                                    },
                                                );
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

                is_connected.store(false, Ordering::Relaxed);
                let _ = event_tx.send(BackendEvent::ServerStatus {
                    connected: false,
                    mic_active: false,
                    bot_is_talking: false,
                });
            }
            Err(_) => {
                is_connected.store(false, Ordering::Relaxed);
                thread::sleep(Duration::from_secs(5));
            }
        }
    }
}

fn execute_llm_chat(
    llm_base_url: &str,
    audio_base_url: &str,
    history: &mut Vec<serde_json::Value>,
    user_text: &str,
    force_screenshot: bool,
    event_tx: &Sender<BackendEvent>,
    voice_muted: &Arc<AtomicBool>,
) {
    let _ = event_tx.send(BackendEvent::AssistantState {
        phase: "thinking".to_string(),
        emotion: CompanionEmotion::Thinking,
    });

    let take_screen = force_screenshot || should_take_screenshot(user_text);
    let image_b64 = if take_screen {
        match capture_screen_base64() {
            Ok(b64) => Some(b64),
            Err(e) => {
                println!("[Vision] Screen capture failed: {e}");
                None
            }
        }
    } else {
        None
    };

    let user_msg = if let Some(b64) = image_b64 {
        serde_json::json!({
            "role": "user",
            "content": [
                { "type": "text", "text": user_text },
                { "type": "image_url", "image_url": { "url": format!("data:image/jpeg;base64,{b64}") } }
            ]
        })
    } else {
        serde_json::json!({
            "role": "user",
            "content": user_text
        })
    };

    let mut messages = Vec::with_capacity(history.len() + 2);
    messages.push(serde_json::json!({
        "role": "system",
        "content": SYSTEM_PROMPT
    }));
    messages.extend(history.iter().cloned());
    messages.push(user_msg);

    let payload = serde_json::json!({
        "model": "Ternary-Bonsai-2.5-27B-Q2_0",
        "messages": messages,
        "temperature": 0.7,
        "max_tokens": 256
    });

    let chat_url = format!("{llm_base_url}/v1/chat/completions");
    match ureq::post(&chat_url)
        .timeout(Duration::from_secs(60))
        .send_json(&payload)
    {
        Ok(res) => match res.into_json::<serde_json::Value>() {
            Ok(json) => {
                let raw_reply = json
                    .get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("message"))
                    .and_then(|m| m.get("content"))
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();

                if raw_reply.is_empty() {
                    let _ = event_tx.send(BackendEvent::Error(
                        "Bonsai returned an empty response.".to_string(),
                    ));
                    let _ = event_tx.send(BackendEvent::AssistantState {
                        phase: "idle".to_string(),
                        emotion: CompanionEmotion::Idle,
                    });
                    return;
                }

                // Append plain text turns to rolling history (avoid persisting base64 screenshots)
                history.push(serde_json::json!({ "role": "user", "content": user_text }));
                history.push(serde_json::json!({ "role": "assistant", "content": raw_reply }));
                if history.len() > 8 {
                    let excess = history.len() - 8;
                    history.drain(0..excess);
                }

                let emotion = CompanionEmotion::from_tag(&raw_reply);
                let display_text = strip_emotion_for_display(&raw_reply);

                let _ = event_tx.send(BackendEvent::AssistantMessage {
                    text: display_text,
                    emotion,
                });
                let _ = event_tx.send(BackendEvent::AssistantState {
                    phase: "idle".to_string(),
                    emotion,
                });

                // Trigger Kokoro TTS playback if voice is active (best-effort)
                if !voice_muted.load(Ordering::Relaxed) {
                    let speak_url = format!("{audio_base_url}/speak");
                    let _ = ureq::post(&speak_url)
                        .timeout(Duration::from_secs(2))
                        .send_json(serde_json::json!({ "text": raw_reply }));
                }
            }
            Err(e) => {
                let _ = event_tx.send(BackendEvent::Error(format!(
                    "Invalid JSON from Bonsai llama-server: {e}"
                )));
                let _ = event_tx.send(BackendEvent::AssistantState {
                    phase: "idle".to_string(),
                    emotion: CompanionEmotion::Sad,
                });
            }
        },
        Err(ureq::Error::Status(503, _)) => {
            let _ = event_tx.send(BackendEvent::Error(
                "Bonsai 27B is still loading into VRAM. Please wait a moment.".to_string(),
            ));
            let _ = event_tx.send(BackendEvent::AssistantState {
                phase: "idle".to_string(),
                emotion: CompanionEmotion::Thinking,
            });
        }
        Err(e) => {
            let _ = event_tx.send(BackendEvent::Error(format!(
                "Could not reach PrismML llama-server at {llm_base_url} ({e})"
            )));
            let _ = event_tx.send(BackendEvent::AssistantState {
                phase: "idle".to_string(),
                emotion: CompanionEmotion::Sad,
            });
        }
    }
}

fn run_backend_worker(
    cfg: RuntimeConfig,
    cmd_rx: Receiver<BackendCommand>,
    event_tx: Sender<BackendEvent>,
    is_connected: Arc<AtomicBool>,
    child_holder: Arc<Mutex<Option<Child>>>,
    we_spawned: Arc<AtomicBool>,
    voice_muted: Arc<AtomicBool>,
) {
    try_start_local_backend_if_needed(&child_holder, &we_spawned);

    let llm_base_url = cfg.llm_base_url();
    let mut history: Vec<serde_json::Value> = Vec::new();
    let mut last_fallback_check = std::time::Instant::now();
    let mut prev_mic = false;

    loop {
        let audio_base_url = get_server_base_url();

        match cmd_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(cmd) => match cmd {
                BackendCommand::SendMessage {
                    text,
                    force_screenshot,
                } => {
                    execute_llm_chat(
                        &llm_base_url,
                        &audio_base_url,
                        &mut history,
                        &text,
                        force_screenshot,
                        &event_tx,
                        &voice_muted,
                    );
                }
                BackendCommand::ToggleMic => {
                    let mic_url = format!("{audio_base_url}/toggle_mic");
                    match ureq::post(&mic_url).timeout(Duration::from_secs(3)).call() {
                        Ok(res) => {
                            is_connected.store(true, Ordering::Relaxed);
                            if let Ok(json) = res.into_json::<serde_json::Value>() {
                                if let Some(active) =
                                    json.get("mic_active").and_then(|v| v.as_bool())
                                {
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
                            let _ = event_tx
                                .send(BackendEvent::Error(format!("Toggle mic failed: {e}")));
                        }
                    }
                }
                BackendCommand::CaptureScreen { prompt } => {
                    let prompt_text =
                        prompt.unwrap_or_else(|| "What is on my screen right now?".to_string());
                    execute_llm_chat(
                        &llm_base_url,
                        &audio_base_url,
                        &mut history,
                        &prompt_text,
                        true,
                        &event_tx,
                        &voice_muted,
                    );
                }
            },
            Err(_) => {}
        }

        // Condition-based fallback check for Kokoro/Whisper audio server when WebSocket is disconnected
        if !is_connected.load(Ordering::Relaxed)
            && last_fallback_check.elapsed() >= Duration::from_secs(60)
        {
            last_fallback_check = std::time::Instant::now();

            let status_url = format!("{audio_base_url}/status");
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

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub root_dir: PathBuf,
    pub bin_dir: PathBuf,
    pub models_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub llama_server_bin: PathBuf,
    pub model_path: PathBuf,
    pub mmproj_path: Option<PathBuf>,
    pub backend_override: Option<String>,
    pub auto_vram_sizing: bool,
    pub host: String,
    pub port: u16,
    pub n_gpu_layers: u32,
    pub ctx_size: u32,
    pub kv_cache_type: String,
    pub batch_size: u32,
    pub ubatch_size: u32,
    pub parallel: u32,
    pub max_tokens: u32,
    pub reasoning_budget: i32,
    pub request_timeout_secs: u64,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        let bin_name = if cfg!(windows) {
            "runtime/bin/llama-server.exe"
        } else {
            "runtime/bin/llama-server"
        };

        Self {
            root_dir: PathBuf::from("runtime"),
            bin_dir: PathBuf::from("runtime/bin"),
            models_dir: PathBuf::from("runtime/models"),
            cache_dir: PathBuf::from("runtime/cache"),
            llama_server_bin: PathBuf::from(bin_name),
            model_path: PathBuf::from("runtime/models/Ternary-Bonsai-2.5-27B-Q2_0.gguf"),
            mmproj_path: None,
            backend_override: None,
            auto_vram_sizing: true,
            host: "127.0.0.1".to_string(),
            port: 8080,
            n_gpu_layers: 99,
            ctx_size: 16384,
            kv_cache_type: "q8_0".to_string(),
            batch_size: 512,
            ubatch_size: 256,
            parallel: 1,
            max_tokens: 2048,
            reasoning_budget: 1024,
            request_timeout_secs: 300,
        }
    }
}

/// Detects if `exe_dir` is inside a macOS `.app` bundle (`.../App.app/Contents/MacOS`),
/// returning `(bundle_parent_dir, contents_resources_dir)`.
pub fn detect_macos_app_bundle(exe_dir: &Path) -> Option<(PathBuf, PathBuf)> {
    let dir_name = exe_dir.file_name().and_then(|n| n.to_str())?;
    if dir_name != "MacOS" {
        return None;
    }
    let contents_dir = exe_dir.parent()?;
    if contents_dir.file_name().and_then(|n| n.to_str()) != Some("Contents") {
        return None;
    }
    let app_bundle = contents_dir.parent()?;
    let bundle_name = app_bundle.file_name().and_then(|n| n.to_str())?;
    if !bundle_name.ends_with(".app") {
        return None;
    }
    let parent_dir = app_bundle.parent()?.to_path_buf();
    let resources_dir = contents_dir.join("Resources");
    Some((parent_dir, resources_dir))
}

/// Returns ordered candidate roots for configs, assets, and optional backends.
/// Inside a macOS `.app` bundle, searches both the `.app` parent directory (easy sharing)
/// and `Contents/Resources/` (bundled resources).
pub fn app_search_roots() -> Vec<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            if let Some((parent_dir, resources_dir)) = detect_macos_app_bundle(exe_dir) {
                let mut roots = vec![parent_dir];
                if resources_dir != roots[0] {
                    roots.push(resources_dir);
                }
                return roots;
            }
        }
    }
    vec![app_root_dir()]
}

/// Resolves the self-contained application root directory without leaving the project folder.
pub fn app_root_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            if let Some((parent_dir, resources_dir)) = detect_macos_app_bundle(exe_dir) {
                if parent_dir.join("config.toml").exists() || parent_dir.join("runtime").exists() {
                    return parent_dir;
                }
                if resources_dir.exists() {
                    return resources_dir;
                }
                return parent_dir;
            }

            if exe_dir.join("config.toml").exists() {
                return exe_dir.to_path_buf();
            }

            // When running from target/release or target/debug, resolve to the Cargo project root
            let is_profile_dir = exe_dir
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n == "release" || n == "debug")
                .unwrap_or(false);
            if is_profile_dir {
                if let Some(target_dir) = exe_dir.parent() {
                    if target_dir.file_name().and_then(|n| n.to_str()) == Some("target") {
                        if let Some(project_root) = target_dir.parent() {
                            if project_root.join("config.toml").exists()
                                || project_root.join("Cargo.toml").exists()
                            {
                                return project_root.to_path_buf();
                            }
                        }
                    }
                }
            }

            return exe_dir.to_path_buf();
        }
    }

    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

impl RuntimeConfig {
    /// Loads `config.toml` from `app_root_dir()`, applies env overrides, resolves relative paths
    /// against the config base directory, and auto-discovers `*mmproj*.gguf` if unset.
    pub fn load() -> Self {
        let mut cfg = Self::default();
        let root = app_root_dir();
        let config_path = find_config_toml(&root);

        let base_dir = if let Some(ref cfg_file) = config_path {
            if let Ok(contents) = std::fs::read_to_string(cfg_file) {
                cfg.apply_toml(&contents);
                println!("[Config] Loaded runtime config from {:?}", cfg_file);
            }
            cfg_file.parent().map(Path::to_path_buf).unwrap_or(root)
        } else {
            println!("[Config] config.toml not found; using default local runtime paths.");
            root
        };

        cfg.apply_env_overrides();
        cfg.normalize_platform_bin();
        cfg.resolve_relative_paths(&base_dir);
        cfg.resolve_mmproj();
        cfg
    }

    pub fn llm_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    pub fn llm_base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    pub fn resolve_mmproj(&mut self) {
        if let Some(ref p) = self.mmproj_path {
            if p.exists() {
                return;
            }
        }

        let mut search_dirs = vec![self.models_dir.clone()];
        if let Some(parent) = self.model_path.parent() {
            let p_buf = parent.to_path_buf();
            if p_buf != self.models_dir {
                search_dirs.insert(0, p_buf);
            }
        }

        for dir in search_dirs {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        let lower = name.to_ascii_lowercase();
                        if lower.contains("mmproj") && lower.ends_with(".gguf") {
                            self.mmproj_path = Some(path);
                            return;
                        }
                    }
                }
            }
        }
    }

    fn normalize_platform_bin(&mut self) {
        if !cfg!(windows) {
            if let Some(s) = self.llama_server_bin.to_str() {
                if s.ends_with(".exe") {
                    self.llama_server_bin = PathBuf::from(s.trim_end_matches(".exe"));
                }
            }
        } else if let Some(s) = self.llama_server_bin.to_str() {
            if s.ends_with("llama-server") {
                self.llama_server_bin = PathBuf::from(format!("{s}.exe"));
            }
        }
    }

    fn resolve_relative_paths(&mut self, base_dir: &Path) {
        if self.root_dir.is_relative() {
            self.root_dir = base_dir.join(&self.root_dir);
        }

        // Keep bin_dir, models_dir, cache_dir aligned under root_dir
        self.bin_dir = self.root_dir.join("bin");
        self.models_dir = self.root_dir.join("models");
        self.cache_dir = self.root_dir.join("cache");

        if self.llama_server_bin.is_relative() {
            self.llama_server_bin = base_dir.join(&self.llama_server_bin);
        }
        if let Some(parent) = self.llama_server_bin.parent() {
            self.bin_dir = parent.to_path_buf();
        }

        if self.model_path.is_relative() {
            self.model_path = base_dir.join(&self.model_path);
        }
        if let Some(parent) = self.model_path.parent() {
            self.models_dir = parent.to_path_buf();
        }

        if let Some(ref p) = self.mmproj_path {
            if p.is_relative() {
                self.mmproj_path = Some(base_dir.join(p));
            }
        }
    }

    fn apply_toml(&mut self, raw: &str) {
        let mut section = String::new();

        for line in raw.lines() {
            let trimmed = strip_toml_comment(line).trim();
            if trimmed.is_empty() {
                continue;
            }

            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                section = trimmed[1..trimmed.len() - 1].trim().to_ascii_lowercase();
                continue;
            }

            let Some((k, v)) = trimmed.split_once('=') else {
                continue;
            };
            let key = k.trim();
            let val = parse_toml_value(v.trim());

            match (section.as_str(), key) {
                ("runtime", "root_dir") if !val.is_empty() => self.root_dir = PathBuf::from(val),
                ("runtime", "llama_server_bin") if !val.is_empty() => {
                    self.llama_server_bin = PathBuf::from(val);
                }
                ("runtime", "model_path") if !val.is_empty() => {
                    self.model_path = PathBuf::from(val);
                }
                ("runtime", "mmproj_path") => {
                    if val.is_empty() {
                        self.mmproj_path = None;
                    } else {
                        self.mmproj_path = Some(PathBuf::from(val));
                    }
                }
                ("llm", "backend") => {
                    if val.is_empty() || val.eq_ignore_ascii_case("auto") {
                        self.backend_override = None;
                    } else {
                        self.backend_override = Some(val);
                    }
                }
                ("llm", "auto_vram_sizing") => {
                    self.auto_vram_sizing = val.eq_ignore_ascii_case("true") || val == "1";
                }
                ("llm", "host") if !val.is_empty() => self.host = val,
                ("llm", "port") => {
                    if let Ok(p) = val.parse::<u16>() {
                        self.port = p;
                    }
                }
                ("llm", "n_gpu_layers") => {
                    if let Ok(n) = val.parse::<u32>() {
                        self.n_gpu_layers = n;
                    }
                }
                ("llm", "ctx_size") => {
                    if let Ok(c) = val.parse::<u32>() {
                        self.ctx_size = c;
                    }
                }
                ("llm", "kv_cache_type") if !val.is_empty() => {
                    self.kv_cache_type = val;
                }
                ("llm", "batch_size") => {
                    if let Ok(b) = val.parse::<u32>() {
                        self.batch_size = b;
                    }
                }
                ("llm", "ubatch_size") => {
                    if let Ok(ub) = val.parse::<u32>() {
                        self.ubatch_size = ub;
                    }
                }
                ("llm", "parallel") => {
                    if let Ok(p) = val.parse::<u32>() {
                        self.parallel = p;
                    }
                }
                ("llm", "max_tokens") => {
                    if let Ok(m) = val.parse::<u32>() {
                        self.max_tokens = m;
                    }
                }
                ("llm", "reasoning_budget") => {
                    if let Ok(r) = val.parse::<i32>() {
                        self.reasoning_budget = r;
                    }
                }
                ("llm", "request_timeout_secs") => {
                    if let Ok(t) = val.parse::<u64>() {
                        self.request_timeout_secs = t;
                    }
                }
                _ => {}
            }
        }
    }

    fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var("LOCAL_AI_LLAMA_SERVER") {
            if !v.trim().is_empty() {
                self.llama_server_bin = PathBuf::from(v.trim());
            }
        }
        if let Ok(v) = std::env::var("LOCAL_AI_MODEL_PATH") {
            if !v.trim().is_empty() {
                self.model_path = PathBuf::from(v.trim());
            }
        }
        if let Ok(v) = std::env::var("LOCAL_AI_MMPROJ_PATH") {
            let trimmed = v.trim();
            if trimmed.is_empty() {
                self.mmproj_path = None;
            } else {
                self.mmproj_path = Some(PathBuf::from(trimmed));
            }
        }
        if let Ok(v) = std::env::var("LOCAL_AI_BACKEND") {
            let trimmed = v.trim();
            if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
                self.backend_override = None;
            } else {
                self.backend_override = Some(trimmed.to_string());
            }
        }
        if let Ok(v) = std::env::var("LOCAL_AI_LLM_HOST") {
            if !v.trim().is_empty() {
                self.host = v.trim().to_string();
            }
        }
        if let Ok(v) = std::env::var("LOCAL_AI_LLM_PORT") {
            if let Ok(p) = v.trim().parse::<u16>() {
                self.port = p;
            }
        }
    }
}

fn find_config_toml(root: &Path) -> Option<PathBuf> {
    let root_cand = root.join("config.toml");
    if root_cand.exists() {
        return Some(root_cand);
    }

    for candidate_root in app_search_roots() {
        let cand = candidate_root.join("config.toml");
        if cand.exists() {
            return Some(cand);
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let cwd_cand = cwd.join("config.toml");
        if cwd_cand.exists() {
            return Some(cwd_cand);
        }
    }

    None
}

fn strip_toml_comment(line: &str) -> &str {
    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;

    for (idx, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_double && ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' && !in_single {
            in_double = !in_double;
        } else if ch == '\'' && !in_double {
            in_single = !in_single;
        } else if ch == '#' && !in_double && !in_single {
            return &line[..idx];
        }
    }
    line
}

fn parse_toml_value(raw: &str) -> String {
    if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
        let inner = &raw[1..raw.len() - 1];
        inner.replace("\\\\", "\\").replace("\\\"", "\"")
    } else if raw.len() >= 2 && raw.starts_with('\'') && raw.ends_with('\'') {
        raw[1..raw.len() - 1].to_string()
    } else {
        raw.to_string()
    }
}

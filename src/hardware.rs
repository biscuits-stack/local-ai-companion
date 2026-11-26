#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOs {
    Windows,
    MacOs,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetArch {
    X64,
    Arm64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceleratorBackend {
    Metal,
    Cuda { major: u32, minor: u32 },
    Rocm,
    Sycl,
    Vulkan,
    Cpu,
}

impl AcceleratorBackend {
    pub fn display_name(&self) -> String {
        match self {
            AcceleratorBackend::Metal => "Apple Metal".to_string(),
            AcceleratorBackend::Cuda { major, minor } => format!("NVIDIA CUDA {major}.{minor}"),
            AcceleratorBackend::Rocm => "AMD ROCm/HIP".to_string(),
            AcceleratorBackend::Sycl => "Intel SYCL".to_string(),
            AcceleratorBackend::Vulkan => "Vulkan GPU".to_string(),
            AcceleratorBackend::Cpu => "CPU".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HardwareProfile {
    pub os: TargetOs,
    pub arch: TargetArch,
    pub backend: AcceleratorBackend,
    pub gpu_name: Option<String>,
    pub vram_mb: Option<u64>,
    pub recommended_ngl: u32,
    pub recommended_ctx: u32,
}

#[derive(Debug, Clone)]
pub struct ReleaseAsset {
    pub name: String,
    pub download_url: String,
    #[allow(dead_code)]
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct SelectedAssets {
    pub tag: String,
    pub main: ReleaseAsset,
    pub companion_cudart: Option<ReleaseAsset>,
}

fn current_os() -> TargetOs {
    match std::env::consts::OS {
        "windows" => TargetOs::Windows,
        "macos" => TargetOs::MacOs,
        _ => TargetOs::Linux,
    }
}

fn current_arch() -> TargetArch {
    match std::env::consts::ARCH {
        "aarch64" => TargetArch::Arm64,
        _ => TargetArch::X64,
    }
}

fn run_hidden(program: &str, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if stdout.is_empty() {
        None
    } else {
        Some(stdout)
    }
}

fn system_dll_exists(dll_name: &str) -> bool {
    if !cfg!(windows) {
        return false;
    }
    if let Ok(win_dir) = std::env::var("WINDIR").or_else(|_| std::env::var("SYSTEMROOT")) {
        let p = PathBuf::from(win_dir).join("System32").join(dll_name);
        if p.exists() {
            return true;
        }
    }
    false
}

fn linux_so_exists(so_name: &str) -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    let dirs = [
        "/usr/lib",
        "/usr/lib64",
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib/aarch64-linux-gnu",
        "/usr/local/lib",
    ];
    dirs.iter().any(|d| PathBuf::from(d).join(so_name).exists())
}

fn parse_cuda_version_from_smi(text: &str) -> Option<(u32, u32)> {
    let marker = "CUDA Version:";
    let idx = text.find(marker)?;
    let rest = text[idx + marker.len()..].trim_start();
    let ver_token: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let mut parts = ver_token.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts
        .next()
        .and_then(|m| m.parse::<u32>().ok())
        .unwrap_or(0);
    Some((major, minor))
}

fn probe_nvidia() -> Option<(u32, u32, Option<String>, Option<u64>)> {
    if let Some(smi_full) = run_hidden("nvidia-smi", &[]) {
        let (major, minor) = parse_cuda_version_from_smi(&smi_full).unwrap_or((12, 4));
        let mut gpu_name = None;
        let mut vram_mb = None;

        if let Some(csv) = run_hidden(
            "nvidia-smi",
            &[
                "--query-gpu=name,memory.total",
                "--format=csv,noheader,nounits",
            ],
        ) {
            if let Some(first_line) = csv.lines().next() {
                let mut cols = first_line.split(',');
                if let Some(name) = cols.next() {
                    let trimmed = name.trim();
                    if !trimmed.is_empty() {
                        gpu_name = Some(trimmed.to_string());
                    }
                }
                if let Some(mem) = cols.next() {
                    if let Ok(mb) = mem.trim().parse::<u64>() {
                        vram_mb = Some(mb);
                    }
                }
            }
        }

        return Some((major, minor, gpu_name, vram_mb));
    }

    if system_dll_exists("nvcuda.dll") || linux_so_exists("libcuda.so.1") {
        return Some((12, 4, Some("NVIDIA CUDA GPU".to_string()), None));
    }

    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GpuVendor {
    Amd,
    Intel,
}

fn probe_amd_or_intel_gpu() -> Option<(GpuVendor, Option<String>, Option<u64>)> {
    if cfg!(windows) {
        if let Some(out) = run_hidden(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_VideoController | ForEach-Object { \"$($_.PNPDeviceID)|$($_.Name)|$($_.AdapterRAM)\" }",
            ],
        ) {
            // Prefer AMD discrete/integrated first, then Intel
            let mut found_intel = None;
            for line in out.lines() {
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() < 2 {
                    continue;
                }
                let pnp = parts[0].to_ascii_uppercase();
                let name = parts[1].trim();
                let vram_mb = parts
                    .get(2)
                    .and_then(|s| s.trim().parse::<u64>().ok())
                    .map(|bytes| bytes / (1024 * 1024))
                    .filter(|mb| *mb > 0);

                if pnp.contains("VEN_1002") {
                    return Some((GpuVendor::Amd, Some(name.to_string()), vram_mb));
                }
                if pnp.contains("VEN_8086") && found_intel.is_none() {
                    found_intel = Some((GpuVendor::Intel, Some(name.to_string()), vram_mb));
                }
            }
            if let Some(intel) = found_intel {
                return Some(intel);
            }
        }
        return None;
    }

    if cfg!(target_os = "linux") {
        if run_hidden("rocminfo", &[]).is_some() {
            return Some((GpuVendor::Amd, Some("AMD ROCm GPU".to_string()), None));
        }

        if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
            let mut found_intel = None;
            for entry in entries.flatten() {
                let vendor_path = entry.path().join("device").join("vendor");
                if let Ok(vendor) = std::fs::read_to_string(vendor_path) {
                    let v = vendor.trim().to_ascii_lowercase();
                    if v == "0x1002" {
                        return Some((GpuVendor::Amd, Some("AMD GPU".to_string()), None));
                    }
                    if v == "0x8086" && found_intel.is_none() {
                        found_intel = Some((GpuVendor::Intel, Some("Intel GPU".to_string()), None));
                    }
                }
            }
            if let Some(intel) = found_intel {
                return Some(intel);
            }
        }
    }

    None
}

fn probe_apple_unified_mem_mb() -> Option<u64> {
    let out = run_hidden("sysctl", &["-n", "hw.memsize"])?;
    let bytes = out.trim().parse::<u64>().ok()?;
    Some(bytes / (1024 * 1024))
}

fn has_vulkan_runtime() -> bool {
    system_dll_exists("vulkan-1.dll")
        || linux_so_exists("libvulkan.so.1")
        || run_hidden("vulkaninfo", &["--summary"]).is_some()
}

fn compute_ngl_and_ctx(backend: &AcceleratorBackend, vram_mb: Option<u64>) -> (u32, u32) {
    match backend {
        AcceleratorBackend::Cpu => (0, 2048),
        AcceleratorBackend::Metal => match vram_mb {
            Some(mb) if mb <= 8_192 => {
                println!(
                    "[Hardware][WARNING] Apple Silicon unified memory is {} MiB (<= 8 GB). \
                     Running a 7.25 GB 27B Q2_0 model + BF16 vision projector may exceed safe wired Metal memory; \
                     capping context to 4096.",
                    mb
                );
                (99, 4096)
            }
            Some(mb) if mb < 16_384 => (99, 4096),
            Some(_) => (99, 8192),
            None => (99, 4096),
        },
        _ => match vram_mb {
            Some(mb) if mb >= 12_288 => (99, 8192),
            Some(mb) if mb >= 7_168 => (99, 4096),
            Some(mb) if mb >= 3_500 => (20, 2048),
            Some(_) => (0, 2048),
            None => (99, 4096),
        },
    }
}

/// Detects the active OS, CPU architecture, primary compute backend, and safe VRAM-aware launch defaults.
pub fn detect_hardware(backend_override: Option<&str>) -> HardwareProfile {
    let os = current_os();
    let arch = current_arch();

    // Respect explicit user override if set in config.toml / LOCAL_AI_BACKEND
    if let Some(ov) = backend_override {
        let low = ov.trim().to_ascii_lowercase();
        let (backend, gpu_name, vram_mb) = match low.as_str() {
            "metal" => (
                AcceleratorBackend::Metal,
                Some("Apple Silicon".to_string()),
                probe_apple_unified_mem_mb(),
            ),
            "cuda13" | "cuda-13" => {
                let (_, _, name, vram) = probe_nvidia().unwrap_or((13, 0, None, None));
                (AcceleratorBackend::Cuda { major: 13, minor: 0 }, name, vram)
            }
            "cuda12" | "cuda-12" => {
                let (_, _, name, vram) = probe_nvidia().unwrap_or((12, 4, None, None));
                (AcceleratorBackend::Cuda { major: 12, minor: 4 }, name, vram)
            }
            "cuda" => {
                if let Some((maj, min, name, vram)) = probe_nvidia() {
                    (AcceleratorBackend::Cuda { major: maj, minor: min }, name, vram)
                } else {
                    (AcceleratorBackend::Cuda { major: 12, minor: 4 }, None, None)
                }
            }
            "rocm" | "hip" => (AcceleratorBackend::Rocm, None, None),
            "sycl" => (AcceleratorBackend::Sycl, None, None),
            "vulkan" => (AcceleratorBackend::Vulkan, None, None),
            "cpu" => (AcceleratorBackend::Cpu, None, None),
            _ => return detect_hardware(None),
        };
        let (recommended_ngl, recommended_ctx) = compute_ngl_and_ctx(&backend, vram_mb);
        return HardwareProfile {
            os,
            arch,
            backend,
            gpu_name,
            vram_mb,
            recommended_ngl,
            recommended_ctx,
        };
    }

    // 1. Apple Silicon
    if os == TargetOs::MacOs && arch == TargetArch::Arm64 {
        let vram_mb = probe_apple_unified_mem_mb();
        let backend = AcceleratorBackend::Metal;
        let (recommended_ngl, recommended_ctx) = compute_ngl_and_ctx(&backend, vram_mb);
        return HardwareProfile {
            os,
            arch,
            backend,
            gpu_name: Some("Apple Silicon (Unified Memory)".to_string()),
            vram_mb,
            recommended_ngl,
            recommended_ctx,
        };
    }

    // 2. NVIDIA CUDA
    if let Some((major, minor, gpu_name, vram_mb)) = probe_nvidia() {
        let backend = AcceleratorBackend::Cuda { major, minor };
        let (recommended_ngl, recommended_ctx) = compute_ngl_and_ctx(&backend, vram_mb);
        return HardwareProfile {
            os,
            arch,
            backend,
            gpu_name,
            vram_mb,
            recommended_ngl,
            recommended_ctx,
        };
    }

    // 3 & 4. AMD ROCm/HIP or Intel SYCL
    if let Some((vendor, gpu_name, vram_mb)) = probe_amd_or_intel_gpu() {
        let backend = match vendor {
            GpuVendor::Amd => AcceleratorBackend::Rocm,
            GpuVendor::Intel => AcceleratorBackend::Sycl,
        };
        let (recommended_ngl, recommended_ctx) = compute_ngl_and_ctx(&backend, vram_mb);
        return HardwareProfile {
            os,
            arch,
            backend,
            gpu_name,
            vram_mb,
            recommended_ngl,
            recommended_ctx,
        };
    }

    // 5. Generic Vulkan runtime fallback
    if has_vulkan_runtime() {
        let backend = AcceleratorBackend::Vulkan;
        let (recommended_ngl, recommended_ctx) = compute_ngl_and_ctx(&backend, None);
        return HardwareProfile {
            os,
            arch,
            backend,
            gpu_name: Some("Vulkan GPU".to_string()),
            vram_mb: None,
            recommended_ngl,
            recommended_ctx,
        };
    }

    // 6. CPU fallback
    HardwareProfile {
        os,
        arch,
        backend: AcceleratorBackend::Cpu,
        gpu_name: None,
        vram_mb: None,
        recommended_ngl: 0,
        recommended_ctx: 2048,
    }
}

fn find_asset_matching<'a>(
    assets: &'a [ReleaseAsset],
    include: &[&str],
    exclude: &[&str],
) -> Option<&'a ReleaseAsset> {
    assets.iter().find(|a| {
        let low = a.name.to_ascii_lowercase();
        (low.ends_with(".zip") || low.ends_with(".tar.gz"))
            && include.iter().all(|pat| low.contains(pat))
            && exclude.iter().all(|pat| !low.contains(pat))
    })
}

/// Selects the matching main `llama-server` release archive and optional companion CUDA runtime DLL archive.
pub fn select_release_assets(
    tag: &str,
    assets: &[ReleaseAsset],
    profile: &HardwareProfile,
) -> Option<SelectedAssets> {
    match (profile.os, profile.arch, &profile.backend) {
        (TargetOs::Windows, TargetArch::X64, AcceleratorBackend::Cuda { major, .. }) => {
            let (main, cudart) = if *major >= 13 {
                if let Some(m13) = find_asset_matching(
                    assets,
                    &["bin-win-cuda-13", "-x64"],
                    &["cudart"],
                ) {
                    let c13 = find_asset_matching(
                        assets,
                        &["cudart-llama-bin-win-cuda-13", "-x64"],
                        &[],
                    )
                    .cloned();
                    (m13.clone(), c13)
                } else if let Some(m12) = find_asset_matching(
                    assets,
                    &["bin-win-cuda-12", "-x64"],
                    &["cudart"],
                ) {
                    let c12 = find_asset_matching(
                        assets,
                        &["cudart-llama-bin-win-cuda-12", "-x64"],
                        &[],
                    )
                    .cloned();
                    (m12.clone(), c12)
                } else {
                    return None;
                }
            } else if let Some(m12) = find_asset_matching(
                assets,
                &["bin-win-cuda-12", "-x64"],
                &["cudart"],
            ) {
                let c12 = find_asset_matching(
                    assets,
                    &["cudart-llama-bin-win-cuda-12", "-x64"],
                    &[],
                )
                .cloned();
                (m12.clone(), c12)
            } else {
                return None;
            };

            Some(SelectedAssets {
                tag: tag.to_string(),
                main,
                companion_cudart: cudart,
            })
        }
        (TargetOs::Windows, TargetArch::X64, AcceleratorBackend::Rocm) => {
            let main = find_asset_matching(assets, &["bin-win-hip-radeon-x64"], &[])
                .or_else(|| find_asset_matching(assets, &["bin-win-vulkan-x64"], &[]))?
                .clone();
            Some(SelectedAssets {
                tag: tag.to_string(),
                main,
                companion_cudart: None,
            })
        }
        (TargetOs::Windows, TargetArch::X64, AcceleratorBackend::Sycl) => {
            let main = find_asset_matching(assets, &["bin-win-sycl-x64"], &[])
                .or_else(|| find_asset_matching(assets, &["bin-win-vulkan-x64"], &[]))?
                .clone();
            Some(SelectedAssets {
                tag: tag.to_string(),
                main,
                companion_cudart: None,
            })
        }
        (TargetOs::Windows, TargetArch::X64, AcceleratorBackend::Vulkan) => {
            let main = find_asset_matching(assets, &["bin-win-vulkan-x64"], &[])
                .or_else(|| find_asset_matching(assets, &["bin-win-cpu-x64"], &[]))?
                .clone();
            Some(SelectedAssets {
                tag: tag.to_string(),
                main,
                companion_cudart: None,
            })
        }
        (TargetOs::Windows, TargetArch::X64, _) => {
            let main = find_asset_matching(assets, &["bin-win-cpu-x64"], &[])?.clone();
            Some(SelectedAssets {
                tag: tag.to_string(),
                main,
                companion_cudart: None,
            })
        }
        (TargetOs::Windows, TargetArch::Arm64, AcceleratorBackend::Cuda { .. }) => {
            if let Some(m) =
                find_asset_matching(assets, &["bin-win-cuda-", "-arm64"], &["cudart"])
            {
                let c = find_asset_matching(
                    assets,
                    &["cudart-llama-bin-win-cuda-", "-arm64"],
                    &[],
                )
                .cloned();
                Some(SelectedAssets {
                    tag: tag.to_string(),
                    main: m.clone(),
                    companion_cudart: c,
                })
            } else {
                let main = find_asset_matching(assets, &["bin-win-cpu-arm64"], &[])?.clone();
                Some(SelectedAssets {
                    tag: tag.to_string(),
                    main,
                    companion_cudart: None,
                })
            }
        }
        (TargetOs::Windows, TargetArch::Arm64, _) => {
            let main = find_asset_matching(assets, &["bin-win-cpu-arm64"], &[])?.clone();
            Some(SelectedAssets {
                tag: tag.to_string(),
                main,
                companion_cudart: None,
            })
        }
        (TargetOs::MacOs, TargetArch::Arm64, _) => {
            let main = find_asset_matching(assets, &["bin-macos-arm64.tar.gz"], &["kleidiai"])
                .or_else(|| find_asset_matching(assets, &["bin-macos-arm64"], &[]))?
                .clone();
            Some(SelectedAssets {
                tag: tag.to_string(),
                main,
                companion_cudart: None,
            })
        }
        (TargetOs::MacOs, TargetArch::X64, _) => {
            let main = find_asset_matching(assets, &["bin-macos-x64.tar.gz"], &[])?.clone();
            Some(SelectedAssets {
                tag: tag.to_string(),
                main,
                companion_cudart: None,
            })
        }
        (TargetOs::Linux, TargetArch::X64, AcceleratorBackend::Cuda { major, minor }) => {
            let main = if *major >= 13 {
                find_asset_matching(assets, &["bin-linux-cuda-13", "-x64"], &[])
                    .or_else(|| find_asset_matching(assets, &["bin-linux-cuda-12.8", "-x64"], &[]))
                    .or_else(|| find_asset_matching(assets, &["bin-linux-cuda-12.4", "-x64"], &[]))
            } else if *major == 12 && *minor >= 8 {
                find_asset_matching(assets, &["bin-linux-cuda-12.8", "-x64"], &[])
                    .or_else(|| find_asset_matching(assets, &["bin-linux-cuda-12.4", "-x64"], &[]))
            } else {
                find_asset_matching(assets, &["bin-linux-cuda-12.4", "-x64"], &[])
            }?
            .clone();
            Some(SelectedAssets {
                tag: tag.to_string(),
                main,
                companion_cudart: None,
            })
        }
        (TargetOs::Linux, TargetArch::X64, AcceleratorBackend::Rocm) => {
            let main = find_asset_matching(assets, &["bin-ubuntu-rocm", "-x64"], &[])
                .or_else(|| find_asset_matching(assets, &["bin-ubuntu-vulkan-x64"], &[]))?
                .clone();
            Some(SelectedAssets {
                tag: tag.to_string(),
                main,
                companion_cudart: None,
            })
        }
        (TargetOs::Linux, TargetArch::X64, AcceleratorBackend::Sycl) => {
            let main = find_asset_matching(assets, &["bin-ubuntu-sycl-fp16-x64"], &[])
                .or_else(|| find_asset_matching(assets, &["bin-ubuntu-vulkan-x64"], &[]))?
                .clone();
            Some(SelectedAssets {
                tag: tag.to_string(),
                main,
                companion_cudart: None,
            })
        }
        (TargetOs::Linux, TargetArch::X64, AcceleratorBackend::Vulkan) => {
            let main = find_asset_matching(assets, &["bin-ubuntu-vulkan-x64"], &[])
                .or_else(|| find_asset_matching(assets, &["bin-ubuntu-x64.tar.gz"], &[]))?
                .clone();
            Some(SelectedAssets {
                tag: tag.to_string(),
                main,
                companion_cudart: None,
            })
        }
        (TargetOs::Linux, TargetArch::X64, _) => {
            let main = find_asset_matching(assets, &["bin-ubuntu-x64.tar.gz"], &[])?.clone();
            Some(SelectedAssets {
                tag: tag.to_string(),
                main,
                companion_cudart: None,
            })
        }
        (TargetOs::Linux, TargetArch::Arm64, AcceleratorBackend::Vulkan) => {
            let main = find_asset_matching(assets, &["bin-ubuntu-vulkan-arm64.tar.gz"], &[])
                .or_else(|| find_asset_matching(assets, &["bin-ubuntu-arm64.tar.gz"], &[]))?
                .clone();
            Some(SelectedAssets {
                tag: tag.to_string(),
                main,
                companion_cudart: None,
            })
        }
        (TargetOs::Linux, TargetArch::Arm64, _) => {
            let main = find_asset_matching(assets, &["bin-ubuntu-arm64.tar.gz"], &[])?.clone();
            Some(SelectedAssets {
                tag: tag.to_string(),
                main,
                companion_cudart: None,
            })
        }
    }
}

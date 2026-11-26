#Requires -Version 5.1
param(
    [string]$RuntimeRoot = "",
    [string]$Backend = "auto",
    [switch]$SkipRuntime,
    [switch]$SkipModel
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
if ([string]::IsNullOrWhiteSpace($RuntimeRoot)) {
    $RuntimeRoot = Join-Path $RepoRoot "runtime"
}
$ConfigPath = Join-Path $RepoRoot "config.toml"

Write-Host "==================================================================" -ForegroundColor Cyan
Write-Host "  PrismML Ternary-Bonsai-2.5-27B-GGUF Plug-and-Play Bootstrapper" -ForegroundColor Cyan
Write-Host "  Runtime Root : $RuntimeRoot"
Write-Host "  Config File  : $ConfigPath"
Write-Host "==================================================================" -ForegroundColor Cyan

$DriveRoot = [System.IO.Path]::GetPathRoot($RuntimeRoot)
if ($DriveRoot -and -not (Test-Path $DriveRoot)) {
    Write-Error "Target drive '$DriveRoot' does not exist on this system. Pass -RuntimeRoot <path> to use another path."
    exit 2
}

$BinDir    = Join-Path $RuntimeRoot "bin"
$ModelsDir = Join-Path $RuntimeRoot "models"
$CacheDir  = Join-Path $RuntimeRoot "cache"

New-Item -ItemType Directory -Force -Path $RuntimeRoot, $BinDir, $ModelsDir, $CacheDir | Out-Null

# ------------------------------------------------------------------------------
# 1. Hardware & Backend Auto-Detection (No Compiler Required)
# ------------------------------------------------------------------------------
function Detect-HardwareProfile {
    param([string]$RequestedBackend)

    $arch = if ($env:PROCESSOR_ARCHITECTURE -match "ARM64" -or $env:PROCESSOR_ARCHITEW6432 -match "ARM64") { "arm64" } else { "x64" }
    $detected = $RequestedBackend.ToLowerInvariant()
    $cudaMajor = 12
    $cudaMinor = 4
    $gpuName = "Unknown"
    $vramMb = 0

    $nvidiaSmi = Get-Command "nvidia-smi.exe" -ErrorAction SilentlyContinue
    if ($nvidiaSmi) {
        try {
            $smiOut = (& $nvidiaSmi.Source 2>$null) -join "`n"
            if ($smiOut -match "CUDA Version:\s*(\d+)\.(\d+)") {
                $cudaMajor = [int]$Matches[1]
                $cudaMinor = [int]$Matches[2]
            }
            $csvOut = (& $nvidiaSmi.Source --query-gpu=name,memory.total --format=csv,noheader,nounits 2>$null) | Select-Object -First 1
            if ($csvOut -and $csvOut -match "^([^,]+),\s*(\d+)") {
                $gpuName = $Matches[1].Trim()
                $vramMb  = [int]$Matches[2]
            }
        } catch {}
    }

    if ($detected -eq "auto") {
        if ($nvidiaSmi -or (Test-Path "$env:WINDIR\System32\nvcuda.dll")) {
            $detected = if ($cudaMajor -ge 13) { "cuda13" } else { "cuda12" }
        } else {
            $gpus = Get-CimInstance Win32_VideoController -ErrorAction SilentlyContinue
            $amdGpu = $gpus | Where-Object { $_.PNPDeviceID -match "VEN_1002" } | Select-Object -First 1
            $intelGpu = $gpus | Where-Object { $_.PNPDeviceID -match "VEN_8086" } | Select-Object -First 1
            if ($amdGpu) {
                $detected = "rocm"
                $gpuName = $amdGpu.Name
                if ($amdGpu.AdapterRAM) { $vramMb = [math]::Floor($amdGpu.AdapterRAM / 1MB) }
            } elseif ($intelGpu) {
                $detected = "sycl"
                $gpuName = $intelGpu.Name
                if ($intelGpu.AdapterRAM) { $vramMb = [math]::Floor($intelGpu.AdapterRAM / 1MB) }
            } elseif (Test-Path "$env:WINDIR\System32\vulkan-1.dll") {
                $detected = "vulkan"
                $gpuName = "Vulkan GPU"
            } else {
                $detected = "cpu"
                $gpuName = "CPU"
            }
        }
    }

    return [PSCustomObject]@{
        Arch      = $arch
        Backend   = $detected
        CudaMajor = $cudaMajor
        CudaMinor = $cudaMinor
        GpuName   = $gpuName
        VramMb    = $vramMb
    }
}

$Profile = Detect-HardwareProfile -RequestedBackend $Backend
Write-Host ""
Write-Host "[1/3] Hardware Detection" -ForegroundColor Yellow
Write-Host "  Arch    : win-$($Profile.Arch)"
Write-Host "  GPU     : $($Profile.GpuName) (VRAM: $($Profile.VramMb) MiB)"
Write-Host "  Backend : $($Profile.Backend) (CUDA $($Profile.CudaMajor).$($Profile.CudaMinor))"

# ------------------------------------------------------------------------------
# 2. Download & Extract Pre-Built PrismML llama-server.exe Release
# ------------------------------------------------------------------------------
$ServerBin = Join-Path $BinDir "llama-server.exe"

function Download-FileWithResume {
    param([string]$Url, [string]$DestPath)
    if ((Test-Path $DestPath) -and ((Get-Item $DestPath).Length -gt 1024)) {
        Write-Host "  Using cached archive: $DestPath"
        return
    }
    $part = "$DestPath.part"
    if ((Test-Path $part) -and ((Get-Item $part).Length -le 1024)) {
        Remove-Item -Force $part -ErrorAction SilentlyContinue
    }
    Write-Host "  Downloading $Url -> $DestPath ..."
    & curl.exe -fL -C - --connect-timeout 15 --speed-limit 10240 --speed-time 20 --retry 6 --retry-delay 2 --retry-all-errors $Url -o $part
    if ($LASTEXITCODE -ne 0) {
        throw "curl.exe failed with exit code $LASTEXITCODE downloading $Url"
    }
    Move-Item -Path $part -Destination $DestPath -Force
}

function Extract-Binaries {
    param([string]$ArchivePath, [string]$OutBinDir)
    $tmpDir = Join-Path $CacheDir ("extract_" + [System.IO.Path]::GetFileNameWithoutExtension($ArchivePath))
    if (Test-Path $tmpDir) { Remove-Item -Recurse -Force $tmpDir }
    New-Item -ItemType Directory -Force -Path $tmpDir | Out-Null

    Write-Host "  Extracting $(Split-Path -Leaf $ArchivePath) ..."
    $tarExe = Get-Command "tar.exe" -ErrorAction SilentlyContinue
    if ($tarExe) {
        & $tarExe.Source -xf $ArchivePath -C $tmpDir
        if ($LASTEXITCODE -ne 0) {
            Expand-Archive -Path $ArchivePath -DestinationPath $tmpDir -Force
        }
    } else {
        Expand-Archive -Path $ArchivePath -DestinationPath $tmpDir -Force
    }

    $copied = 0
    Get-ChildItem -Path $tmpDir -Recurse -File | Where-Object {
        $_.Name -eq "llama-server.exe" -or $_.Extension -ieq ".dll"
    } | ForEach-Object {
        Copy-Item -Path $_.FullName -Destination (Join-Path $OutBinDir $_.Name) -Force
        $copied++
    }
    Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue
    Write-Host "  Installed $copied runtime file(s) into $OutBinDir" -ForegroundColor Green
}

if (-not $SkipRuntime) {
    Write-Host ""
    Write-Host "[2/3] Fetching Pre-Built PrismML llama.cpp Release ..." -ForegroundColor Cyan

    $Headers = @{ "User-Agent" = "LocalAI-Companion-Setup" }
    $Releases = Invoke-RestMethod -Uri "https://api.github.com/repos/PrismML-Eng/llama.cpp/releases?per_page=6" -Headers $Headers -TimeoutSec 30

    $SelectedRelease = $null
    $MainAsset = $null
    $CudartAsset = $null

    foreach ($rel in $Releases) {
        if (-not $rel.assets -or $rel.assets.Count -eq 0) { continue }
        $assets = $rel.assets
        $archTag = "-$($Profile.Arch)"

        switch -Wildcard ($Profile.Backend) {
            "cuda13" {
                $MainAsset = $assets | Where-Object { $_.name -match "bin-win-cuda-13.*$archTag\.zip$" -and $_.name -notmatch "cudart" } | Select-Object -First 1
                if ($MainAsset) {
                    $CudartAsset = $assets | Where-Object { $_.name -match "cudart-llama-bin-win-cuda-13.*$archTag\.zip$" } | Select-Object -First 1
                } else {
                    $MainAsset = $assets | Where-Object { $_.name -match "bin-win-cuda-12.*$archTag\.zip$" -and $_.name -notmatch "cudart" } | Select-Object -First 1
                    $CudartAsset = $assets | Where-Object { $_.name -match "cudart-llama-bin-win-cuda-12.*$archTag\.zip$" } | Select-Object -First 1
                }
                break
            }
            "cuda*" {
                $MainAsset = $assets | Where-Object { $_.name -match "bin-win-cuda-12.*$archTag\.zip$" -and $_.name -notmatch "cudart" } | Select-Object -First 1
                if ($MainAsset) {
                    $CudartAsset = $assets | Where-Object { $_.name -match "cudart-llama-bin-win-cuda-12.*$archTag\.zip$" } | Select-Object -First 1
                }
                break
            }
            "rocm" {
                $MainAsset = $assets | Where-Object { $_.name -match "bin-win-hip-radeon$archTag\.zip$" } | Select-Object -First 1
                if (-not $MainAsset) { $MainAsset = $assets | Where-Object { $_.name -match "bin-win-vulkan$archTag\.zip$" } | Select-Object -First 1 }
                break
            }
            "sycl" {
                $MainAsset = $assets | Where-Object { $_.name -match "bin-win-sycl$archTag\.zip$" } | Select-Object -First 1
                if (-not $MainAsset) { $MainAsset = $assets | Where-Object { $_.name -match "bin-win-vulkan$archTag\.zip$" } | Select-Object -First 1 }
                break
            }
            "vulkan" {
                $MainAsset = $assets | Where-Object { $_.name -match "bin-win-vulkan$archTag\.zip$" } | Select-Object -First 1
                break
            }
            default {
                $MainAsset = $assets | Where-Object { $_.name -match "bin-win-cpu$archTag\.zip$" } | Select-Object -First 1
                break
            }
        }

        if ($MainAsset) {
            $SelectedRelease = $rel
            break
        }
    }

    if (-not $MainAsset) {
        throw "Could not find a matching pre-built PrismML release archive for win-$($Profile.Arch) / $($Profile.Backend)."
    }

    Write-Host "  Selected Release : $($SelectedRelease.tag_name)"
    Write-Host "  Main Archive     : $($MainAsset.name)"
    if ($CudartAsset) { Write-Host "  CUDA Runtime Zip : $($CudartAsset.name)" }

    $MainZipPath = Join-Path $CacheDir $MainAsset.name
    Download-FileWithResume -Url $MainAsset.browser_download_url -DestPath $MainZipPath
    Extract-Binaries -ArchivePath $MainZipPath -OutBinDir $BinDir

    if ($CudartAsset) {
        $CudartZipPath = Join-Path $CacheDir $CudartAsset.name
        Download-FileWithResume -Url $CudartAsset.browser_download_url -DestPath $CudartZipPath
        Extract-Binaries -ArchivePath $CudartZipPath -OutBinDir $BinDir
    }
}

# ------------------------------------------------------------------------------
# 3. Verify / Download Ternary-Bonsai-2.5-27B-Q2_0 + mmproj from Hugging Face
# ------------------------------------------------------------------------------
$ModelFilename  = "Ternary-Bonsai-2.5-27B-Q2_0.gguf"
$MmprojFilename = $null
$RepoId         = "prism-ml/Ternary-Bonsai-2.5-27B-GGUF"

if (-not $SkipModel) {
    Write-Host ""
    Write-Host "[3/3] Verifying Hugging Face Model & Vision Projector ($RepoId) ..." -ForegroundColor Cyan

    $ExistingModel = Join-Path $ModelsDir $ModelFilename
    $ExistingMmproj = Get-ChildItem -Path $ModelsDir -Filter "*mmproj*.gguf" -ErrorAction SilentlyContinue | Select-Object -First 1

    if ((Test-Path $ExistingModel) -and ((Get-Item $ExistingModel).Length -gt 1MB) -and $ExistingMmproj) {
        $MmprojFilename = $ExistingMmproj.Name
        Write-Host "  Model & mmproj already present in $ModelsDir — skipping download." -ForegroundColor Green
    } else {
        $ApiUrl = "https://huggingface.co/api/models/$RepoId"
        $RepoMeta = Invoke-RestMethod -Uri $ApiUrl -Method Get -TimeoutSec 30
        $Files = $RepoMeta.siblings | ForEach-Object { $_.rfilename }

        $ExactQ2 = $Files | Where-Object { $_ -eq $ModelFilename } | Select-Object -First 1
        if (-not $ExactQ2) { $ExactQ2 = $Files | Where-Object { $_ -match "Q2_0.*\.gguf$" } | Select-Object -First 1 }
        if (-not $ExactQ2) { throw "Could not find Q2_0 GGUF file in $RepoId." }
        $ModelFilename = $ExactQ2

        $MmprojFilename = $Files | Where-Object { $_ -match "mmproj.*\.gguf$" } | Select-Object -First 1

        $HfCmd = Get-Command "hf.exe" -ErrorAction SilentlyContinue
        foreach ($File in @($ModelFilename, $MmprojFilename) | Where-Object { $_ }) {
            $DestPath = Join-Path $ModelsDir (Split-Path -Leaf $File)
            if ((Test-Path $DestPath) -and ((Get-Item $DestPath).Length -gt 1MB)) {
                Write-Host "  Already present: $DestPath"
                continue
            }
            if ($HfCmd) {
                & $HfCmd.Source download $RepoId $File --local-dir $ModelsDir
                if ($LASTEXITCODE -ne 0) { throw "hf download failed for $File" }
            } else {
                $DownloadUrl = "https://huggingface.co/$RepoId/resolve/main/$File"
                Write-Host "  Downloading $DownloadUrl -> $DestPath ..."
                & curl.exe -fL -C - --progress-bar $DownloadUrl -o $DestPath
                if ($LASTEXITCODE -ne 0) { throw "curl.exe download failed for $File" }
            }
        }
    }
}

# ------------------------------------------------------------------------------
# Write folder-relative config.toml
# ------------------------------------------------------------------------------
function To-ConfigPath {
    param([string]$AbsPath, [string]$BaseDir)
    $normAbs  = [System.IO.Path]::GetFullPath($AbsPath)
    $normBase = [System.IO.Path]::GetFullPath($BaseDir).TrimEnd('\', '/') + "\"
    if ($normAbs.StartsWith($normBase, [System.StringComparison]::OrdinalIgnoreCase)) {
        return $normAbs.Substring($normBase.Length).Replace("\", "/")
    }
    return $normAbs.Replace("\", "\\")
}

$ResolvedModelPath = Join-Path $ModelsDir (Split-Path -Leaf $ModelFilename)
$DiscoveredMmproj  = Get-ChildItem -Path $ModelsDir -Filter "*mmproj*.gguf" -ErrorAction SilentlyContinue | Select-Object -First 1
$ResolvedMmproj    = if ($DiscoveredMmproj) { $DiscoveredMmproj.FullName } else { "" }

$CfgRoot   = To-ConfigPath $RuntimeRoot $RepoRoot
$CfgBin    = To-ConfigPath $ServerBin $RepoRoot
$CfgModel  = To-ConfigPath $ResolvedModelPath $RepoRoot
$CfgMmproj = if ($ResolvedMmproj) { To-ConfigPath $ResolvedMmproj $RepoRoot } else { "" }

$ConfigContent = @"
[runtime]
root_dir = "$CfgRoot"
llama_server_bin = "$CfgBin"
model_path = "$CfgModel"
mmproj_path = "$CfgMmproj"

[llm]
backend = "$Backend"
auto_vram_sizing = true
host = "127.0.0.1"
port = 8080
n_gpu_layers = 99
ctx_size = 8192
batch_size = 512
ubatch_size = 64
parallel = 1
"@

Set-Content -Path $ConfigPath -Value $ConfigContent -Encoding UTF8

Write-Host ""
Write-Host "==================================================================" -ForegroundColor Green
Write-Host "  Plug-and-Play Setup Complete" -ForegroundColor Green
Write-Host "  llama-server.exe : $(if (Test-Path $ServerBin) { 'READY' } else { 'MISSING' })"
Write-Host "  Bonsai Q2_0 GGUF : $(if (Test-Path $ResolvedModelPath) { 'READY' } else { 'MISSING' })"
Write-Host "  Vision mmproj    : $(if ($ResolvedMmproj -and (Test-Path $ResolvedMmproj)) { $ResolvedMmproj } else { 'NOT FOUND' })"
Write-Host "==================================================================" -ForegroundColor Green

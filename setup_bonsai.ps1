#Requires -Version 5.1
param(
    [string]$RuntimeRoot = "",
    [switch]$SkipBuild,
    [switch]$SkipDownload
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
if ([string]::IsNullOrWhiteSpace($RuntimeRoot)) {
    $RuntimeRoot = Join-Path $RepoRoot "runtime"
}
$ConfigPath = Join-Path $RepoRoot "config.toml"

Write-Host "==================================================================" -ForegroundColor Cyan
Write-Host "  PrismML Ternary-Bonsai-2.5-27B-GGUF + CUDA Setup" -ForegroundColor Cyan
Write-Host "  Runtime Root : $RuntimeRoot"
Write-Host "  Config File  : $ConfigPath"
Write-Host "==================================================================" -ForegroundColor Cyan

# Verify target drive exists when a rooted path is supplied
$DriveRoot = [System.IO.Path]::GetPathRoot($RuntimeRoot)
if ($DriveRoot -and -not (Test-Path $DriveRoot)) {
    Write-Error "Target drive '$DriveRoot' does not exist on this system. Pass -RuntimeRoot <path> to use another path."
    exit 2
}

$LlamaSrcDir = Join-Path $RuntimeRoot "llama-cpp-prism"
$BinDir      = Join-Path $RuntimeRoot "bin"
$ModelsDir   = Join-Path $RuntimeRoot "models"

New-Item -ItemType Directory -Force -Path $RuntimeRoot, $BinDir, $ModelsDir | Out-Null

function Resolve-Tool {
    param(
        [string]$ExeName,
        [string[]]$ExtraPaths
    )
    $cmd = Get-Command $ExeName -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    foreach ($p in $ExtraPaths) {
        if ($p -and (Test-Path $p)) {
            $dir = Split-Path -Parent $p
            if ($env:PATH -notlike "*$dir*") {
                $env:PATH = "$dir;$env:PATH"
            }
            return $p
        }
    }
    return $null
}

$CudaBins = @()
if ($env:CUDA_PATH) {
    $CudaBins += (Join-Path $env:CUDA_PATH "bin\nvcc.exe")
}
if ($env:ProgramFiles) {
    $CudaToolkitRoot = Join-Path $env:ProgramFiles "NVIDIA GPU Computing Toolkit\CUDA"
    if (Test-Path $CudaToolkitRoot) {
        $CudaBins += Get-ChildItem $CudaToolkitRoot -Directory |
            Sort-Object Name -Descending |
            ForEach-Object { Join-Path $_.FullName "bin\nvcc.exe" }
    }
}

$VsCmakeBins = @()
if ($env:ProgramFiles) {
    $VsRoot = Join-Path $env:ProgramFiles "Microsoft Visual Studio"
    if (Test-Path $VsRoot) {
        $VsCmakeBins += Get-ChildItem $VsRoot -Directory | ForEach-Object {
            Get-ChildItem $_.FullName -Directory -ErrorAction SilentlyContinue |
                ForEach-Object { Join-Path $_.FullName "Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe" }
        }
    }
}

$GitPaths = @()
if ($env:ProgramFiles) {
    $GitPaths += (Join-Path $env:ProgramFiles "Git\cmd\git.exe")
    $GitPaths += (Join-Path $env:ProgramFiles "Git\bin\git.exe")
}
if ($env:LOCALAPPDATA) {
    $GitPaths += (Join-Path $env:LOCALAPPDATA "Programs\Git\cmd\git.exe")
}

$CmakePaths = @()
if ($env:ProgramFiles) {
    $CmakePaths += (Join-Path $env:ProgramFiles "CMake\bin\cmake.exe")
}
$CmakePaths += $VsCmakeBins

$HfPaths = @(
    (Join-Path $RepoRoot "backend\python_env\Scripts\hf.exe"),
    (Join-Path $RepoRoot "backend\python_env\Scripts\huggingface-cli.exe"),
    (Join-Path $RepoRoot "runtime\backend\python_env\Scripts\hf.exe"),
    (Join-Path $RepoRoot "runtime\backend\python_env\Scripts\huggingface-cli.exe")
)

$GitExe   = Resolve-Tool "git.exe" $GitPaths
$CmakeExe = Resolve-Tool "cmake.exe" $CmakePaths
$NvccExe  = Resolve-Tool "nvcc.exe" $CudaBins
$HfExe    = Resolve-Tool "hf.exe" $HfPaths

Write-Host ""
Write-Host "[Tool Discovery]" -ForegroundColor Yellow
Write-Host "  git   : $(if ($GitExe) { $GitExe } else { 'NOT FOUND' })"
Write-Host "  cmake : $(if ($CmakeExe) { $CmakeExe } else { 'NOT FOUND' })"
Write-Host "  nvcc  : $(if ($NvccExe) { $NvccExe } else { 'NOT FOUND' })"
Write-Host "  hf    : $(if ($HfExe) { $HfExe } else { 'NOT FOUND (will use curl.exe)' })"

$MissingBuildTools = @()
if (-not $GitExe)   { $MissingBuildTools += "Git" }
if (-not $CmakeExe) { $MissingBuildTools += "CMake" }
if (-not $NvccExe)  { $MissingBuildTools += "NVIDIA CUDA Toolkit (nvcc)" }

# ------------------------------------------------------------------------------
# PART 1: Clone & Build PrismML llama.cpp Fork (CUDA)
# ------------------------------------------------------------------------------
if (-not $SkipBuild) {
    Write-Host ""
    Write-Host "[1/3] PrismML llama.cpp CUDA Build" -ForegroundColor Cyan

    if ($MissingBuildTools.Count -gt 0) {
        Write-Warning ("Skipping llama.cpp CUDA build because required tools are missing: " + ($MissingBuildTools -join ", "))
    } else {
        if (-not (Test-Path (Join-Path $LlamaSrcDir ".git"))) {
            Write-Host "Cloning https://github.com/PrismML-Eng/llama.cpp.git into $LlamaSrcDir ..."
            & $GitExe clone https://github.com/PrismML-Eng/llama.cpp.git $LlamaSrcDir
            if ($LASTEXITCODE -ne 0) { throw "git clone failed with exit code $LASTEXITCODE" }
        } else {
            Write-Host "Updating existing repository in $LlamaSrcDir ..."
            Push-Location $LlamaSrcDir
            & $GitExe fetch --all --prune
            Pop-Location
        }

        Push-Location $LlamaSrcDir
        try {
            $remoteBranches = & $GitExe branch -r
            if ($remoteBranches -match "pr/q2_0-cuda") {
                Write-Host "Checking out branch pr/q2_0-cuda ..."
                & $GitExe checkout pr/q2_0-cuda
                if ($LASTEXITCODE -ne 0) {
                    & $GitExe checkout -b pr/q2_0-cuda origin/pr/q2_0-cuda
                }
            } else {
                Write-Host "Branch pr/q2_0-cuda not found; keeping default branch."
            }

            Write-Host "Configuring CMake with -DGGML_CUDA=ON ..."
            & $CmakeExe -B build -DGGML_CUDA=ON -DCMAKE_BUILD_TYPE=Release
            if ($LASTEXITCODE -ne 0) { throw "CMake configure failed with exit code $LASTEXITCODE" }

            Write-Host "Building llama-server (Release) ..."
            & $CmakeExe --build build --config Release --target llama-server -j
            if ($LASTEXITCODE -ne 0) { throw "CMake build failed with exit code $LASTEXITCODE" }

            $SearchDirs = @(
                (Join-Path $LlamaSrcDir "build\bin\Release"),
                (Join-Path $LlamaSrcDir "build\bin")
            )

            $CopiedServer = $false
            foreach ($dir in $SearchDirs) {
                $serverCandidate = Join-Path $dir "llama-server.exe"
                if (Test-Path $serverCandidate) {
                    Copy-Item -Path $serverCandidate -Destination $BinDir -Force
                    Get-ChildItem -Path $dir -Filter "ggml*.dll" -ErrorAction SilentlyContinue |
                        Copy-Item -Destination $BinDir -Force
                    Get-ChildItem -Path $dir -Filter "llama*.dll" -ErrorAction SilentlyContinue |
                        Copy-Item -Destination $BinDir -Force
                    $CopiedServer = $true
                    break
                }
            }

            if (-not $CopiedServer) {
                throw "Build finished, but llama-server.exe was not found in build\bin\Release or build\bin."
            }

            Write-Host "Copied llama-server.exe and runtime DLLs to $BinDir" -ForegroundColor Green
        } finally {
            Pop-Location
        }
    }
}

# ------------------------------------------------------------------------------
# PART 2: Query Hugging Face & Download Q2_0 Model + mmproj
# ------------------------------------------------------------------------------
$ModelFilename  = "Ternary-Bonsai-2.5-27B-Q2_0.gguf"
$MmprojFilename = $null
$RepoId         = "prism-ml/Ternary-Bonsai-2.5-27B-GGUF"

if (-not $SkipDownload) {
    Write-Host ""
    Write-Host "[2/3] Querying Hugging Face repo $RepoId ..." -ForegroundColor Cyan

    $ApiUrl = "https://huggingface.co/api/models/$RepoId"
    $RepoMeta = Invoke-RestMethod -Uri $ApiUrl -Method Get -TimeoutSec 30
    $Files = $RepoMeta.siblings | ForEach-Object { $_.rfilename }

    $ExactQ2 = $Files | Where-Object { $_ -eq $ModelFilename } | Select-Object -First 1
    if (-not $ExactQ2) {
        $ExactQ2 = $Files | Where-Object { $_ -match "Q2_0.*\.gguf$" } | Select-Object -First 1
    }
    if (-not $ExactQ2) {
        throw "Could not find Q2_0 GGUF file in $RepoId. Available files: $($Files -join ', ')"
    }
    $ModelFilename = $ExactQ2

    $MmprojFilename = $Files | Where-Object { $_ -match "^mmproj.*\.gguf$" } | Select-Object -First 1
    if (-not $MmprojFilename) {
        $MmprojFilename = $Files | Where-Object { $_ -match "mmproj.*\.gguf$" } | Select-Object -First 1
    }

    Write-Host "  Model file  : $ModelFilename"
    Write-Host "  Vision file : $(if ($MmprojFilename) { $MmprojFilename } else { 'None listed in repo' })"

    $ToDownload = @($ModelFilename)
    if ($MmprojFilename) {
        $ToDownload += $MmprojFilename
    }

    foreach ($File in $ToDownload) {
        $DestPath = Join-Path $ModelsDir (Split-Path -Leaf $File)
        if ($HfExe) {
            Write-Host "Downloading $File via hf CLI into $ModelsDir ..."
            & $HfExe download $RepoId $File --local-dir $ModelsDir
            if ($LASTEXITCODE -ne 0) {
                throw "hf download failed for $File with exit code $LASTEXITCODE"
            }
        } else {
            $DownloadUrl = "https://huggingface.co/$RepoId/resolve/main/$File"
            Write-Host "Downloading $DownloadUrl -> $DestPath ..."
            & curl.exe -fL -C - --progress-bar $DownloadUrl -o $DestPath
            if ($LASTEXITCODE -ne 0) {
                throw "curl.exe download failed for $File with exit code $LASTEXITCODE"
            }
        }
    }
}

# ------------------------------------------------------------------------------
# PART 3: Write config.toml (folder-relative paths when inside $RepoRoot)
# ------------------------------------------------------------------------------
Write-Host ""
Write-Host "[3/3] Updating $ConfigPath ..." -ForegroundColor Cyan

function To-ConfigPath {
    param([string]$AbsPath, [string]$BaseDir)
    $normAbs  = [System.IO.Path]::GetFullPath($AbsPath)
    $normBase = [System.IO.Path]::GetFullPath($BaseDir).TrimEnd('\', '/') + "\"
    if ($normAbs.StartsWith($normBase, [System.StringComparison]::OrdinalIgnoreCase)) {
        return $normAbs.Substring($normBase.Length).Replace("\", "/")
    }
    return $normAbs.Replace("\", "\\")
}

$ResolvedServerBin = Join-Path $BinDir "llama-server.exe"
$ResolvedModelPath = Join-Path $ModelsDir (Split-Path -Leaf $ModelFilename)
$ResolvedMmprojPath = ""
if ($MmprojFilename) {
    $CandidateMmproj = Join-Path $ModelsDir (Split-Path -Leaf $MmprojFilename)
    if (Test-Path $CandidateMmproj) {
        $ResolvedMmprojPath = $CandidateMmproj
    }
} else {
    $DiscoveredMmproj = Get-ChildItem -Path $ModelsDir -Filter "mmproj*.gguf" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($DiscoveredMmproj) {
        $ResolvedMmprojPath = $DiscoveredMmproj.FullName
    }
}

$CfgRoot   = To-ConfigPath $RuntimeRoot $RepoRoot
$CfgBin    = To-ConfigPath $ResolvedServerBin $RepoRoot
$CfgModel  = To-ConfigPath $ResolvedModelPath $RepoRoot
$CfgMmproj = if ($ResolvedMmprojPath) { To-ConfigPath $ResolvedMmprojPath $RepoRoot } else { "" }

$ConfigContent = @"
[runtime]
root_dir = "$CfgRoot"
llama_server_bin = "$CfgBin"
model_path = "$CfgModel"
mmproj_path = "$CfgMmproj"

[llm]
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
Write-Host "  Setup Summary" -ForegroundColor Green
Write-Host "  llama-server.exe : $(if (Test-Path $ResolvedServerBin) { 'READY' } else { 'MISSING (see build warnings above)' })"
Write-Host "  Bonsai Q2_0 GGUF : $(if (Test-Path $ResolvedModelPath) { 'READY' } else { 'MISSING' })"
Write-Host "  Vision mmproj    : $(if ($ResolvedMmprojPath -and (Test-Path $ResolvedMmprojPath)) { $ResolvedMmprojPath } else { 'NOT FOUND' })"
Write-Host "==================================================================" -ForegroundColor Green

if ($MissingBuildTools.Count -gt 0 -and -not (Test-Path $ResolvedServerBin)) {
    exit 3
}

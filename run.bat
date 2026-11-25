@echo off
setlocal EnableExtensions
cd /d "%~dp0"
title AI Desktop Companion (Rust)
echo ========================================================
echo       Launching AI Desktop Companion (Pure Rust)
echo       Transparent Overlay ^| Kokoro TTS ^| Whisper STT
echo ========================================================

REM Dynamically locate optional audio backend strictly inside this folder
set "BACKEND_DIR="
if exist "%~dp0backend\convene_server.py" set "BACKEND_DIR=%~dp0backend"
if exist "%~dp0Backend\convene_server.py" set "BACKEND_DIR=%~dp0Backend"
if exist "%~dp0runtime\backend\convene_server.py" set "BACKEND_DIR=%~dp0runtime\backend"
if exist "%~dp0runtime\Backend\convene_server.py" set "BACKEND_DIR=%~dp0runtime\Backend"

REM Check if Kokoro TTS & Whisper backend is already running on port 8000
netstat -aon 2>nul | findstr ":8000" >nul
if not errorlevel 1 goto :backend_already_running

if not defined BACKEND_DIR (
    echo [Info] Local audio backend not bundled; launching in standalone companion mode.
    goto :launch_companion
)

echo [System] Starting Kokoro TTS + Faster-Whisper backend service...
set "PYTHON_EXE="
if exist "%BACKEND_DIR%\python_env\python.exe" set "PYTHON_EXE=%BACKEND_DIR%\python_env\python.exe"
if exist "%BACKEND_DIR%\.venv\Scripts\python.exe" set "PYTHON_EXE=%BACKEND_DIR%\.venv\Scripts\python.exe"
if not defined PYTHON_EXE (
    where python >nul 2>nul
    if not errorlevel 1 set "PYTHON_EXE=python"
)

if not defined PYTHON_EXE goto :no_python

start "Convene_Backend" /d "%BACKEND_DIR%" /min "%PYTHON_EXE%" convene_server.py
echo [System] Initializing neural voice ^& speech models (Kokoro + Whisper)...
ping 127.0.0.1 -n 3 >nul
goto :launch_companion

:no_python
echo [Warning] Python executable not found in %BACKEND_DIR%; companion will run without Python audio service.
goto :launch_companion

:backend_already_running
echo [System] Kokoro TTS ^& Whisper STT service is already active on port 8000.

:launch_companion
echo.
echo [System] Launching Desktop Companion overlay...
echo [System] Console window will stay open to display live logs.
echo ========================================================
echo.

if exist "%~dp0local_ai_companion.exe" goto :run_local_exe
if exist "%~dp0target\release\local_ai_companion.exe" goto :run_release
if exist "%~dp0target\debug\local_ai_companion.exe" goto :run_debug
where cargo >nul 2>nul
if not errorlevel 1 (
    cargo run --release
    goto :after_run
)
echo [Error] Could not find local_ai_companion.exe or cargo on PATH.
exit /b 1

:run_local_exe
"%~dp0local_ai_companion.exe"
goto :after_run

:run_release
"%~dp0target\release\local_ai_companion.exe"
goto :after_run

:run_debug
"%~dp0target\debug\local_ai_companion.exe"
goto :after_run

:after_run
set "EXIT_CODE=%errorlevel%"
echo.
echo ========================================================
echo [System] Desktop Companion session ended (Exit code: %EXIT_CODE%).
echo [System] Press any key to close this terminal...
echo ========================================================
pause >nul

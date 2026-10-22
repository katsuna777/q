# Build Otpuska.exe for Windows into dist\Otpuska.
# Needs Rust (https://rustup.rs) with the MSVC toolchain and the "Desktop development with C++" build tools.
#   powershell -ExecutionPolicy Bypass -File packaging\windows\build.ps1
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..\..")
cargo build --release -p otpuska-gui
New-Item -ItemType Directory -Force dist\Otpuska | Out-Null
Copy-Item target\release\otpuska.exe dist\Otpuska\Otpuska.exe -Force
Write-Host "built dist\Otpuska\Otpuska.exe"

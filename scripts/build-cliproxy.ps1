#requires -Version 5.1
<#
.SYNOPSIS
  Build the CLIProxyAPI sidecar for the Tauri app on Windows.

.DESCRIPTION
  Compiles docs/CLIProxyAPI/cmd/server into src-tauri/binaries/cliproxy-<triple>.exe
  using the Tauri externalBin naming convention. Pass a Rust target triple as
  the first argument to cross-compile (e.g. x86_64-pc-windows-msvc).
#>
[CmdletBinding()]
param(
  [string]$TargetTriple
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RootDir = Split-Path -Parent $ScriptDir
$SourceDir = Join-Path $RootDir "docs/CLIProxyAPI"
$OutputDir = Join-Path $RootDir "src-tauri/binaries"
$BaseName = "cliproxy"

if (-not (Get-Command go -ErrorAction SilentlyContinue)) {
  Write-Error "Go toolchain not found. Install from https://go.dev/dl/."
  exit 1
}

if (-not (Test-Path $SourceDir)) {
  Write-Error "Source not found at $SourceDir. Re-clone with: git clone --depth 1 https://github.com/router-for-me/CLIProxyAPI.git docs/CLIProxyAPI"
  exit 1
}

if (-not (Test-Path $OutputDir)) {
  New-Item -ItemType Directory -Path $OutputDir | Out-Null
}

if (-not $TargetTriple -or $TargetTriple.Length -eq 0) {
  $arch = $env:PROCESSOR_ARCHITECTURE
  switch ($arch) {
    "AMD64" { $TargetTriple = "x86_64-pc-windows-msvc" }
    "ARM64" { $TargetTriple = "aarch64-pc-windows-msvc" }
    default { Write-Error "Unsupported PROCESSOR_ARCHITECTURE=$arch. Pass a target triple explicitly."; exit 1 }
  }
}

switch -Wildcard ($TargetTriple) {
  "*x86_64*windows*"   { $goos = "windows"; $goarch = "amd64" }
  "*aarch64*windows*"  { $goos = "windows"; $goarch = "arm64" }
  default {
    Write-Error "Unsupported target triple: $TargetTriple"
    exit 1
  }
}

$ext = ".exe"
$OutputPath = Join-Path $OutputDir ("$BaseName-$TargetTriple$ext")

Write-Host "[build-cliproxy] target=$TargetTriple  go=$goos/$goarch"
Write-Host "[build-cliproxy] output=$OutputPath"

Push-Location $SourceDir
try {
  $env:CGO_ENABLED = "0"
  $env:GOOS = $goos
  $env:GOARCH = $goarch
  & go build -trimpath -ldflags "-s -w" -o $OutputPath ./cmd/server
  if ($LASTEXITCODE -ne 0) {
    Write-Error "go build failed with exit code $LASTEXITCODE"
    exit $LASTEXITCODE
  }
}
finally {
  Pop-Location
}

Write-Host "[build-cliproxy] done."

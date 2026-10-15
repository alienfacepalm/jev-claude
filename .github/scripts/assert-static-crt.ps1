# Fails unless every one of the seven Windows programs in -Dir links the C runtime statically.
# rust/.cargo/config.toml asks for +crt-static so the programs start on a machine without the
# Visual C++ Redistributable. The CI runners have the Redistributable installed, so a dynamic build
# would pass every other check there and then fail on a bare user machine with "vcruntime140.dll
# was not found". A RUSTFLAGS environment variable replaces the config file's rustflags rather than
# adding to them, so setting one anywhere in CI quietly drops crt-static; this catches that.
#
#   pwsh .github/scripts/assert-static-crt.ps1 -Dir rust/target/release
#
# -Dumpbin names the dumpbin to run; by default it is found with vswhere, which every Windows
# runner has.
param(
  [Parameter(Mandatory = $true)][string]$Dir,
  [string]$Dumpbin
)
$ErrorActionPreference = 'Stop'

if (-not $Dumpbin) {
  $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
  if (-not (Test-Path $vswhere)) { throw "vswhere.exe was not found at $vswhere" }
  $Dumpbin = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
    -find 'VC\Tools\MSVC\**\bin\Hostx64\x64\dumpbin.exe' | Select-Object -First 1
  if (-not $Dumpbin) { throw 'dumpbin.exe was not found (vswhere found no MSVC x64 tools)' }
}

$programs = 'jev-claude', 'jev-statusline', 'jev-explain', 'jev-legend', 'jev-check', 'jev-update-check', 'jev-proxy-host'
$bad = @()
foreach ($name in $programs) {
  $exe = Join-Path $Dir "$name.exe"
  if (-not (Test-Path $exe)) { throw "$exe does not exist" }
  $deps = & $Dumpbin /nologo /dependents $exe
  if ($LASTEXITCODE -ne 0) { throw "dumpbin failed on $exe" }
  # The dynamic C runtime: vcruntime140.dll, msvcp140.dll, ucrtbase.dll and the api-ms-win-crt-*
  # forwarders. A static build imports only system DLLs (KERNEL32, ntdll, bcryptprimitives,
  # ws2_32, advapi32, api-ms-win-core-*, ...).
  $hits = @($deps | Select-String -Pattern 'vcruntime|msvcp|ucrtbase|api-ms-win-crt' | ForEach-Object { $_.Line.Trim() })
  if ($hits.Count -gt 0) {
    $bad += "$name.exe imports $($hits -join ', ')"
  } else {
    Write-Host "static C runtime: $name.exe"
  }
}
if ($bad.Count -gt 0) {
  Write-Host "::error::The C runtime is linked dynamically (crt-static was lost; is RUSTFLAGS overriding rust/.cargo/config.toml?)"
  $bad | ForEach-Object { Write-Host "  $_" }
  exit 1
}

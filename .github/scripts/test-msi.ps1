# Installs the Windows installer (MSI) for real, checks the install, and uninstalls it:
#
#   pwsh .github/scripts/test-msi.ps1 -Msi dist/jev-claude-windows-x64.msi
#
# This changes the machine it runs on (files under %LocalAppData%\Programs\jev-claude and the user PATH in
# HKCU), and puts both back on the way out. Run it on a CI runner or a throwaway machine, not on one with
# a jev-claude install you want to keep: that install's folder is the one this MSI uses.
#
# Checks: install exits 0; the seven programs, the skills and package.json are in place; the user PATH
# (read from the registry, as a new terminal would) names the bin folder; a program runs from there;
# uninstall exits 0 and leaves neither the folder nor the PATH entry behind.
param([Parameter(Mandatory = $true)][string]$Msi)
$ErrorActionPreference = 'Stop'
$Msi = (Resolve-Path $Msi).Path
$installDir = Join-Path $env:LOCALAPPDATA 'Programs\jev-claude'
$binDir = Join-Path $installDir 'bin'
$log = Join-Path ([IO.Path]::GetTempPath()) 'jev-msi-test.log'
$programs = 'jev-claude', 'jev-statusline', 'jev-explain', 'jev-legend', 'jev-check', 'jev-update-check', 'jev-proxy-host'

if (Test-Path -LiteralPath $installDir) { throw "$installDir already exists; refusing to touch an existing install" }

function Get-UserPath {
  $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
  try { [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) } finally { $key.Close() }
}
function Test-OnPath { (Get-UserPath) -split ';' | Where-Object { $_.TrimEnd('\') -ieq $binDir } }
function Invoke-Msiexec([string[]]$arguments) {
  $process = Start-Process msiexec -ArgumentList ($arguments + '/qn', '/norestart', '/l*v', "`"$log`"") -Wait -PassThru
  $process.ExitCode
}
function Assert([bool]$condition, [string]$message) { if (-not $condition) { throw "FAIL: $message" } else { Write-Host "ok    $message" } }

$installed = $false
try {
  Assert (-not (Test-OnPath)) 'the bin folder is not on the user PATH before the install'

  $code = Invoke-Msiexec @('/i', "`"$Msi`"")
  $installed = $true
  Assert ($code -eq 0) "msiexec /i exits 0 (got $code)"

  foreach ($program in $programs) { Assert (Test-Path -LiteralPath (Join-Path $binDir "$program.exe") -PathType Leaf) "$program.exe is installed" }
  Assert (Test-Path -LiteralPath (Join-Path $installDir '.claude\skills\jev-calibrate\SKILL.md') -PathType Leaf) 'the skills are installed (.claude\skills)'
  $version = (Get-Content -LiteralPath (Join-Path $installDir 'package.json') -Raw | ConvertFrom-Json).version
  Assert ($version -match '^\d+\.\d+\.\d+$') "package.json carries the release version ($version)"
  Assert ([bool](Test-OnPath)) 'the bin folder is on the user PATH'

  $output = & (Join-Path $binDir 'jev-legend.exe') 2>&1 | Out-String
  Assert (($LASTEXITCODE -eq 0) -and ($output -match 'Status line key')) 'an installed program runs'

  # Installing the same package again repairs it rather than failing or doubling the PATH entry.
  $code = Invoke-Msiexec @('/i', "`"$Msi`"")
  Assert ($code -eq 0) "msiexec /i again exits 0 (got $code)"
  $entries = @((Get-UserPath) -split ';' | Where-Object { $_.TrimEnd('\') -ieq $binDir }).Count
  Assert ($entries -eq 1) "the PATH names the bin folder once after a second install (found $entries)"

  $code = Invoke-Msiexec @('/x', "`"$Msi`"")
  $installed = $false
  Assert ($code -eq 0) "msiexec /x exits 0 (got $code)"
  Assert (-not (Test-Path -LiteralPath $installDir)) 'the install folder is gone after uninstall'
  Assert (-not (Test-OnPath)) 'the bin folder is off the user PATH after uninstall'
} catch {
  if (Test-Path -LiteralPath $log) { Write-Host '--- last 60 lines of the msiexec log ---'; Get-Content -LiteralPath $log -Tail 60 }
  throw
} finally {
  if ($installed) { Invoke-Msiexec @('/x', "`"$Msi`"") | Out-Null }
}
Write-Host 'the MSI installs, runs and uninstalls cleanly'

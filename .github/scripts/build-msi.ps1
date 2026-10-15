# Builds the Windows installer (MSI) from the release archive, so it holds exactly what was checked and
# what the other installers install:
#
#   pwsh .github/scripts/build-msi.ps1 -Archive dist/jev-claude-windows-x64.tar.gz -Out dist
#
# Writes <Out>/jev-claude-windows-x64.msi. The version comes from the archive's package.json. WiX is
# installed into a throwaway folder with `dotnet tool install --tool-path`, so nothing global changes;
# its UI extension is cached under ~/.wix by `wix extension add`. -Wix names a wix.exe to use instead.
param(
  [Parameter(Mandatory = $true)][string]$Archive,
  [Parameter(Mandatory = $true)][string]$Out,
  [string]$Wix
)
$ErrorActionPreference = 'Stop'
$WixVersion = '5.0.2'
$repo = Split-Path (Split-Path $PSScriptRoot)
$work = Join-Path ([IO.Path]::GetTempPath()) ("jev-msi-" + [guid]::NewGuid().ToString('N'))
$stage = Join-Path $work 'stage'
New-Item -ItemType Directory -Path $stage -Force | Out-Null
try {
  # Windows' own tar: a GNU tar earlier on PATH (Git for Windows) reads "C:\..." as a remote host.
  & (Join-Path $env:SystemRoot 'System32\tar.exe') -xzf (Resolve-Path $Archive).Path -C $stage
  if ($LASTEXITCODE -ne 0) { throw "could not unpack $Archive" }
  $version = (Get-Content -LiteralPath (Join-Path $stage 'package.json') -Raw | ConvertFrom-Json).version
  if ($version -notmatch '^\d+\.\d+\.\d+$') { throw "the archive's version '$version' is not x.y.z" }

  if (-not $Wix) {
    $tools = Join-Path $work 'tools'
    dotnet tool install wix --version $WixVersion --tool-path $tools | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'could not install WiX' }
    $Wix = Join-Path $tools 'wix.exe'
  }
  & $Wix extension add -g "WixToolset.UI.wixext/$WixVersion" | Out-Null
  if ($LASTEXITCODE -ne 0) { throw 'could not add the WiX UI extension' }

  # The licence page shows RTF. LICENSE is plain ASCII text; a backslash or brace would need escaping.
  $text = (Get-Content -LiteralPath (Join-Path $repo 'LICENSE') -Raw) -replace '([\{}])', '\$1'
  $rtf = '{\rtf1\ansi\deff0{\fonttbl{\f0 Segoe UI;}}\f0\fs20 ' + (($text -replace "`r?`n", "\par`n")) + '}'
  $licenseRtf = Join-Path $work 'license.rtf'
  [IO.File]::WriteAllText($licenseRtf, $rtf, [Text.Encoding]::ASCII)

  New-Item -ItemType Directory -Path $Out -Force | Out-Null
  $msi = Join-Path (Resolve-Path $Out) 'jev-claude-windows-x64.msi'
  & $Wix build (Join-Path $repo 'installer\windows\jev-claude.wxs') -arch x64 -ext WixToolset.UI.wixext `
    -d "Version=$version" -d "StageDir=$stage" -d "License=$licenseRtf" -o $msi
  if ($LASTEXITCODE -ne 0) { throw 'wix build failed' }
  Write-Host "built $msi (version $version)"
} finally {
  Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}

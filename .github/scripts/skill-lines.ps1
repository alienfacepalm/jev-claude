# Runs the skills' command lines the way Claude Code's PowerShell tool runs them on native Windows:
# each `!`<program> ...`` line of .claude/skills/*/SKILL.md (read from the files, so this follows
# any change to them) handed to Windows PowerShell 5.1 and to PowerShell 7 with -Command. The
# skills rely on two things there, and this checks both:
#   - with the program on PATH, the line exits 0 and prints the program's output;
#   - with it missing, the line still exits 0 (thanks to the trailing `echo`) and says the program
#     "is not recognized", which is the text the skills tell the agent to look for.
# It also reports, without failing, whether jev-legend's non-ASCII output (the symbol icon set)
# comes through each PowerShell byte for byte, or is re-encoded through [Console]::OutputEncoding.
#
#   pwsh .github/scripts/skill-lines.ps1 -Bin built [-Root <repository>]
#
# Run it under PowerShell 7 (it starts both shells as child processes). On macOS and Linux only
# pwsh is tried, which is how this script is checked locally.
param(
  [Parameter(Mandatory = $true)][string]$Bin,
  [string]$Root = (Join-Path $PSScriptRoot '../..')
)
$ErrorActionPreference = 'Stop'

$onWindows = [System.Environment]::OSVersion.Platform -eq 'Win32NT'
$exe = if ($onWindows) { '.exe' } else { '' }
$bin = (Resolve-Path $Bin).Path
$legend = Join-Path $bin "jev-legend$exe"
if (-not (Test-Path $legend)) { throw "$legend does not exist" }

$scratchHome = Join-Path ([System.IO.Path]::GetTempPath()) ("jev-skill-lines-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $scratchHome | Out-Null

$sep = [System.IO.Path]::PathSeparator
$entries = @($env:PATH -split [regex]::Escape($sep) | Where-Object { $_ })
$withBin = (@($bin) + $entries) -join $sep
# Off PATH: every folder holding one of the skills' programs (an installed copy included) is dropped.
$withoutBin = @($entries | Where-Object {
    $dir = $_
    -not (@('jev-legend', 'jev-check', 'jev-explain') | Where-Object { Test-Path (Join-Path $dir "$_$exe") })
  }) -join $sep

# Starts a program with captured output. The bytes are kept exactly as written (Latin-1 maps each
# byte to one character), so nothing here re-encodes them.
function Invoke-Captured([string]$File, [string[]]$Arguments, [string]$PathValue, [string[]]$Remove) {
  $psi = [System.Diagnostics.ProcessStartInfo]::new($File)
  foreach ($a in $Arguments) { $psi.ArgumentList.Add($a) }
  $psi.UseShellExecute = $false
  $psi.RedirectStandardInput = $true
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $psi.StandardOutputEncoding = [System.Text.Encoding]::Latin1
  $psi.StandardErrorEncoding = [System.Text.Encoding]::Latin1
  $psi.Environment['PATH'] = $PathValue
  $psi.Environment['HOME'] = $scratchHome
  $psi.Environment['USERPROFILE'] = $scratchHome
  $psi.Environment['JEV_ICONS'] = 'symbols'
  foreach ($name in @('JEV_ROOT', 'JEV_API_KEY', 'ANTHROPIC_API_KEY') + $Remove) { [void]$psi.Environment.Remove($name) }
  $process = [System.Diagnostics.Process]::Start($psi)
  $process.StandardInput.Close()
  $stderr = $process.StandardError.ReadToEndAsync()
  $stdout = $process.StandardOutput.ReadToEnd()
  $process.WaitForExit()
  $latin1 = [System.Text.Encoding]::Latin1
  [pscustomobject]@{
    Code = $process.ExitCode
    Out  = $latin1.GetBytes($stdout)
    Err  = $latin1.GetBytes($stderr.GetAwaiter().GetResult())
  }
}

# The text of a run, with PowerShell's CRLF line ends made LF and the trailing blank lines (the
# `echo`) dropped, for comparing with the program run directly.
function ConvertTo-RunText([byte[]]$Bytes) {
  [System.Text.Encoding]::UTF8.GetString($Bytes).Replace("`r`n", "`n").TrimEnd("`r", "`n")
}

$failures = [System.Collections.Generic.List[string]]::new()
function Test-That([bool]$Ok, [string]$What) {
  Write-Host ("{0}  {1}" -f $(if ($Ok) { '  pass' } else { '  FAIL' }), $What)
  if (-not $Ok) { $failures.Add($What) }
}

$direct = Invoke-Captured $legend @() $withBin @()
Test-That ($direct.Code -eq 0) "jev-legend run directly exits 0 (got $($direct.Code))"
$expected = ConvertTo-RunText $direct.Out

$shells = @()
if ($onWindows) { $shells += [pscustomobject]@{ Name = 'Windows PowerShell 5.1'; File = 'powershell.exe'; Remove = @('PSModulePath') } }
$shells += [pscustomobject]@{ Name = 'PowerShell 7'; File = 'pwsh'; Remove = @() }

# The skills' lines: `!`jev-check ; echo``. Claude Code fills in ${CLAUDE_SESSION_ID} before
# running a line, so a made-up id stands in for it here.
$lines = foreach ($skill in 'jev-legend', 'jev-calibrate', 'jev-explain') {
  $file = Join-Path $Root ".claude/skills/$skill/SKILL.md"
  $found = @(Get-Content -LiteralPath $file | ForEach-Object { if ($_ -match '^!`(.+)`$') { $Matches[1] } })
  if ($found.Count -ne 1) { throw "$file must hold exactly one command line to run (found $($found.Count))" }
  $line = $found[0].Replace('${CLAUDE_SESSION_ID}', 'skill-line-check')
  [pscustomobject]@{ Skill = $skill; Program = ($line -split ' ')[0]; Line = $line }
}

foreach ($shell in $shells) {
  Write-Host "`n$($shell.Name)"
  foreach ($entry in $lines) {
    $line = $entry.Line
    $run = Invoke-Captured $shell.File @('-NoProfile', '-Command', $line) $withBin $shell.Remove
    $text = ConvertTo-RunText $run.Out
    Test-That ($run.Code -eq 0) "/$($entry.Skill): ``$line`` exits 0 (got $($run.Code))"
    if ($run.Code -ne 0) { Write-Host "stdout: $text`nstderr: $(ConvertTo-RunText $run.Err)" }
    if ($entry.Program -eq 'jev-legend') {
      Test-That ($text.Contains('Status line key')) "/$($entry.Skill): ``$line`` prints the key's header"
      # Reported, not checked: what Claude Code's own PowerShell tool does with the output is not
      # reproduced here exactly, so a difference is information for the skills, not a failure.
      if ($text -ceq $expected) {
        Write-Host "  note  the legend's non-ASCII text comes through byte for byte"
      } else {
        Write-Host "::notice::$($shell.Name) changes jev-legend's output (non-ASCII re-encoded through [Console]::OutputEncoding?)"
        Write-Host "  direct:  $expected"
        Write-Host "  through: $text"
      }
    }

    $missing = Invoke-Captured $shell.File @('-NoProfile', '-Command', $line) $withoutBin $shell.Remove
    $said = (ConvertTo-RunText ([byte[]]($missing.Out + $missing.Err))).ToLowerInvariant()
    Test-That ($missing.Code -eq 0) "/$($entry.Skill): with $($entry.Program) missing, the line still exits 0 (got $($missing.Code))"
    Test-That ($said.Contains('not recognized')) "/$($entry.Skill): with $($entry.Program) missing, it says the program is not recognized"
    if ($missing.Code -ne 0 -or -not $said.Contains('not recognized')) { Write-Host "output: $said" }
  }
}

Remove-Item -Recurse -Force $scratchHome -ErrorAction SilentlyContinue
if ($failures.Count -gt 0) {
  Write-Host "`n$($failures.Count) check(s) failed"
  exit 1
}
Write-Host "`nall skill-line checks passed"

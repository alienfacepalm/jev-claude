# Installs or updates jev-claude (a fork of jev-router, for Claude Code) on Windows PowerShell 5.1 or PowerShell 7:
#
#   irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 | iex
#
# From Command Prompt (cmd.exe), the same script runs through Windows PowerShell; the double quotes keep
# cmd.exe from reading the | itself:
#
#   powershell -NoProfile -ExecutionPolicy Bypass -Command "irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 | iex"
#
# To answer yes to every question, set the variable first ($env:JEV_ASSUME_YES = '1' in PowerShell,
# `set JEV_ASSUME_YES=1` on its own line in cmd.exe). Or run .\install.ps1 from a clone. Running it again
# updates the install.
#
# It installs ready-made native programs from the latest release: no Node.js, no pnpm, no git clone and
# no Rust toolchain. Claude Code is what jev-claude launches, so the installer checks for it first and,
# after asking, installs it with Anthropic's own installer. The optional git is offered through winget.
# Every change to your system shows its exact command first and waits for a yes; with nobody to ask (no
# terminal, or a Claude Code session), it prints the command instead, unless JEV_ASSUME_YES=1.
#
# macOS, Linux, WSL and Git Bash use install.sh, which behaves the same way:
#
#   curl -fsSL https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.sh | bash
#
# Settings are environment variables, so they work through `iex` as well as from a file:
#
#   $env:JEV_INSTALL_DIR      where the programs go (default ~\jev-claude-rust)
#   $env:JEV_VERSION          a release tag such as v0.11.0 (default: the latest release)
#   $env:JEV_RELEASE_URL      where releases are downloaded from: an https:// address or a file:// folder on
#                             disk (default this repository's GitHub releases)
#   $env:JEV_NO_MODIFY_PATH   1 to leave your user PATH alone; the installer then prints the folders to add
#   $env:JEV_INSTALL_CLAUDE   1 to install Claude Code when it is missing without asking, 0 never to (the
#                             install then stops, because jev-claude cannot run without it)
#   $env:JEV_ASSUME_YES       1 to answer yes to every prerequisite question
#   $env:JEV_API_KEY          your Jev key, to save it without the prompt
#   $env:ANTHROPIC_API_KEY    an Anthropic API key to run Claude Code on, to save it without the prompt
#
# Run from a file, -Yes does what JEV_ASSUME_YES=1 does, and -Help prints these settings. The old -Rust and
# -Node options and JEV_IMPL are accepted and ignored: this is the only edition. (JEV_BIN_DIR is for
# install.sh, which links the programs into a common folder; Windows puts the install's own bin folder on
# your user PATH instead.)
#
# Yes/no settings (JEV_ASSUME_YES, JEV_INSTALL_CLAUDE, JEV_NO_MODIFY_PATH) take 1, true or yes for "on"
# and 0, false or no for "off". JEV_INSTALL_DIR may be relative; it is made absolute (against the folder
# you run this from) before anything is written.
#
# No param() block: through `iex` the script is a string and has no parameters, so the options are read
# from $args, which is empty there. Errors are thrown rather than exited on: run through `iex`, `exit`
# would close the window.
#
# Test-only settings, for the installer smoke test (they redirect what the installer would change on a
# real machine; leave them unset):
#
#   $env:JEV_CLAUDE_INSTALL_BASE  where Anthropic's installer is fetched from (default https://claude.ai)
#   $env:JEV_TEST_USER_ENV_KEY    a registry subkey of HKCU to use instead of "Environment" as the user
#                                 PATH; running programs are not notified of a change to it
& {
  $ErrorActionPreference = 'Stop'
  # A caller's $PSNativeCommandUseErrorActionPreference (PowerShell 7.3+) reaches this script through
  # `iex`; it would turn a non-zero exit of tar, jev-check or the Claude Code installer into an exception
  # that skips the handling and the clean-up below.
  $PSNativeCommandUseErrorActionPreference = $false
  $options = @($args | Where-Object { $null -ne $_ })
  # This script's own file, when it was run as one ($null through `iex`); -Help reads its header.
  $selfPath = $PSCommandPath
  # The profile folder, as the installed programs find it: USERPROFILE on Windows, then HOME.
  $userHome = if ($env:USERPROFILE) { $env:USERPROFILE } else { $HOME }
  $envFile = Join-Path $userHome '.jev-router.env'

  function Say([string]$message) { Write-Host "[jev] $message" }
  function Have([string]$command) { [bool](Get-Command $command -ErrorAction SilentlyContinue) }
  # Does this setting say yes (1, true, yes) or no (0, false, no), in any letter case?
  function Test-FlagOn([string]$value) { $value.Trim() -match '^(1|true|yes)$' }
  function Test-FlagOff([string]$value) { $value.Trim() -match '^(0|false|no)$' }
  # A URL without any user:password@ part, for printing (a private mirror's address can carry one).
  function Hide-UrlSecret([string]$url) { $url -replace '://[^/@]*@', '://' }

  $releaseBase = if ($env:JEV_RELEASE_URL) { $env:JEV_RELEASE_URL.TrimEnd('/') } else { 'https://github.com/alienfacepalm/jev-claude/releases' }
  # TEST ONLY: JEV_CLAUDE_INSTALL_BASE moves Anthropic's installer to a stand-in (same name as install.sh's).
  $claudeBase = if ($env:JEV_CLAUDE_INSTALL_BASE) { $env:JEV_CLAUDE_INSTALL_BASE.TrimEnd('/') } else { 'https://claude.ai' }
  $claudeInstallPs1 = "$claudeBase/install.ps1"
  # TEST ONLY: JEV_TEST_USER_ENV_KEY is a subkey of HKCU standing in for "Environment", the user PATH.
  $userEnvKey = if ($env:JEV_TEST_USER_ENV_KEY) { $env:JEV_TEST_USER_ENV_KEY } else { 'Environment' }
  $claudeCommand = "irm $claudeInstallPs1 | iex"
  $claudeSetupDoc = 'https://code.claude.com/docs/en/setup'
  $installShUrl = 'https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.sh'
  $sourceBuildDoc = 'https://github.com/alienfacepalm/jev-claude/blob/master/doc/RUST.md#install-the-toolchain'
  $programs = @('jev-claude', 'jev-statusline', 'jev-explain', 'jev-legend', 'jev-check', 'jev-update-check', 'jev-proxy-host')
  # Where Anthropic's installer puts claude.exe.
  $claudeLocal = Join-Path (Join-Path $userHome '.local') 'bin'

  # One line of the readiness report: status (ok, info, warn, FAIL), what was checked, what was found.
  function Check([string]$status, [string]$name, [string]$detail) {
    Write-Host ('[jev]   {0,-5} {1,-18} {2}' -f $status, $name, $detail)
  }
  # The SHA-256 of a file, in lower-case hex, without depending on the Get-FileHash module.
  function Sha256-Of([string]$path) {
    $sha = [Security.Cryptography.SHA256]::Create()
    $stream = [IO.File]::OpenRead($path)
    try { ([BitConverter]::ToString($sha.ComputeHash($stream)) -replace '-', '').ToLower() } finally { $stream.Close(); $sha.Dispose() }
  }
  function Version-In([string]$folder) {
    try { (Get-Content -LiteralPath (Join-Path $folder 'package.json') -Raw | ConvertFrom-Json).version } catch { $null }
  }
  # Is this folder a jev-claude install, which this installer may replace? Its package.json names
  # jev-router and its bin folder holds jev-claude.exe. Any other folder is never moved or deleted.
  function Test-JevInstall([string]$folder) {
    if (-not (Test-Path -LiteralPath (Join-Path $folder 'bin\jev-claude.exe') -PathType Leaf)) { return $false }
    try { return ((Get-Content -LiteralPath (Join-Path $folder 'package.json') -Raw | ConvertFrom-Json).name -eq 'jev-router') } catch { return $false }
  }
  # The HTTP status behind a failed request: 0 when nothing answered.
  function Http-Status($errorRecord) {
    $response = $errorRecord.Exception.Response
    if ($response) { [int]$response.StatusCode } else { 0 }
  }
  # Downloads a file, trying three times when the connection fails or the server errs. Returns the HTTP
  # status of the last attempt: 200 when the file arrived, 0 when nothing answered, -1 when the transfer
  # started and then broke off (a partial file with no HTTP error). A file:// address is copied, for a
  # mirror on disk (404 when it is not there), as install.sh does.
  function Get-File([string]$url, [string]$out) {
    if ($url -match '^file://') {
      try { Copy-Item -LiteralPath ([Uri]$url).LocalPath -Destination $out -ErrorAction Stop; return 200 } catch { return 404 }
    }
    $status = 0
    for ($attempt = 1; $attempt -le 3; $attempt++) {
      # A partial file from the last attempt must not make a plain connection failure look like an interruption.
      Remove-Item -LiteralPath $out -Force -ErrorAction SilentlyContinue
      try {
        $response = Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $out -PassThru
        # Windows PowerShell 5.1 does not complain when the connection closes before the body is whole, so
        # the length the server announced is compared with what arrived (unless the body was compressed in
        # transit, when the two differ by design).
        $announced = 0L
        if ($response -and $response.Headers -and -not $response.Headers['Content-Encoding']) {
          [void][long]::TryParse(("$(@($response.Headers['Content-Length'])[0])"), [ref]$announced)
        }
        if ($announced -gt 0 -and (Get-Item -LiteralPath $out).Length -ne $announced) {
          $status = -1
        } else {
          return 200
        }
      } catch {
        $status = Http-Status $_
        if ($status -ge 400 -and $status -lt 500) { return $status }
        # No HTTP answer, yet bytes arrived: the server began the download and the connection broke off.
        # (Not keyed on the exception: its type and wording differ between Windows PowerShell and 7.)
        if ($status -eq 0 -and (Test-Path -LiteralPath $out -PathType Leaf) -and (Get-Item -LiteralPath $out).Length -gt 0) { $status = -1 }
      }
      if ($attempt -lt 3) { Start-Sleep -Seconds 2 }
    }
    return $status
  }
  # Is there a person to ask? False inside a Claude Code session (CLAUDECODE is set), where an agent, not
  # a person, would be answering. The ISE and the VS Code terminal prompt in their own window, so they
  # count even though they have no console, and VS Code starts its shell with -NonInteractive. A console
  # is asked unless its input is piped or redirected or it was started with -NonInteractive; any other
  # host (a remote session, a runspace) is not asked.
  function Test-Interactive {
    if ($env:CLAUDECODE) { return $false }
    try {
      if (-not [Environment]::UserInteractive) { return $false }
      switch ($Host.Name) {
        'Windows PowerShell ISE Host' { return $true }
        'Visual Studio Code Host' { return $true }
        'ConsoleHost' {
          if ([Console]::IsInputRedirected) { return $false }
          if (@([Environment]::GetCommandLineArgs() | Where-Object { $_ -match '^(-{1,2}|/)noni' }).Count) { return $false }
          return $true
        }
      }
      return $false
    } catch { return $false }
  }
  # Asks a yes/no question. JEV_ASSUME_YES (or -Yes) answers yes; with nobody to ask the answer is no.
  function Ask-YesNo([string]$question) {
    if ($assumeYes) { return $true }
    if (-not (Test-Interactive)) { return $false }
    try { return [bool]((Read-Host "[jev] $question") -match '^[Yy]') } catch { return $false }
  }
  # Tells Explorer, and so every terminal opened from now on, that the environment changed. Windows
  # waits for each open window to answer, so a window that hangs is skipped after two seconds
  # (SMTO_ABORTIFHUNG) instead of holding the installer up, as [Environment]::SetEnvironmentVariable would.
  function Send-EnvironmentChange {
    if ($env:JEV_TEST_USER_ENV_KEY) { return }
    try {
      if (-not ('Jev.Native' -as [type])) {
        Add-Type -Namespace Jev -Name Native -MemberDefinition '[DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)] public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint msg, UIntPtr wParam, string lParam, uint flags, uint timeout, out UIntPtr result);'
      }
      $result = [UIntPtr]::Zero
      [void][Jev.Native]::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, 'Environment', 2, 2000, [ref]$result)
    } catch {
      Say '        (could not notify running programs; sign out and in, or open a new terminal from the Start menu, for the PATH change to apply everywhere)'
    }
  }
  # One PATH entry as Windows reads it, for comparing: %VAR% expanded, quotes, spaces and a trailing
  # separator dropped.
  function Get-PathEntryKey([string]$entry) {
    [Environment]::ExpandEnvironmentVariables($entry.Trim().Trim('"').Trim()).TrimEnd('\', '/')
  }
  # Is this folder on the PATH that new windows get (the system PATH, then the user PATH)?
  function Test-PersistentPath([string]$folder) {
    $want = Get-PathEntryKey $folder
    $raw = ''
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($userEnvKey)
    if ($key) {
      try { $raw = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) } finally { $key.Close() }
    }
    $machine = [string][Environment]::GetEnvironmentVariable('Path', 'Machine')
    return [bool](@("$machine;$raw" -split ';' | Where-Object { $_.Trim() -and (Get-PathEntryKey $_) -ieq $want }).Count)
  }
  # Puts a folder on the user's PATH, keeping every other entry as it was (%VAR% entries unexpanded), and
  # drops other spellings of the same folder. By default the folder goes first: returns 'added', 'moved'
  # (it was there, but behind another folder) or 'present' (it was already first). With -Append it goes
  # last, and an entry already there anywhere is left alone ('present'). Send-EnvironmentChange tells
  # running programs afterwards.
  function Add-UserPath([string]$folder, [switch]$Append) {
    $want = Get-PathEntryKey $folder
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($userEnvKey, $true)
    if (-not $key) { $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($userEnvKey) }
    try {
      $raw = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
      $parts = @($raw -split ';' | Where-Object { $_.Trim() })
      $rest = @($parts | Where-Object { (Get-PathEntryKey $_) -ine $want })
      if ($Append) {
        if ($rest.Count -lt $parts.Count) { return 'present' }
        $outcome = 'added'
        $updated = @($parts) + @($folder)
      } else {
        if ($parts.Count -gt 0 -and (Get-PathEntryKey $parts[0]) -ieq $want) { return 'present' }
        $outcome = if ($rest.Count -lt $parts.Count) { 'moved' } else { 'added' }
        $updated = @($folder) + $rest
      }
      $key.SetValue('Path', ($updated -join ';'), [Microsoft.Win32.RegistryValueKind]::ExpandString)
    } finally { $key.Close() }
    return $outcome
  }
  # Puts a folder first on this window's PATH, unless it is already there.
  function Add-SessionPath([string]$folder) {
    $want = Get-PathEntryKey $folder
    if (-not @(($env:Path -split ';') | Where-Object { $_.Trim() -and (Get-PathEntryKey $_) -ieq $want }).Count) { $env:Path = "$folder;$env:Path" }
  }
  # Where Claude Code is, or $null. Anthropic's installer puts claude.exe in ~\.local\bin, which a
  # terminal that has not been restarted may not list yet; that folder joins this run's PATH.
  function Find-Claude {
    $found = Get-Command claude -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($found -and $found.Source) { return $found.Source }
    $local = Join-Path $claudeLocal 'claude.exe'
    if (Test-Path -LiteralPath $local) {
      Add-SessionPath $claudeLocal
      return $local
    }
    return $null
  }
  # Runs `claude --version`: its first output line and whether it ran (exit 0 and a version number). A
  # program that is there but cannot start (a damaged or wrong-architecture file) is not Claude Code working.
  function Get-ClaudeVersion([string]$claude) {
    $ErrorActionPreference = 'Continue'
    $lines = @()
    $code = 0
    try {
      $lines = @(& $claude --version 2>&1 | ForEach-Object { "$_" })
      $code = $LASTEXITCODE
    } catch {
      $lines = @($_.Exception.Message)
      $code = 1
    }
    $first = [string]($lines | Select-Object -First 1)
    $version = ($first -split ' ' | Select-Object -First 1)
    [pscustomobject]@{ First = $first; Version = $version; Ran = (($code -eq 0 -or $null -eq $code) -and $version -match '^\d') }
  }
  # Does this claude need Node.js? Only when it runs a JavaScript file: the old npm package. npm's current
  # package installs the native binary, reached through a .cmd or .ps1 shim that names a .exe and does not
  # invoke Node. (The same rule as launch.rs's shim_script, which finds the script an npm shim runs.)
  function Test-ClaudeNeedsNode([string]$claude) {
    if ($claude -match '\.(js|mjs|cjs)$') { return $true }
    if ($claude -notmatch '\.(cmd|bat|ps1)$') { return $false }
    try { $text = Get-Content -LiteralPath $claude -Raw -ErrorAction Stop } catch { return $false }
    if (-not $text) { return $false }
    return [bool]($text -match '(?i)"%~?dp0%?\\[^"]+?\.[cm]?js"' -or $text -match '(?i)\$basedir[/\\][^"''\s]+?\.[cm]?js(?=["''\s]|$)')
  }
  # The Claude Code line of the report: native build or the npm package (a script run by Node.js).
  # Returns $false (after a FAIL row) when the program is there but `claude --version` does not run.
  function Write-ClaudeRow([string]$claude) {
    $needsNode = Test-ClaudeNeedsNode $claude
    if ($needsNode -and -not (Have node)) {
      # (without Node.js the npm package cannot be asked for its version)
      Check 'warn' 'Claude Code' 'installed is the npm package, which runs on Node.js, and Node.js is not installed'
      Say "        Reinstall Claude Code natively: $claudeCommand"
      return $true
    }
    $run = Get-ClaudeVersion $claude
    if (-not $run.Ran) {
      $first = if ($run.First.Length -gt 100) { $run.First.Substring(0, 100) } else { $run.First }
      $why = if ($first) { ": $first" } else { '' }
      Check 'FAIL' 'Claude Code' "found at $claude but it does not run (claude --version failed$why)"
      return $false
    }
    if ($needsNode) {
      Check 'ok' 'Claude Code' "$($run.Version) (npm package, runs on Node.js $(node --version))"
    } else {
      Check 'ok' 'Claude Code' "$($run.Version) (native build, needs no Node.js)"
    }
    return $true
  }
  # Is Claude Code in Anthropic's folder, and is that folder missing from the PATH new windows get?
  # Anthropic's Windows installer does not put it there, so jev-claude, which finds claude on the PATH,
  # would not find it from a new terminal.
  function Test-ClaudeFolderMissing([string]$claude) {
    if (-not $claude) { return $false }
    if ((Get-PathEntryKey (Split-Path -Parent $claude)) -ine (Get-PathEntryKey $claudeLocal)) { return $false }
    try { return -not (Test-PersistentPath $claudeLocal) } catch { return $false }
  }
  # The Windows build number. Windows PowerShell can be told an older version by [Environment]::OSVersion,
  # so the registry is asked first.
  function Get-WindowsBuild {
    try { [int](Get-ItemPropertyValue -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -Name 'CurrentBuildNumber') } catch { [Environment]::OSVersion.Version.Build }
  }
  # The shell that runs Anthropic's installer: a child, so its own `exit` or settings cannot close or
  # change this one. Windows PowerShell, which every Windows has, chosen by its full name and not as this
  # process: the ISE or VS Code host would open a window instead, and a Store PowerShell 7 may not be
  # started by its path. From a 32-bit shell on 64-bit Windows the 64-bit one is used (through Sysnative),
  # because Anthropic's installer refuses to run as a 32-bit process.
  function Get-ChildShell {
    if ($env:SystemRoot) {
      if ([Environment]::Is64BitOperatingSystem -and -not [Environment]::Is64BitProcess) {
        $native = Join-Path $env:SystemRoot 'Sysnative\WindowsPowerShell\v1.0\powershell.exe'
        if (Test-Path -LiteralPath $native) { return $native }
      }
      $windowsPowerShell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
      if (Test-Path -LiteralPath $windowsPowerShell) { return $windowsPowerShell }
    }
    $pwsh = Get-Command pwsh -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($pwsh) { return $pwsh.Source }
    return 'powershell.exe'
  }
  # A name beside the install folder that nothing uses yet: "<dir>.new.<pid>", or with ".1", ".2"... after it.
  function Get-FreeName([string]$base) {
    $name = $base
    for ($n = 1; Test-Path -LiteralPath $name; $n++) { $name = "$base.$n" }
    return $name
  }
  # Moves a folder, trying again for a few seconds: antivirus scanners often hold just-unpacked programs
  # open for a moment. Returns $null when it moved, else why it did not.
  function Move-Folder([string]$from, [string]$to) {
    $problem = $null
    for ($attempt = 1; $attempt -le 5; $attempt++) {
      if (Test-Path -LiteralPath $to) { return "$to already exists" }
      try {
        # A rename, which either happens whole or not at all. (Move-Item can fall back to copying the folder
        # and deleting the original, which leaves a half-moved install behind when a program in it is running.)
        $fromFull = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($from)
        $toFull = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($to)
        [System.IO.Directory]::Move($fromFull, $toFull)
        return $null
      } catch { $problem = $_.Exception.Message }
      if ($attempt -lt 5) { Start-Sleep -Milliseconds (400 * $attempt) }
    }
    return $problem
  }
  function Stop-WithFailures([string]$heading) {
    Say ''
    Say $heading
    $failures | ForEach-Object { Say "  - $_" }
    throw '[jev] A required check failed.'
  }

  # ------------------------------------------------------------------------------------------
  # The options.
  # ------------------------------------------------------------------------------------------
  $assumeYes = Test-FlagOn $env:JEV_ASSUME_YES
  # Options and settings from older installers: accepted, and reported as ignored.
  $notes = @()
  foreach ($option in $options) {
    switch -Regex ("$option") {
      '^--?(y|yes)$' { $assumeYes = $true }
      '^--?rust$' { $notes += , @("$option", 'is no longer needed: the native programs are the only edition, so it is ignored') }
      '^--?node$' { $notes += , @("$option", 'the Node.js edition has been retired: installing the native programs instead') }
      '^(--?(h|help)|-\?)$' {
        # Run from a file: print the settings from this script's own header (as install.sh does), up to the
        # implementation note. Through `iex` there is no file, so point to where they are listed.
        $header = @()
        if ($selfPath -and (Test-Path -LiteralPath $selfPath -PathType Leaf)) {
          foreach ($line in @(Get-Content -LiteralPath $selfPath -TotalCount 80)) {
            if ($line -notmatch '^#' -or $line -match '^# No param\(\) block') { break }
            $header += ($line -replace '^# ?', '')
          }
        }
        if ($header.Count -gt 5 -and $header[0] -match '^Installs or updates jev-claude') {
          while ($header.Count -and -not $header[-1].Trim()) { $header = @($header[0..($header.Count - 2)]) }
          $header | ForEach-Object { Write-Host $_ }
        } else {
          Write-Host 'Usage: install.ps1 [-Yes] [-Help]'
          Write-Host '  -Yes   answer yes to every "install this?" question (same as JEV_ASSUME_YES=1)'
          Write-Host 'Settings are environment variables; they are listed at the top of install.ps1:'
          Write-Host 'https://github.com/alienfacepalm/jev-claude/blob/master/install.ps1'
        }
        return
      }
      default { throw "[jev] Unknown option $option (try -Yes or -Help)" }
    }
  }
  if ($env:JEV_IMPL) { $notes += , @('JEV_IMPL', 'is no longer needed: the native programs are the only edition, so it is ignored') }

  # ------------------------------------------------------------------------------------------
  # Step 1: detect everything first (nothing changes yet), then install what is missing.
  # ------------------------------------------------------------------------------------------
  Say 'Installing jev-claude: native programs; no Node.js, pnpm, git clone or Rust toolchain needed.'
  Say ''
  Say 'Step 1 of 6: checking this machine'
  $failures = @()
  # Windows PowerShell 5.1 defaults to old TLS versions that GitHub refuses, and its progress bar slows downloads to a crawl.
  [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
  $ProgressPreference = 'SilentlyContinue'
  # A proxy that asks for your Windows sign-in gets it, as it does from a browser. (The second line is for
  # PowerShell 7; Windows PowerShell 5.1 has no HttpClient.DefaultProxy, and its try skips it.)
  try { if ([Net.WebRequest]::DefaultWebProxy) { [Net.WebRequest]::DefaultWebProxy.Credentials = [Net.CredentialCache]::DefaultNetworkCredentials } } catch { $null = $_ }
  try { if ([System.Net.Http.HttpClient]::DefaultProxy) { [System.Net.Http.HttpClient]::DefaultProxy.Credentials = [Net.CredentialCache]::DefaultNetworkCredentials } } catch { $null = $_ }
  $newShell = $false

  foreach ($note in $notes) { Check 'info' $note[0] $note[1] }

  # The platform decides which archive to fetch. Only Windows runs this script; the rest use install.sh.
  if ([Environment]::OSVersion.Platform.ToString() -ne 'Win32NT') {
    Check 'FAIL' 'Platform' 'install.ps1 is for Windows'
    Say ''
    Say "Nothing was installed. On macOS, Linux and WSL run: curl -fsSL $installShUrl | bash"
    throw '[jev] This is not Windows.'
  }
  $arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
  switch ($arch) {
    'AMD64' { Check 'ok' 'Platform' 'windows-x64' }
    'ARM64' {
      # Windows on ARM runs x64 programs through emulation from Windows 11 (build 22000) on; Windows 10
      # on ARM emulates only 32-bit x86, and there is no ARM64 build of the programs.
      $build = Get-WindowsBuild
      if ($build -ge 22000) {
        Check 'ok' 'Platform' 'windows-x64 (this ARM PC runs it through emulation)'
      } else {
        Check 'FAIL' 'Platform' "Windows 10 on ARM (build $build) cannot run the x64 programs"
        $failures += 'ARM PCs run the Windows (x64) programs through emulation from Windows 11 on. Update to Windows 11, then run this installer again.'
        $failures += "Other platforms can build from source (see $sourceBuildDoc), but that is not automated here."
      }
    }
    default {
      Check 'FAIL' 'Platform' "Windows $arch has no prebuilt programs"
      $failures += 'Prebuilt programs exist for macOS (Apple silicon and Intel), Linux (x64 and arm64, glibc and musl) and Windows (x64; ARM PCs run it by emulation).'
      $failures += "Other platforms can build from source (see $sourceBuildDoc), but that is not automated here."
    }
  }
  $asset = 'jev-claude-windows-x64.tar.gz'
  Check 'ok' 'PowerShell' "$($PSVersionTable.PSVersion)"
  # PowerShell's own web cmdlets download; nothing to install.
  Check 'ok' 'Download tool' 'PowerShell web cmdlets'

  # System32's tar.exe (bsdtar, Windows 10 version 1803 and later). Without it: a tar.exe on the PATH, or
  # Git for Windows' GNU tar, whose folder its default install leaves off the PATH (its gzip is beside it,
  # so that folder joins the PATH while tar runs).
  $tarPath = $null
  $tarFolder = $null
  if ($env:SystemRoot -and (Test-Path -LiteralPath (Join-Path $env:SystemRoot 'System32\tar.exe'))) {
    $tarPath = Join-Path $env:SystemRoot 'System32\tar.exe'
  } else {
    $tarCommand = Get-Command tar.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($tarCommand) {
      $tarPath = $tarCommand.Source
    } else {
      foreach ($programFiles in @($env:ProgramW6432, $env:ProgramFiles) | Where-Object { $_ }) {
        $gitTar = Join-Path $programFiles 'Git\usr\bin\tar.exe'
        if (Test-Path -LiteralPath $gitTar) {
          $tarPath = $gitTar
          $tarFolder = Split-Path -Parent $gitTar
          break
        }
      }
    }
  }
  if ($tarPath) {
    Check 'ok' 'tar' $tarPath
  } else {
    Check 'FAIL' 'tar' 'tar.exe was not found (it ships with Windows 10 version 1803 and later)'
    $failures += 'Update Windows to version 1803 or later, or install Git for Windows (https://git-scm.com/downloads), whose tar this installer can use; then run this installer again'
  }

  # Checksums come from .NET, which every PowerShell has.
  try {
    [Security.Cryptography.SHA256]::Create().Dispose()
    Check 'ok' 'Checksum tool' '.NET SHA-256'
  } catch {
    Check 'FAIL' 'Checksum tool' '.NET SHA-256 is not available'
    $failures += 'Use a current Windows PowerShell or PowerShell 7: the download cannot be trusted unchecked'
  }

  # Can the release host be reached? Any HTTP answer counts; only silence is a network problem.
  $origin = if ($releaseBase -match '^(https?://[^/]+)') { $Matches[1] }
  if ($origin) {
    $reach = 200
    try { Invoke-WebRequest -UseBasicParsing -Method Head -Uri $origin -TimeoutSec 20 | Out-Null } catch { $reach = Http-Status $_ }
    if ($reach -eq 0) {
      Check 'FAIL' 'Network' "cannot reach $(Hide-UrlSecret $origin)"
      $failures += 'Check your internet connection, VPN or proxy, then run this installer again'
    } elseif ($reach -eq 407) {
      Check 'warn' 'Network' "a proxy between this PC and $(Hide-UrlSecret $origin) wants a sign-in (HTTP 407); trying anyway"
    } else {
      Check 'ok' 'Network' "$(Hide-UrlSecret $origin) answers"
    }
  }

  # Claude Code is required: jev-claude is a launcher for it, and nothing works without it.
  $claudePath = Find-Claude
  $needClaude = -not $claudePath
  if ($claudePath) {
    if (-not (Write-ClaudeRow $claudePath)) {
      $failures += "Claude Code at $claudePath does not run. Reinstall it ($claudeSetupDoc): $claudeCommand, then run this installer again"
    }
    if (Test-ClaudeFolderMissing $claudePath) { Say "        It is in $claudeLocal, which is not on your user PATH yet; step 5 sees to that" }
  } else {
    Check 'FAIL' 'Claude Code' 'not found (jev-claude launches the real Claude Code, so it is required)'
  }

  # Optional tools: say what each one is for, and do not fail without it.
  $offerGit = $false
  if (Have git) {
    Check 'ok' 'git' ((git --version) -replace '^git version ', '')
  } else {
    Check 'warn' 'git' 'not found: the status line will not show the branch or worktree'
    $offerGit = $true
  }
  if (Have rustc) { Check 'info' 'Rust' "$(((rustc --version) -split ' ')[1]); not used, the prebuilt programs are installed" }
  if (Have node) { Check 'info' 'Node.js' "$(node --version) found; jev-claude does not use it" } else { Check 'info' 'Node.js' 'not installed; jev-claude does not need it' }

  # The install folder, and what an earlier install left on the PATH. The old Node edition is a pnpm
  # link (jev-claude.cmd / .ps1) and a clone at ~\jev-claude; this edition's programs are .exe files.
  $installDir = if ($env:JEV_INSTALL_DIR) { $env:JEV_INSTALL_DIR } else { Join-Path $userHome 'jev-claude-rust' }
  # A full path without a trailing separator, so the staging folders beside it ("<dir>.new.<pid>") are
  # never inside it, and a relative JEV_INSTALL_DIR means the folder this shell is in.
  $installDir = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($installDir)
  if ($installDir.Length -gt 3) { $installDir = $installDir.TrimEnd('\', '/') }
  $binDir = Join-Path $installDir 'bin'
  $onPath = @(Get-Command jev-claude -All -CommandType Application, ExternalScript -ErrorAction SilentlyContinue | Where-Object { $_.Source } | ForEach-Object { $_.Source } | Select-Object -Unique)
  $others = @($onPath | Where-Object { (Get-PathEntryKey (Split-Path -Parent $_)) -ine (Get-PathEntryKey $binDir) })
  $nodeLinks = @($others | Where-Object { $_ -match '\.(cmd|ps1|bat|mjs|js)$' })
  $nodeClone = Join-Path $userHome 'jev-claude'
  $hasNodeClone = Test-Path -LiteralPath (Join-Path $nodeClone 'node\bin\jev-claude.mjs')
  if ($nodeLinks.Count -or $hasNodeClone) {
    Check 'warn' 'Old Node edition' "found: $((@($nodeLinks) + @(if ($hasNodeClone) { $nodeClone })) -join ', ')"
    Say '        jev-claude no longer has a Node.js edition. Nothing was deleted for you; remove the old one so'
    Say '        only this install is left:'
    if ($nodeLinks.Count) { Say '          pnpm remove --global jev-router' }
    if ($hasNodeClone) { Say "          Remove-Item -Recurse -Force -LiteralPath '$nodeClone'" }
    Say '        Until then, whichever folder comes first on your PATH wins; this install puts its own folder first.'
  }
  # Only a jev-claude install, or an empty folder, may be replaced: anything else in the way is left alone.
  if ((Test-Path -LiteralPath $installDir) -and -not (Test-JevInstall $installDir)) {
    $isEmptyFolder = (Test-Path -LiteralPath $installDir -PathType Container) -and -not @(Get-ChildItem -LiteralPath $installDir -Force).Count
    if (-not $isEmptyFolder) {
      Check 'FAIL' 'Install folder' "$installDir exists but is not a jev-claude install"
      $failures += "Set JEV_INSTALL_DIR to another folder (nothing in $installDir was touched), then run this installer again"
    }
  }

  if ($failures.Count) { Stop-WithFailures 'Nothing was installed: a required check failed. To fix it:' }

  # What to do about each missing prerequisite is decided here, all questions first; then, if nobody
  # objected, it acts. Claude Code first: it is required, so a refusal ends the install before anything
  # else changes.
  $doClaude = $false
  $doGit = $false
  $gitCommand = 'winget install --id Git.Git -e --accept-package-agreements --accept-source-agreements'
  if ($needClaude) {
    if (Test-FlagOff $env:JEV_INSTALL_CLAUDE) {
      $failures += "Claude Code is required and JEV_INSTALL_CLAUDE=0 says not to install it. Install it yourself ($claudeSetupDoc): $claudeCommand"
    } elseif (Test-FlagOn $env:JEV_INSTALL_CLAUDE) {
      $doClaude = $true
    } else {
      Say "        Claude Code is required. Anthropic's own installer is: $claudeCommand"
      if (-not $assumeYes -and -not (Test-Interactive)) {
        $failures += 'There is no terminal to ask on. Run that command yourself, or run this installer again with JEV_ASSUME_YES=1 (or JEV_INSTALL_CLAUDE=1) to let it install Claude Code'
      } elseif (Ask-YesNo 'Run it now? [y/N]') {
        $doClaude = $true
      } else {
        $failures += "Claude Code is required. Install it ($claudeSetupDoc): $claudeCommand, then run this installer again"
      }
    }
  }
  # git is optional: offered, never required, and never an error to decline. Not offered when the
  # install is about to stop anyway.
  if ($offerGit -and -not $failures.Count) {
    if (Have winget) {
      if ($assumeYes -or (Test-Interactive)) {
        Say "        Optional: git shows the branch in the status line. To install it: $gitCommand"
        if (Ask-YesNo 'Run it now? [y/N]') { $doGit = $true }
      } else {
        Say "        Optional: git shows the branch in the status line. To install it later: $gitCommand"
      }
    } else {
      Say '        Optional: git shows the branch in the status line: https://git-scm.com/downloads'
    }
  }
  if ($failures.Count) { Stop-WithFailures 'Nothing was installed: a required check failed. To fix it:' }

  if ($doClaude) {
    Say "        Running: $claudeCommand (output below)"
    $childShell = Get-ChildShell
    $childCommand = "[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12; $claudeCommand"
    try {
      & $childShell -NoProfile -ExecutionPolicy Bypass -Command $childCommand | ForEach-Object { Write-Host "        $_" }
    } catch {
      Say "        Anthropic's installer did not run: $($_.Exception.Message)"
    }
    $claudePath = Find-Claude
    if (-not $claudePath) {
      $failures += "Anthropic's installer did not leave a claude program. Install Claude Code yourself ($claudeSetupDoc): $claudeCommand, then run this installer again"
      Stop-WithFailures 'Claude Code could not be installed. To fix it:'
    }
    Add-SessionPath (Split-Path -Parent $claudePath)
    $newShell = $true
    if (-not (Write-ClaudeRow $claudePath)) {
      $failures += "Anthropic's installer left $claudePath, but it does not run. Install Claude Code yourself ($claudeSetupDoc): $claudeCommand, then run this installer again"
      Stop-WithFailures 'Claude Code could not be installed. To fix it:'
    }
  }
  if ($doGit) {
    Say "        Running: $gitCommand"
    try { & winget install --id Git.Git -e --accept-package-agreements --accept-source-agreements | Out-Host } catch { Say "        winget did not finish: $($_.Exception.Message)" }
    # A new install's folder is on the user PATH in the registry, not yet in this window's.
    foreach ($programFiles in @($env:ProgramW6432, $env:ProgramFiles) | Where-Object { $_ }) {
      $gitFolder = Join-Path $programFiles 'Git\cmd'
      if ((-not (Have git)) -and (Test-Path -LiteralPath (Join-Path $gitFolder 'git.exe'))) { Add-SessionPath $gitFolder }
    }
    if (Have git) {
      Check 'ok' 'git' ((git --version) -replace '^git version ', '')
      $newShell = $true
    } else {
      Check 'warn' 'git' 'could not be installed; the status line will not show the branch (optional)'
    }
  }
  Say '        All required checks passed.'
  Say ''

  # ------------------------------------------------------------------------------------------
  # Steps 2-6: download, verify, install, PATH, prove it runs.
  # ------------------------------------------------------------------------------------------
  # Folders an earlier run left beside the install (an antivirus lock can stop their removal) go first,
  # but only ones that hold a jev-claude install or this installer's download, and only after ten
  # minutes, so a run still going in another window keeps its own.
  $parent = Split-Path -Parent $installDir
  $leaf = Split-Path -Leaf $installDir
  if ($parent -and (Test-Path -LiteralPath $parent)) {
    $pattern = '^' + [regex]::Escape($leaf) + '\.(new|old)\.\d+(\.\d+)?$'
    Get-ChildItem -LiteralPath $parent -Directory -Force -ErrorAction SilentlyContinue | Where-Object {
      $_.Name -match $pattern -and $_.LastWriteTime -lt (Get-Date).AddMinutes(-10) -and ((Test-JevInstall $_.FullName) -or (Test-Path -LiteralPath (Join-Path $_.FullName '.download')))
    } | ForEach-Object { Remove-Item -Recurse -Force -LiteralPath $_.FullName -ErrorAction SilentlyContinue }
  }

  # 2. Download, into the folder the new install is unpacked in, beside the install folder (".download"
  # inside it). tar is then given only short relative names that are plain ASCII: Windows' tar.exe reads
  # its arguments in the ANSI code page, so a user folder named outside it would not survive as a path.
  if ($env:JEV_VERSION) {
    $url = "$releaseBase/download/$($env:JEV_VERSION)/$asset"
    $sumsUrl = "$releaseBase/download/$($env:JEV_VERSION)/SHA256SUMS"
    Say "Step 2 of 6: downloading release $($env:JEV_VERSION)"
  } else {
    $url = "$releaseBase/latest/download/$asset"
    $sumsUrl = "$releaseBase/latest/download/SHA256SUMS"
    Say 'Step 2 of 6: downloading the latest release'
  }
  Say "        from $(Hide-UrlSecret $url)"
  $stage = Get-FreeName "$installDir.new.$PID"
  $download = Join-Path $stage '.download'
  # The earlier install, set aside while the new one proves itself (step 6); $null once that is settled.
  $oldDir = $null
  $swapped = $false
  try {
    New-Item -ItemType Directory -Force $download | Out-Null
  } catch {
    throw "[jev] Could not create $stage. Check that you can write to that folder, or set JEV_INSTALL_DIR to another folder."
  }
  try {
    $archive = Join-Path $download $asset
    $status = Get-File $url $archive
    if ($status -ne 200) {
      if ($status -eq 404) {
        Say 'No prebuilt programs are published at that address (HTTP 404).'
        Say "If you pinned a version with JEV_VERSION, check that the tag exists: $(Hide-UrlSecret $releaseBase)"
      } elseif ($status -eq -1) {
        Say 'The download was interrupted before it finished. Run this installer again.'
      } elseif ($status -eq 0) {
        Say 'The download did not start: the connection failed. Check your network, VPN or proxy and run this installer again.'
      } elseif ($status -eq 407) {
        Say 'The download failed (HTTP 407): a proxy wants a sign-in that it did not accept. Ask whoever runs it, or run this installer on another network.'
      } else {
        Say "The download failed (HTTP $status). Run this installer again; if it keeps failing, report it with this output."
      }
      throw '[jev] Nothing was installed.'
    }
    Say "        downloaded $((Get-Item -LiteralPath $archive).Length) bytes"

    # 3. Verify. The archive must match the checksum the release published for it.
    Say ''
    Say 'Step 3 of 6: verifying the download'
    $sums = Join-Path $download 'SHA256SUMS'
    $sumsStatus = Get-File $sumsUrl $sums
    if ($sumsStatus -eq -1) { throw "[jev] The download of the release's SHA256SUMS was interrupted before it finished; nothing was installed. Run this installer again." }
    if ($sumsStatus -ne 200) { throw "[jev] Could not fetch the release's SHA256SUMS (HTTP $sumsStatus); nothing was installed." }
    $expected = $null
    foreach ($line in Get-Content -LiteralPath $sums) {
      if ($line -match '^([0-9a-fA-F]{64}) [ *](.+)$' -and $Matches[2] -eq $asset -and -not $expected) { $expected = $Matches[1].ToLower() }
    }
    if (-not $expected) { throw "[jev] SHA256SUMS has no entry for $asset; nothing was installed." }
    $actual = Sha256-Of $archive
    Say "        expected $expected"
    Say "        actual   $actual"
    if ($expected -ne $actual) { throw '[jev] The checksums differ, so the download is not what was published. Nothing was installed.' }
    Say '        checksum matches'

    # 4. Install: unpack beside the destination, then swap, so a failed unpack never leaves half an install.
    Say ''
    Say "Step 4 of 6: installing into $installDir"
    # A GNU tar reads "C:\..." as a host name unless told the path is local (the names here are relative, but say so anyway).
    $tarArgs = @()
    $savedPath = $env:Path
    if ($tarFolder) { $env:Path = "$tarFolder;$env:Path" }
    try {
      try { if ((& $tarPath --version 2>&1 | Out-String) -match 'GNU tar') { $tarArgs = @('--force-local') } } catch { $tarArgs = @() }
      Push-Location -LiteralPath $stage
      try { & $tarPath @tarArgs -xzf ".download/$asset" } finally { Pop-Location }
      $tarExit = $LASTEXITCODE
    } finally { $env:Path = $savedPath }
    if ($tarExit) { throw '[jev] Could not unpack the archive.' }
    Remove-Item -Recurse -Force -LiteralPath $download
    $installed = Version-In $stage
    if (-not $installed -or -not (Test-JevInstall $stage)) { throw '[jev] The archive is not a jev-claude release.' }
    # Every program must be there before the earlier install is replaced.
    foreach ($program in $programs) {
      if (-not (Test-Path -LiteralPath (Join-Path (Join-Path $stage 'bin') "$program.exe"))) {
        throw "[jev] $program is missing from the release archive; nothing was changed."
      }
    }
    if ((Test-Path -LiteralPath $installDir) -and -not (Test-JevInstall $installDir)) {
      # Step 1 let this through only as an empty folder; anything that arrived since is left alone.
      if (@(Get-ChildItem -LiteralPath $installDir -Force).Count) { throw "[jev] $installDir exists but is not a jev-claude install. Set JEV_INSTALL_DIR to another folder." }
      Remove-Item -LiteralPath $installDir
    }
    if (Test-Path -LiteralPath $installDir) {
      $previous = Version-In $installDir
      $old = Get-FreeName "$installDir.old.$PID"
      if (Move-Folder $installDir $old) {
        throw "[jev] Could not replace $installDir. Close any running jev-claude and Claude Code sessions that use it, then run this installer again."
      }
      $problem = Move-Folder $stage $installDir
      if ($problem) {
        # Put the earlier install back rather than leave none.
        $restoreProblem = Move-Folder $old $installDir
        if ($restoreProblem) {
          throw "[jev] Could not move the new programs into $installDir ($problem), nor put the earlier install back ($restoreProblem). The earlier install is in $($old): rename it to $installDir, or run this installer again."
        }
        throw "[jev] Could not move the new programs into $installDir ($problem). The earlier install was kept."
      }
      $oldDir = $old
      $swapped = $true
      if ($previous -eq $installed) { Say "        reinstalled version $installed" } else { Say "        updated $(if ($previous) { $previous } else { 'an earlier install' }) -> $installed" }
    } else {
      $problem = Move-Folder $stage $installDir
      if ($problem) { throw "[jev] Could not move the new programs into $installDir ($problem)." }
      Say "        installed version $installed"
    }
    Say "        programs: $($programs -join ' ')"
    Say '        skills:   /jev-calibrate /jev-explain /jev-legend (they run those programs)'

    # 5. PATH: this install's bin folder goes first on the user PATH (kept in the registry, %VAR% entries
    # unexpanded). Windows has no reliable symlinks for the programs, so they stay beside their package.json.
    # This is the same Windows user PATH that install.sh sets when run from Git Bash; new PowerShell, cmd and
    # Git Bash windows all start from it. Claude Code's folder goes on too, last, when Anthropic's installer
    # left it off.
    Say ''
    Say 'Step 5 of 6: putting jev-claude on your PATH'
    $addClaudeFolder = Test-ClaudeFolderMissing $claudePath
    $pathChanged = $false
    if (Test-FlagOn $env:JEV_NO_MODIFY_PATH) {
      Say '        JEV_NO_MODIFY_PATH=1, so your Windows user PATH is untouched. Add this folder to it:'
      Say "          $binDir"
      if ($addClaudeFolder) {
        Say "        and Claude Code's folder, which its installer leaves off the PATH (jev-claude finds claude there):"
        Say "          $claudeLocal"
      }
    } else {
      try {
        switch (Add-UserPath $binDir) {
          'added' { Say "        added $binDir to your Windows user PATH (PowerShell and cmd, in new windows)"; $pathChanged = $true }
          'moved' { Say "        moved $binDir to the front of your Windows user PATH (PowerShell and cmd, in new windows)"; $pathChanged = $true }
          default { Say "        $binDir is already first on your Windows user PATH" }
        }
      } catch {
        Check 'warn' 'Windows PATH' "could not change the Windows user PATH: $($_.Exception.Message)"
        Say "        PowerShell and cmd will not find jev-claude until this folder is on your user PATH: $binDir"
        Say '        (Settings > System > About > Advanced system settings > Environment Variables.)'
      }
      if ($addClaudeFolder) {
        try {
          if ((Add-UserPath $claudeLocal -Append) -eq 'added') {
            Say "        added $claudeLocal to your Windows user PATH too: Claude Code is there, and its installer leaves it off"
            $pathChanged = $true
          }
        } catch {
          Check 'warn' 'Windows PATH' "could not add Claude Code's folder to the Windows user PATH: $($_.Exception.Message)"
          Say "        jev-claude will not find claude in new windows until this folder is on your user PATH: $claudeLocal"
        }
      }
    }
    if ($pathChanged) {
      Send-EnvironmentChange
      $newShell = $true
    }
    Add-SessionPath $binDir
    # Other jev-claude programs this shell could find before the install: which one runs depends on the order.
    if ($others.Count) {
      $winner = Get-Command jev-claude -CommandType Application, ExternalScript -ErrorAction SilentlyContinue | Select-Object -First 1
      $winner = if ($winner) { $winner.Source } else { $null }
      Say "        jev-claude was already on your PATH: $($others -join ' ')"
      if ($winner -and (Get-PathEntryKey (Split-Path -Parent $winner)) -ieq (Get-PathEntryKey $binDir)) {
        Say "        Whichever folder comes first wins; in this shell that is this install's ($binDir)."
      } else {
        Say "        Whichever folder comes first wins; in this shell that is $(if ($winner) { $winner } else { 'another one' }), not this install's ($binDir)."
      }
      Say '        New windows put the system PATH before your user PATH, so a jev-claude on the system PATH would come first there.'
    }

    # 6. Prove the installed programs run: the read-only setup check, from the install folder.
    Say ''
    Say 'Step 6 of 6: running the installed setup check (jev-check)'
    # (No 2>&1: in Windows PowerShell 5.1 a redirected stderr line becomes an error, which 'Stop' throws.)
    try {
      $report = & (Join-Path $binDir 'jev-check.exe')
      $checkExit = $LASTEXITCODE
    } catch {
      $report = $_.Exception.Message
      $checkExit = 1
    }
    $report | ForEach-Object { Say "       $_" }
    # A new install that cannot run must not replace one that can: the finally below puts it back.
    if ($checkExit) { throw '[jev] jev-check failed to run, so the install is not usable. Report this with the output above.' }
    # The new install works: the earlier one is no longer needed.
    if ($oldDir) {
      Remove-Item -Recurse -Force -LiteralPath $oldDir -ErrorAction SilentlyContinue
      if (Test-Path -LiteralPath $oldDir) { Say "        (the earlier install could not be deleted; remove $oldDir yourself)" }
      $oldDir = $null
    }
    Say ''
  } finally {
    # A run that stops before the new install has proved itself puts the earlier one back. Only the new
    # install, which this run put there, is ever removed to make room for it.
    if ($oldDir) {
      if ($swapped -and (Test-Path -LiteralPath $installDir)) { Remove-Item -Recurse -Force -LiteralPath $installDir -ErrorAction SilentlyContinue }
      if (-not (Test-Path -LiteralPath $installDir) -and -not (Move-Folder $oldDir $installDir)) {
        Say 'The earlier install was put back.'
      } else {
        Say "The earlier install is kept in $oldDir; move it back to $installDir by hand."
      }
    }
    # Whatever is still in the staging folder was put there by this run.
    if (Test-Path -LiteralPath $stage) { Remove-Item -Recurse -Force -LiteralPath $stage -ErrorAction SilentlyContinue }
  }

  # The keys, kept in ~\.jev-router.env inside your own profile.
  if (-not (Test-Path -LiteralPath $envFile)) {
    try { Copy-Item -LiteralPath (Join-Path $installDir '.env.example') -Destination $envFile -ErrorAction Stop }
    catch { throw "[jev] Could not create $envFile. The programs are installed; copy $(Join-Path $installDir '.env.example') there yourself, then add your keys." }
  }

  # Sets NAME=value: over an existing NAME= line or its commented-out "# NAME=" placeholder from
  # .env.example, or as a new line at the end.
  function Set-Key([string]$name, [string]$value) {
    $done = $false
    $lines = @(Get-Content -LiteralPath $envFile -Encoding UTF8 | ForEach-Object {
      if (-not $done -and ($_ -match "^(# )?$name=")) { $done = $true; "$name=$value" } else { $_ }
    })
    if (-not $done) { $lines += "$name=$value" }
    # UTF-8 without a byte-order mark, which Windows PowerShell's own writers would add.
    [IO.File]::WriteAllLines($envFile, [string[]]$lines)
  }
  function Has-Key([string]$pattern) { [bool](Select-String -LiteralPath $envFile -Pattern "^($pattern)=.+" -Quiet) }
  # Reads a secret without showing it; empty when there is nobody to ask, and always empty inside a
  # Claude Code session (an agent must never be asked for a key).
  function Ask-Secret([string]$prompt) {
    if ($env:CLAUDECODE -or -not (Test-Interactive)) { return '' }
    try {
      $secure = Read-Host "[jev] $prompt" -AsSecureString
      $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
      try { return [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr) } finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }
    } catch { return '' }
  }

  # The Jev key is required for routing.
  if (Has-Key 'JEV_API_KEY|TYPESAFE_API_KEY') {
    Say "Using the Jev key already in $envFile"
  } else {
    $key = $env:JEV_API_KEY
    if (-not $key) {
      Say 'Get a Jev API key at https://console.typesafe.ai/keys'
      $key = Ask-Secret 'Paste it here (it will not be shown), or press Enter to add it later'
    }
    if ($key) {
      Set-Key 'JEV_API_KEY' $key
      Say "Saved your Jev key to $envFile"
    } else {
      Say "Add your Jev key later: open $envFile and paste it after JEV_API_KEY="
    }
  }

  # An Anthropic API key is optional: without one, Claude Code uses your own sign-in.
  if (Has-Key 'ANTHROPIC_API_KEY') {
    Say "Using the Anthropic API key already in $envFile"
  } else {
    $key = $env:ANTHROPIC_API_KEY
    if (-not $key) {
      Say 'Optional: run Claude Code on an Anthropic API key, billed per token, instead of your Claude sign-in.'
      $key = Ask-Secret 'Paste the key (it will not be shown), or press Enter to keep your sign-in'
    }
    if ($key) {
      Set-Key 'ANTHROPIC_API_KEY' $key
      Say 'Saved your Anthropic API key. Claude Code asks once, on its first start, whether to use it.'
    }
  }

  Say 'Done. Start it with: jev-claude'
  if ($newShell) {
    Say 'Open a new terminal first, so it can find the jev-claude command.'
    Say '(Windows Terminal and VS Code give new tabs the PATH they started with: restart the app if a new tab cannot find it.)'
  }
} @args

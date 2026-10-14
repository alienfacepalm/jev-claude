# Installs or updates jev-claude (a fork of jev-router, for Claude Code) on Windows:
#
#   irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 | iex
#
# or run .\install.ps1 from a clone. Running it again updates the install.
#
# It installs one of two editions of the same program:
#
#   node (default)  a git clone run with Node.js 22+ and pnpm
#   rust            ready-made native programs from the latest release: no Node.js, no pnpm, no clone
#
# Pick the Rust edition with $env:JEV_IMPL = 'rust' before running it.
#
#   $env:JEV_IMPL           node (default) or rust
#   $env:JEV_CLAUDE_DIR     where to clone (default ~\jev-claude); ignored when run from a clone
#   $env:JEV_CLAUDE_REPO    the repository to clone (default this one)
#   $env:JEV_API_KEY        your Jev key, to save it without the prompt
#   $env:ANTHROPIC_API_KEY  an Anthropic API key to run Claude Code on, to save it without the prompt
#
# Rust edition only:
#
#   $env:JEV_INSTALL_DIR      where the programs go (default ~\jev-claude-rust)
#   $env:JEV_VERSION          a release tag such as v0.10.0 (default: the latest release)
#   $env:JEV_RELEASE_URL      where releases are downloaded from (default this repository's GitHub releases)
#   $env:JEV_NO_MODIFY_PATH   1 to leave your PATH alone; the installer then prints the folder to add
#   $env:JEV_INSTALL_CLAUDE   1 to install Claude Code when it is missing without asking, 0 never to offer
#
# Errors are thrown rather than exited on: run through `iex`, `exit` would close the window.
& {
  $ErrorActionPreference = 'Stop'
  $repo = if ($env:JEV_CLAUDE_REPO) { $env:JEV_CLAUDE_REPO } else { 'https://github.com/alienfacepalm/jev-claude.git' }
  $envFile = Join-Path $HOME '.jev-router.env'

  function Say([string]$message) { Write-Host "[jev] $message" }
  function Have([string]$command) { [bool](Get-Command $command -ErrorAction SilentlyContinue) }
  # Native commands report failure only through their exit code.
  function Run([scriptblock]$command, [string]$failure) {
    & $command
    if ($LASTEXITCODE) { throw "[jev] $failure" }
  }

  # --------------------------------------------------------------------------------------------
  # The Rust edition: native programs from a release, no Node.js.
  # --------------------------------------------------------------------------------------------
  $releaseBase = if ($env:JEV_RELEASE_URL) { $env:JEV_RELEASE_URL } else { 'https://github.com/alienfacepalm/jev-claude/releases' }
  $claudeInstallPs1 = 'https://claude.ai/install.ps1'
  $sourceBuildDoc = 'https://github.com/alienfacepalm/jev-claude/blob/master/doc/RUST.md#install-the-toolchain'

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
    try { (Get-Content (Join-Path $folder 'package.json') -Raw | ConvertFrom-Json).version } catch { $null }
  }
  # The HTTP status behind a failed request: 0 when nothing answered.
  function Http-Status($errorRecord) {
    $response = $errorRecord.Exception.Response
    if ($response) { [int]$response.StatusCode } else { 0 }
  }
  # Tells Explorer, and so every terminal opened from now on, that the environment changed. Windows
  # waits for each open window to answer, so a window that hangs is skipped after two seconds
  # (SMTO_ABORTIFHUNG) instead of holding the installer up, as [Environment]::SetEnvironmentVariable would.
  function Send-EnvironmentChange {
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
  # Puts a folder first on the user's PATH, keeping any %VAR% entries unexpanded; false when it was already there.
  function Add-UserPath([string]$folder) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
    try {
      $raw = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
      $parts = @($raw -split ';' | Where-Object { $_ })
      if ($parts | Where-Object { $_.TrimEnd('\') -ieq $folder.TrimEnd('\') }) { return $false }
      $key.SetValue('Path', ((@($folder) + $parts) -join ';'), [Microsoft.Win32.RegistryValueKind]::ExpandString)
    } finally { $key.Close() }
    Send-EnvironmentChange
    $true
  }

  function Install-Rust {
    Say 'Installing jev-claude, Rust edition: native programs, no Node.js or pnpm needed.'
    Say ''
    Say 'Step 1 of 6: checking this machine'
    $failures = @()
    # Windows PowerShell 5.1 defaults to old TLS versions that GitHub refuses, and its progress bar slows downloads to a crawl.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    $ProgressPreference = 'SilentlyContinue'

    $arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
    switch ($arch) {
      'AMD64' { Check 'ok' 'Platform' 'windows-x64' }
      'ARM64' { Check 'ok' 'Platform' 'windows-arm64 (runs the windows-x64 programs through emulation)' }
      default {
        Check 'FAIL' 'Platform' "Windows on $arch has no prebuilt programs"
        $failures += "Build from source instead: $sourceBuildDoc"
      }
    }
    $asset = 'jev-claude-windows-x64.tar.gz'
    Check 'ok' 'PowerShell' "$($PSVersionTable.PSVersion)"

    $systemTar = Join-Path $env:SystemRoot 'System32\tar.exe'
    $tarPath = if (Test-Path $systemTar) { $systemTar } else { $c = Get-Command tar.exe -ErrorAction SilentlyContinue; if ($c) { $c.Source } }
    if ($tarPath) {
      Check 'ok' 'tar' $tarPath
    } else {
      Check 'FAIL' 'tar' 'tar.exe was not found (it ships with Windows 10 version 1803 and later)'
      $failures += 'Update Windows, or install a tar (for example Git for Windows), then run this installer again'
    }

    $origin = if ($releaseBase -match '^(https?://[^/]+)') { $Matches[1] }
    if ($origin) {
      $reachable = $true
      try { Invoke-WebRequest -UseBasicParsing -Method Head -Uri $origin -TimeoutSec 20 | Out-Null } catch { if ((Http-Status $_) -eq 0) { $reachable = $false } }
      if ($reachable) {
        Check 'ok' 'Network' "$origin answers"
      } else {
        Check 'FAIL' 'Network' "cannot reach $origin"
        $failures += 'Check your internet connection, VPN or proxy, then run this installer again'
      }
    }

    # Claude Code is what jev-claude runs.
    $claude = Get-Command claude -ErrorAction SilentlyContinue
    $claudePath = if ($claude) { $claude.Source } elseif (Test-Path (Join-Path $HOME '.local\bin\claude.exe')) { Join-Path $HOME '.local\bin\claude.exe' }
    if (-not $claudePath) {
      Check 'warn' 'Claude Code' 'not found (jev-claude runs the real Claude Code)'
      $offer = if ($env:JEV_INSTALL_CLAUDE) { $env:JEV_INSTALL_CLAUDE } else { 'ask' }
      if ($offer -eq 'ask') {
        Say "        Anthropic's own installer is: irm $claudeInstallPs1 | iex"
        if ([Environment]::UserInteractive -and -not [Console]::IsInputRedirected) {
          $offer = if ((Read-Host '[jev] Run it now? [y/N]') -match '^[Yy]') { '1' } else { '0' }
        } else { $offer = '0' }
      }
      if ($offer -eq '1') {
        Say "        Running Anthropic's installer (output below)"
        try { Invoke-RestMethod $claudeInstallPs1 | Invoke-Expression } catch { Say "        Anthropic's installer did not finish ($($_.Exception.Message)); install Claude Code yourself before running jev-claude." }
        $local = Join-Path $HOME '.local\bin'
        if (Test-Path (Join-Path $local 'claude.exe')) { $env:Path = "$local;$env:Path"; $claudePath = Join-Path $local 'claude.exe' }
      } else {
        Say '        Install it before running jev-claude: https://code.claude.com/docs/en/setup'
      }
    }
    if ($claudePath) {
      $claudeVersion = try { ((& $claudePath --version) | Select-Object -First 1) -split ' ' | Select-Object -First 1 } catch { $null }
      $needsNode = $claudePath -match '\.(cmd|ps1|js|mjs|cjs)$'
      if ($needsNode -and -not (Have node)) {
        Check 'warn' 'Claude Code' "$claudeVersion is the npm package, which runs on Node.js, and Node.js is not installed"
        Say "        Reinstall Claude Code natively: irm $claudeInstallPs1 | iex"
      } elseif ($needsNode) {
        Check 'ok' 'Claude Code' "$claudeVersion (npm package, runs on Node.js $(node --version))"
      } else {
        Check 'ok' 'Claude Code' "$claudeVersion (native, needs no Node.js)"
      }
    }

    # Optional tools: say what each one is for, and do not fail without it.
    if (Have git) { Check 'ok' 'git' ((git --version) -replace '^git version ', '') } else { Check 'warn' 'git' 'not found: the status line will not show the branch or worktree' }
    if (Have node) { Check 'info' 'Node.js' "$(node --version) found; the Rust edition does not use it" } else { Check 'info' 'Node.js' 'not installed; the Rust edition does not need it' }

    if ($failures.Count) {
      Say ''
      Say 'Nothing was installed: a required check failed. To fix it:'
      $failures | ForEach-Object { Say "  - $_" }
      throw '[jev] A required check failed.'
    }
    Say '        All required checks passed.'
    Say ''

    # 2. Download.
    if ($env:JEV_VERSION) {
      $url = "$releaseBase/download/$($env:JEV_VERSION)/$asset"
      $sumsUrl = "$releaseBase/download/$($env:JEV_VERSION)/SHA256SUMS"
      Say "Step 2 of 6: downloading release $($env:JEV_VERSION)"
    } else {
      $url = "$releaseBase/latest/download/$asset"
      $sumsUrl = "$releaseBase/latest/download/SHA256SUMS"
      Say 'Step 2 of 6: downloading the latest release'
    }
    Say "        from $url"
    $tmp = Join-Path ([IO.Path]::GetTempPath()) "jev-install-$PID"
    New-Item -ItemType Directory -Force $tmp | Out-Null
    try {
      $archive = Join-Path $tmp $asset
      try {
        Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $archive
      } catch {
        $status = Http-Status $_
        if ($status -eq 404) {
          Say 'No prebuilt programs are published at that address yet (HTTP 404).'
          Say 'Releases carry them from the version that added the Rust edition on. If you pinned a'
          Say "version with JEV_VERSION, try a newer one; otherwise build from source: $sourceBuildDoc"
        } elseif ($status -eq 0) {
          Say 'The download did not start: the connection failed. Check your network, VPN or proxy and run this installer again.'
        } else {
          Say "The download failed (HTTP $status). Run this installer again; if it keeps failing, build from source: $sourceBuildDoc"
        }
        throw '[jev] Nothing was installed.'
      }
      Say "        downloaded $((Get-Item $archive).Length) bytes"

      # 3. Verify. The archive must match the checksum the release published for it.
      Say ''
      Say 'Step 3 of 6: verifying the download'
      $sums = Join-Path $tmp 'SHA256SUMS'
      try { Invoke-WebRequest -UseBasicParsing -Uri $sumsUrl -OutFile $sums } catch { throw "[jev] Could not fetch the release's SHA256SUMS (HTTP $(Http-Status $_)); nothing was installed." }
      $expected = $null
      foreach ($line in Get-Content $sums) {
        if ($line -match '^([0-9a-fA-F]{64}) [ *](.+)$' -and $Matches[2] -eq $asset) { $expected = $Matches[1].ToLower() }
      }
      if (-not $expected) { throw "[jev] SHA256SUMS has no entry for $asset; nothing was installed." }
      $actual = Sha256-Of $archive
      Say "        expected $expected"
      Say "        actual   $actual"
      if ($expected -ne $actual) { throw '[jev] The checksums differ, so the download is not what was published. Nothing was installed.' }
      Say '        checksum matches'

      # 4. Install: unpack beside the destination, then swap, so a failed unpack never leaves half an install.
      $installDir = if ($env:JEV_INSTALL_DIR) { $env:JEV_INSTALL_DIR } else { Join-Path $HOME 'jev-claude-rust' }
      Say ''
      Say "Step 4 of 6: installing into $installDir"
      $stage = "$installDir.new.$PID"
      if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
      New-Item -ItemType Directory -Force $stage | Out-Null
      Push-Location $tmp
      try { & $tarPath -xzf $asset -C $stage } finally { Pop-Location }
      if ($LASTEXITCODE) { Remove-Item -Recurse -Force $stage; throw '[jev] Could not unpack the archive.' }
      $installed = Version-In $stage
      if (-not $installed -or -not (Test-Path (Join-Path $stage 'bin\jev-claude.exe'))) {
        Remove-Item -Recurse -Force $stage
        throw '[jev] The archive is not a jev-claude release.'
      }
      if (Test-Path $installDir) {
        if (-not (Test-Path (Join-Path $installDir 'package.json'))) {
          Remove-Item -Recurse -Force $stage
          throw "[jev] $installDir exists but is not a jev-claude install. Set JEV_INSTALL_DIR to another folder."
        }
        $previous = Version-In $installDir
        $old = "$installDir.old.$PID"
        try {
          Move-Item $installDir $old
        } catch {
          Remove-Item -Recurse -Force $stage
          throw "[jev] Could not replace $installDir. Close any running jev-claude and Claude Code sessions that use it, then run this installer again."
        }
        Move-Item $stage $installDir
        Remove-Item -Recurse -Force $old -ErrorAction SilentlyContinue
        if ($previous -eq $installed) { Say "        reinstalled version $installed" } else { Say "        updated $(if ($previous) { $previous } else { 'an earlier install' }) -> $installed" }
      } else {
        Move-Item $stage $installDir
        Say "        installed version $installed"
      }
      foreach ($program in 'jev-claude', 'jev-statusline', 'jev-explain', 'jev-legend', 'jev-check', 'jev-update-check', 'jev-proxy-host') {
        if (-not (Test-Path (Join-Path $installDir "bin\$program.exe"))) { throw "[jev] $program is missing from the install." }
      }
      Say '        programs: jev-claude jev-statusline jev-explain jev-legend jev-check jev-update-check jev-proxy-host'
      Say '        skills:   /jev-calibrate /jev-explain /jev-legend (they run those programs)'
    } finally {
      Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }

    # 5. PATH.
    Say ''
    Say 'Step 5 of 6: putting jev-claude on your PATH'
    $binDir = Join-Path $installDir 'bin'
    $newShell = $false
    # What the PATH finds today, before this install is added: the Node edition may be there.
    $found = @(Get-Command jev-claude -All -ErrorAction SilentlyContinue | ForEach-Object { Split-Path $_.Source } | Select-Object -Unique)
    if ($env:JEV_NO_MODIFY_PATH -eq '1') {
      Say '        JEV_NO_MODIFY_PATH=1, so your PATH is untouched. Add this folder to it:'
      Say "          $binDir"
    } elseif (Add-UserPath $binDir) {
      Say "        added $binDir to your user PATH"
      $newShell = $true
    } else {
      Say "        $binDir is already on your user PATH"
    }
    if (-not (($env:Path -split ';') | Where-Object { $_.TrimEnd('\') -ieq $binDir.TrimEnd('\') })) { $env:Path = "$binDir;$env:Path" }
    if ($found.Count) {
      Say "        jev-claude was already on your PATH, in: $($found -join ', ')"
      Say "        Whichever folder comes first wins. This install's programs are in $binDir."
    }

    # 6. Prove the installed programs run: the read-only setup check, from the install folder.
    Say ''
    Say 'Step 6 of 6: running the installed setup check (jev-check)'
    $report = & (Join-Path $binDir 'jev-check.exe')
    if ($LASTEXITCODE) {
      $report | ForEach-Object { Say "       $_" }
      throw '[jev] jev-check failed to run, so the install is not usable. Report this with the output above.'
    }
    $report | ForEach-Object { Say "       $_" }
    Say ''
    [pscustomobject]@{ Dir = $installDir; NewShell = $newShell }
  }

  $impl = if ($env:JEV_IMPL) { $env:JEV_IMPL.ToLower() } else { 'node' }
  if ($impl -ne 'node' -and $impl -ne 'rust') { throw "[jev] JEV_IMPL must be node or rust, not '$impl'" }

  # 1-5. The programs, in the chosen edition.
  $newShell = $false
  if ($impl -eq 'rust') {
    $result = @(Install-Rust) | Select-Object -Last 1
    $dir = $result.Dir
    $newShell = $result.NewShell
  } else {
    # 1. Node.js 22 or later, the oldest release line Node.js still supports.
    function Node-Help {
      if (Have nvm) {
        Say 'With nvm-windows: nvm install 24, then nvm use 24, then run this installer again.'
      } else {
        Say 'Install the LTS: winget install OpenJS.NodeJS.LTS (or from https://nodejs.org), then open a new terminal.'
      }
      Say 'More ways, including nvm-windows, fnm, and Volta: https://github.com/alienfacepalm/jev-claude#installing-nodejs'
      Say "Or install the Rust edition, which needs no Node.js: set $env:JEV_IMPL = 'rust' and run this installer again."
    }
    if (-not (Have node)) {
      Say 'Node.js 22 or later is required (24 LTS recommended).'
      Node-Help
      throw '[jev] Node.js is not installed.'
    }
    $nodeVersion = [version](node -p 'process.versions.node')
    if ($nodeVersion.Major -lt 22) {
      Say "Node.js 22 or later is required (24 LTS recommended); this is $nodeVersion."
      Node-Help
      throw '[jev] Node.js is too old.'
    }

    # 2. pnpm, through Corepack (which ships with Node) when it is missing.
    if (-not (Have pnpm)) {
      Say 'pnpm not found; installing it'
      corepack enable pnpm 2>$null
      if ($LASTEXITCODE -or -not (Have pnpm)) { Run { npm install --global pnpm } 'Could not install pnpm: https://pnpm.io/installation' }
    }

    # 3. Claude Code is what jev-claude runs; it can be installed afterwards.
    if (-not (Have claude)) {
      Say 'Claude Code was not found. Install it before running jev-claude: https://code.claude.com/docs/en/setup'
    }

    # 4. The code: this clone when run from one, otherwise a clone kept in JEV_CLAUDE_DIR.
    $here = if ($PSScriptRoot -and (Test-Path (Join-Path $PSScriptRoot 'node\bin\jev-claude.mjs'))) { $PSScriptRoot }
    $dir = if ($here) { $here } elseif ($env:JEV_CLAUDE_DIR) { $env:JEV_CLAUDE_DIR } else { Join-Path $HOME 'jev-claude' }
    if (-not $here) {
      if (-not (Have git)) { throw '[jev] git is required: https://git-scm.com/downloads' }
      if (Test-Path (Join-Path $dir '.git')) {
        Say "Updating $dir"
        Run { git -C $dir pull --ff-only } "Could not update $dir"
      } elseif (Test-Path $dir) {
        throw "[jev] $dir exists but is not a jev-claude clone. Set JEV_CLAUDE_DIR to another folder."
      } else {
        Say "Downloading to $dir"
        Run { git clone --depth 1 $repo $dir } "Could not download $repo"
      }
    }

    # 5. Dependencies, and the jev-claude command on the PATH.
    Say 'Installing dependencies'
    Run { pnpm --dir $dir install --frozen-lockfile } 'Could not install dependencies'

    $newShell = $false
    pnpm bin --global *> $null
    if ($LASTEXITCODE) {
      # pnpm has nowhere to put global commands yet. `pnpm setup` adds one to the user's PATH; this
      # session needs it too, so the folder is set here for the rest of the run.
      Say "Setting up pnpm's folder for commands"
      Run { pnpm setup *> $null } 'Could not set up pnpm'
      if (-not $env:PNPM_HOME) { $env:PNPM_HOME = Join-Path $env:LOCALAPPDATA 'pnpm' }
      $env:Path = "$env:PNPM_HOME\bin;$env:PNPM_HOME;$env:Path"
      $newShell = $true
    }
    Say 'Installing the jev-claude command'
    Run { pnpm add --global "link:$dir" *> $null } 'Could not install the jev-claude command'
  }

  # 6. The keys, kept in ~\.jev-router.env inside your own profile.
  if (-not (Test-Path $envFile)) { Copy-Item (Join-Path $dir '.env.example') $envFile }

  # Sets NAME=value: over an existing NAME= line or its commented-out "# NAME=" placeholder from
  # .env.example, or as a new line at the end.
  function Set-Key([string]$name, [string]$value) {
    $done = $false
    $lines = @(Get-Content $envFile | ForEach-Object {
      if (-not $done -and ($_ -match "^(# )?$name=")) { $done = $true; "$name=$value" } else { $_ }
    })
    if (-not $done) { $lines += "$name=$value" }
    # UTF-8 without a byte-order mark, which Windows PowerShell's own writers would add.
    [IO.File]::WriteAllLines($envFile, [string[]]$lines)
  }
  function Has-Key([string]$pattern) { [bool](Select-String -Path $envFile -Pattern "^($pattern)=.+" -Quiet) }
  function Ask-Secret([string]$prompt) {
    $secure = Read-Host "[jev] $prompt" -AsSecureString
    [Runtime.InteropServices.Marshal]::PtrToStringBSTR([Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure))
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
  if ($newShell) { Say 'Open a new terminal first, so it can find the jev-claude command.' }
}

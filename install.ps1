# Installs or updates jev-claude (a fork of jev-router, for Claude Code) on Windows:
#
#   irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 | iex
#
# or run .\install.ps1 from a clone. Running it again updates the install.
#
#   $env:JEV_CLAUDE_DIR     where to clone (default ~\jev-claude); ignored when run from a clone
#   $env:JEV_CLAUDE_REPO    the repository to clone (default this one)
#   $env:JEV_API_KEY        your Jev key, to save it without the prompt
#   $env:ANTHROPIC_API_KEY  an Anthropic API key to run Claude Code on, to save it without the prompt
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

  # 1. Node.js 22 or later, the oldest release line Node.js still supports.
  function Node-Help {
    if (Have nvm) {
      Say 'With nvm-windows: nvm install 24, then nvm use 24, then run this installer again.'
    } else {
      Say 'Install the LTS: winget install OpenJS.NodeJS.LTS (or from https://nodejs.org), then open a new terminal.'
    }
    Say 'More ways, including nvm-windows, fnm, and Volta: https://github.com/alienfacepalm/jev-claude#installing-nodejs'
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
  $here = if ($PSScriptRoot -and (Test-Path (Join-Path $PSScriptRoot 'bin\jev-claude.mjs'))) { $PSScriptRoot }
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

# Installs or updates jev-claude (Jev Router for Claude Code) on Windows:
#
#   irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 | iex
#
# or run .\install.ps1 from a clone. Running it again updates the install.
#
#   $env:JEV_CLAUDE_DIR   where to clone (default ~\jev-claude); ignored when run from a clone
#   $env:JEV_CLAUDE_REPO  the repository to clone (default this one)
#   $env:JEV_API_KEY      your Jev key, to skip the prompt
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

  # 1. Node.js 20.12 or later.
  if (-not (Have node)) { throw '[jev] Node.js 20.12 or later is required: https://nodejs.org' }
  $nodeVersion = [version](node -p 'process.versions.node')
  if ($nodeVersion -lt [version]'20.12') { throw "[jev] Node.js 20.12 or later is required; this is $nodeVersion." }

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

  # 6. The Jev key, kept in ~\.jev-router.env inside your own profile.
  $hasKey = (Test-Path $envFile) -and (Select-String -Path $envFile -Pattern '^(JEV_API_KEY|TYPESAFE_API_KEY)=.+' -Quiet)
  if ($hasKey) {
    Say "Using the Jev key already in $envFile"
  } else {
    $key = $env:JEV_API_KEY
    if (-not $key) {
      Say 'Get a Jev API key at https://console.typesafe.ai/keys'
      $secure = Read-Host '[jev] Paste it here (it will not be shown), or press Enter to add it later' -AsSecureString
      $key = [Runtime.InteropServices.Marshal]::PtrToStringBSTR([Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure))
    }
    if (-not (Test-Path $envFile)) { Copy-Item (Join-Path $dir '.env.example') $envFile }
    if ($key) {
      $lines = @(Get-Content $envFile)
      if ($lines -match '^JEV_API_KEY=') {
        $lines = $lines | ForEach-Object { if ($_ -match '^JEV_API_KEY=') { "JEV_API_KEY=$key" } else { $_ } }
      } else {
        $lines += "JEV_API_KEY=$key"
      }
      # UTF-8 without a byte-order mark, which Windows PowerShell's own writers would add.
      [IO.File]::WriteAllLines($envFile, [string[]]$lines)
      Say "Saved your key to $envFile"
    } else {
      Say "Add your key later: open $envFile and paste it after JEV_API_KEY="
    }
  }

  Say 'Done. Start it with: jev-claude'
  if ($newShell) { Say 'Open a new terminal first, so it can find the jev-claude command.' }
}

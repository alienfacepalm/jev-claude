#!/usr/bin/env bash
# Installs or updates jev-claude (a fork of jev-router, for Claude Code) on macOS, Linux, or Git Bash:
#
#   curl -fsSL https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.sh | bash
#
# or run ./install.sh from a clone. Running it again updates the install.
#
#   JEV_CLAUDE_DIR     where to clone (default ~/jev-claude); ignored when run from a clone
#   JEV_CLAUDE_REPO    the repository to clone (default this one)
#   JEV_API_KEY        your Jev key, to save it without the prompt
#   ANTHROPIC_API_KEY  an Anthropic API key to run Claude Code on, to save it without the prompt
set -euo pipefail

REPO="${JEV_CLAUDE_REPO:-https://github.com/alienfacepalm/jev-claude.git}"
ENV_FILE="$HOME/.jev-router.env"

say() { printf '[jev] %s\n' "$*"; }
fail() {
  printf '[jev] %s\n' "$*" >&2
  exit 1
}

# 1. Node.js 22 or later, the oldest release line Node.js still supports.
node_help() {
  # nvm is a shell function that a piped script cannot see, so look for its files instead.
  if [ -s "${NVM_DIR:-$HOME/.nvm}/nvm.sh" ]; then
    say "With nvm: nvm install 24 && nvm alias default 24, then run this installer again."
  elif command -v nvm.exe >/dev/null 2>&1; then
    say "With nvm-windows: nvm install 24, then nvm use 24, then run this installer again."
  else
    say "Install the LTS from https://nodejs.org, or see the README for nvm, fnm, Volta, and package managers:"
  fi
  say "https://github.com/alienfacepalm/jev-claude#installing-nodejs"
}
if ! command -v node >/dev/null 2>&1; then
  say "Node.js 22 or later is required (24 LTS recommended)."
  node_help
  exit 1
fi
if ! node -e 'process.exit(Number(process.versions.node.split(".")[0]) >= 22 ? 0 : 1)'; then
  say "Node.js 22 or later is required (24 LTS recommended); this is $(node --version)."
  node_help
  exit 1
fi

# 2. pnpm, through Corepack (which ships with Node) when it is missing.
if ! command -v pnpm >/dev/null 2>&1; then
  say "pnpm not found; installing it"
  corepack enable pnpm 2>/dev/null || npm install --global pnpm || fail "Could not install pnpm: https://pnpm.io/installation"
fi

# 3. Claude Code is what jev-claude runs; it can be installed afterwards.
command -v claude >/dev/null 2>&1 ||
  say "Claude Code was not found. Install it before running jev-claude: https://code.claude.com/docs/en/setup"

# 4. The code: this clone when run from one, otherwise a clone kept in JEV_CLAUDE_DIR.
here=""
if [ -f "$(dirname "${BASH_SOURCE[0]:-.}")/node/bin/jev-claude.mjs" ]; then
  here="$(cd "$(dirname "${BASH_SOURCE[0]:-.}")" && pwd)"
fi
DIR="${here:-${JEV_CLAUDE_DIR:-$HOME/jev-claude}}"
if [ -z "$here" ]; then
  command -v git >/dev/null 2>&1 || fail "git is required: https://git-scm.com/downloads"
  if [ -d "$DIR/.git" ]; then
    say "Updating $DIR"
    git -C "$DIR" pull --ff-only
  elif [ -e "$DIR" ]; then
    fail "$DIR exists but is not a jev-claude clone. Set JEV_CLAUDE_DIR to another folder."
  else
    say "Downloading to $DIR"
    git clone --depth 1 "$REPO" "$DIR"
  fi
fi

# 5. Dependencies, and the jev-claude command on the PATH.
say "Installing dependencies"
(cd "$DIR" && pnpm install --frozen-lockfile)

new_shell=0
if ! pnpm bin --global >/dev/null 2>&1; then
  # pnpm has nowhere to put global commands yet. `pnpm setup` adds one to the shell profile; this
  # script's own PATH needs it too, so the folder is set here for the rest of the run.
  say "Setting up pnpm's folder for commands"
  pnpm setup >/dev/null
  case "$(uname -s)" in
    Darwin) export PNPM_HOME="${PNPM_HOME:-$HOME/Library/pnpm}" ;;
    MINGW* | MSYS* | CYGWIN*) export PNPM_HOME="${PNPM_HOME:-$LOCALAPPDATA/pnpm}" ;;
    *) export PNPM_HOME="${PNPM_HOME:-${XDG_DATA_HOME:-$HOME/.local/share}/pnpm}" ;;
  esac
  export PATH="$PNPM_HOME/bin:$PNPM_HOME:$PATH"
  new_shell=1
fi
say "Installing the jev-claude command"
# pnpm is a native program: on Git Bash, MSYS, or Cygwin it needs the Windows form of the path,
# and reads /c/Users/... as a folder that does not exist, leaving a global install it cannot read.
link_dir="$DIR"
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) link_dir="$(cygpath -w "$DIR")" ;;
esac
pnpm add --global "link:$link_dir" >/dev/null

# 6. The keys, kept in ~/.jev-router.env where only you can read it.
[ -f "$ENV_FILE" ] || cp "$DIR/.env.example" "$ENV_FILE"
chmod 600 "$ENV_FILE"

# Sets NAME=value in the settings file: over an existing NAME= line or its commented-out
# "# NAME=" placeholder from .env.example, or as a new line at the end.
set_key() {
  local name="$1" value="$2" tmp line done=0
  tmp="$(mktemp)"
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
      "$name="* | "# $name="*)
        if [ "$done" = 0 ]; then
          printf '%s=%s\n' "$name" "$value"
          done=1
        else
          printf '%s\n' "$line"
        fi
        ;;
      *) printf '%s\n' "$line" ;;
    esac
  done <"$ENV_FILE" >"$tmp"
  [ "$done" = 1 ] || printf '%s=%s\n' "$name" "$value" >>"$tmp"
  mv "$tmp" "$ENV_FILE"
  chmod 600 "$ENV_FILE"
}

# Reads a secret from the terminal without showing it; empty when there is no terminal.
ask_secret() {
  local answer=""
  if { : </dev/tty; } 2>/dev/null; then
    printf '[jev] %s' "$1" >/dev/tty
    IFS= read -rs answer </dev/tty || answer=""
    printf '\n' >/dev/tty
  fi
  printf '%s' "$answer"
}

has_key() { grep -Eq "^($1)=.+" "$ENV_FILE"; }

# The Jev key is required for routing.
if has_key 'JEV_API_KEY|TYPESAFE_API_KEY'; then
  say "Using the Jev key already in $ENV_FILE"
else
  key="${JEV_API_KEY:-}"
  if [ -z "$key" ]; then
    say "Get a Jev API key at https://console.typesafe.ai/keys"
    key="$(ask_secret 'Paste it here (it will not be shown), or press Enter to add it later: ')"
  fi
  if [ -n "$key" ]; then
    set_key JEV_API_KEY "$key"
    say "Saved your Jev key to $ENV_FILE"
  else
    say "Add your Jev key later: open $ENV_FILE and paste it after JEV_API_KEY="
  fi
fi

# An Anthropic API key is optional: without one, Claude Code uses your own sign-in.
if has_key 'ANTHROPIC_API_KEY'; then
  say "Using the Anthropic API key already in $ENV_FILE"
else
  key="${ANTHROPIC_API_KEY:-}"
  if [ -z "$key" ]; then
    say "Optional: run Claude Code on an Anthropic API key, billed per token, instead of your Claude sign-in."
    key="$(ask_secret 'Paste the key (it will not be shown), or press Enter to keep your sign-in: ')"
  fi
  if [ -n "$key" ]; then
    set_key ANTHROPIC_API_KEY "$key"
    say "Saved your Anthropic API key. Claude Code asks once, on its first start, whether to use it."
  fi
fi

say "Done. Start it with: jev-claude"
[ "$new_shell" = 0 ] || say "Open a new terminal first, so it can find the jev-claude command."

#!/usr/bin/env bash
# Installs or updates jev-claude (a fork of jev-router, for Claude Code) on macOS, Linux, or Git Bash:
#
#   curl -fsSL https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.sh | bash
#
# or run ./install.sh from a clone. Running it again updates the install.
#
# It installs one of two editions of the same program:
#
#   node (default)  a git clone run with Node.js 22+ and pnpm
#   rust            ready-made native programs from the latest release: no Node.js, no pnpm, no clone
#
# Pick the Rust edition with `--rust` (curl ... | bash -s -- --rust) or JEV_IMPL=rust.
#
#   JEV_IMPL           node (default) or rust
#   JEV_CLAUDE_DIR     where to clone (default ~/jev-claude); ignored when run from a clone
#   JEV_CLAUDE_REPO    the repository to clone (default this one)
#   JEV_API_KEY        your Jev key, to save it without the prompt
#   ANTHROPIC_API_KEY  an Anthropic API key to run Claude Code on, to save it without the prompt
#
# Rust edition only:
#
#   JEV_INSTALL_DIR      where the programs go (default ~/jev-claude-rust)
#   JEV_VERSION          a release tag such as v0.10.0 (default: the latest release)
#   JEV_RELEASE_URL      where releases are downloaded from (default this repository's GitHub releases)
#   JEV_NO_MODIFY_PATH   1 to leave your shell profile alone; the installer then prints the line to add
#   JEV_INSTALL_CLAUDE   1 to install Claude Code when it is missing without asking, 0 never to offer
set -euo pipefail

REPO="${JEV_CLAUDE_REPO:-https://github.com/alienfacepalm/jev-claude.git}"
ENV_FILE="$HOME/.jev-router.env"
IMPL="${JEV_IMPL:-node}"

for arg in "$@"; do
  case "$arg" in
    --rust) IMPL=rust ;;
    --node) IMPL=node ;;
    -h | --help)
      printf 'Usage: install.sh [--node | --rust]\n'
      printf '  --node  the Node.js edition (default): a git clone run with Node.js 22+ and pnpm\n'
      printf '  --rust  the Rust edition: native programs from the latest release, no Node.js\n'
      printf 'The settings that change the install are listed at the top of this file.\n'
      exit 0
      ;;
    *)
      printf '[jev] Unknown option %s (try --rust, --node or --help)\n' "$arg" >&2
      exit 1
      ;;
  esac
done
case "$IMPL" in
  node | rust) ;;
  *)
    printf '[jev] JEV_IMPL must be node or rust, not "%s"\n' "$IMPL" >&2
    exit 1
    ;;
esac

say() { printf '[jev] %s\n' "$*"; }
fail() {
  printf '[jev] %s\n' "$*" >&2
  exit 1
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

# Asks a yes/no question on the terminal; "no" when there is no terminal. $2 is the default.
ask_yes_no() {
  local answer=""
  if { : </dev/tty; } 2>/dev/null; then
    printf '[jev] %s ' "$1" >/dev/tty
    IFS= read -r answer </dev/tty || answer=""
  fi
  case "${answer:-$2}" in
    [Yy]*) return 0 ;;
    *) return 1 ;;
  esac
}

# --------------------------------------------------------------------------------------------
# The Node edition: a clone, run with Node.js and pnpm.
# --------------------------------------------------------------------------------------------
install_node() {
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
    say "Or install the Rust edition, which needs no Node.js: run this installer again with --rust"
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
}

# --------------------------------------------------------------------------------------------
# The Rust edition: native programs from a release, no Node.js.
# --------------------------------------------------------------------------------------------
RELEASE_BASE="${JEV_RELEASE_URL:-https://github.com/alienfacepalm/jev-claude/releases}"
CLAUDE_INSTALL_SH="https://claude.ai/install.sh"
SOURCE_BUILD_DOC="https://github.com/alienfacepalm/jev-claude/blob/master/doc/RUST.md#install-the-toolchain"

# One line of the readiness report: status (ok, info, warn, FAIL), what was checked, what was found.
check() { printf '[jev]   %-5s %-18s %s\n' "$1" "$2" "$3"; }

# Reads "version" out of a package.json without Node or jq.
version_in() { sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "$1/package.json" 2>/dev/null | head -n 1; }

# Prints the SHA-256 of a file with whichever tool the machine has.
sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d ' ' -f 1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d ' ' -f 1
  else
    openssl dgst -sha256 "$1" | sed 's/^.*= *//'
  fi
}

# The HTTP status of a URL ("000" when nothing answered), for telling "offline" from "not published".
http_status() { curl -s -o /dev/null -w '%{http_code}' --max-time 20 -L "$@" 2>/dev/null || true; }

# Does this `claude` need Node.js? The old npm package is a script run by node; the native one is not.
claude_needs_node() {
  local file="$1" first
  file="$(readlink -f "$file" 2>/dev/null || printf '%s' "$file")"
  case "$file" in *.js | *.mjs | *.cjs | *.cmd) return 0 ;; esac
  first="$(head -c 80 "$file" 2>/dev/null | head -n 1 | tr -d '\0\r')"
  case "$first" in '#!'*node*) return 0 ;; esac
  return 1
}

install_rust() {
  local os arch target asset url sums_url failures="" claude_path claude_version native_note
  local previous="" exe installed sum_expected sum_actual stage old

  say "Installing jev-claude, Rust edition: native programs, no Node.js or pnpm needed."
  say ""
  say "Step 1 of 6: checking this machine"

  # The platform decides which archive to fetch.
  case "$(uname -s)" in
    Linux) os=linux ;;
    Darwin) os=macos ;;
    MINGW* | MSYS* | CYGWIN*) os=windows ;;
    *) os="" ;;
  esac
  case "$(uname -m)" in
    x86_64 | amd64) arch=x64 ;;
    arm64 | aarch64) arch=arm64 ;;
    *) arch="" ;;
  esac
  # A shell started under Rosetta reports x86_64 on an Apple-silicon Mac; the native programs run there.
  if [ "$os" = macos ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then arch=arm64; fi
  target="$os-$arch"
  case "$target" in
    linux-x64 | linux-arm64 | macos-x64 | macos-arm64 | windows-x64)
      check ok "Platform" "$target"
      ;;
    *)
      check FAIL "Platform" "$(uname -s) $(uname -m) has no prebuilt programs"
      failures="$failures
[jev]   - Build from source instead: $SOURCE_BUILD_DOC"
      ;;
  esac
  asset="jev-claude-$target.tar.gz"

  # What the installer itself needs.
  if command -v curl >/dev/null 2>&1; then
    check ok "Download tool" "$(curl --version | head -n 1 | cut -d ' ' -f 1-2)"
  else
    check FAIL "Download tool" "curl was not found"
    failures="$failures
[jev]   - Install curl (macOS has it; Debian/Ubuntu: sudo apt-get install curl), then run this installer again"
  fi
  if command -v tar >/dev/null 2>&1; then
    check ok "tar" "$(command -v tar)"
  else
    check FAIL "tar" "tar was not found"
    failures="$failures
[jev]   - Install tar (Debian/Ubuntu: sudo apt-get install tar gzip), then run this installer again"
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    check ok "Checksum tool" "sha256sum"
  elif command -v shasum >/dev/null 2>&1; then
    check ok "Checksum tool" "shasum"
  elif command -v openssl >/dev/null 2>&1; then
    check ok "Checksum tool" "openssl"
  else
    check FAIL "Checksum tool" "none of sha256sum, shasum or openssl was found"
    failures="$failures
[jev]   - Install coreutils (sha256sum), then run this installer again: the download cannot be trusted unchecked"
  fi

  # Can the release host be reached? Any HTTP answer counts; only silence is a network problem.
  if command -v curl >/dev/null 2>&1; then
    local origin
    origin="$(printf '%s' "$RELEASE_BASE" | sed -n 's|^\(https\{0,1\}://[^/]*\).*|\1|p')"
    if [ -n "$origin" ]; then
      if [ "$(http_status -I "$origin")" = 000 ]; then
        check FAIL "Network" "cannot reach $origin"
        failures="$failures
[jev]   - Check your internet connection, VPN or proxy (HTTPS_PROXY), then run this installer again"
      else
        check ok "Network" "$origin answers"
      fi
    fi
  fi

  # Claude Code is what jev-claude runs.
  claude_path="$(command -v claude 2>/dev/null || true)"
  if [ -z "$claude_path" ] && [ -x "$HOME/.local/bin/claude" ]; then
    # Anthropic's installer puts it here, and a shell that has not been restarted may not list it.
    claude_path="$HOME/.local/bin/claude"
    export PATH="$HOME/.local/bin:$PATH"
  fi
  if [ -z "$claude_path" ]; then
    check warn "Claude Code" "not found (jev-claude runs the real Claude Code)"
    case "$os" in
      windows) say "        Install it from PowerShell: irm https://claude.ai/install.ps1 | iex" ;;
      *)
        local install_claude="${JEV_INSTALL_CLAUDE:-ask}"
        if [ "$install_claude" = ask ]; then
          say "        Anthropic's own installer is: curl -fsSL $CLAUDE_INSTALL_SH | bash"
          if ask_yes_no "Run it now? [y/N]" n; then install_claude=1; else install_claude=0; fi
        fi
        if [ "$install_claude" = 1 ]; then
          say "        Running Anthropic's installer (output below)"
          curl -fsSL "$CLAUDE_INSTALL_SH" | bash || say "        Anthropic's installer did not finish; install Claude Code yourself before running jev-claude."
          if [ -x "$HOME/.local/bin/claude" ]; then
            export PATH="$HOME/.local/bin:$PATH"
            claude_path="$HOME/.local/bin/claude"
          fi
        else
          say "        Install it before running jev-claude: https://code.claude.com/docs/en/setup"
        fi
        ;;
    esac
  fi
  if [ -n "$claude_path" ]; then
    claude_version="$("$claude_path" --version 2>/dev/null | head -n 1 | cut -d " " -f 1 || true)"
    if claude_needs_node "$claude_path"; then
      if command -v node >/dev/null 2>&1; then
        native_note="npm package, runs on Node.js $(node --version)"
        check ok "Claude Code" "${claude_version:-installed} ($native_note)"
      else
        check warn "Claude Code" "${claude_version:-installed} is the npm package, which runs on Node.js, and Node.js is not installed"
        say "        Reinstall Claude Code natively: curl -fsSL $CLAUDE_INSTALL_SH | bash"
      fi
    else
      check ok "Claude Code" "${claude_version:-installed} (native, needs no Node.js)"
    fi
  fi

  # Optional tools: say what each one is for, and do not fail without it.
  if command -v git >/dev/null 2>&1; then
    check ok "git" "$(git --version | cut -d ' ' -f 3)"
  else
    check warn "git" "not found: the status line will not show the branch or worktree"
  fi
  if command -v node >/dev/null 2>&1; then
    check info "Node.js" "$(node --version) found; the Rust edition does not use it"
  else
    check info "Node.js" "not installed; the Rust edition does not need it"
  fi

  if [ -n "$failures" ]; then
    say ""
    say "Nothing was installed: a required check failed. To fix it:$failures"
    exit 1
  fi
  say "        All required checks passed."
  say ""

  # 2. Download.
  if [ -n "${JEV_VERSION:-}" ]; then
    url="$RELEASE_BASE/download/$JEV_VERSION/$asset"
    sums_url="$RELEASE_BASE/download/$JEV_VERSION/SHA256SUMS"
    say "Step 2 of 6: downloading release $JEV_VERSION"
  else
    url="$RELEASE_BASE/latest/download/$asset"
    sums_url="$RELEASE_BASE/latest/download/SHA256SUMS"
    say "Step 2 of 6: downloading the latest release"
  fi
  say "        from $url"
  # Global, not local: the EXIT trap runs after this function has returned.
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  local code progress="-sS" report
  [ -t 2 ] && progress="--progress-bar"
  code="$(curl -L --retry 3 --retry-delay 2 $progress -o "$tmp/$asset" -w '%{http_code}' "$url" || true)"
  if [ "$code" != 200 ] || [ ! -s "$tmp/$asset" ]; then
    case "$code" in
      404)
        say "No prebuilt programs are published at that address yet (HTTP 404)."
        say "Releases carry them from the version that added the Rust edition on. If you pinned a"
        say "version with JEV_VERSION, try a newer one; otherwise build from source: $SOURCE_BUILD_DOC"
        ;;
      000) say "The download did not start: the connection failed. Check your network, VPN or proxy and run this installer again." ;;
      *) say "The download failed (HTTP $code). Run this installer again; if it keeps failing, build from source: $SOURCE_BUILD_DOC" ;;
    esac
    exit 1
  fi
  say "        downloaded $(wc -c <"$tmp/$asset" | tr -d ' ') bytes"

  # 3. Verify. The archive must match the checksum the release published for it.
  say ""
  say "Step 3 of 6: verifying the download"
  code="$(curl -L --retry 3 -sS -o "$tmp/SHA256SUMS" -w '%{http_code}' "$sums_url" 2>/dev/null || true)"
  [ "$code" = 200 ] || fail "Could not fetch the release's SHA256SUMS (HTTP $code); nothing was installed."
  sum_expected="$(awk -v a="$asset" '{ f = $2; sub(/^\*/, "", f) } f == a { print $1 }' "$tmp/SHA256SUMS" | head -n 1)"
  [ -n "$sum_expected" ] || fail "SHA256SUMS has no entry for $asset; nothing was installed."
  sum_actual="$(sha256_of "$tmp/$asset")"
  say "        expected $sum_expected"
  say "        actual   $sum_actual"
  [ "$sum_expected" = "$sum_actual" ] || fail "The checksums differ, so the download is not what was published. Nothing was installed."
  say "        checksum matches"

  # 4. Install: unpack beside the destination, then swap, so a failed unpack never leaves half an install.
  DIR="${JEV_INSTALL_DIR:-$HOME/jev-claude-rust}"
  # Git Bash's tar reads "C:\..." as a host name, so work with the POSIX form of the path.
  [ "$os" = windows ] && DIR="$(cygpath -u "$DIR")"
  say ""
  say "Step 4 of 6: installing into $DIR"
  stage="$DIR.new.$$"
  rm -rf "$stage"
  mkdir -p "$stage"
  tar -xzf "$tmp/$asset" -C "$stage" || fail "Could not unpack the archive."
  installed="$(version_in "$stage")"
  if [ -z "$installed" ] || { [ ! -f "$stage/bin/jev-claude" ] && [ ! -f "$stage/bin/jev-claude.exe" ]; }; then
    rm -rf "$stage"
    fail "The archive is not a jev-claude release."
  fi
  if [ -d "$DIR" ]; then
    previous="$(version_in "$DIR")"
    old="$DIR.old.$$"
    if ! mv "$DIR" "$old" 2>/dev/null; then
      rm -rf "$stage"
      fail "Could not replace $DIR. Close any running jev-claude and Claude Code sessions that use it, then run this installer again."
    fi
    mv "$stage" "$DIR"
    rm -rf "$old"
    if [ "$previous" = "$installed" ]; then
      say "        reinstalled version $installed"
    else
      say "        updated ${previous:-an earlier install} -> $installed"
    fi
  else
    [ -e "$DIR" ] && fail "$DIR exists but is not a jev-claude install. Set JEV_INSTALL_DIR to another folder."
    mv "$stage" "$DIR"
    say "        installed version $installed"
  fi
  exe=""
  [ "$os" = windows ] && exe=".exe"
  for program in jev-claude jev-statusline jev-explain jev-legend jev-check jev-update-check jev-proxy-host; do
    [ -f "$DIR/bin/$program$exe" ] || fail "$program is missing from the install."
  done
  say "        programs: jev-claude jev-statusline jev-explain jev-legend jev-check jev-update-check jev-proxy-host"
  say "        skills:   /jev-calibrate /jev-explain /jev-legend (they run those programs)"

  # 5. PATH.
  say ""
  say "Step 5 of 6: putting jev-claude on your PATH"
  new_shell=0
  # What the PATH finds today, before this install is added: the Node edition may be there.
  local found
  found="$(type -a jev-claude 2>/dev/null | sed 's/^jev-claude is //' | tr '\n' ' ')"
  case ":$PATH:" in
    *":$DIR/bin:"*) say "        $DIR/bin is already on your PATH" ;;
    *)
      export PATH="$DIR/bin:$PATH"
      if [ "${JEV_NO_MODIFY_PATH:-0}" = 1 ]; then
        say "        JEV_NO_MODIFY_PATH=1, so your shell profile is untouched. Add this line to it:"
        say "          export PATH=\"$DIR/bin:\$PATH\""
      else
        local profile
        case "${SHELL:-}" in
          */zsh) profile="$HOME/.zshrc" ;;
          */bash)
            profile="$HOME/.bashrc"
            [ "$os" = macos ] && [ ! -f "$profile" ] && [ -f "$HOME/.bash_profile" ] && profile="$HOME/.bash_profile"
            ;;
          *) profile="$HOME/.profile" ;;
        esac
        if grep -qF "# jev-claude (Rust edition)" "$profile" 2>/dev/null; then
          say "        $profile already has a jev-claude line; update it if the folder changed: $DIR/bin"
        else
          printf '\n# jev-claude (Rust edition)\nexport PATH="%s/bin:$PATH"\n' "$DIR" >>"$profile"
          say "        added to $profile: export PATH=\"$DIR/bin:\$PATH\""
        fi
        new_shell=1
      fi
      ;;
  esac
  if [ "$os" = windows ]; then
    say "        PowerShell and cmd keep their own PATH: run install.ps1 there with \$env:JEV_IMPL='rust' to set it."
  fi
  if [ -n "$found" ]; then
    say "        jev-claude was already on your PATH: $found"
    say "        Whichever folder comes first wins. This install's programs are in $DIR/bin;"
    say "        a profile line that adds another folder after this one would put that one first."
  fi

  # 6. Prove the installed programs run: the read-only setup check, from the install folder.
  say ""
  say "Step 6 of 6: running the installed setup check (jev-check)"
  if report="$("$DIR/bin/jev-check$exe" 2>&1)"; then
    printf '%s\n' "$report" | sed 's/^/[jev]        /'
  else
    printf '%s\n' "$report" | sed 's/^/[jev]        /'
    fail "jev-check failed to run, so the install is not usable. Report this with the output above."
  fi
  say ""
}

# 1-5. The programs, in the chosen edition.
new_shell=0
DIR=""
if [ "$IMPL" = rust ]; then install_rust; else install_node; fi

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

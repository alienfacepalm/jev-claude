#!/usr/bin/env bash
# Installs or updates jev-claude (a fork of jev-router, for Claude Code) on macOS, Linux, WSL, or Git Bash:
#
#   curl -fsSL https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.sh | bash
#
# or run ./install.sh from a clone. Running it again updates the install. In Git Bash it also puts the
# install on your Windows user PATH, so PowerShell and cmd find it too; install.ps1 does the same from
# PowerShell, and installs the same programs the same way.
#
# It installs ready-made native programs from the latest release: no Node.js, no pnpm, no git clone and
# no Rust toolchain. Claude Code is what jev-claude launches, so the installer checks for it first and,
# after asking, installs it with Anthropic's own installer. Missing curl, tar or a checksum tool, and the
# optional git, are offered through your package manager. Every change to your system shows its exact
# command first and waits for a yes; with no terminal to ask on (a pipe, CI), it prints the command
# instead, unless you pass -y.
#
#   -y, --yes            answer yes to every "install this?" question (same as JEV_ASSUME_YES=1)
#   -h, --help           print the settings and exit
#
# Settings (environment variables):
#
#   JEV_INSTALL_DIR      where the programs go (default ~/jev-claude-rust)
#   JEV_VERSION          a release tag such as v0.11.0 (default: the latest release)
#   JEV_RELEASE_URL      where releases are downloaded from (default this repository's GitHub releases)
#   JEV_BIN_DIR          where the programs are linked on macOS, Linux and WSL (default ~/.local/bin)
#   JEV_NO_MODIFY_PATH   1 to leave your shell profile (and, on Windows, your user PATH) alone; the
#                        installer then prints what to add
#   JEV_INSTALL_CLAUDE   1 to install Claude Code when it is missing without asking, 0 never to (the
#                        install then stops, because jev-claude cannot run without it)
#   JEV_ASSUME_YES       1 to answer yes to every prerequisite question
#   JEV_CLAUDE_INSTALL_BASE   test hook only: where Anthropic's install.sh and install.ps1 are fetched
#                        from (default https://claude.ai), so a test can stand in for Anthropic
#   JEV_TEST_USER_ENV_KEY   test hook only: a registry subkey of HKCU to use instead of "Environment" as
#                        the Windows user PATH (Git Bash); running programs are not notified of a change
#   JEV_TEST_PROC_VERSION, JEV_TEST_WSL_MOUNT   test hooks only: the file that says whether this is WSL
#                        (default /proc/version) and where WSL mounts the Windows drives (default /mnt)
#   JEV_API_KEY          your Jev key, to save it without the prompt
#   ANTHROPIC_API_KEY    an Anthropic API key to run Claude Code on, to save it without the prompt
#
# The old --rust and --node options and JEV_IMPL are accepted and ignored: this is the only edition.
#
# Yes/no settings (JEV_ASSUME_YES, JEV_INSTALL_CLAUDE, JEV_NO_MODIFY_PATH) take 1, true or yes for
# "on" and 0, false or no for "off". JEV_INSTALL_DIR and JEV_BIN_DIR may be relative; they are made
# absolute (against the folder you run this from) before anything is written.
# `sh` and `zsh` cannot run this script (it needs pipefail): a pipe into them stops with a message.

if [ -z "${BASH_VERSION:-}" ]; then
  printf '[jev] This installer needs bash: curl -fsSL https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.sh | bash\n' >&2
  exit 1
fi
set -euo pipefail

if [ -z "${HOME:-}" ]; then
  printf '[jev] HOME is not set, so there is nowhere to put the settings file. Set HOME and run this installer again.\n' >&2
  exit 1
fi
ENV_FILE="$HOME/.jev-router.env"
RELEASE_BASE="${JEV_RELEASE_URL:-https://github.com/alienfacepalm/jev-claude/releases}"
RELEASE_BASE="${RELEASE_BASE%/}"
CLAUDE_BASE="${JEV_CLAUDE_INSTALL_BASE:-https://claude.ai}"
CLAUDE_BASE="${CLAUDE_BASE%/}"
CLAUDE_INSTALL_SH="$CLAUDE_BASE/install.sh"
CLAUDE_INSTALL_PS1_CMD="irm $CLAUDE_BASE/install.ps1 | iex"
CLAUDE_SETUP_DOC="https://code.claude.com/docs/en/setup"
SOURCE_BUILD_DOC="https://github.com/alienfacepalm/jev-claude/blob/master/doc/RUST.md#install-the-toolchain"
PROGRAMS="jev-claude jev-statusline jev-explain jev-legend jev-check jev-update-check jev-proxy-host"

ORIG_PATH="$PATH"

# Does this setting say yes (1, true, yes) or no (0, false, no), in any letter case?
flag_on() { case "$(printf '%s' "${1:-}" | tr '[:upper:]' '[:lower:]')" in 1 | true | yes) return 0 ;; *) return 1 ;; esac; }
flag_off() { case "$(printf '%s' "${1:-}" | tr '[:upper:]' '[:lower:]')" in 0 | false | no) return 0 ;; *) return 1 ;; esac; }

ASSUME_YES=0
flag_on "${JEV_ASSUME_YES:-}" && ASSUME_YES=1
NOTES=""
for arg in "$@"; do
  case "$arg" in
    -y | --yes) ASSUME_YES=1 ;;
    --rust) NOTES="$NOTES
--rust|is no longer needed: the native programs are the only edition, so it is ignored" ;;
    --node) NOTES="$NOTES
--node|the Node.js edition has been retired: installing the native programs instead" ;;
    -h | --help)
      # Piped into bash, $0 is the shell itself (not this text), and BASH_SOURCE is empty.
      src="${BASH_SOURCE[0]:-}"
      if [ -n "$src" ] && [ -f "$src" ] && [ "$(head -c 2 "$src")" = '#!' ]; then
        sed -n '2,/^$/p' "$src" | sed -e '$d' -e 's/^# \{0,1\}//'
      else
        printf 'Usage: install.sh [-y | --yes]\nSettings are environment variables; they are listed at the top of install.sh:\nhttps://github.com/alienfacepalm/jev-claude/blob/master/install.sh\n'
      fi
      exit 0
      ;;
    *)
      printf '[jev] Unknown option %s (try --yes or --help)\n' "$arg" >&2
      exit 1
      ;;
  esac
done
case "${JEV_IMPL:-}" in
  "") ;;
  *) NOTES="$NOTES
JEV_IMPL|is no longer needed: the native programs are the only edition, so it is ignored" ;;
esac

say() { printf '[jev] %s\n' "$*"; }
fail() {
  printf '[jev] %s\n' "$*" >&2
  exit 1
}

# One line of the readiness report: status (ok, info, warn, FAIL), what was checked, what was found.
check() { printf '[jev]   %-5s %-18s %s\n' "$1" "$2" "$3"; }

# Is there a terminal to ask on? A piped install has no stdin to read, but usually still has /dev/tty.
# Inside a Claude Code session there is none: an agent's shell may be handed a terminal, but nothing in
# it is to be asked, or answered, there (only -y gets past a question).
have_tty() { [ -z "${CLAUDECODE:-}" ] && { : </dev/tty; } 2>/dev/null; }

# Can a question be answered at all: by -y, or by a person at a terminal?
can_ask() { [ "$ASSUME_YES" = 1 ] || have_tty; }

# Reads a secret from the terminal without showing it; empty when there is no terminal, and always
# empty inside a Claude Code session (an agent must never be asked for a key).
ask_secret() {
  local answer=""
  if have_tty; then
    printf '[jev] %s' "$1" >/dev/tty
    IFS= read -rs answer </dev/tty || answer=""
    printf '\n' >/dev/tty
  fi
  printf '%s' "$answer"
}

# Asks a yes/no question. -y answers yes; with no terminal the answer is no. $2 is the default.
ask_yes_no() {
  local answer=""
  [ "$ASSUME_YES" = 1 ] && return 0
  if have_tty; then
    printf '[jev] %s ' "$1" >/dev/tty
    IFS= read -r answer </dev/tty || answer=""
  fi
  case "${answer:-$2}" in
    [Yy]*) return 0 ;;
    *) return 1 ;;
  esac
}

# Reads "version" out of a package.json without Node or jq.
version_in() { sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "$1/package.json" 2>/dev/null | head -n 1 || true; }

# Does this folder look like a jev-claude install: the package.json the release ships (name jev-router)
# and a jev-claude program in bin/? Anything else is somebody else's folder and is never replaced.
is_jev_install() {
  grep -Eq '"name"[[:space:]]*:[[:space:]]*"jev-router"' "$1/package.json" 2>/dev/null &&
    { [ -f "$1/bin/jev-claude" ] || [ -f "$1/bin/jev-claude.exe" ]; }
}

# A name beside the install that nothing uses yet: the name itself, or with ".1", ".2"... after it. The
# staging folders are never emptied to make room, because whatever is there is not this run's.
free_name() {
  local base="$1" name="$1" n=1
  while [ -e "$name" ] || [ -L "$name" ]; do
    name="$base.$n"
    n=$((n + 1))
  done
  printf '%s' "$name"
}

# Removes the folders an earlier run left beside the install (killed with kill -9, or the power went):
# "<dir>.new.N[.M]" and "<dir>.old.N[.M]", only when they hold a jev-claude install or this installer's
# .download, are older than ten minutes and do not belong to a process that is still running, so a run
# going in another terminal keeps its own. install.ps1 does the same.
sweep_stale_folders() {
  local d rest pid
  for d in "$DIR".new.* "$DIR".old.*; do
    [ -d "$d" ] && [ ! -L "$d" ] || continue
    rest="${d#"$DIR".???.}"
    case "$rest" in '' | *[!0-9.]* | .* | *. | *..*) continue ;; esac
    pid="${rest%%.*}"
    if kill -0 "$pid" 2>/dev/null; then continue; fi
    [ -n "$(find "$d" -prune -mmin +10 2>/dev/null)" ] || continue
    if is_jev_install "$d" || [ -d "$d/.download" ]; then rm -rf "$d" 2>/dev/null || true; fi
  done
}

# An absolute path with no trailing slash, whatever was given: "dir" means the folder this is run from,
# "~/dir" the home folder (a shell does not expand ~ inside an environment variable), and "." or ".."
# are resolved. A bare root stays "/". A Windows drive path (C:\...) is already absolute.
absolute_path() {
  local p="$1" resolved
  # shellcheck disable=SC2088 # the literal "~" is what is being matched
  case "$p" in
    "~") p="$HOME" ;;
    "~/"*) p="$HOME/${p#\~/}" ;;
  esac
  case "$p" in
    /* | [A-Za-z]:[\\/]*) ;;
    *) p="$PWD/$p" ;;
  esac
  while [ "${#p}" -gt 1 ] && [ "${p%/}" != "$p" ]; do p="${p%/}"; done
  case "${p##*/}" in
    . | ..)
      resolved="$(cd "$p" 2>/dev/null && pwd || true)"
      [ -z "$resolved" ] || p="$resolved"
      ;;
  esac
  printf '%s' "$p"
}

# A URL without any user:password@ part, for printing (a private mirror's address can carry one).
redact_url() { printf '%s' "$1" | sed 's|://[^/@]*@|://|'; }

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

# The tool that downloads: curl, else wget (Alpine and minimal containers ship wget only).
find_downloader() {
  if command -v curl >/dev/null 2>&1; then
    DOWNLOADER=curl
  elif command -v wget >/dev/null 2>&1; then
    DOWNLOADER=wget
  else
    DOWNLOADER=""
  fi
}

# download URL FILE [quiet]: prints the HTTP status ("000" when nothing answered, "error" when wget only
# says the server refused). A file:// address is copied, for a mirror on disk.
download() {
  local url="$1" out="$2" progress="-sS" status=0 code=""
  case "$url" in
    file://*)
      if cp "${url#file://}" "$out" 2>/dev/null; then printf '200'; else printf '404'; fi
      return 0
      ;;
  esac
  if [ "$DOWNLOADER" = curl ]; then
    [ -t 2 ] && [ "${3:-}" != quiet ] && progress="--progress-bar"
    # shellcheck disable=SC2086 # $progress is one option or none
    code="$(curl -L --retry 3 --retry-delay 2 $progress -o "$out" -w '%{http_code}' "$url")" || status=$?
    # The server answered 200 but the transfer broke off: not a finished download.
    if [ "$status" != 0 ] && [ "$code" = 200 ]; then code=interrupted; fi
    printf '%s' "${code:-000}"
  else
    wget -q -O "$out" "$url" 2>/dev/null || status=$?
    case "$status" in
      0) printf '200' ;;
      8) printf 'error' ;;
      *) printf '000' ;;
    esac
  fi
}

# The URL's contents on stdout, for piping Anthropic's installer into bash.
download_to_stdout() {
  if [ "$DOWNLOADER" = curl ]; then curl -fsSL "$1"; else wget -qO- "$1"; fi
}

# Does anything answer at this address? Any HTTP answer counts; only silence is a network problem.
# Prints yes, no or unknown (wget cannot always tell a refusal from silence).
reachable() {
  local status=0
  if [ "$DOWNLOADER" = curl ]; then
    [ "$(curl -s -o /dev/null -w '%{http_code}' --max-time 20 -L -I "$1" 2>/dev/null || true)" = 000 ] && echo no || echo yes
  else
    wget -q --spider -T 20 "$1" 2>/dev/null || status=$?
    case "$status" in 0 | 8) echo yes ;; *) echo unknown ;; esac
  fi
}

# Does this `claude` need Node.js? Only the old npm package does: a script run by node. The native build,
# Anthropic's installer and today's npm package alike (it ships the native claude.exe), does not. npm's and
# pnpm's shims for the native one are small scripts that run bin/claude.exe, so they are native too.
claude_needs_node() {
  local file="$1" first
  file="$(readlink -f "$file" 2>/dev/null || printf '%s' "$file")"
  case "$file" in *.js | *.mjs | *.cjs) return 0 ;; esac
  first="$(head -c 80 "$file" 2>/dev/null | head -n 1 | tr -d '\0\r')"
  case "$first" in '#!'*node*) return 0 ;; esac
  # A small wrapper script (a native program is megabytes of machine code): it either runs the native
  # claude.exe, or names node and the package's cli.js.
  if [ "$(wc -c <"$file" 2>/dev/null | tr -d ' ')" -lt 8192 ] 2>/dev/null; then
    head -c 4096 "$file" 2>/dev/null | grep -aqE 'bin[/\\]claude\.exe' && return 1
    head -c 4096 "$file" 2>/dev/null | grep -aqE 'cli\.js|exec node|"node"' && return 0
  fi
  return 1
}

# Is this a Windows program seen from WSL (a path under the mounted Windows drives, /mnt/c/...)? WSL puts the
# Windows PATH on its own, so a Windows Claude Code can be found there; it is not the Linux one jev-claude needs.
is_windows_side() {
  [ "${WSL:-0}" = 1 ] || return 1
  case "$1" in "${JEV_TEST_WSL_MOUNT:-/mnt}"/[A-Za-z]/*) return 0 ;; esac
  return 1
}

# Finds Claude Code. Anthropic's installer puts it in ~/.local/bin, which a shell that has not been
# restarted may not list yet; that folder joins this run's PATH. Under WSL a copy on the Windows side is
# skipped (SKIPPED_WIN_CLAUDE says which): WSL needs a Claude Code of its own.
find_claude() {
  local candidate
  CLAUDE_PATH=""
  SKIPPED_WIN_CLAUDE=""
  while IFS= read -r candidate; do
    [ -n "$candidate" ] || continue
    if is_windows_side "$candidate"; then
      [ -n "$SKIPPED_WIN_CLAUDE" ] || SKIPPED_WIN_CLAUDE="$candidate"
      continue
    fi
    CLAUDE_PATH="$candidate"
    break
  done <<EOT
$(type -ap claude 2>/dev/null || true)
EOT
  if [ -z "$CLAUDE_PATH" ] && [ -x "$HOME/.local/bin/claude" ]; then
    CLAUDE_PATH="$HOME/.local/bin/claude"
    case ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) export PATH="$HOME/.local/bin:$PATH" ;; esac
  fi
}

# Is this git usable? On a Mac without the command line tools, /usr/bin/git is a stub that opens a
# dialog; it is not asked for its version.
git_usable() {
  command -v git >/dev/null 2>&1 || return 1
  if [ "$OS" = macos ] && [ "$(command -v git)" = /usr/bin/git ] && ! xcode-select -p >/dev/null 2>&1; then return 1; fi
  return 0
}

# --------------------------------------------------------------------------------------------
# Package managers, for the prerequisites that can be installed.
# --------------------------------------------------------------------------------------------
PM=""
SUDO=""
PM_NOTE=""
PM_PROBLEM=""

# Is any of these commands installed?
have_any() {
  local name
  for name in "$@"; do command -v "$name" >/dev/null 2>&1 && return 0; done
  return 1
}

find_package_manager() {
  local candidate
  PM=""
  SUDO=""
  PM_NOTE=""
  PM_PROBLEM=""
  for candidate in brew apt-get dnf yum zypper apk pacman; do
    # On Linux, Homebrew is a user's choice; the distribution's own manager comes first.
    if [ "$candidate" = brew ] && [ "$OS" = linux ] && have_any apt-get dnf yum zypper apk pacman; then continue; fi
    if command -v "$candidate" >/dev/null 2>&1; then
      PM="$candidate"
      break
    fi
  done
  # Homebrew refuses to run as root and needs no sudo, so as root it cannot be used at all.
  if [ "$PM" = brew ] && [ "$(id -u)" = 0 ]; then
    PM_PROBLEM="Homebrew will not run as root, so run this installer again as your normal user (or install them yourself)"
    PM=""
  fi
  # Every other manager changes the system and needs root, except in a Windows shell (MSYS2's pacman
  # works as the logged-in user). Sudo is used only when not root and it exists.
  if [ -n "$PM" ] && [ "$PM" != brew ] && [ "$OS" != windows ] && [ "$(id -u)" != 0 ]; then
    if command -v sudo >/dev/null 2>&1; then
      SUDO="sudo"
      PM_NOTE="uses sudo, which may ask for your password"
    else
      PM_NOTE="no-root"
    fi
  fi
}

# The package(s) that provide a tool, for the package manager in use; nothing when it has none.
# Names checked per manager: curl is "curl" everywhere (plus ca-certificates on Debian, whose curl only
# recommends it); tar is "tar", with gzip beside it because tar -z runs the gzip program; the checksum
# tool is coreutils (sha256sum). Homebrew has none of tar or sha256sum under those names (gnu-tar and
# coreutils install as gtar and gsha256sum), and macOS ships tar and shasum anyway.
package_for() {
  case "$1" in
    curl)
      case "$PM" in
        apt-get) echo "curl ca-certificates" ;;
        *) echo curl ;;
      esac
      ;;
    tar)
      case "$PM" in
        brew) echo "" ;;
        apk) echo tar ;;
        *) echo "tar gzip" ;;
      esac
      ;;
    sha256sum)
      case "$PM" in
        brew) echo "" ;;
        *) echo coreutils ;;
      esac
      ;;
    *) echo "$1" ;;
  esac
}

# The exact command that installs the packages in "$@" ("sudo" included when it will be used). Keep it in
# step with pm_run. The flags, per manager: apt-get needs an index first on a fresh machine and -y;
# dnf and yum take -y; zypper takes --non-interactive; apk needs --no-cache so a container with no
# package index can still find the package; pacman takes --noconfirm, and --needed skips what is
# already current.
pm_command() {
  local pre=""
  [ -n "$SUDO" ] && pre="sudo "
  case "$PM" in
    brew) echo "brew install $*" ;;
    apt-get) echo "${pre}apt-get update && ${pre}apt-get install -y $*" ;;
    dnf) echo "${pre}dnf install -y $*" ;;
    yum) echo "${pre}yum install -y $*" ;;
    zypper) echo "${pre}zypper --non-interactive install $*" ;;
    apk) echo "${pre}apk add --no-cache $*" ;;
    pacman) echo "${pre}pacman -S --noconfirm --needed $*" ;;
  esac
}

# Runs that command. Output is shown; it reads nothing from the script's own stdin.
pm_run() {
  # shellcheck disable=SC2086 # $SUDO is "sudo" or nothing
  case "$PM" in
    brew) brew install "$@" </dev/null ;;
    apt-get) $SUDO apt-get update </dev/null && $SUDO apt-get install -y "$@" </dev/null ;;
    dnf) $SUDO dnf install -y "$@" </dev/null ;;
    yum) $SUDO yum install -y "$@" </dev/null ;;
    zypper) $SUDO zypper --non-interactive install "$@" </dev/null ;;
    apk) $SUDO apk add --no-cache "$@" </dev/null ;;
    pacman) $SUDO pacman -S --noconfirm --needed "$@" </dev/null ;;
  esac
}

# --------------------------------------------------------------------------------------------
# Step 1: detect everything first (nothing changes yet), then install what is missing.
# --------------------------------------------------------------------------------------------
OS=""
ARCH=""
TARGET=""
DIR=""
EXE=""
failures=""   # problems the installer cannot fix itself, or that the user declined to let it fix
fixes=""      # the manual fix for each prerequisite that is missing but can be installed
UNFIXABLE=0
MISSING_TOOLS=""
NEED_CLAUDE=0
BROKEN_CLAUDE=0   # 1 when a claude was found but does not run
SKIPPED_WIN_CLAUDE=""
WSL=0
MUSL=0
WIN_ARM_BUILD=""  # set on a Windows-on-ARM Git Bash: its build number, or "unknown"
OFFER_GIT=0
CLAUDE_CMD=""
ASSET=""
CLAUDE_PATH=""
DOWNLOADER=""
FOUND_BEFORE=""
OLD_NODE_LINKS=""
OLD_NODE_CLONE=""

add_failure() { failures="$failures
[jev]   - $1"; }
add_fix() { fixes="$fixes
[jev]   - $1"; }

# Prints the claude version and whether it needs Node.js (the old npm package) or not (the native build).
# Returns 1 (after printing a FAIL row) when the program is there but `claude --version` does not run:
# a native build that cannot load (Alpine without libstdc++), a wrong-architecture or damaged file.
report_claude() {
  local out rc=0 version native_note first
  out="$("$CLAUDE_PATH" --version </dev/null 2>&1)" || rc=$?
  version="$(printf '%s\n' "$out" | head -n 1 | cut -d " " -f 1 || true)"
  if claude_needs_node "$CLAUDE_PATH" && ! command -v node >/dev/null 2>&1; then
    # (without Node.js the npm package cannot be asked for its version)
    check warn "Claude Code" "${version:-installed} is the npm package, which runs on Node.js, and Node.js is not installed"
    say "        Reinstall Claude Code natively: curl -fsSL $CLAUDE_INSTALL_SH | bash"
    return 0
  fi
  case "$version" in
    [0-9]*) ;;
    *) rc=1 ;;
  esac
  if [ "$rc" != 0 ]; then
    first="$(printf '%s\n' "$out" | head -n 1 | cut -c 1-100 || true)"
    check FAIL "Claude Code" "found at $CLAUDE_PATH but it does not run (claude --version failed${first:+: $first})"
    return 1
  fi
  if claude_needs_node "$CLAUDE_PATH"; then
    native_note="npm package, runs on Node.js $(node --version)"
    check ok "Claude Code" "$version ($native_note)"
  else
    check ok "Claude Code" "$version (native build, needs no Node.js)"
  fi
}

# Is this program the Node edition of jev-claude (a script run by node, or a pnpm shim for one)?
is_node_edition() {
  local file="$1" resolved
  resolved="$(readlink -f "$file" 2>/dev/null || printf '%s' "$file")"
  case "$resolved" in *.mjs | *.js | *.cmd | *.ps1 | *.bat | */node/bin/jev-claude*) return 0 ;; esac
  # A shim is a short script that names the .mjs file; a native program is megabytes of machine code.
  if [ -f "$resolved" ] && [ "$(wc -c <"$resolved" 2>/dev/null | tr -d ' ')" -lt 8192 ] 2>/dev/null; then
    head -c 4096 "$resolved" 2>/dev/null | grep -aq '\.mjs' && return 0
  fi
  return 1
}

# Is this a musl libc system (Alpine)?
is_musl() {
  ls /lib/ld-musl-* /usr/lib/ld-musl-* >/dev/null 2>&1 || { ldd --version 2>&1 || true; } | grep -qi musl
}

# The Windows build number on a Windows Git Bash: the last part of `uname -s` (MINGW64_NT-10.0-19045), else
# what `ver` says. Prints nothing when neither tells.
windows_build() {
  local build="${1##*-}"
  case "$build" in
    '' | *[!0-9]*) build="" ;;
  esac
  if [ -z "$build" ]; then
    build="$(MSYS2_ARG_CONV_EXCL='*' MSYS_NO_PATHCONV=1 cmd.exe /d /c ver 2>/dev/null </dev/null | tr -d '\r' | sed -n 's/.*Version [0-9]*\.[0-9]*\.\([0-9][0-9]*\).*/\1/p' | head -n 1 || true)"
  fi
  printf '%s' "$build"
}

detect_platform() {
  local kernel machine libc=""
  kernel="$(uname -s)"
  machine="$(uname -m)"
  case "$kernel" in
    Linux) OS=linux ;;
    Darwin) OS=macos ;;
    MINGW* | MSYS* | CYGWIN*) OS=windows ;;
    *) OS="" ;;
  esac
  case "$machine" in
    x86_64 | amd64) ARCH=x64 ;;
    arm64 | aarch64) ARCH=arm64 ;;
    *) ARCH="" ;;
  esac
  # A shell started under Rosetta reports x86_64 on an Apple-silicon Mac; the native programs run there.
  if [ "$OS" = macos ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then ARCH=arm64; fi
  # Windows on ARM runs the x64 programs through emulation (from Windows 11, build 22000), and a Git Bash
  # there reports either CPU. The programs are the x64 ones either way; the build decides whether they run.
  if [ "$OS" = windows ] && [ "$ARCH" = arm64 ]; then
    ARCH=x64
    WIN_ARM_BUILD="$(windows_build "$kernel")"
    [ -n "$WIN_ARM_BUILD" ] || WIN_ARM_BUILD=unknown
  fi
  TARGET="$OS-$ARCH"
  case "$TARGET" in
    linux-x64 | linux-arm64 | macos-x64 | macos-arm64 | windows-x64)
      if [ "$OS" = linux ]; then
        # The Linux programs are static (musl), so Alpine and glibc systems take the same archive.
        if is_musl; then
          MUSL=1
          libc=", musl libc: the programs are static, so this works"
        fi
        if grep -qi microsoft "${JEV_TEST_PROC_VERSION:-/proc/version}" 2>/dev/null; then
          WSL=1
          libc="$libc (WSL, treated as Linux)"
        fi
      fi
      if [ -n "$WIN_ARM_BUILD" ] && [ "$WIN_ARM_BUILD" != unknown ] && [ "$WIN_ARM_BUILD" -lt 22000 ]; then
        check FAIL "Platform" "Windows 10 on ARM (build $WIN_ARM_BUILD) cannot run the x64 programs"
        add_failure "ARM PCs run the Windows (x64) programs through emulation from Windows 11 on. Update to Windows 11, then run this installer again."
        add_failure "Other platforms can build from source (see $SOURCE_BUILD_DOC), but that is not automated here."
        UNFIXABLE=1
      elif [ -n "$WIN_ARM_BUILD" ] && [ "$WIN_ARM_BUILD" != unknown ]; then
        check ok "Platform" "windows-x64 (this ARM PC runs it through emulation)"
      else
        check ok "Platform" "$TARGET$libc"
      fi
      ;;
    *)
      check FAIL "Platform" "$kernel $machine has no prebuilt programs"
      add_failure "Prebuilt programs exist for macOS (Apple silicon and Intel), Linux (x64 and arm64, glibc and musl) and Windows (x64; ARM PCs run it by emulation)."
      add_failure "Other platforms can build from source (see $SOURCE_BUILD_DOC), but that is not automated here."
      UNFIXABLE=1
      ;;
  esac
  ASSET="jev-claude-$TARGET.tar.gz"
  if [ "$OS" = windows ]; then
    EXE=".exe"
    # The programs find their settings under the Windows profile folder (as install.ps1 does), which a Git
    # Bash with a custom HOME does not share.
    if [ -n "${USERPROFILE:-}" ] && command -v cygpath >/dev/null 2>&1; then
      HOME="$(cygpath -u "$USERPROFILE")"
      export HOME
      ENV_FILE="$HOME/.jev-router.env"
    fi
  fi
  DIR="${JEV_INSTALL_DIR:-$HOME/jev-claude-rust}"
  # Git Bash's tar reads "C:\..." as a host name, so work with the POSIX form of the path.
  if [ "$OS" = windows ] && command -v cygpath >/dev/null 2>&1; then DIR="$(cygpath -u "$DIR")"; fi
  # Absolute and without a trailing slash, so the staging folders beside it are never inside it and the
  # links made later never dangle.
  DIR="$(absolute_path "$DIR")"
}

detect_tools() {
  local origin reach sha_tool=""
  find_downloader
  if [ "$DOWNLOADER" = curl ]; then
    check ok "Download tool" "$(curl --version 2>/dev/null | head -n 1 | cut -d ' ' -f 1-2 || true)"
  elif [ "$DOWNLOADER" = wget ]; then
    check ok "Download tool" "wget (curl was not found)"
  else
    check FAIL "Download tool" "neither curl nor wget was found"
    MISSING_TOOLS="$MISSING_TOOLS curl"
    add_fix "Install curl (macOS has it; Debian/Ubuntu: sudo apt-get install curl), then run this installer again"
  fi
  if command -v tar >/dev/null 2>&1; then
    check ok "tar" "$(command -v tar)"
  else
    check FAIL "tar" "tar was not found"
    MISSING_TOOLS="$MISSING_TOOLS tar"
    add_fix "Install tar (Debian/Ubuntu: sudo apt-get install tar gzip), then run this installer again"
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    sha_tool=sha256sum
  elif command -v shasum >/dev/null 2>&1; then
    sha_tool=shasum
  elif command -v openssl >/dev/null 2>&1; then
    sha_tool=openssl
  fi
  if [ -n "$sha_tool" ]; then
    check ok "Checksum tool" "$sha_tool"
  else
    check FAIL "Checksum tool" "none of sha256sum, shasum or openssl was found"
    MISSING_TOOLS="$MISSING_TOOLS sha256sum"
    add_fix "Install coreutils (sha256sum), then run this installer again: the download cannot be trusted unchecked"
  fi

  # Can the release host be reached? Any HTTP answer counts; only silence is a network problem.
  if [ -n "$DOWNLOADER" ]; then
    origin="$(printf '%s' "$RELEASE_BASE" | sed -n 's|^\(https\{0,1\}://[^/]*\).*|\1|p')"
    if [ -n "$origin" ]; then
      reach="$(reachable "$origin")"
      case "$reach" in
        no)
          check FAIL "Network" "cannot reach $(redact_url "$origin")"
          add_failure "Check your internet connection, VPN or proxy (HTTPS_PROXY), then run this installer again"
          UNFIXABLE=1
          ;;
        unknown) check warn "Network" "could not confirm that $(redact_url "$origin") answers; trying anyway" ;;
        *) check ok "Network" "$(redact_url "$origin") answers" ;;
      esac
    fi
  fi
}

detect_claude() {
  find_claude
  if [ -n "$CLAUDE_PATH" ]; then
    if ! report_claude; then BROKEN_CLAUDE=1; fi
  elif [ -n "$SKIPPED_WIN_CLAUDE" ]; then
    check warn "Claude Code" "only a Windows copy at $SKIPPED_WIN_CLAUDE; WSL needs its own"
    NEED_CLAUDE=1
  else
    check FAIL "Claude Code" "not found (jev-claude launches the real Claude Code, so it is required)"
    NEED_CLAUDE=1
  fi
}

# An existing install folder must be an earlier jev-claude install (or empty): it is replaced, and a
# folder that is somebody else's is never touched. Checked now, with every other check, so nothing has
# been installed by the time this stops the run.
detect_install_dir() {
  [ -n "$DIR" ] || return 0
  if [ -e "$DIR" ] || [ -L "$DIR" ]; then
    if [ -d "$DIR" ] && [ -z "$(ls -A "$DIR" 2>/dev/null)" ]; then return 0; fi
    if ! is_jev_install "$DIR"; then
      check FAIL "Install folder" "$DIR exists but is not a jev-claude install"
      add_failure "Set JEV_INSTALL_DIR to another folder (nothing in $DIR was touched), then run this installer again"
      UNFIXABLE=1
    fi
  fi
}

detect_optional() {
  local rust_version
  if git_usable; then
    check ok "git" "$(git --version | cut -d ' ' -f 3)"
  else
    check warn "git" "not found: the status line will not show the branch or worktree"
    OFFER_GIT=1
  fi
  if command -v rustc >/dev/null 2>&1; then
    rust_version="$(rustc --version 2>/dev/null | cut -d ' ' -f 2 || true)"
    check info "Rust" "${rust_version:-found}; not used, the prebuilt programs are installed"
  fi
  if command -v node >/dev/null 2>&1; then
    check info "Node.js" "$(node --version) found; jev-claude does not use it"
  else
    check info "Node.js" "not installed; jev-claude does not need it"
  fi
}

# An old Node edition (a pnpm link, or a clone in ~/jev-claude) may still be on this machine. It is
# only reported, never removed; the report says how to remove it, and step 5 says which program wins on PATH.
detect_old_node() {
  local entry
  # `type -a` fails when nothing is found, which is the normal case on a clean machine and must not stop a `set -e -o pipefail` script.
  FOUND_BEFORE="$(type -a jev-claude 2>/dev/null | sed 's/^jev-claude is //' | awk '!seen[$0]++' || true)"
  while IFS= read -r entry; do
    [ -n "$entry" ] || continue
    if is_node_edition "$entry"; then OLD_NODE_LINKS="${OLD_NODE_LINKS:+$OLD_NODE_LINKS, }$entry"; fi
  done <<EOT
$FOUND_BEFORE
EOT
  if [ -f "$HOME/jev-claude/node/bin/jev-claude.mjs" ]; then
    OLD_NODE_CLONE="$HOME/jev-claude"
  fi
  if [ -n "$OLD_NODE_LINKS" ] || [ -n "$OLD_NODE_CLONE" ]; then
    check warn "Old Node edition" "found: ${OLD_NODE_LINKS}${OLD_NODE_LINKS:+${OLD_NODE_CLONE:+, }}${OLD_NODE_CLONE}"
    say "        jev-claude no longer has a Node.js edition. Nothing was deleted for you; remove the old one so"
    say "        only this install is left:"
    [ -z "$OLD_NODE_LINKS" ] || say "          pnpm remove --global jev-router"
    [ -z "$OLD_NODE_CLONE" ] || say "          rm -rf \"$OLD_NODE_CLONE\""
    say "        Until then, whichever folder comes first on your PATH wins; this install puts its own folder first."
  fi
}

stop_with_failures() {
  say ""
  say "$1$failures"
  exit 1
}

# What the installer will do about each missing prerequisite is decided here, all questions first;
# then, if nobody objected, it acts.
DO_TOOLS=0
DO_CLAUDE=0
DO_GIT=0
DO_MUSL=0
TOOL_PACKAGES=""
MUSL_PACKAGES=""
GIT_PM_COMMAND=""

# Anthropic's own installer, as the command a person would type.
claude_install_command() {
  if [ "$OS" = windows ]; then
    printf '%s' "powershell.exe -NoProfile -ExecutionPolicy Bypass -Command \"$CLAUDE_INSTALL_PS1_CMD\""
  elif [ "$DOWNLOADER" = wget ]; then
    printf '%s' "wget -qO- $CLAUDE_INSTALL_SH | bash"
  else
    printf '%s' "curl -fsSL $CLAUDE_INSTALL_SH | bash"
  fi
}

# On Alpine, what else Claude Code needs (for a failure message); nothing elsewhere.
musl_note() {
  [ "$MUSL" = 1 ] || return 0
  printf '%s' " (on Alpine it needs libgcc, libstdc++ and ripgrep too: apk add libgcc libstdc++ ripgrep)"
}

# Claude Code's musl build needs libgcc and libstdc++ to load and ripgrep to search; Alpine does not have
# them by default. Works out which are missing and asks to install them (apk only; another musl
# distribution is told the names).
plan_musl_runtime() {
  local pkg cmd
  find_package_manager
  MUSL_PACKAGES=""
  if [ "$PM" != apk ]; then
    say "        Claude Code's musl build needs libgcc, libstdc++ and ripgrep from your distribution ($CLAUDE_SETUP_DOC); this installer installs them only with apk"
    return 0
  fi
  for pkg in libgcc libstdc++ ripgrep; do
    if [ "$pkg" = ripgrep ] && command -v rg >/dev/null 2>&1; then continue; fi
    apk info -e "$pkg" >/dev/null 2>&1 || MUSL_PACKAGES="$MUSL_PACKAGES $pkg"
  done
  MUSL_PACKAGES="${MUSL_PACKAGES# }"
  [ -n "$MUSL_PACKAGES" ] || return 0
  if [ "$PM_NOTE" = no-root ]; then
    add_failure "Claude Code on Alpine needs ${MUSL_PACKAGES// /, }. Installing them needs root and sudo is not installed; as root run: $(pm_command "$MUSL_PACKAGES")"
    return 0
  fi
  cmd="$(pm_command "$MUSL_PACKAGES")"
  say "        Claude Code's musl build needs ${MUSL_PACKAGES// /, } ($CLAUDE_SETUP_DOC). To install${PM_NOTE:+ ($PM_NOTE)}: $cmd"
  if [ -n "$failures" ]; then
    add_failure "Claude Code needs ${MUSL_PACKAGES// /, }. Run: $cmd, then run this installer again"
  elif ! can_ask; then
    add_failure "There is no terminal to ask on. Run that command yourself, or run this installer again with -y"
  elif ask_yes_no "Run it now? [y/N]" n; then
    DO_MUSL=1
  else
    add_failure "Claude Code needs ${MUSL_PACKAGES// /, }. Run: $cmd, then run this installer again"
  fi
}

plan_prerequisites() {
  local tool claude_cmd tools_cmd git_cmd unprovided="" pkg

  # Claude Code first: it is required, so a refusal ends the install before anything else changes.
  if [ "$NEED_CLAUDE" = 1 ]; then
    claude_cmd="$(claude_install_command)"
    CLAUDE_CMD="$claude_cmd"
    # (flag_on / flag_off, not a case on the raw value, so 1/true/yes and 0/false/no work in any letter case.)
    if flag_off "${JEV_INSTALL_CLAUDE:-}"; then
      add_failure "Claude Code is required and JEV_INSTALL_CLAUDE=0 says not to install it. Install it yourself ($CLAUDE_SETUP_DOC): $claude_cmd"
    elif flag_on "${JEV_INSTALL_CLAUDE:-}"; then
      DO_CLAUDE=1
    else
      say "        Claude Code is required. Anthropic's own installer is: $claude_cmd"
      if ! can_ask; then
        add_failure "There is no terminal to ask on. Run that command yourself, or run this installer again with -y (or JEV_INSTALL_CLAUDE=1) to let it install Claude Code"
      elif ask_yes_no "Run it now? [y/N]" n; then
        DO_CLAUDE=1
      else
        add_failure "Claude Code is required. Install it ($CLAUDE_SETUP_DOC): $claude_cmd, then run this installer again"
      fi
    fi
  fi

  # Missing curl, tar or a checksum tool: the package manager can install them.
  if [ -n "$MISSING_TOOLS" ]; then
    find_package_manager
    for tool in $MISSING_TOOLS; do
      pkg="$(package_for "$tool")"
      if [ -n "$pkg" ]; then TOOL_PACKAGES="$TOOL_PACKAGES $pkg"; else unprovided="$unprovided $tool"; fi
    done
    TOOL_PACKAGES="${TOOL_PACKAGES# }"
    if [ -z "$PM" ]; then
      if [ -n "$PM_PROBLEM" ]; then
        add_failure "Missing:${MISSING_TOOLS}. $PM_PROBLEM"
      else
        add_failure "Missing:${MISSING_TOOLS}. No package manager was found to install them; install them yourself, then run this installer again"
      fi
    elif [ -n "$unprovided" ]; then
      add_failure "Missing:${unprovided}. $PM cannot provide them. Install them yourself (on a Mac, xcode-select --install brings tar and shasum), then run this installer again"
    elif [ "$PM_NOTE" = no-root ]; then
      add_failure "Missing:${MISSING_TOOLS}. Installing them needs root and sudo is not installed; as root run: $(pm_command "$TOOL_PACKAGES")"
    else
      tools_cmd="$(pm_command "$TOOL_PACKAGES")"
      say "        Missing:${MISSING_TOOLS}. To install${PM_NOTE:+ ($PM_NOTE)}: $tools_cmd"
      if [ -n "$failures" ]; then
        # The install is already stopping (Claude Code was refused), so there is no point asking.
        add_failure "Missing:${MISSING_TOOLS}. Run: $tools_cmd, then run this installer again"
      elif ! can_ask; then
        add_failure "There is no terminal to ask on. Run that command yourself, or run this installer again with -y"
      elif ask_yes_no "Run it now? [y/N]" n; then
        DO_TOOLS=1
      else
        add_failure "Missing:${MISSING_TOOLS}. Run: $tools_cmd, then run this installer again"
      fi
    fi
  fi

  # Claude Code's build for musl (Alpine) needs libgcc, libstdc++ and ripgrep from the distribution.
  if [ "$MUSL" = 1 ] && { [ "$NEED_CLAUDE" = 1 ] || [ "$BROKEN_CLAUDE" = 1 ]; }; then plan_musl_runtime; fi
  if [ "$BROKEN_CLAUDE" = 1 ] && [ "$DO_MUSL" = 0 ] && [ -z "$failures" ]; then
    add_failure "Claude Code at $CLAUDE_PATH does not run. Reinstall it ($CLAUDE_SETUP_DOC): $(claude_install_command)$(musl_note), then run this installer again"
  fi

  # git is optional: offered, never required, and never an error to decline.
  if [ "$OFFER_GIT" = 1 ] && [ -z "$failures" ]; then
    find_package_manager
    if [ -n "$PM" ] && [ "$PM_NOTE" != no-root ]; then
      git_cmd="$(pm_command git)"
      GIT_PM_COMMAND="$git_cmd"
      if can_ask; then
        say "        Optional: git shows the branch in the status line. To install it${PM_NOTE:+ ($PM_NOTE)}: $git_cmd"
        if ask_yes_no "Run it now? [y/N]" n; then DO_GIT=1; fi
      else
        say "        Optional: git shows the branch in the status line. To install it later: $git_cmd"
      fi
    elif [ "$OS" = macos ]; then
      say "        Optional: git shows the branch in the status line. Install it with: xcode-select --install"
    else
      say "        Optional: git shows the branch in the status line: https://git-scm.com/downloads"
    fi
  fi
}

act_on_prerequisites() {
  if [ "$DO_TOOLS" = 1 ]; then
    say "        Running: $(pm_command "$TOOL_PACKAGES")"
    # shellcheck disable=SC2086 # one word per package
    if ! pm_run $TOOL_PACKAGES; then
      add_failure "The package manager did not finish. Run it yourself: $(pm_command "$TOOL_PACKAGES"), then run this installer again"
      [ "$PM" != pacman ] || add_failure "On Arch, a \"target not found\" or 404 means the package database is old: run sudo pacman -Syu, then this installer again"
      stop_with_failures "A prerequisite could not be installed. To fix it:"
    fi
    find_downloader
    if [ -z "$DOWNLOADER" ] || ! command -v tar >/dev/null 2>&1 || ! { command -v sha256sum || command -v shasum || command -v openssl; } >/dev/null 2>&1; then
      add_failure "curl, tar or a checksum tool is still missing after the install. Install them yourself, then run this installer again"
      stop_with_failures "A prerequisite could not be installed. To fix it:"
    fi
    check ok "Prerequisites" "installed${MISSING_TOOLS}"
  fi
  if [ "$DO_MUSL" = 1 ]; then
    say "        Running: $(pm_command "$MUSL_PACKAGES")"
    # shellcheck disable=SC2086 # one word per package
    if ! pm_run $MUSL_PACKAGES; then
      add_failure "The package manager did not finish. Run it yourself: $(pm_command "$MUSL_PACKAGES"), then run this installer again"
      case " $MUSL_PACKAGES " in *" ripgrep "*) add_failure "ripgrep is in Alpine's community repository: enable it in /etc/apk/repositories, then run this installer again" ;; esac
      stop_with_failures "A prerequisite could not be installed. To fix it:"
    fi
    check ok "Prerequisites" "installed ${MUSL_PACKAGES// /, }"
  fi
  if [ "$DO_CLAUDE" = 1 ]; then
    say "        Running: $CLAUDE_CMD (output below)"
    if [ "$OS" = windows ]; then
      powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "$CLAUDE_INSTALL_PS1_CMD" </dev/null || true
    else
      download_to_stdout "$CLAUDE_INSTALL_SH" | bash || true
    fi
    find_claude
    if [ -z "$CLAUDE_PATH" ]; then
      add_failure "Anthropic's installer did not leave a claude program. Install Claude Code yourself ($CLAUDE_SETUP_DOC): $CLAUDE_CMD, then run this installer again"
      stop_with_failures "Claude Code could not be installed. To fix it:"
    fi
    if ! report_claude; then
      add_failure "Anthropic's installer left $CLAUDE_PATH, but it does not run. Install Claude Code yourself ($CLAUDE_SETUP_DOC): $CLAUDE_CMD$(musl_note), then run this installer again"
      stop_with_failures "Claude Code could not be installed. To fix it:"
    fi
    NEW_SHELL=1
  elif [ "$DO_MUSL" = 1 ] && [ "$BROKEN_CLAUDE" = 1 ]; then
    # The libraries were what the claude that was already there lacked: ask it again.
    find_claude
    if ! report_claude; then
      add_failure "Claude Code at $CLAUDE_PATH still does not run after installing ${MUSL_PACKAGES}. Reinstall it ($CLAUDE_SETUP_DOC): $(claude_install_command), then run this installer again"
      stop_with_failures "Claude Code could not be made to run. To fix it:"
    fi
    BROKEN_CLAUDE=0
  fi
  if [ "$DO_GIT" = 1 ]; then
    say "        Running: $GIT_PM_COMMAND"
    if pm_run git && git_usable; then
      check ok "git" "$(git --version | cut -d ' ' -f 3)"
    else
      check warn "git" "could not be installed; the status line will not show the branch (optional)"
    fi
  fi
}

# Alpine: Claude Code is told to use the ripgrep installed with apk (USE_BUILTIN_RIPGREP=0), in the "env" of
# ~/.claude/settings.json. Offered like every other change; with no one to ask, the setting is printed.
configure_musl_ripgrep() {
  local settings="$HOME/.claude/settings.json" tmp
  [ "$MUSL" = 1 ] || return 0
  command -v rg >/dev/null 2>&1 || return 0
  [ -n "$CLAUDE_PATH" ] && [ "$BROKEN_CLAUDE" = 0 ] || return 0
  [ -z "${USE_BUILTIN_RIPGREP:-}" ] || return 0
  if grep -q USE_BUILTIN_RIPGREP "$settings" 2>/dev/null; then return 0; fi
  say "        On Alpine, Claude Code needs USE_BUILTIN_RIPGREP=0, to use the ripgrep installed with apk. In $settings, under \"env\":"
  say "          {\"env\": {\"USE_BUILTIN_RIPGREP\": \"0\"}}"
  if ! can_ask || ! ask_yes_no "Add it for you now? [y/N]" n; then return 0; fi
  if [ ! -e "$settings" ]; then
    if { mkdir -p "$(dirname "$settings")" && printf '{\n  "env": {\n    "USE_BUILTIN_RIPGREP": "0"\n  }\n}\n' >"$settings"; } 2>/dev/null; then
      check ok "ripgrep" "USE_BUILTIN_RIPGREP=0 saved in $settings"
    else
      check warn "ripgrep" "could not write $settings; add the setting above yourself"
    fi
  elif command -v jq >/dev/null 2>&1 && tmp="$(mktemp 2>/dev/null)" && jq '.env = ((.env // {}) + {"USE_BUILTIN_RIPGREP": "0"})' "$settings" >"$tmp" 2>/dev/null && [ -s "$tmp" ]; then
    # (written through, so a settings file that is a link into a dotfiles folder stays a link)
    if cat "$tmp" >"$settings" 2>/dev/null; then check ok "ripgrep" "USE_BUILTIN_RIPGREP=0 added to $settings"; else check warn "ripgrep" "could not write $settings; add the setting above yourself"; fi
    rm -f "$tmp"
  else
    [ -z "${tmp:-}" ] || rm -f "$tmp"
    check warn "ripgrep" "$settings already exists and was not changed (jq is needed to edit it, or it is not valid JSON); add the setting above yourself"
  fi
}

step_check() {
  local name detail
  say "Installing jev-claude: native programs; no Node.js, pnpm, git clone or Rust toolchain needed."
  say ""
  say "Step 1 of 6: checking this machine"
  while IFS='|' read -r name detail; do
    [ -n "$name" ] && check info "$name" "$detail"
  done <<EOT
$NOTES
EOT
  detect_platform
  detect_tools
  detect_claude
  detect_optional
  detect_old_node
  detect_install_dir

  if [ "$UNFIXABLE" = 1 ]; then
    failures="$failures$fixes"
    stop_with_failures "Nothing was installed: a required check failed. To fix it:"
  fi
  if [ -n "$MISSING_TOOLS" ] || [ "$NEED_CLAUDE" = 1 ] || [ "$BROKEN_CLAUDE" = 1 ] || [ "$OFFER_GIT" = 1 ]; then
    plan_prerequisites
    if [ -n "$failures" ]; then
      stop_with_failures "Nothing was installed: a required check failed. To fix it:"
    fi
    act_on_prerequisites
  fi
  configure_musl_ripgrep
  say "        All required checks passed."
  say ""
}

# --------------------------------------------------------------------------------------------
# Steps 2-6: download, verify, install, PATH, prove it runs.
# --------------------------------------------------------------------------------------------
TMP=""
STAGE=""      # the folder the archive is unpacked into, beside the install
OLD_DIR=""    # the earlier install, set aside while the new one proves itself; empty once that is settled
SWAPPED=0     # 1 once the new install is in place at $DIR
NEW_SHELL=0
PATH_DIR=""

# Puts the earlier install back when the new one failed, or the run was cut short, before it was settled.
# Only a folder this run created (the new install) is ever removed, and only to make room for the old one.
rollback_install() {
  [ -n "$OLD_DIR" ] || return 0
  if [ -d "$OLD_DIR" ]; then
    if [ "$SWAPPED" = 1 ]; then rm -rf "$DIR"; fi
    if [ ! -e "$DIR" ] && mv "$OLD_DIR" "$DIR" 2>/dev/null; then
      [ "${1:-}" = quiet ] || printf '[jev] The earlier install was put back.\n' >&2
    else
      printf '[jev] The earlier install is kept in %s; move it back to %s by hand.\n' "$OLD_DIR" "$DIR" >&2
    fi
  fi
  OLD_DIR=""
  SWAPPED=0
}

cleanup() {
  rollback_install
  [ -z "$STAGE" ] || rm -rf "$STAGE"
  [ -z "$TMP" ] || rm -rf "$TMP"
}
trap cleanup EXIT
trap 'exit 130' INT HUP TERM

# The shell profile that should carry the PATH line, and the line itself, for this shell.
add_path_to_profile() {
  local profile line candidate
  case "${SHELL:-}" in
    */zsh) profile="${ZDOTDIR:-$HOME}/.zshrc" ;;
    */fish) profile="$HOME/.config/fish/config.fish" ;;
    */bash)
      profile="$HOME/.bashrc"
      if [ "$OS" = macos ]; then
        # Terminal and iTerm start bash as a login shell, which reads the first of these that exists and
        # never .bashrc. Use that file; with none of them, create .bash_profile (nothing else reads a
        # .bashrc on a Mac login, so creating it takes nothing away from the user).
        profile="$HOME/.bash_profile"
        for candidate in "$HOME/.bash_profile" "$HOME/.bash_login" "$HOME/.profile"; do
          if [ -f "$candidate" ]; then
            profile="$candidate"
            break
          fi
        done
      fi
      ;;
    *) profile="$HOME/.profile" ;;
  esac
  case "$profile" in
    */config.fish) line="fish_add_path \"$PATH_DIR\"" ;;
    *) line="export PATH=\"$PATH_DIR:\$PATH\"" ;;
  esac
  # (The marker keeps its old "Rust edition" wording so a profile written by an earlier run is still recognised.)
  if grep -qF "# jev-claude (Rust edition)" "$profile" 2>/dev/null; then
    say "        $profile already has a jev-claude line; update it if the folder changed: $PATH_DIR"
  elif { mkdir -p "$(dirname "$profile")" && printf '\n# jev-claude (Rust edition)\n%s\n' "$line" >>"$profile"; } 2>/dev/null; then
    say "        added to $profile: $line"
    NEW_SHELL=1
  else
    check warn "Profile" "could not write to $profile, so a new terminal will not find jev-claude yet"
    say "        Add this line to your shell profile yourself: $line"
  fi
}

# Where PowerShell is, for the Windows PATH: on PATH, or else by its full name under %SystemRoot%
# (a shell started oddly may lack Windows' own folders). Sets POWERSHELL, or empty.
find_powershell() {
  local candidate="" root
  POWERSHELL="$(command -v powershell.exe 2>/dev/null || command -v pwsh.exe 2>/dev/null || true)"
  if [ -z "$POWERSHELL" ] && [ -n "${SYSTEMROOT:-${SystemRoot:-}}" ] && command -v cygpath >/dev/null 2>&1; then
    root="$(cygpath -u "${SYSTEMROOT:-$SystemRoot}" 2>/dev/null || true)"
    candidate="$root/System32/WindowsPowerShell/v1.0/powershell.exe"
    [ -n "$root" ] && [ -x "$candidate" ] && POWERSHELL="$candidate"
  fi
  return 0
}

# Windows: puts $DIR/bin first on the user PATH in the registry, the way install.ps1 does (the same
# PowerShell code), so PowerShell, cmd and every terminal opened from now on find the programs. %VAR%
# entries stay unexpanded, an entry that is already first is left alone, and the change is broadcast so
# new windows see it. Claude Code's own folder (%USERPROFILE%\.local\bin) goes on last when claude lives
# there and the folder is on neither the user nor the system PATH, because Anthropic's Windows installer
# leaves it off and jev-claude finds claude on the PATH. The script is passed encoded (-EncodedCommand),
# so no quoting or execution policy can get in its way, and the folders are passed in environment
# variables for the same reason.
add_path_to_windows() {
  local winbin claudewin="" want have script encoded out="" outcome="" claude_outcome="" detail
  winbin="$DIR/bin"
  command -v cygpath >/dev/null 2>&1 && winbin="$(cygpath -w "$DIR/bin")"
  # Is Claude Code in Anthropic's folder? Compared as Windows paths, in any letter case.
  if [ -n "$CLAUDE_PATH" ] && command -v cygpath >/dev/null 2>&1; then
    want="$(cygpath -w "$HOME/.local/bin" 2>/dev/null || true)"
    have="$(cygpath -w "$(dirname "$CLAUDE_PATH")" 2>/dev/null || true)"
    want="${want%\\}"
    have="${have%\\}"
    if [ -n "$want" ] && [ "$(printf '%s' "$have" | tr '[:upper:]' '[:lower:]')" = "$(printf '%s' "$want" | tr '[:upper:]' '[:lower:]')" ]; then claudewin="$want"; fi
  fi
  if flag_on "${JEV_NO_MODIFY_PATH:-}"; then
    say "        JEV_NO_MODIFY_PATH=1, so your Windows user PATH is untouched. Add this folder to it:"
    say "          $winbin"
    if [ -n "$claudewin" ]; then
      say "        and Claude Code's folder, which its installer leaves off the PATH (jev-claude finds claude there):"
      say "          $claudewin"
    fi
    return 0
  fi
  if ! command -v cygpath >/dev/null 2>&1; then
    check warn "Windows PATH" "cygpath was not found, so the Windows user PATH was not changed"
    say "        Add this folder to your user PATH yourself: $winbin"
    return 0
  fi
  find_powershell
  if [ -z "$POWERSHELL" ]; then
    check warn "Windows PATH" "powershell.exe was not found, so the Windows user PATH was not changed"
    say "        PowerShell and cmd will not find jev-claude until this folder is on your user PATH"
    say "        (Settings > System > About > Advanced system settings > Environment Variables): $winbin"
    if [ -n "$claudewin" ]; then say "        Claude Code's folder, which its installer leaves off the PATH, needs to be on it too: $claudewin"; fi
    say "        or until you run install.ps1 from PowerShell."
    return 0
  fi
  # (read, not $(cat <<EOT): bash 3.2 misparses a here-document with quotes and parentheses inside $( ).)
  IFS= read -r -d '' script <<'EOT' || true
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
# One PATH entry as Windows reads it, for comparing: %VAR% expanded, quotes, spaces and a trailing separator dropped.
function Get-PathEntryKey([string]$entry) {
  [Environment]::ExpandEnvironmentVariables($entry.Trim().Trim('"').Trim()).TrimEnd('\', '/')
}
try {
  $folder = $env:JEV_WIN_BIN
  $claude = $env:JEV_WIN_CLAUDE
  $want = Get-PathEntryKey $folder
  $claudeOutcome = ''
  # TEST ONLY: JEV_TEST_USER_ENV_KEY is a subkey of HKCU standing in for "Environment", the user PATH.
  $userEnvKey = if ($env:JEV_TEST_USER_ENV_KEY) { $env:JEV_TEST_USER_ENV_KEY } else { 'Environment' }
  $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($userEnvKey, $true)
  if (-not $key) { $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($userEnvKey) }
  try {
    $raw = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $parts = @($raw -split ';' | Where-Object { $_.Trim() })
    $rest = @($parts | Where-Object { (Get-PathEntryKey $_) -ine $want })
    $changed = $false
    if ($parts.Count -gt 0 -and (Get-PathEntryKey $parts[0]) -ieq $want) {
      $outcome = 'present'
    } else {
      $outcome = if ($rest.Count -lt $parts.Count) { 'moved' } else { 'added' }
      $parts = @($folder) + $rest
      $changed = $true
    }
    if ($claude) {
      $cwant = Get-PathEntryKey $claude
      $machine = ''
      try { $machine = [string][Environment]::GetEnvironmentVariable('Path', 'Machine') } catch { $machine = '' }
      $known = @(("$machine;" + ($parts -join ';')) -split ';' | Where-Object { $_.Trim() -and (Get-PathEntryKey $_) -ieq $cwant })
      if ($known.Count) {
        $claudeOutcome = 'present'
      } else {
        $parts = @($parts) + @($claude)
        $claudeOutcome = 'added'
        $changed = $true
      }
    }
    if ($changed) { $key.SetValue('Path', ($parts -join ';'), [Microsoft.Win32.RegistryValueKind]::ExpandString) }
  } finally { $key.Close() }
  if (-not $env:JEV_TEST_USER_ENV_KEY -and ($outcome -ne 'present' -or $claudeOutcome -eq 'added')) {
    try {
      if (-not ('Jev.Native' -as [type])) {
        Add-Type -Namespace Jev -Name Native -MemberDefinition '[DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)] public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint msg, UIntPtr wParam, string lParam, uint flags, uint timeout, out UIntPtr result);'
      }
      $result = [UIntPtr]::Zero
      [void][Jev.Native]::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, 'Environment', 2, 2000, [ref]$result)
    } catch { Write-Output 'NOTIFY=failed' }
  }
  Write-Output ('RESULT=' + $outcome)
  if ($claudeOutcome) { Write-Output ('CLAUDE=' + $claudeOutcome) }
} catch {
  Write-Output ('ERROR=' + $_.Exception.Message)
  exit 1
}
EOT
  # -EncodedCommand wants the script as base64 of UTF-16LE text, which iconv makes (Git for Windows ships it).
  encoded="$(printf '%s' "$script" | iconv -f UTF-8 -t UTF-16LE 2>/dev/null | base64 2>/dev/null | tr -d '\r\n' || true)"
  if [ -z "$encoded" ]; then
    out="ERROR=iconv or base64 was not found"
  else
    # Git Bash rewrites arguments and variables that look like POSIX paths; neither of these is one.
    out="$(MSYS2_ARG_CONV_EXCL='*' MSYS_NO_PATHCONV=1 JEV_WIN_BIN="$winbin" JEV_WIN_CLAUDE="$claudewin" "$POWERSHELL" -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand "$encoded" </dev/null 2>&1 | tr -d '\r' || true)"
  fi
  outcome="$(printf '%s\n' "$out" | sed -n 's/^RESULT=//p' | head -n 1)"
  claude_outcome="$(printf '%s\n' "$out" | sed -n 's/^CLAUDE=//p' | head -n 1)"
  case "$outcome" in
    added)
      say "        added $winbin to your Windows user PATH (PowerShell and cmd, in new windows)"
      NEW_SHELL=1
      ;;
    moved)
      say "        moved $winbin to the front of your Windows user PATH (PowerShell and cmd, in new windows)"
      NEW_SHELL=1
      ;;
    present) say "        $winbin is already first on your Windows user PATH" ;;
    *)
      detail="$(printf '%s\n' "$out" | sed -n 's/^ERROR=//p' | head -n 1)"
      check warn "Windows PATH" "could not change the Windows user PATH${detail:+: $detail}"
      say "        PowerShell and cmd will not find jev-claude until this folder is on your user PATH: $winbin"
      say "        (Settings > System > About > Advanced system settings > Environment Variables, or run install.ps1 from PowerShell.)"
      if [ -n "$claudewin" ]; then
        check warn "Windows PATH" "could not add Claude Code's folder to the Windows user PATH${detail:+: $detail}"
        say "        jev-claude will not find claude in new windows until this folder is on your user PATH: $claudewin"
      fi
      ;;
  esac
  if [ "$claude_outcome" = added ]; then
    say "        added $claudewin to your Windows user PATH too: Claude Code is there, and its installer leaves it off"
    NEW_SHELL=1
  fi
  if printf '%s\n' "$out" | grep -q '^NOTIFY=failed'; then
    say "        (could not notify running programs; sign out and in, or open a new terminal from the Start menu, for the PATH change to apply everywhere)"
  fi
}

install_programs() {
  local url sums_url code sum_expected sum_actual stage previous="" installed program report entry others="" winner
  local linked link_target link_failed=0 move_error

  # An existing folder at the install address must be an earlier jev-claude install (or empty) before anything
  # is downloaded: it is replaced below, and a folder that is somebody else's is never touched.
  if [ -e "$DIR" ] || [ -L "$DIR" ]; then
    if [ -d "$DIR" ] && [ -z "$(ls -A "$DIR" 2>/dev/null)" ]; then
      :
    elif ! is_jev_install "$DIR"; then
      fail "$DIR exists but is not a jev-claude install. Set JEV_INSTALL_DIR to another folder."
    fi
  fi

  # 2. Download.
  if [ -n "${JEV_VERSION:-}" ]; then
    url="$RELEASE_BASE/download/$JEV_VERSION/$ASSET"
    sums_url="$RELEASE_BASE/download/$JEV_VERSION/SHA256SUMS"
    say "Step 2 of 6: downloading release $JEV_VERSION"
  else
    url="$RELEASE_BASE/latest/download/$ASSET"
    sums_url="$RELEASE_BASE/latest/download/SHA256SUMS"
    say "Step 2 of 6: downloading the latest release"
  fi
  say "        from $(redact_url "$url")"
  TMP="$(mktemp -d)" || fail "Could not create a temporary folder (TMPDIR is ${TMPDIR:-/tmp}). Nothing was installed."
  code="$(download "$url" "$TMP/$ASSET")"
  if [ "$code" != 200 ] || [ ! -s "$TMP/$ASSET" ]; then
    case "$code" in
      404)
        say "No prebuilt programs are published at that address (HTTP 404)."
        say "If you pinned a version with JEV_VERSION, check that the tag exists: $(redact_url "$RELEASE_BASE")"
        ;;
      000) say "The download did not start: the connection failed. Check your network, VPN or proxy and run this installer again." ;;
      interrupted) say "The download was interrupted before it finished. Run this installer again." ;;
      *) say "The download failed (HTTP $code). Run this installer again; if it keeps failing, report it with this output." ;;
    esac
    exit 1
  fi
  say "        downloaded $(wc -c <"$TMP/$ASSET" | tr -d ' ') bytes"

  # 3. Verify. The archive must match the checksum the release published for it.
  say ""
  say "Step 3 of 6: verifying the download"
  code="$(download "$sums_url" "$TMP/SHA256SUMS" quiet)"
  [ "$code" = 200 ] || fail "Could not fetch the release's SHA256SUMS (HTTP $code); nothing was installed."
  sum_expected="$(awk -v a="$ASSET" '{ sub(/\r$/, ""); f = $2; sub(/^\*/, "", f) } f == a { print tolower($1) }' "$TMP/SHA256SUMS" | head -n 1)"
  [ -n "$sum_expected" ] || fail "SHA256SUMS has no entry for $ASSET; nothing was installed."
  sum_actual="$(sha256_of "$TMP/$ASSET" | tr '[:upper:]' '[:lower:]')"
  say "        expected $sum_expected"
  say "        actual   $sum_actual"
  [ "$sum_expected" = "$sum_actual" ] || fail "The checksums differ, so the download is not what was published. Nothing was installed."
  say "        checksum matches"

  # 4. Install: unpack beside the destination and check it there, then swap. A failed unpack or a short
  # archive never touches the earlier install, and that one is kept until the new one has run (step 6).
  say ""
  say "Step 4 of 6: installing into $DIR"
  sweep_stale_folders
  stage="$(free_name "$DIR.new.$$")"
  STAGE="$stage"
  mkdir -p "$stage" || fail "Could not create $stage. Check that you can write to that folder, or set JEV_INSTALL_DIR to another folder."
  tar -xzf "$TMP/$ASSET" -C "$stage" || fail "Could not unpack the archive."
  installed="$(version_in "$stage")"
  if [ -z "$installed" ] || { [ ! -f "$stage/bin/jev-claude" ] && [ ! -f "$stage/bin/jev-claude.exe" ]; }; then
    fail "The archive is not a jev-claude release."
  fi
  # Every program must be there before the earlier install is replaced.
  for program in $PROGRAMS; do
    [ -f "$stage/bin/$program$EXE" ] || fail "$program is missing from the release archive; nothing was changed."
  done
  if [ -d "$DIR" ] && [ -n "$(ls -A "$DIR" 2>/dev/null)" ]; then
    is_jev_install "$DIR" || fail "$DIR exists but is not a jev-claude install. Set JEV_INSTALL_DIR to another folder."
    previous="$(version_in "$DIR")"
    OLD_DIR="$(free_name "$DIR.old.$$")"
    if ! mv "$DIR" "$OLD_DIR" 2>/dev/null; then
      OLD_DIR=""
      fail "Could not replace $DIR. Close any running jev-claude and Claude Code sessions that use it, then run this installer again."
    fi
    if ! move_error="$(mv "$stage" "$DIR" 2>&1)"; then
      rollback_install quiet
      fail "Could not move the new programs into $DIR ($move_error). The earlier install was kept."
    fi
    STAGE=""
    SWAPPED=1
    if [ "$previous" = "$installed" ]; then
      say "        reinstalled version $installed"
    else
      say "        updated ${previous:-an earlier install} -> $installed"
    fi
  else
    # Nothing there, or an empty folder (checked above): the new install takes its place.
    if [ -d "$DIR" ]; then rmdir "$DIR" 2>/dev/null || true; fi
    if ! move_error="$(mv "$stage" "$DIR" 2>&1)"; then
      fail "Could not move the new programs into $DIR ($move_error)."
    fi
    STAGE=""
    say "        installed version $installed"
  fi
  say "        programs: $PROGRAMS"
  say "        skills:   /jev-calibrate /jev-explain /jev-legend (they run those programs)"

  # 5. PATH. On macOS, Linux and WSL the programs are linked into a common bin (default ~/.local/bin,
  # which Claude Code's own installer also uses); symlinks keep each program beside its siblings and
  # the package.json it reads its version from. Windows (Git Bash included) has no reliable symlinks,
  # so the install's own bin goes on the Windows user PATH instead (a new Git Bash window inherits it).
  # Nothing here may end the run with a raw tool error: a folder or profile that cannot be written is
  # reported with the manual fix, and the install carries on to the settings file.
  say ""
  say "Step 5 of 6: putting jev-claude on your PATH"
  PATH_DIR="$DIR/bin"
  if [ "$OS" != windows ]; then
    PATH_DIR="$(absolute_path "${JEV_BIN_DIR:-$HOME/.local/bin}")"
    if [ "$PATH_DIR" != "$DIR/bin" ]; then
      mkdir -p "$PATH_DIR" 2>/dev/null || link_failed=1
      if [ "$link_failed" = 0 ]; then
        for program in $PROGRAMS; do
          linked="$PATH_DIR/$program"
          if [ -L "$linked" ]; then
            # Replace this installer's own links (and an old Node edition's); anything else is the user's.
            link_target="$(readlink "$linked" 2>/dev/null || true)"
            case "$link_target" in
              */bin/"$program" | */bin/"$program".mjs) ;;
              *)
                say "        $linked is a link to $link_target, so it was left alone"
                continue
                ;;
            esac
          elif [ -e "$linked" ]; then
            say "        $linked is not a link, so it was left alone"
            continue
          fi
          { rm -f "$linked" && ln -s "$DIR/bin/$program" "$linked"; } 2>/dev/null || link_failed=1
        done
      fi
      if [ "$link_failed" = 1 ]; then
        check warn "Links" "could not create the links in $PATH_DIR (it needs to be a folder you can write to)"
        say "        Putting the install's own folder on your PATH instead: $DIR/bin"
        PATH_DIR="$DIR/bin"
      else
        say "        linked the programs into $PATH_DIR"
      fi
    fi
  fi
  # The PATH this shell started with, not the one this run extended while looking for Claude Code.
  case ":$ORIG_PATH:" in
    *":$PATH_DIR:"*) say "        $PATH_DIR is already on your PATH" ;;
    *)
      export PATH="$PATH_DIR:$PATH"
      if [ "$OS" = windows ]; then
        :   # the Windows user PATH, below, is what every shell starts from
      elif flag_on "${JEV_NO_MODIFY_PATH:-}"; then
        say "        JEV_NO_MODIFY_PATH=1, so your shell profile is untouched. Add this line to it:"
        say "          export PATH=\"$PATH_DIR:\$PATH\""
      else
        add_path_to_profile
      fi
      ;;
  esac
  # Windows: the user PATH in the registry, which PowerShell, cmd and a new Git Bash window all start from.
  if [ "$OS" = windows ]; then add_path_to_windows; fi
  # Other jev-claude programs this shell could find before the install: which one runs depends on the order.
  while IFS= read -r entry; do
    [ -n "$entry" ] || continue
    [ "$entry" = "$PATH_DIR/jev-claude" ] && continue
    [ "$entry" = "$DIR/bin/jev-claude$EXE" ] && continue
    others="$others $entry"
  done <<EOT
$FOUND_BEFORE
EOT
  if [ -n "$others" ]; then
    hash -r 2>/dev/null || true
    winner="$(command -v jev-claude 2>/dev/null || true)"
    say "        jev-claude was already on your PATH:$others"
    if [ "$winner" = "$PATH_DIR/jev-claude" ] || [ "$winner" = "$DIR/bin/jev-claude$EXE" ]; then
      say "        Whichever folder comes first wins; in this shell that is this install's ($PATH_DIR)."
    else
      say "        Whichever folder comes first wins; in this shell that is ${winner:-another one}, not this install's ($PATH_DIR)."
    fi
    say "        A profile line that adds another folder after this one would put that one first."
  fi

  # 6. Prove the installed programs run: the read-only setup check, from the install folder.
  say ""
  say "Step 6 of 6: running the installed setup check (jev-check)"
  if report="$("$DIR/bin/jev-check$EXE" </dev/null 2>&1)"; then
    printf '%s\n' "$report" | sed 's/^/[jev]        /'
  else
    printf '%s\n' "$report" | sed 's/^/[jev]        /'
    # A new install that cannot run must not replace one that can.
    rollback_install
    fail "jev-check failed to run, so the install is not usable. Report this with the output above."
  fi
  # The new install works: the earlier one is no longer needed.
  if [ -n "$OLD_DIR" ]; then
    rm -rf "$OLD_DIR" 2>/dev/null || say "        (the earlier install could not be deleted; remove $OLD_DIR yourself)"
    OLD_DIR=""
  fi
  say ""
}

step_check
install_programs

# The keys, kept in ~/.jev-router.env where only you can read it.
if [ ! -f "$ENV_FILE" ]; then
  cp "$DIR/.env.example" "$ENV_FILE" 2>/dev/null || fail "Could not create $ENV_FILE. The programs are installed; copy $DIR/.env.example there yourself, then add your keys."
fi
chmod 600 "$ENV_FILE" 2>/dev/null || say "        (could not restrict $ENV_FILE to your account; run: chmod 600 \"$ENV_FILE\")"

# Sets NAME=value in the settings file: over an existing NAME= line or its commented-out
# "# NAME=" placeholder from .env.example, or as a new line at the end.
set_key() {
  local name="$1" value="$2" tmp line done=0
  tmp="$(mktemp)" || fail "Could not create a temporary file to save the key. Add it yourself: open $ENV_FILE and set $name="
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
  if [ -L "$ENV_FILE" ]; then
    # A settings file that is a link into a dotfiles folder stays a link: write through it.
    cat "$tmp" >"$ENV_FILE" && rm -f "$tmp"
  else
    mv "$tmp" "$ENV_FILE"
  fi
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
[ "$NEW_SHELL" = 0 ] || say "Open a new terminal first, so it can find the jev-claude command."

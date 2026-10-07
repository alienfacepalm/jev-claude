import { spawn } from "node:child_process";
import { accessSync, constants, readFileSync } from "node:fs";
import { dirname, join } from "node:path";

const WIN = process.platform === "win32";

/**
 * Finds an executable on PATH. Resolving it here rather than leaning on the shell means
 * arguments are passed as an array and a missing install produces a useful message instead of
 * a shell error. `exts` sets the Windows search order; elsewhere the bare name is all there is.
 */
export function resolveCommand(name, { exts, path = process.env.PATH ?? "", win = WIN } = {}) {
  const suffixes = win ? (exts ?? (process.env.PATHEXT ?? ".COM;.EXE;.BAT;.CMD").split(";")) : [""];
  for (const dir of path.split(win ? ";" : ":")) {
    if (!dir) continue;
    for (const ext of suffixes) {
      const file = join(dir.replace(/^"|"$/g, ""), `${name}${ext}`);
      try {
        accessSync(file, win ? constants.F_OK : constants.X_OK);
        return file;
      } catch {
        // Not here; keep looking.
      }
    }
  }
  return null;
}

/**
 * The Node script an npm `.cmd` shim launches, or null.
 *
 * npm writes a shim of the form `"%_prog%" "%dp0%\node_modules\pkg\cli.js" %*`. Running that
 * script with this Node directly sidesteps cmd.exe, whose quoting rules strip the quotes from
 * quoted config values and split any argument containing a space.
 */
export function shimScript(file) {
  try {
    const m = /"%~?dp0%?\\([^"]+?\.[cm]?js)"/i.exec(readFileSync(file, "utf8"));
    if (!m) return null;
    // The shim spells the path with backslashes; split them so it resolves on any platform.
    const script = join(dirname(file), ...m[1].split("\\"));
    accessSync(script, constants.F_OK);
    return script;
  } catch {
    return null;
  }
}

// cmd.exe metacharacters, escaped with a caret. Same set and approach as cross-spawn.
const META = /([()\][%!^"`<>&|;, *?])/g;

/**
 * Quotes one argument for a command line that cmd.exe parses and a batch file then re-parses:
 * MSVCRT quoting first, then the metacharacters caret-escaped once per parse.
 */
export function quoteForCmd(arg) {
  let quoted = String(arg)
    .replace(/(\\*)"/g, '$1$1\\"')
    .replace(/(\\*)$/, "$1$1");
  quoted = `"${quoted}"`.replace(META, "^$1");
  return quoted.replace(META, "^$1");
}

/** How to start a resolved executable: a command, the arguments that precede the user's. */
export function launchSpec(file) {
  if (/\.ps1$/i.test(file)) {
    // A client install's default execution policy refuses to run scripts at all.
    return { command: "powershell.exe", prefix: ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", file] };
  }
  if (/\.(cmd|bat)$/i.test(file)) {
    const script = shimScript(file);
    if (script) return { command: process.execPath, prefix: [script] };
    return { command: process.env.ComSpec ?? "cmd.exe", prefix: [], shim: file };
  }
  return { command: file, prefix: [] };
}

/** Spawns a launch spec with `args`, never through an implicit shell. */
export function spawnSpec(spec, args, options = {}) {
  if (!spec.shim) return spawn(spec.command, [...spec.prefix, ...args], { ...options, shell: false });
  // A shim with no script to run directly. Built verbatim so Node's own quoting stays out of it.
  const line = [spec.shim.replace(META, "^$1"), ...args.map(quoteForCmd)].join(" ");
  return spawn(spec.command, ["/d", "/s", "/c", `"${line}"`], {
    ...options,
    shell: false,
    windowsVerbatimArguments: true,
  });
}

/**
 * The subcommands of the `claude` CLI (as of Claude Code 2.1.292). They manage Claude Code itself
 * rather than start a session, and none of them takes `--add-dir` (`claude mcp list --add-dir x`
 * fails with "unknown option"), so a launch that names one runs it untouched: no proxy, no model
 * routing, no status line, no added arguments.
 */
const CLAUDE_SUBCOMMANDS = new Set([
  "agents",
  "attach",
  "auth",
  "auto-mode",
  "doctor",
  "gateway",
  "import",
  "install",
  "kill",
  "logs",
  "mcp",
  "plugin",
  "plugins",
  "purge",
  "respawn",
  "rm",
  "setup-token",
  "stop",
  "ultrareview",
  "update",
  "upgrade",
]);

/** Whether `args` run a `claude` subcommand: the first argument is exactly one of its names. */
export const isClaudeSubcommand = (args) => CLAUDE_SUBCOMMANDS.has(args[0]);

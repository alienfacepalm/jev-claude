// Single-colour glyphs, so the line takes the terminal's own colours rather than emoji's. The branch
// is the Powerline glyph (U+E0A0) that zsh themes such as agnoster and powerlevel10k use. The rest
// are plain Unicode that stock fonts (Menlo, SF Mono, Cascadia, Consolas) draw; the Nerd Font
// glyphs for effort and directory (U+F0E4, U+F07B) show as a "?" box without a patched font.
// The main model is the sub-agents' star with an outlined one behind it: the same family, and the
// outline marks the one the sub-agents belong to.
const SYMBOLS = { model: "✧✦", effort: "◔", agents: "✦", dir: "❐", branch: "", worktree: "⌂", context: "≡" };

// For a console that cannot draw them: the word instead of the glyph.
const TEXT = { model: "model", effort: "effort", agents: "agents", dir: "dir", branch: "branch", worktree: "worktree", context: "ctx" };

/**
 * The labels for the status line's items. Glyphs everywhere except the legacy Windows console
 * (conhost, as cmd.exe and PowerShell open in by default), whose fonts lack them. Windows
 * Terminal, the VS Code terminal, ConEmu and mintty (Git Bash) draw them and each announces
 * itself in the environment, so the fallback only applies when none of them is present.
 * `JEV_ICONS=symbols` or `text` overrides the guess either way.
 */
export function icons(env = process.env, platform = process.platform) {
  const choice = (env.JEV_ICONS ?? "").toLowerCase();
  if (choice === "symbols") return SYMBOLS;
  if (choice === "text" || choice === "ascii") return TEXT;
  const modern = env.WT_SESSION || env.TERM_PROGRAM || env.ConEmuPID;
  return platform === "win32" && !modern ? TEXT : SYMBOLS;
}

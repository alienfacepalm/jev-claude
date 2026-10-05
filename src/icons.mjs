const EMOJI = { model: "🤖 ", confidence: "🎯", effort: "🧠 ", dir: "📁 ", branch: "🌿 ", worktree: "🌳 ", context: "📊 ", waiting: "⏳ " };

// Plain text for a console that cannot draw emoji. The branch and worktree labels are drawn
// either way, so they need nothing here; the rest are labelled only where the bare value is vague.
const TEXT = { model: "", confidence: "conf", effort: "", dir: "dir ", branch: "", worktree: "", context: "", waiting: "" };

/**
 * The icon set for the status line. Emoji everywhere except the legacy Windows console (conhost,
 * as cmd.exe and PowerShell open in by default), which draws them as empty boxes. Windows
 * Terminal, the VS Code terminal, ConEmu and mintty (Git Bash) all draw them and each announces
 * itself in the environment, so the fallback only applies when none of them is present.
 * `JEV_ICONS=emoji` or `text` overrides the guess either way.
 */
export function icons(env = process.env, platform = process.platform) {
  const choice = (env.JEV_ICONS ?? "").toLowerCase();
  if (choice === "emoji") return EMOJI;
  if (choice === "text" || choice === "ascii") return TEXT;
  const modern = env.WT_SESSION || env.TERM_PROGRAM || env.ConEmuPID;
  return platform === "win32" && !modern ? TEXT : EMOJI;
}

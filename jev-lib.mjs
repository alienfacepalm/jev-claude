// Shared by agent-router.mjs (PreToolUse: Agent) and prompt-router.mjs (UserPromptSubmit).
// Asks TypeSafe Jev which Claude model tier a piece of work needs. Read-only, fails open.
import { readFileSync, appendFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const DIR = dirname(fileURLToPath(import.meta.url));
export const ALLOWED = new Set(["haiku", "sonnet", "opus", "fable"]);
export const MAX_PROMPT_CHARS = 8000;
const TIMEOUT_MS = 6000;

export function loadEnv() {
  const env = {};
  try {
    for (const line of readFileSync(join(DIR, ".env"), "utf8").split("\n")) {
      const t = line.trim();
      const eq = t.indexOf("=");
      if (!t || t.startsWith("#") || eq === -1) continue;
      env[t.slice(0, eq).trim()] = t.slice(eq + 1).trim();
    }
  } catch {}
  return env;
}

export function log(msg) {
  try {
    appendFileSync(join(DIR, "router.log"), `${new Date().toISOString()} ${msg}\n`);
  } catch {}
}

export async function classify(apiKey, query) {
  const res = await fetch("https://api.typesafe.ai/v1/systemone", {
    method: "POST",
    headers: { Authorization: `Bearer ${apiKey}`, "Content-Type": "application/json" },
    signal: AbortSignal.timeout(TIMEOUT_MS),
    body: JSON.stringify({
      state: { query },
      model: "jev-latest",
      questions: {
        model_choice: {
          type: "choice",
          instructions: "Given `query`, which Claude model should handle this task?",
          criteria: {
            haiku: "Quick lookups, file searches, formatting, boilerplate, low-stakes small edits",
            sonnet: "Default day-to-day coding, feature work, most agentic building",
            opus: "Architecture decisions, hard debugging, security-sensitive work, large multi-file refactors, complex planning",
            fable: "Only the very hardest work: novel research-level problems, deep whole-system redesigns, subtle correctness or concurrency bugs that stump strong models, high-stakes decisions needing maximum reasoning",
          },
        },
      },
    }),
  });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const answer = (await res.json()).answers?.model_choice;
  if (!ALLOWED.has(answer?.choice)) throw new Error("unexpected response shape");
  return answer;
}

export function badge(choice) {
  return { haiku: "🟢 HAIKU (cheap)", sonnet: "🟡 SONNET (balanced)", opus: "🔴 OPUS (max power)", fable: "🟣 FABLE (top tier)" }[choice];
}

export function pct(confidence) {
  return Math.round(Number(confidence) * 100);
}

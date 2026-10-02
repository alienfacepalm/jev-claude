#!/usr/bin/env node
// PreToolUse hook (matcher: Agent). Asks TypeSafe Jev which Claude model tier
// (haiku/sonnet/opus/fable) a subagent task needs and rewrites tool_input.model.
// Fails open: any error, timeout or unexpected response leaves the call untouched.
// Only sends the subagent prompt to https://api.typesafe.ai. Key is read from ./.env.
import { readFileSync } from "node:fs";
import { loadEnv, log, classify, badge, pct, MAX_PROMPT_CHARS } from "./jev-lib.mjs";

async function main() {
  const input = JSON.parse(readFileSync(0, "utf8"));
  const ti = input.tool_input ?? {};
  // Forks inherit the parent's model; an explicit model is a deliberate choice.
  if (ti.subagent_type === "fork" || ti.model || !ti.prompt) return;

  const apiKey = loadEnv().TYPESAFE_API_KEY;
  if (!apiKey) return;

  const answer = await classify(apiKey, String(ti.prompt).slice(0, MAX_PROMPT_CHARS));
  log(`agent choice=${answer.choice} conf=${answer.confidence} prompt=${JSON.stringify(String(ti.prompt).slice(0, 60))}`);
  process.stdout.write(
    JSON.stringify({
      systemMessage: `🧭 Jev routed subagent → ${badge(answer.choice)} · ${pct(answer.confidence)}% confident`,
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: "allow",
        updatedInput: { ...ti, model: answer.choice },
      },
    })
  );
}

main().catch((e) => log(`agent error: ${e.message}`)).finally(() => process.exit(0));

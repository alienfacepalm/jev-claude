#!/usr/bin/env node
// UserPromptSubmit hook. Asks TypeSafe Jev which Claude model tier the prompt
// needs and shows the verdict as a system message on every prompt.
// Display only: a UserPromptSubmit hook cannot change the main session's model.
// The subagent hook (agent-router.mjs) is the one that actually rewrites the model.
// Fails open and silent. Only sends the prompt text to https://api.typesafe.ai.
import { readFileSync } from "node:fs";
import { loadEnv, log, classify, badge, pct, MAX_PROMPT_CHARS } from "./jev-lib.mjs";

async function main() {
  const input = JSON.parse(readFileSync(0, "utf8"));
  const prompt = String(input.prompt ?? "").trim();
  if (!prompt) return;

  const apiKey = loadEnv().TYPESAFE_API_KEY;
  if (!apiKey) return;

  const answer = await classify(apiKey, prompt.slice(0, MAX_PROMPT_CHARS));
  log(`prompt choice=${answer.choice} conf=${answer.confidence} prompt=${JSON.stringify(prompt.slice(0, 60))}`);
  process.stdout.write(
    JSON.stringify({
      systemMessage: `🧭 Jev: this prompt → ${badge(answer.choice)} · ${pct(answer.confidence)}% confident (main model unchanged; subagents get routed)`,
      suppressOutput: true,
    })
  );
}

main().catch((e) => log(`prompt error: ${e.message}`)).finally(() => process.exit(0));

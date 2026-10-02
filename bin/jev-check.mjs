#!/usr/bin/env node
// The setup report behind /jev-calibrate. It reads and changes nothing: whether routing is on,
// which models the router is tuned for, which ones this account offers, and whether any of those
// are newer than the tuning. It runs inside Claude Code, which jev-claude starts without the Jev
// key, so routing is read from the session environment jev-claude sets rather than from the key.
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { TIERS, AUTO_MODEL, fableAllowed, tierOf } from "../src/config.mjs";
import { readCalibration } from "../src/status.mjs";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
// A checkout with the calibration tooling and its git history is where tuning happens; an
// installed copy only reports, and gets new tuning through updates.
const repository = existsSync(join(ROOT, ".git")) && existsSync(join(ROOT, "scripts", "calibrate.mjs"));
const routing = process.env.ANTHROPIC_CUSTOM_MODEL_OPTION === AUTO_MODEL;
const { newer, models, at } = readCalibration();

const row = (label, text) => `${label.padEnd(13)}${text}`;
const lines = ["Jev Router setup check (read-only: nothing was changed)", ""];

// jev-claude leaves a model the user set with ANTHROPIC_MODEL alone, so such a session starts on
// that model rather than on the router.
const pinned = process.env.ANTHROPIC_MODEL && process.env.ANTHROPIC_MODEL !== AUTO_MODEL;
lines.push(
  row(
    "Routing",
    !routing
      ? "off - no JEV_API_KEY found. Add JEV_API_KEY=... to ~/.jev-router.env and restart jev-claude."
      : pinned
        ? `available, but this session started on ${process.env.ANTHROPIC_MODEL} because ANTHROPIC_MODEL is set. Choose Jev Router in /model to route.`
        : "on - Jev Router picks a model for each turn",
  ),
);
lines.push(row("Tuned for", TIERS.map((t) => `${t.name} ${t.id}`).join(", ")));

if (at === null) {
  lines.push(row("Your account", "not read yet - Claude Code has not loaded the model list. Run /jev-calibrate again shortly."));
} else {
  lines.push(row("Your account", `${models.join(", ") || "no Claude models listed"} (as of ${new Date(at).toLocaleString()})`));
  const offered = new Set(models.map((id) => tierOf(id)));
  const missing = TIERS.filter((t) => !offered.has(t.name)).map((t) => t.name);
  if (missing.length) lines.push(row("", `not offered to this account: ${missing.join(", ")} (routing steps around them)`));
  lines.push(
    row(
      "Newer models",
      newer.length
        ? `${newer.join(", ")} - routing already uses them, but the router was tuned on the versions before. Update jev-router to get tuning for them.`
        : "none - the router is tuned for the newest models your account offers",
    ),
  );
}

lines.push(
  row(
    "Fable",
    fableAllowed()
      ? "offered when the work calls for it (bills extra usage credits; JEV_ALLOW_FABLE=0 turns it off)"
      : "off (JEV_ALLOW_FABLE is set to turn it off)",
  ),
);
lines.push(row("Mode", repository ? "repository" : "installed"));

console.log(lines.join("\n"));

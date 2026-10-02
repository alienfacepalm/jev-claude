// Scores routing against the labelled prompts in calibration-cases.mjs, so a change to the tier
// guidance, costs, effort or thresholds in src/config.mjs is judged against the version before it
// rather than against one lucky run. Jev's confidence moves a few hundredths between identical
// runs, so each prompt is asked more than once.
//
// Needs JEV_API_KEY. Calls the real Jev, never Claude. Not part of `pnpm test`.
//   node scripts/calibrate.mjs [--runs 2]
import { loadEnv } from "../src/env.mjs";
import { CASES } from "./calibration-cases.mjs";

loadEnv();
const { askJev } = await import("../src/router.mjs");
const { claudeModels, newestPerTier } = await import("../src/proxy.mjs");
const { decide } = await import("../src/policy.mjs");
const { availableTiers, THRESHOLDS } = await import("../src/config.mjs");

const runsAt = process.argv.indexOf("--runs");
const runs = runsAt > 0 ? Number(process.argv[runsAt + 1]) || 2 : 2;

const models = newestPerTier(claudeModels().filter((m) => availableTiers().includes(m.tier)));
const available = [...new Set(models.map((m) => m.tier))];
const current = THRESHOLDS.uncertainDefault;
// A case for a tier this machine cannot route to measures nothing.
const cases = CASES.filter((c) => available.includes(c.want));
const skipped = CASES.length - cases.length;

let hits = 0;
let asked = 0;
let failed = 0;
const byTier = {};
const confidences = [];
for (let run = 1; run <= runs; run++) {
  console.log(`--- run ${run}`);
  for (const { want, prompt } of cases) {
    asked++;
    const tally = (byTier[want] ??= { hits: 0, asked: 0 });
    tally.asked++;
    const answer = await askJev({
      prompt,
      current: models.find((m) => m.tier === current)?.id,
      contextTokens: 0,
      models,
    });
    if (!answer) {
      failed++;
      console.log(`FAIL want=${want.padEnd(6)} no answer from Jev | ${prompt.slice(0, 60)}`);
      continue;
    }
    const chosen = models.find((m) => m.id === answer.choice);
    const { tier } = decide({
      prompt,
      jev: chosen ? { choice: chosen.tier, confidence: answer.confidence } : null,
      current,
      available,
      contextTokens: 0,
    });
    const confidence = Number(answer.confidence);
    confidences.push(confidence);
    if (tier === want) {
      hits++;
      tally.hits++;
    }
    console.log(
      `${tier === want ? "ok  " : "MISS"} want=${want.padEnd(6)} jev=${(chosen?.tier ?? "?").padEnd(6)} ` +
        `conf=${confidence.toFixed(2)} final=${tier.padEnd(6)} | ${prompt.slice(0, 60)}`,
    );
  }
}

confidences.sort((a, b) => a - b);
const median = confidences.length ? confidences[Math.floor(confidences.length / 2)].toFixed(2) : "n/a";
const low = confidences.filter((c) => c < THRESHOLDS.minConfidence).length;
const tiers = Object.entries(byTier)
  .map(([tier, t]) => `${tier} ${t.hits}/${t.asked}`)
  .join(", ");
console.log(
  `\n${hits}/${asked} on the intended tier (${tiers}) · median confidence ${median} · ` +
    `${low} below the ${THRESHOLDS.minConfidence} bar` +
    (failed ? ` · ${failed} unanswered` : "") +
    (skipped ? ` · ${skipped} cases skipped for unavailable tiers` : ""),
);

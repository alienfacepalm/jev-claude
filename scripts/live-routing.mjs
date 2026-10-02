// Asks the real Jev about a few sample prompts and prints its picks. Needs JEV_API_KEY; run with
// `node scripts/live-routing.mjs`. Not part of `pnpm test`, which never calls the network.
import { loadEnv } from "../src/env.mjs";
import { claudeModels, newestPerTier } from "../src/proxy.mjs";

loadEnv();
const { askJev } = await import("../src/router.mjs");
const models = newestPerTier(claudeModels());
const prompts = [
  "fix the typo 'recieve' in README.md",
  "add a unit test for the existing formatDate helper",
  "users intermittently get logged out after deploy, figure out why",
  "migrate the entire monorepo from webpack to vite",
];
for (const prompt of prompts) {
  const a = await askJev({ prompt, current: models.find((m) => m.tier === "sonnet")?.id, contextTokens: 0, models });
  if (!a) {
    console.log(`FAIL  ${prompt}`);
    continue;
  }
  console.log(`${a.choice.padEnd(26)} conf=${Number(a.confidence).toFixed(2)} ${String(a.ms).padStart(5)}ms | ${prompt}`);
}

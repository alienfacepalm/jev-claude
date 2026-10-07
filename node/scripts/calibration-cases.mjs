// Prompts with the tier each should land on, for `scripts/calibrate.mjs`.
//
// The labels are judgements, not ground truth: each follows Anthropic's positioning of the models
// and the published per-task benchmarks noted in src/config.mjs. Revisit them when a new model
// changes what a tier is good at, and say why in the commit. Keep both sides of every boundary
// covered, since a guidance change that pulls one tier's work in tends to push its neighbour's out.
export const CASES = [
  // Sonnet: well-scoped work, documents and knowledge work.
  { want: "sonnet", prompt: "add a unit test for the existing formatDate helper" },
  { want: "sonnet", prompt: "fix the off-by-one in pagination; the bug is the < on line 40 of src/page.ts" },
  { want: "sonnet", prompt: "write a spec.md for adding multi-tenant support to our API" },
  { want: "sonnet", prompt: "turn these meeting notes into a polished project update doc" },
  { want: "sonnet", prompt: "review this PR for style issues" },

  // Opus: open-ended or multi-step coding, judgement, security.
  { want: "opus", prompt: "users intermittently get logged out after deploy, figure out why" },
  { want: "opus", prompt: "refactor the auth middleware across the api and worker modules" },
  { want: "opus", prompt: "our tests fail intermittently in CI but never locally; find and fix the cause" },
  { want: "opus", prompt: "add rate limiting to the public API without breaking existing clients" },
  { want: "opus", prompt: "audit our login endpoint for security vulnerabilities" },

  // Haiku: trivial and mechanical.
  { want: "haiku", prompt: "fix the typo 'recieve' in README.md" },
  { want: "haiku", prompt: "rename the variable tmp to total in src/sum.js" },

  // Fable: long-horizon work, problems that beat a strong model, adversarial review of plans.
  { want: "fable", prompt: "migrate the entire monorepo from webpack to vite" },
  { want: "fable", prompt: "implement the full payment reconciliation service described in spec.md, end to end" },
  {
    want: "fable",
    prompt: "this race condition has beaten two previous attempts; dig through git history and find the real cause",
  },
  { want: "fable", prompt: "build a financial model spreadsheet and a slide deck from the Q3 data in data/" },
  { want: "fable", prompt: "red-team our OAuth migration plan in docs/plan.md and poke holes in the rollout" },
];

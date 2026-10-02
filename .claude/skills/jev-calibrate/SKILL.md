---
name: jev-calibrate
description: Check for new Claude models and re-tune Jev Router's tiers, effort, costs and guidance against measured results.
disable-model-invocation: true
---

Re-calibrate Jev Router for the current Claude models. The repo is the directory containing this
skill's `.claude` folder: `${CLAUDE_SKILL_DIR}/../../..`. Work there, on a branch.

The goal is the cheapest route to the best end result. Quality comes first, then cost; speed does
not matter. When two models would both succeed, prefer the cheaper; when it is unclear whether the
cheaper one would, prefer the stronger, because a failed turn is paid for twice.

## 1. Find out what changed

- Read the `claude-api` skill's current models table and the migration notes for any model newer
  than those in `TIERS` in `src/config.mjs`: model ids, prices, default effort, what each is
  positioned for, and request-shape changes (thinking, effort levels) the proxy must handle.
- Search for independent per-task measurements of the new models (Artificial Analysis and similar),
  at the effort levels the router uses. Per-task cost and score matter; per-token price alone
  misleads, because models spend very different numbers of tokens.
- Check what effort Claude Code actually sends: run one tiny prompt through `jev-claude` with
  `JEV_DUMP=1`, read `output_config.effort` from the dumps in the status directory, then delete
  the dumps (they hold the whole request). The tier effort in `TIERS` only applies when the
  request names none.

Report what changed before editing anything. If nothing did, say so and stop.

## 2. Update `src/config.mjs`

- `TIERS`: the newest id per tier (the fallback for when the account catalog has not loaded),
  the capability flags, and `floor` (the tier's effort) - the vendor default unless a measurement
  shows a better-value level. A tier's `id` is also what the router counts as calibrated: the
  status line flags any newer model in the account until that `id` is updated.
- `COST`: per-task cost relative to the neighbouring tiers, with the source and month in the
  comment above it.
- `GUIDANCE`: describe each tier by the type of task it does best, from the vendor positioning
  and the measurements. Keep every claim sourced in a comment.

## 3. Measure, one change at a time

Run `node scripts/calibrate.mjs --runs 2` before the first edit and after each one. Keep a change
only if the hit count does not drop and the tiers it targets clear the confidence bar in both
runs. Report each version's summary line side by side. Stop after three iterations whatever the
result, and report rather than keep tuning.

Lessons already learned here:

- A long list of everything a tier is good at lowers Jev's confidence: it overlaps the neighbouring
  tier, and Jev splits between them. A few signals that do not overlap work better.
- Describe a tier by task type. "Whatever the tier below cannot do" asks Jev for a counterfactual it
  cannot be sure of.
- Changing one tier's wording moves its neighbours' scores. Check both sides of each boundary.
- Jev matches words. A case that changes because a signal lost a word is not a real regression.
- If you wrote a signal after reading a case, that case flatters it. Say so when reporting.

## 4. Update the cases and finish

- Update `scripts/calibration-cases.mjs` when a model changes what a tier is good at, and give the
  reason in the commit.
- Run `pnpm test`, update the README if a setting or default changed, and commit on the branch.
  Do not merge or push without the user's go-ahead.

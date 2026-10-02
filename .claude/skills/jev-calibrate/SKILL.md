---
name: jev-calibrate
description: Check Jev Router's setup and models; in the router's own repository, re-tune it for new Claude models.
disable-model-invocation: true
allowed-tools: Bash(node *)
---

!`node "${CLAUDE_SKILL_DIR}/../../../bin/jev-check.mjs"`

## Installed copy: report only

If the report above says `Mode installed`, this is an installed copy of Jev Router. Return the
report verbatim in a plain text code block. Then add at most two sentences, only about what needs
the user's attention: routing being off, a model newer than the tuning, or tiers the account does
not offer. Stop there. Do not edit files, run other tools, or search the web: tuning is done in
the router's repository and reaches users through jev-router updates.

## Repository: re-calibrate

If it says `Mode repository`, re-calibrate Jev Router for the current Claude models, using the
report as part of step 1. The repo is the directory containing this skill's `.claude` folder:
`${CLAUDE_SKILL_DIR}/../../..`. Work there, on a branch.

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
- Check what effort Claude Code actually sends. Ask the user first, since this spends a Claude
  request on their account: run one tiny prompt through `jev-claude` with `JEV_DUMP=1`, read
  `output_config.effort` from the dumps in the status directory, then delete the dumps (they
  hold the whole request). The tier effort in `TIERS` only applies when the request names none;
  as of 2026-10 Claude Code sends `high` on every request.
- The status line only notices newer versions of the tiers the router already knows. A new model
  line (a new name, not a new version) is not detected: look for one in the models table, and if
  it fits a tier, add it to `TIERS` with its `family`.

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

Run `node scripts/calibrate.mjs --runs 2` before the first edit and after each one. It reports
two scores: how often Jev picked the intended tier (what the guidance controls) and where the turn
finally ran (which also depends on policy: an unsure pick runs one tier lower by design, shown as
`step`). Keep a guidance change only if the pick score does not drop and the tiers it targets
clear the confidence bar in both runs. Report each version's summary line side by side. Stop
after three iterations whatever the result, and report rather than keep tuning.

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

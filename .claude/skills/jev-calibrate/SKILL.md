---
name: jev-calibrate
description: Check Jev Router's setup and models (read-only). In the router's own repository, `retune` re-tunes it for new Claude models.
disable-model-invocation: true
allowed-tools: Bash(jev-check *) PowerShell(jev-check *)
---

!`jev-check ; echo ""`

## Report only (the default)

If the text above says `jev-check` was not found or is not recognized, jev-claude is not
installed, or it was installed after this terminal was opened and a new terminal will find it. Say
so, give the install command for the user's system, and stop:

- macOS, Linux, WSL, Git Bash: `curl -fsSL https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.sh | bash`
- Windows PowerShell: `irm https://raw.githubusercontent.com/alienfacepalm/jev-claude/master/install.ps1 | iex`

Re-tuning happens only when the arguments are exactly `retune` (they are: `$ARGUMENTS`), the
report above says `Mode repository`, and this skill still has its re-calibration steps below (an
installed copy of the skill has none; treat that like `Mode installed`). In every other
case, report only: no arguments, `check` (the first-launch offer's), `Mode installed`, anything
else. `Mode repository` alone is not a request to
re-tune: it only says the report came from a source checkout. Return the report verbatim in a
plain text code block. Then add at most two sentences, only about what needs the user's attention:
routing being off, a model newer than the tuning, or tiers the account does not offer. Stop there.
Do not edit files, run other tools, or search the web: tuning is done in the router's repository
and reaches users through new releases.

If the arguments are `retune` but the report says `Mode installed`, say that re-tuning is done in
a clone of the router's repository, not an installed copy, and stop. The report comes from the
`jev-check` on PATH, so in a clone it says `Mode repository` only when that is the clone's own
build: add that the user can start `jev-claude` with `JEV_ROOT` set to the clone, or put the
clone's `rust/target/release` first on PATH, and run `/jev-calibrate retune` again.

## Repository: re-calibrate (only on `retune`)

When the arguments are `retune` and the report says `Mode repository`, re-calibrate Jev Router for
the current Claude models, using the report as part of step 1. The repo is the directory containing
this skill's `.claude` folder: `${CLAUDE_SKILL_DIR}/../../..`. Work there, on a branch. Every
command below runs from that directory.

The goal is the cheapest route to the best end result. Quality comes first, then cost; speed does
not matter. When two models would both succeed, prefer the cheaper; when it is unclear whether the
cheaper one would, prefer the stronger, because a failed turn is paid for twice.

## 1. Find out what changed

- Read the `claude-api` skill's current models table and the migration notes for any model newer
  than those in `TIERS` in `rust/src/config.rs`: model ids, prices, default effort, what each is
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

## 2. Update `rust/src/config.rs`

It is the source of truth for the tuning (SPEC.md section 4).

- `TIERS`: the newest `id` per tier (the fallback for when the account catalog has not loaded),
  the capability flags (`thinking`, `effort`), and `floor` (the tier's effort) - the vendor
  default unless a measurement shows a better-value level. A tier's `id` is also what the router
  counts as calibrated: the status line flags any newer model in the account until that `id` is
  updated.
- `cost`: per-task cost relative to the neighbouring tiers, with the source and month in a
  comment beside it.
- `guidance`: describe each tier by the type of task it does best (`what`, `signals`, `not_for`),
  from the vendor positioning and the measurements. Keep every claim sourced in a comment.

## 3. Measure, one change at a time

Run `cargo run --manifest-path rust/Cargo.toml --release --example calibrate -- --runs 2` before
the first edit and after each one (it spends Jev credit, never Claude usage). It reports two
scores: how often Jev picked the intended tier (what the guidance controls) and where the turn
finally ran (which also depends on policy: an unsure pick runs one tier lower by design, shown as
`step`). Keep a guidance change only if the pick score does not drop and the tiers it targets
clear the confidence bar in both runs. Report each version's summary line side by side. Stop
after three iterations whatever the result, and report rather than keep tuning. For a quick look
at live picks without scoring, run
`cargo run --manifest-path rust/Cargo.toml --release --example live_routing`.

Lessons already learned here:

- A long list of everything a tier is good at lowers Jev's confidence: it overlaps the neighbouring
  tier, and Jev splits between them. A few signals that do not overlap work better.
- Describe a tier by task type. "Whatever the tier below cannot do" asks Jev for a counterfactual it
  cannot be sure of.
- Changing one tier's wording moves its neighbours' scores. Check both sides of each boundary.
- Jev matches words. A case that changes because a signal lost a word is not a real regression.
- If you wrote a signal after reading a case, that case flatters it. Say so when reporting.

## 4. Update the cases and finish

- Update `rust/examples/calibration-cases.json` (the prompts the `calibrate` example scores) when a
  model changes what a tier is good at, and give the reason in the commit.
- Carry the change everywhere it is pinned (SPEC.md section 17): the tier table in SPEC.md 4.1 when
  `TIERS` changed; and, since the prose goes to Jev verbatim, the golden case
  `conformance/cases/jev-request.json` and the conformance tests'
  `conformance/fixtures/harness-expectations.json`, edited by hand as
  `conformance/cases/README.md` and `conformance/README.md` describe.
- Verify from inside `rust/` (not with `--manifest-path`: `rust/.cargo/config.toml`, which makes
  rustdoc warnings fatal and defines `cargo xtask`, is read only from there), the same gate CI runs
  (doc/RUST.md "The full gate"):
  `cd rust`
  `cargo fmt --all --check`
  `cargo clippy --workspace --all-targets --locked -- -D warnings`
  `cargo test --workspace --locked` (the unit tests, the golden cases, the black-box conformance
  tests against the built programs and the xtask tests)
  `cargo doc --workspace --no-deps --locked`
  All must pass.
- Update the README if a setting or default changed, and commit on the branch. Do not merge or
  push without the user's go-ahead.

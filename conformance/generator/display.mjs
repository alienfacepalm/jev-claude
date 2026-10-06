// Cases for what a person reads: icons, reason wording, the explanation panel, the legend, the
// location lookup, and the status line itself run as a real child process.
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// The status line judges sub-agent freshness by the real clock, so fresh entries sit far in the
// future (2100-01-01) and stale ones at 0 or with no `at` (SPEC 16.2).
const FUTURE = 4_102_444_800_000;

const sessionIdOf = (text) => {
  try {
    const id = JSON.parse(text).session_id;
    return typeof id === "string" ? id : "";
  } catch {
    return "";
  }
};

/** The environment every status-line child gets: nothing inherited that changes its output. */
export function cleanEnv(extra) {
  const env = {};
  for (const [key, value] of Object.entries(process.env)) {
    if (!/^(JEV_|TYPESAFE_|ANTHROPIC_|CLAUDE_)/i.test(key)) env[key] = value;
  }
  return { ...env, ...extra };
}

export default function displayCases({ root, reasons, icons, legend, explain, worktree }) {
  // ---- icons ---------------------------------------------------------------------------------
  const iconInputs = [];
  for (const platform of ["darwin", "linux", "win32"]) {
    for (const [label, env] of [
      ["no env", {}],
      ["WT_SESSION", { WT_SESSION: "1" }],
      ["empty WT_SESSION", { WT_SESSION: "" }],
      ["TERM_PROGRAM vscode", { TERM_PROGRAM: "vscode" }],
      ["TERM_PROGRAM mintty", { TERM_PROGRAM: "mintty" }],
      ["ConEmuPID", { ConEmuPID: "42" }],
      ["JEV_ICONS text", { JEV_ICONS: "text" }],
      ["JEV_ICONS ASCII", { JEV_ICONS: "ASCII" }],
      ["JEV_ICONS Symbols", { JEV_ICONS: "Symbols" }],
      ["JEV_ICONS symbols with WT", { JEV_ICONS: "symbols", WT_SESSION: "1" }],
      ["JEV_ICONS text with WT", { JEV_ICONS: "Text", WT_SESSION: "1" }],
      ["JEV_ICONS padded is not a choice", { JEV_ICONS: " text " }],
      ["JEV_ICONS unknown", { JEV_ICONS: "emoji" }],
      ["JEV_ICONS empty", { JEV_ICONS: "" }],
    ]) {
      iconInputs.push({ name: `${platform} ${label}`, input: { env, platform }, expected: icons.icons(env, platform) });
    }
  }

  // ---- reasons -------------------------------------------------------------------------------
  const reasonCodes = [
    "jev", "jev/no-change", "override", "override/no-change", "override+unavailable", "override+unavailable/no-change",
    "jev-unavailable", "jev-unavailable/no-change", "jev-unavailable+unavailable", "low-confidence-default",
    "low-confidence-default/no-change", "low-confidence-default+unavailable", "downgrade-not-worth-cache-rebuild",
    "downgrade-not-worth-cache-rebuild/no-change", "jev+unavailable", "jev+unavailable/no-change", "unavailable",
    "no-change", "JEV-UNAVAILABLE", "", null, undefined,
  ];
  const reasonCases = reasonCodes.map((reason) => ({
    name: reason === undefined ? "undefined" : JSON.stringify(reason),
    input: { reason },
    expected: { short: reasons.shortReason(reason), long: reasons.longReason(reason), noChange: reasons.isNoChange(reason) },
  }));

  // ---- formatExplanation ---------------------------------------------------------------------
  const routed = {
    prompt: "Explain the router architecture",
    tier: "sonnet",
    confidence: 0.94,
    reason: "jev",
    jev: {
      request: { state: { session: { current_model: "haiku", context_tokens: 6200 } } },
      response: { answers: { model: { choice: "claude-sonnet-5-5", confidence: 0.94 } } },
    },
    metrics: { taskComplexity: 0.82, reasoningRequired: 0.91, toolComplexity: 0.64, contextSize: 0.31 },
  };
  const explanations = [
    ["routed", routed],
    ["policy overruled jev", { tier: "opus", model: "claude-opus-5-5", confidence: 0.97, reason: "downgrade-not-worth-cache-rebuild/no-change", jev: { response: { answers: { model: { choice: "claude-haiku-4-5-20251001" } } } } }],
    ["model_tier from before the rename", { tier: "opus", jev: { response: { answers: { model_tier: { choice: "sonnet" } } } } }],
    ["unrecognised choice shown as is", { tier: "opus", jev: { response: { answers: { model: { choice: "mystery-9" } } } } }],
    ["null", null],
    ["manual", { ...routed, manual: true }],
    ["contextSize exactly 0.125", { ...routed, metrics: { ...routed.metrics, contextSize: 0.125 } }],
    ["ties and float edges", { ...routed, metrics: { taskComplexity: 0.375, reasoningRequired: 1.005, toolComplexity: -0.125, contextSize: 2.675 } }],
    ["NaN and non-finite metrics", { ...routed, metrics: { taskComplexity: NaN, reasoningRequired: Infinity, toolComplexity: "0.5", contextSize: null } }],
    ["missing metrics", { ...routed, metrics: undefined }],
    ["null metrics", { ...routed, metrics: null }],
    ["partial metrics", { ...routed, metrics: { taskComplexity: 0.1 } }],
    ["long prompt wraps", { ...routed, prompt: "Please look at the whole authentication flow across the api and worker modules and explain where tokens are refreshed" }],
    ["word longer than a row", { ...routed, prompt: `Prompt ${"x".repeat(45)} tail` }],
    ["prompt whitespace collapses", { ...routed, prompt: "  a\u00a0\u00a0b\n\nc\u2028d  " }],
    ["no prompt", { ...routed, prompt: undefined }],
    ["confidence rounding", { ...routed, confidence: 0.945 }],
    ["confidence null", { ...routed, confidence: null }],
    ["confidence string", { ...routed, confidence: "0.5" }],
    ["no reason", { ...routed, reason: undefined }],
    ["null reason", { ...routed, reason: null }],
    ["unavailable reason wraps", { ...routed, reason: "jev+unavailable/no-change" }],
    ["low confidence reason", { ...routed, reason: "low-confidence-default" }],
    ["no jev", { tier: "sonnet", reason: "jev-unavailable/no-change", confidence: null, metrics: null, jev: null }],
    ["long model id clipped", { ...routed, model: "anthropic.claude-sonnet-5-5-20260901-preview-long" }],
    ["empty status", {}],
  ];
  const formatExplanation = explanations.map(([name, status]) => ({ name, input: { status }, expected: explain.formatExplanation(structuredClone(status)) }));

  // ---- formatAgents --------------------------------------------------------------------------
  const now = 10_000_000;
  const agentStatuses = [
    ["main and subs", {
      agents: {
        "k-main": { label: "fix the race", main: true, tier: "opus", model: "claude-opus-5-5", confidence: 0.94, at: now - 4000 },
        "k-1": { label: "grep for callers", main: false, tier: "haiku", model: "claude-haiku-4-5-20251001", confidence: 0.81, at: now - 59_499 },
        "k-2": { label: "pinned", main: false, model: "claude-haiku-4-5", manual: true, at: now - 59_500 },
        "k-3": { label: "main", main: false, tier: "sonnet", at: now - 3_600_000 },
        "k-4": { label: "very long label for a sub-agent that keeps going and going past the box edge", main: false, tier: "fable", confidence: 0.5, at: now - 7_200_000 },
      },
    }],
    ["no agents", { tier: "opus" }],
    ["null", null],
    ["main labelled main", { agents: { m: { label: "main", main: true, model: "claude-sonnet-5-5", confidence: null, at: now } } }],
    ["missing at and future at", { agents: { m: { main: true, tier: "opus" }, s: { main: false, at: now + 60_000 } } }],
    ["minutes and hours", { agents: { a: { main: false, at: now - 3_570_000 }, b: { main: false, at: now - 3_600_000 }, c: { main: false, at: now - 89_999 }, d: { main: false, at: now - 90_000 } } }],
    ["long model id", { agents: { m: { main: true, model: "anthropic.claude-opus-5-5-20260901-long", confidence: 0.999, at: now } } }],
    ["confidence rounding", { agents: { m: { main: true, tier: "opus", confidence: 0.945, at: now }, s: { main: false, tier: "haiku", confidence: 0.005, at: now } } }],
    ["no main", { agents: { s: { main: false, label: "only sub", tier: "haiku", at: now } } }],
  ];
  const formatAgents = agentStatuses.map(([name, status]) => ({ name, input: { status, now }, expected: explain.formatAgents(structuredClone(status), now) }));

  // ---- formatLegend --------------------------------------------------------------------------
  const sets = [
    ["symbols", icons.icons({ JEV_ICONS: "symbols" }, "darwin")],
    ["text", icons.icons({ JEV_ICONS: "text" }, "darwin")],
    ["astral marks count as one", { model: "\ud83d\ude80", effort: "e", agents: "\ud83e\udd16\ud83e\udd16", dir: "dir", branch: "br", worktree: "wt", context: "ctx" }],
    ["long word", { model: "the-model-mark", effort: "x", agents: "y", dir: "z", branch: "b", worktree: "w", context: "c" }],
  ];
  const formatLegend = sets.map(([name, set]) => ({ name, input: { set }, expected: legend.formatLegend(set) }));

  // ---- locationInfo --------------------------------------------------------------------------
  const locations = [
    ["outside a checkout", { workspace: { current_dir: "/nowhere" } }, null],
    ["empty input", {}, null],
    ["undefined input", undefined, null],
    ["main working tree", { workspace: { current_dir: "/repo" } }, "master"],
    ["worktree session carries its branch", { worktree: { name: "my-feature", branch: "worktree-my-feature", path: "/r/.claude/worktrees/my-feature" } }, "never-used"],
    ["linked worktree reads git", { workspace: { current_dir: "/wt/feature-xyz", git_worktree: "feature-xyz" } }, "feature/xyz"],
    ["worktree without branch falls back to git", { worktree: { name: "scratch", path: "/wt/scratch" } }, "main-2"],
    ["detached head in a worktree", { workspace: { current_dir: "/wt/x", git_worktree: "x" } }, ""],
    ["cwd when no current_dir", { cwd: "/from/cwd" }, "dev"],
    ["current_dir beats cwd and path", { workspace: { current_dir: "/a" }, cwd: "/b", worktree: { name: "n", path: "/c" } }, "b1"],
    ["worktree name beats git_worktree", { worktree: { name: "first" }, workspace: { git_worktree: "second" } }, null],
    ["null worktree branch falls back", { worktree: { name: "n", branch: null, path: "/p" } }, "from-git"],
    ["empty worktree name kept", { worktree: { name: "" } }, null],
    ["null names", { worktree: { name: null }, workspace: { git_worktree: null } }, null],
  ];
  const locationInfo = locations.map(([name, input, branch]) => {
    const asked = [];
    const result = worktree.locationInfo(structuredClone(input), (dir) => (asked.push(dir), branch));
    return { name, input: { input, branch }, expected: { result, asked } };
  });

  // ---- status line ---------------------------------------------------------------------------
  const MAIN = { key: "main", label: "main", main: true };
  const routedStatus = (entry, extra = {}) => ({ ...entry, agents: { [MAIN.key]: { label: "main", main: true, ...entry, at: FUTURE } }, history: [], ...extra });
  const sonnet = { tier: "sonnet", model: "claude-sonnet-5-5", confidence: 0.94, effort: "high", reason: "jev", at: FUTURE };
  const { model: _unused, ...withoutModel } = sonnet;
  const stdin = (session_id, extra = {}) => JSON.stringify({ session_id, workspace: { current_dir: "/work/proj" }, context_window: { used_percentage: 8 }, ...extra });
  const lines = [
    ["routed main, symbols", { stdin: stdin("sl-a", { worktree: { name: "login-fix", branch: "fix/login" } }), status: routedStatus(sonnet), icons: "symbols" }],
    ["routed main, text", { stdin: stdin("sl-a"), status: routedStatus(sonnet), icons: "text" }],
    ["xhigh effort", { stdin: stdin("sl-a"), status: routedStatus({ ...sonnet, tier: "opus", model: "claude-opus-5-5", confidence: 0.91, effort: "xhigh" }), icons: "symbols" }],
    ["haiku shows no effort", { stdin: stdin("sl-a"), status: routedStatus({ ...sonnet, tier: "haiku", model: "claude-haiku-4-5-20251001", confidence: 0.97, effort: null }), icons: "symbols" }],
    ["fable colour", { stdin: stdin("sl-a"), status: routedStatus({ ...sonnet, tier: "fable", model: "claude-fable-5-1" }), icons: "symbols" }],
    ["held downgrade says why", { stdin: stdin("sl-a"), status: routedStatus({ ...sonnet, tier: "opus", model: "claude-opus-5-5", reason: "downgrade-not-worth-cache-rebuild/no-change" }), icons: "symbols" }],
    ["router offline without confidence", { stdin: stdin("sl-a"), status: routedStatus({ ...sonnet, confidence: null, reason: "jev-unavailable/no-change" }), icons: "symbols" }],
    ["unparseable model falls back to id", { stdin: stdin("sl-a"), status: routedStatus({ ...sonnet, model: "mystery-9" }), icons: "symbols" }],
    ["no model falls back to tier", { stdin: stdin("sl-a"), status: routedStatus({ ...withoutModel, tier: "opus" }), icons: "symbols" }],
    ["confidence rounds half up", { stdin: stdin("sl-a"), status: routedStatus({ ...sonnet, confidence: 0.125 }), icons: "symbols" }],
    ["manual main with display name", { stdin: stdin("sl-a", { model: { display_name: "Opus 4.6" } }), status: routedStatus({ model: "claude-opus-4-6", manual: true }), icons: "symbols" }],
    ["manual main without display name", { stdin: stdin("sl-a"), status: routedStatus({ model: "claude-opus-4-6", manual: true }), icons: "symbols" }],
    ["manual main with nothing to name", { stdin: stdin("sl-a"), status: routedStatus({ manual: true }), icons: "symbols" }],
    ["flat manual without agents", { stdin: stdin("sl-a", { model: { display_name: "Sonnet 5" } }), status: { model: "claude-sonnet-5", manual: true, at: 1 }, icons: "symbols" }],
    ["routed main outranks flat manual", { stdin: stdin("sl-a"), status: routedStatus(sonnet, { manual: true }), icons: "symbols" }],
    ["pre-agent flat status", { stdin: stdin("sl-a"), status: { ...sonnet }, icons: "text" }],
    ["no status yet", { stdin: stdin("sl-none"), icons: "symbols" }],
    ["malformed stdin", { stdin: "{not json", icons: "symbols" }],
    ["empty stdin", { stdin: "", icons: "text" }],
    ["stdin is a JSON array", { stdin: "[1,2]", icons: "text" }],
    ["malformed status file", { stdin: stdin("sl-a"), statusText: "{\"tier\":", icons: "symbols" }],
    ["session id characters removed", { stdin: stdin("sl/a:b"), statusFile: "slab.json", status: routedStatus(sonnet), icons: "text" }],
    ["sub-agents, first three and a count", { stdin: stdin("sl-a"), status: routedStatus(sonnet, {
      agents: {
        main: { label: "main", main: true, ...sonnet, at: FUTURE },
        s1: { label: "a", main: false, tier: "haiku", model: "claude-haiku-4-5-20251001", at: FUTURE - 1 },
        s2: { label: "b", main: false, model: "claude-opus-4-6", manual: true, at: FUTURE },
        s3: { label: "c", main: false, at: FUTURE - 2 },
        s4: { label: "d", main: false, tier: "fable", model: "odd-model", at: FUTURE - 3 },
        s5: { label: "e", main: false, model: "claude-sonnet-5", at: FUTURE - 4 },
        stale: { label: "f", main: false, tier: "opus", at: 0 },
        noAt: { label: "g", main: false, tier: "opus" },
      },
    }), icons: "symbols" }],
    ["one sub-agent", { stdin: stdin("sl-a"), status: routedStatus(sonnet, {
      agents: { main: { main: true, ...sonnet, at: FUTURE }, s: { main: false, tier: "haiku", model: "claude-haiku-4-5-20251001", at: FUTURE } },
    }), icons: "text" }],
    ["sub-agent with only a model id", { stdin: stdin("sl-a"), status: routedStatus(sonnet, {
      agents: { main: { main: true, ...sonnet, at: FUTURE }, s: { main: false, model: "mystery", at: FUTURE } },
    }), icons: "symbols" }],
    ["worktree and branch", { stdin: stdin("sl-a", { worktree: { name: "login-fix", branch: "fix/login" } }), icons: "text" }],
    ["worktree named like the directory", { stdin: stdin("sl-a", { workspace: { current_dir: "/work/COR-1", git_worktree: "COR-1" } }), icons: "text" }],
    ["long branch clipped", { stdin: stdin("sl-a", { worktree: { name: "wt", branch: "COR-1263/multi-edit-inspection-sync" } }), icons: "text" }],
    ["28-unit branch kept", { stdin: stdin("sl-a", { worktree: { name: "wt", branch: "a".repeat(28) } }), icons: "text" }],
    ["29-unit branch clipped", { stdin: stdin("sl-a", { worktree: { name: "wt", branch: "b".repeat(29) } }), icons: "text" }],
    ["emoji branch clipped after the pair", { stdin: stdin("sl-a", { worktree: { name: "wt", branch: `\ud83d\ude80${"c".repeat(40)}` } }), icons: "symbols" }],
    ["detached head", { stdin: stdin("sl-a", { worktree: { name: "wt", branch: "" } }), icons: "symbols" }],
    ["linked worktree without readable branch", { stdin: stdin("sl-a", { workspace: { current_dir: "/work/proj", git_worktree: "scratch" } }), icons: "text" }],
    ["windows directory", { stdin: stdin("sl-a", { workspace: { current_dir: "C:\\Users\\me\\proj" } }), icons: "text" }],
    ["trailing separator hides directory", { stdin: stdin("sl-a", { workspace: { current_dir: "/work/proj/" } }), icons: "text" }],
    ["cwd field", { stdin: JSON.stringify({ session_id: "sl-a", cwd: "/elsewhere/repo", context_window: { used_percentage: 0 } }), icons: "text" }],
    ["context 12.5 rounds up", { stdin: stdin("sl-a", { context_window: { used_percentage: 12.5 } }), icons: "symbols" }],
    ["context numeric string", { stdin: stdin("sl-a", { context_window: { used_percentage: "41.5" } }), icons: "symbols" }],
    ["context null", { stdin: stdin("sl-a", { context_window: { used_percentage: null } }), icons: "symbols" }],
    ["context not a number", { stdin: stdin("sl-a", { context_window: { used_percentage: "abc" } }), icons: "symbols" }],
    ["context negative zero", { stdin: stdin("sl-a", { context_window: { used_percentage: -0.3 } }), icons: "symbols" }],
    ["context array", { stdin: stdin("sl-a", { context_window: { used_percentage: [50] } }), icons: "symbols" }],
    ["calibration notice, one model", { stdin: stdin("sl-a"), status: routedStatus(sonnet), calibration: { newer: ["claude-opus-6"], models: ["claude-opus-6"], at: 1 }, icons: "symbols" }],
    ["calibration notice, three models", { stdin: stdin("sl-a"), calibration: { newer: ["claude-opus-6", "claude-sonnet-6", "claude-haiku-5"], models: [], at: 1 }, icons: "text" }],
    ["calibration newer not an array", { stdin: stdin("sl-a"), calibration: { newer: "claude-opus-6", models: [], at: 1 }, icons: "text" }],
    ["calibration without models still notices", { stdin: stdin("sl-a"), calibration: { newer: ["claude-opus-6"] }, icons: "text" }],
    ["malformed calibration file", { stdin: stdin("sl-a"), calibrationText: "not json", icons: "text" }],
  ];

  const script = join(root, "node", "bin", "jev-statusline.mjs");
  const statusLine = lines.map(([name, input]) => {
    const base = mkdtempSync(join(tmpdir(), "jev-conformance-sl-"));
    try {
      const dir = join(base, "status");
      const cwd = join(base, "cwd");
      mkdirSync(dir);
      mkdirSync(cwd);
      const sessionFile = input.statusFile ?? `${sessionIdOf(input.stdin).replace(/[^\w-]/g, "")}.json`;
      if (input.status !== undefined) writeFileSync(join(dir, sessionFile), JSON.stringify(input.status));
      if (input.statusText !== undefined) writeFileSync(join(dir, sessionFile), input.statusText);
      if (input.calibration !== undefined) writeFileSync(join(dir, "calibration.json"), JSON.stringify(input.calibration));
      if (input.calibrationText !== undefined) writeFileSync(join(dir, "calibration.json"), input.calibrationText);
      const out = spawnSync(process.execPath, [script], {
        input: input.stdin,
        cwd,
        encoding: "utf8",
        env: cleanEnv({ JEV_STATUS_DIR: dir, JEV_ICONS: input.icons, HOME: base, USERPROFILE: base, TEMP: base, TMP: base, TMPDIR: base }),
      });
      if (out.status !== 0 || out.stderr) throw new Error(`status line failed for ${name}: ${out.stderr}`);
      return { name, input: { ...input, statusFile: input.status !== undefined || input.statusText !== undefined ? sessionFile : undefined }, expected: { stdout: out.stdout } };
    } finally {
      rmSync(base, { recursive: true, force: true });
    }
  });
  for (const c of statusLine) if (c.input.statusFile === undefined) delete c.input.statusFile;

  return {
    icons: iconInputs,
    reasons: reasonCases,
    "format-explanation": formatExplanation,
    "format-agents": formatAgents,
    "format-legend": formatLegend,
    "location-info": locationInfo,
    "status-line": statusLine,
  };
}

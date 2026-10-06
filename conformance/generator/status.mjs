// Cases for status.mjs (SPEC 8.2): the session file after each writeDecision/markManual step, and
// the two readers the status line and explanation use.
import { CLOCK } from "./tagged.mjs";

// Explicit decision `at` values are small, so any real clock reading is newer than all of them and
// a port that uses its real clock sorts agents the same way. While a sequence runs, Date.now is
// pinned to this value; any `at` equal to it came from the clock and is written as the CLOCK tag.
const PINNED_NOW = 1_800_000_000_000;

export default function statusCases({ status }) {
  const main = { key: "k-main", label: "fix the race", main: true };
  const sub = (n) => ({ key: `k-sub-${n}`, label: `sub-agent ${n}`, main: false });
  const decision = (at, extra = {}) => ({
    tier: "opus",
    prompt: `prompt at ${at}`,
    model: "claude-opus-5-5",
    confidence: 0.94,
    metrics: { taskComplexity: 0.5555555555555556, reasoningRequired: 0.4444444444444444, toolComplexity: 0.2222222222222222, contextSize: 0.01 },
    reason: "jev",
    jev: { request: { state: { request: `prompt at ${at}` } }, response: { answers: { model: { choice: "claude-opus-5-5", confidence: 0.94, probabilities: { "claude-opus-5-5": 0.94, "claude-sonnet-5-5": 0.06 } } } } },
    effort: "medium",
    at,
    ...extra,
  });
  const wd = (id, d, agent) => ({ op: "writeDecision", id, decision: d, ...(agent ? { agent } : {}) });
  const mm = (id, model, agent) => ({ op: "markManual", id, model, ...(agent ? { agent } : {}) });

  const sequences = [];
  {
    const id = "wd-agents";
    const steps = [
      wd(id, decision(1000), main),
      mm(id, "claude-sonnet-5", main),
      wd(id, decision(3000, { tier: "sonnet", model: "claude-sonnet-5-5", effort: "high" }), main),
    ];
    // Thirteen sub-agents: the map passes 12 keys and the trim keeps the main entry plus the 11
    // newest sub-agents by `at`, ties in insertion order.
    const ats = [2000, 2000, 2000, 1500, 4000, 2000, 1500, 2500, 2500, 2000, 1000, 4000, 2000];
    ats.forEach((at, i) => steps.push(wd(id, decision(at, { tier: "haiku", model: "claude-haiku-4-5-20251001", effort: null, reason: "jev" }), sub(i))));
    steps.push(mm(id, "claude-haiku-4-5", sub(1)));
    steps.push(mm(id, "claude-opus-4-6", sub(99)));
    steps.push(wd(id, decision(5000, { confidence: undefined, reason: "low-confidence-default" }), sub(1)));
    steps.push(wd(id, decision(6000, { reason: "jev-unavailable/no-change", confidence: null, metrics: null, jev: null })));
    steps.push(mm(id, "claude-opus-5-5"));
    steps.push(wd(id, decision(7000), main));
    sequences.push(["agents, trim with ties, manual survives a routed merge", steps]);
  }
  {
    const id = "wd-history";
    const steps = [];
    for (let i = 1; i <= 23; i++) steps.push(wd(id, { tier: "sonnet", prompt: `turn ${i}`, model: "claude-sonnet-5-5", at: i }));
    sequences.push(["history keeps the last 20", steps]);
  }
  sequences.push(["empty id writes nothing", [wd("", decision(1), main), mm("", "claude-opus-5-5", main), mm("", "claude-opus-5-5")]]);
  sequences.push(["id characters outside [A-Za-z0-9_-] are removed", [
    wd("we/ird:id.1 é", decision(10), main),
    wd("weirdid1", decision(20, { tier: "haiku" }), sub(1)),
  ]]);
  sequences.push(["markManual on a fresh session", [
    mm("wd-fresh-sub", "claude-haiku-4-5", sub(1)),
    mm("wd-fresh-sub", "claude-opus-5-5", main),
    mm("wd-fresh-flat", "claude-opus-5-5"),
  ]]);
  sequences.push(["sub-agent manual keeps the session's flag", [
    wd("wd-submanual", decision(1000), main),
    mm("wd-submanual", "claude-haiku-4-5", sub(2)),
    mm("wd-submanual", "claude-sonnet-5", main),
    mm("wd-submanual", "claude-haiku-4-5", sub(3)),
    wd("wd-submanual", decision(2000), sub(2)),
  ]]);
  sequences.push(["decisions without an agent", [
    wd("wd-flat", { prompt: "first", jev: { request: { id: 1 }, response: { confidence: 0.6 } }, at: 1 }),
    wd("wd-flat", { prompt: "second", jev: { request: { id: 2 }, response: { confidence: 0.8 } }, at: 2 }),
  ]]);
  sequences.push(["integer-like keys in a decision", [
    wd("wd-keys", { b: 1, 2: "two", a: { 1: "x", z: 0, 0: "y" }, tier: "haiku", at: 5 }, { key: "10", label: "ten", main: true }),
    wd("wd-keys", { tier: "opus", at: 6 }, { key: "2", label: "two", main: false }),
  ]]);

  const pin = (fn) => {
    const real = Date.now;
    Date.now = () => PINNED_NOW;
    try {
      return fn();
    } finally {
      Date.now = real;
    }
  };
  const tagClock = (value) => {
    if (value === PINNED_NOW) return CLOCK;
    if (Array.isArray(value)) return value.map(tagClock);
    if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, tagClock(v)]));
    return value;
  };
  const writeDecision = sequences.map(([name, steps]) => {
    const files = steps.map((step) =>
      pin(() => {
        const agent = step.agent ?? null;
        if (step.op === "writeDecision") status.writeDecision(step.id, structuredClone(step.decision), agent);
        else status.markManual(step.id, step.model, agent);
        return { file: tagClock(status.readStatus(step.id)) };
      }),
    );
    return { name, input: { steps }, expected: files };
  });

  // ---- agentView -----------------------------------------------------------------------------
  const now = 1_000_000;
  const agents = {
    m: { label: "main", main: true, tier: "opus", at: now - 500_000 },
    fresh: { label: "fresh", main: false, tier: "haiku", at: now - 1000 },
    edge: { label: "at the boundary", main: false, tier: "sonnet", at: now - 90_000 },
    past: { label: "one ms past", main: false, tier: "sonnet", at: now - 90_001 },
    future: { label: "future", main: false, at: now + 5000 },
    tie: { label: "tie with fresh", main: false, at: now - 1000 },
    noAt: { label: "no at", main: false },
  };
  const views = [
    ["mixed agents", { status: { agents }, now }],
    ["boundary only", { status: { agents: { e: { main: false, at: 0 } } }, now: 90_000 }],
    ["boundary plus one", { status: { agents: { e: { main: false, at: 0 } } }, now: 90_001 }],
    ["missing at is fresh near epoch", { status: { agents: { a: { main: false } } }, now: 50_000 }],
    ["missing at is stale later", { status: { agents: { a: { main: false } } }, now: 90_001 }],
    ["custom freshMs", { status: { agents }, now, freshMs: 1000 }],
    ["infinite freshMs", { status: { agents }, now, freshMs: Infinity }],
    ["first truthy main wins", { status: { agents: { x: { main: 0, at: now }, y: { main: "yes", tier: "a" }, z: { main: true, tier: "b" } } }, now }],
    ["no main", { status: { agents: { s: { main: false, at: now } } }, now }],
    ["entry key field overrides map key", { status: { agents: { real: { key: "inner", main: true } } }, now }],
    ["integer-like agent keys", { status: { agents: { b: { main: false, at: now }, 3: { main: false, at: now }, 1: { main: true } } }, now }],
    ["pre-agent status", { status: { tier: "opus", model: "claude-opus-5-5", at: now }, now }],
    ["null status", { status: null, now }],
    ["agents null", { status: { agents: null }, now }],
  ];
  const agentView = views.map(([name, input]) => ({
    name,
    input,
    expected: status.agentView(structuredClone(input.status), { now: input.now, ...(input.freshMs === undefined ? {} : { freshMs: input.freshMs }) }),
  }));

  // ---- mainDecision --------------------------------------------------------------------------
  const decisions = [
    ["null", null],
    ["pre-agent status", { tier: "opus", reason: "jev", at: 1 }],
    ["history without agents", { tier: "opus", history: [{ tier: "haiku" }, { tier: "sonnet" }] }],
    ["newest main entry", { tier: "haiku", history: [{ tier: "opus", agent: { main: true } }, { tier: "sonnet", agent: { main: true } }, { tier: "haiku", agent: { main: false } }] }],
    ["truthy main values", { tier: "x", history: [{ tier: "a", agent: { main: 1 } }, { tier: "b", agent: { main: "" } }] }],
    ["entry without agent", { tier: "x", history: [{ tier: "a", agent: { main: true } }, { tier: "b" }] }],
    ["empty history", { tier: "x", history: [] }],
    ["history string iterates characters", { tier: "x", history: "abc" }],
    ["history null", { tier: "x", history: null }],
  ];
  const mainDecision = decisions.map(([name, s]) => ({ name, input: { status: s }, expected: status.mainDecision(structuredClone(s)) }));

  return { "write-decision": writeDecision, "agent-view": agentView, "main-decision": mainDecision };
}

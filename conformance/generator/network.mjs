// Cases for the Jev exchange (SPEC 5, 6): the exact request body askJev sends and the result it
// builds from a given response. The real askJev and SDK run; only global fetch is replaced, so the
// captured body is the one the SDK would put on the wire.
import { readFileSync } from "node:fs";
import { join } from "node:path";

export default async function networkCases({ root, proxy, router, calibration }) {
  const fixture = JSON.parse(readFileSync(join(root, "conformance", "fixtures", "claude-code-print-request.json"), "utf8"));
  const prompt = proxy.newTurnPrompt(fixture.body);
  const contextTokens = Math.round(JSON.stringify(fixture.body.messages).length / 4);

  const statics = proxy.newestPerTier(proxy.claudeModels([]));
  const account = proxy.newestPerTier(proxy.claudeModels([
    { id: "claude-opus-6", display_name: "Claude Opus 6", created_at: "2026-11-01T00:00:00Z", max_input_tokens: 1000000 },
    { id: "claude-opus-5-5", display_name: "Claude Opus 5.5", created_at: "2026-09-02T00:00:00Z" },
    { id: "claude-sonnet-5-5", display_name: "Claude Sonnet 5.5", created_at: "2026-08-11T00:00:00Z", max_input_tokens: 1000000 },
    { id: "claude-haiku-4-5-20251001", display_name: "Claude Haiku 4.5", created_at: "2025-10-01T00:00:00Z", max_input_tokens: 200000 },
  ]));
  const noFable = statics.filter((m) => m.tier !== "fable");

  const answers = (choice, confidence, scores = [3, 4, 2]) => ({
    model: { type: "choice", choice, confidence, probabilities: { [choice]: confidence } },
    task_complexity: { type: "score", score: scores[0], confidence: 0.8, legend: {}, probabilities: { 3: 0.8 } },
    reasoning_required: { type: "score", score: scores[1], confidence: 0.7, legend: {}, probabilities: {} },
    tool_complexity: { type: "score", score: scores[2], confidence: 0.9, legend: {}, probabilities: {} },
  });
  const reply = (body) => JSON.stringify({ model: "jev-1", usage: { input_tokens: 812, output_tokens: 9 }, ...body });
  const good = reply({ answers: answers("claude-haiku-4-5-20251001", 0.92) });

  const inputs = [
    ["captured prompt, static menu", { prompt, current: "claude-sonnet-5-5", contextTokens, models: statics, response: good }],
    ["captured prompt, account menu", { prompt, current: "claude-opus-6", contextTokens, models: account, response: reply({ answers: answers("claude-opus-6", 0.88, [6, 7, 5]) }) }],
    ["fable off", { prompt: "refactor the parser", current: "claude-sonnet-5-5", contextTokens: 0, models: noFable, response: good }],
    ["single model", { prompt: "x", current: "claude-opus-5-5", contextTokens: 1, models: [statics[2]], response: good }],
    ["model with no description and unknown tier", { prompt: "x", current: "claude-sonnet-5-5", contextTokens: 5, models: [{ id: "claude-opus-5-5", tier: "opus" }, { id: "odd", tier: "mystery", description: "odd model" }], response: good }],
    ["context over the window", { prompt: "big", current: "claude-sonnet-5-5", contextTokens: 450000, models: statics, response: good }],
    ["no models makes no request", { prompt: "x", current: "claude-sonnet-5-5", contextTokens: 0, models: [], response: good }],
    ["lone surrogate in prompt", { prompt: "fix \ud800 this", current: "claude-sonnet-5-5", contextTokens: 3, models: statics, response: good }],
    ["unicode and controls in prompt", { prompt: "naïve   \u0007 </x> \"q\"", current: "claude-sonnet-5-5", contextTokens: 3, models: statics, response: good }],
    ["answers.model missing", { prompt: "x", current: "claude-sonnet-5-5", contextTokens: 9, models: statics, response: reply({ answers: (({ model, ...rest }) => rest)(answers("claude-opus-5-5", 0.9)) }) }],
    ["answers.model a string spreads its characters", { prompt: "x", current: "claude-sonnet-5-5", contextTokens: 9, models: statics, response: reply({ answers: { ...answers("a", 1), model: "ab" } }) }],
    ["string scores", { prompt: "x", current: "claude-sonnet-5-5", contextTokens: 9, models: statics, response: reply({ answers: answers("claude-opus-5-5", 0.9, ["3", "", null]) }) }],
    ["score missing is NaN", { prompt: "x", current: "claude-sonnet-5-5", contextTokens: 9, models: statics, response: reply({ answers: { ...answers("claude-opus-5-5", 0.9), tool_complexity: { type: "score" } } }) }],
    ["extra answer fields kept", { prompt: "x", current: "claude-sonnet-5-5", contextTokens: 9, models: statics, response: reply({ answers: { ...answers("claude-opus-5-5", 0.9), model: { choice: "claude-opus-5-5", confidence: 0.9, extra: [1], 2: "two" } }, more: true }) }],
    ["task_complexity null fails", { prompt: "x", current: "claude-sonnet-5-5", contextTokens: 9, models: statics, response: reply({ answers: { ...answers("claude-opus-5-5", 0.9), task_complexity: null } }) }],
    ["reasoning_required missing fails", { prompt: "x", current: "claude-sonnet-5-5", contextTokens: 9, models: statics, response: reply({ answers: (({ reasoning_required, ...rest }) => rest)(answers("claude-opus-5-5", 0.9)) }) }],
    ["answers missing fails", { prompt: "x", current: "claude-sonnet-5-5", contextTokens: 9, models: statics, response: reply({}) }],
    ["answers a number fails", { prompt: "x", current: "claude-sonnet-5-5", contextTokens: 9, models: statics, response: reply({ answers: 5 }) }],
    ["body not JSON fails", { prompt: "x", current: "claude-sonnet-5-5", contextTokens: 9, models: statics, response: "not json" }],
    ["body a JSON array fails", { prompt: "x", current: "claude-sonnet-5-5", contextTokens: 9, models: statics, response: "[1,2]" }],
    ...calibration.CASES.map((c, i) => [`calibration ${i}`, { prompt: c.prompt, current: "claude-sonnet-5-5", contextTokens: 120, models: statics, response: good }]),
  ];

  const realFetch = globalThis.fetch;
  const realWrite = process.stderr.write;
  const cases = [];
  try {
    // A failed call logs "routing failed"; that line is free-form and not part of the case.
    process.stderr.write = () => true;
    for (const [name, input] of inputs) {
      let sent = null;
      globalThis.fetch = async (url, init) => {
        sent = { url: String(url), method: init.method, body: init.body };
        return new Response(input.response, { status: 200, headers: { "content-type": "application/json" } });
      };
      const { response, ...args } = input;
      const result = await router.askJev(structuredClone(args));
      if (result && typeof result.ms !== "number") throw new Error(`${name}: askJev result has no ms`);
      if (result) delete result.ms;
      cases.push({
        name,
        input,
        expected: { url: sent?.url ?? null, method: sent?.method ?? null, body: sent?.body ?? null, result },
      });
    }
  } finally {
    globalThis.fetch = realFetch;
    process.stderr.write = realWrite;
  }
  return { "jev-request": cases };
}

// Cases for config.mjs and policy.mjs: override detection, the policy decision, and the env-driven
// tier settings.
import { readFileSync } from "node:fs";
import { join } from "node:path";

export default function policyCases({ root, config, policy, calibration }) {
  const handback = readFileSync(join(root, "conformance", "fixtures", "subagent-handback-prompt.txt"), "utf8");

  // From node/test/policy.test.mjs, the calibration prompts, the handback fixture, and the edge
  // inputs SPEC 16.2 names: JSWS separators, non-JSWS controls, `ſ`/Kelvin folding, restarts.
  const prompts = [
    ["switch to opus", "switch to opus"],
    ["use haiku", "use haiku"],
    ["use the strong model", "use the strong model"],
    ["Use Claude Haiku for this one", "Use Claude Haiku for this one"],
    ["use haiku to fix this typo", "use haiku to fix this typo"],
    ["the opus of his career", "the opus of his career"],
    ["refactor the parser", "refactor the parser"],
    ["prose fast fourier", "help me with fast fourier transform code"],
    ["prose long polling", "replace the polling loop with long polling"],
    ["prose fast CI", "the test only fails on fast CI runners"],
    ["prose long input", "write tests with long input strings"],
    ["prose fast refresh", "turn on fast refresh in vite"],
    ["prose strong typing", "refactor this to rely on strong typing"],
    ["prose haiku-style", "use haiku-style commit messages"],
    ["prose long variable names", "use long variable names"],
    ["handback fixture", handback],
    ["nbsp separator", "use opus"],
    ["ideographic space separator", "use　opus"],
    ["bom separator", "use﻿opus"],
    ["unit separator is not whitespace", "use\u001fopus"],
    ["next line is not whitespace", "use\u0085opus"],
    ["mongolian vowel separator is not whitespace", "use᠎opus"],
    ["line separator", "use opus"],
    ["newline separator", "use\nopus"],
    ["several spaces", "use   opus"],
    ["long s does not fold to s (use)", "uſe opus"],
    ["long s does not fold to s (switch)", "ſwitch to opus"],
    ["long s does not fold to s (sonnet)", "use ſonnet"],
    ["long s does not fold to s (fast)", "use the faſt model"],
    ["kelvin sign is not k", "use the Kelvin opus"],
    ["ascii upper case", "USE OPUS"],
    ["mixed case", "sWiTcH tO SoNnEt"],
    ["restart after hyphen", "use opus-x use opus"],
    ["hyphen only", "use opus-x"],
    ["underscore after name", "use opus_"],
    ["word continues", "use opuses"],
    ["digit after name", "use opus4"],
    ["dot after name", "use opus."],
    ["version suffix", "use opus-4"],
    ["claude hyphen", "use claude-opus"],
    ["claude space", "use claude opus"],
    ["claude glued", "use claudeopus"],
    ["generic fast tier", "use fast tier"],
    ["generic long model", "use long model"],
    ["generic long spaced model", "use long  model"],
    ["generic balanced", "use the balanced model"],
    ["route to the claude fable", "route to the claude fable"],
    ["switch over to claude sonnet", "switch over to claude sonnet"],
    ["switch double space", "switch  to opus"],
    ["reuse is not use", "reuse opus"],
    ["re-use", "re-use opus"],
    ["non-ascii letter before verb", "éuse opus"],
    ["non-ascii letter after name", "use opusé"],
    ["backticks stripped", "`use opus`"],
    ["quotes stripped", 'say "use opus" here'],
    ["fence stripped", "```\nuse opus\n```"],
    ["reminder stripped", "<system-reminder>use opus</system-reminder> hi"],
    ["agent message stripped", "<agent-message from=x>use opus</agent-message> thanks"],
    ["unclosed quote keeps text", 'he said "use opus'],
    ["quote across newline keeps text", '"a\nuse opus"'],
    ["haiku before opus in tier order", "use opus and use haiku"],
    ["fable last in tier order", "use fable or use sonnet"],
    ["negated: do not use", "do not use fable"],
    ["negated: never use", "never use fable for this"],
    ["negated: don't switch to", "please don't switch to fable"],
    ["negated: curly apostrophe", "please don’t switch to fable"],
    ["negated: dont", "dont use haiku"],
    ["negated: cannot use", "we cannot use opus on this account"],
    ["negated: upper case", "do NOT route to sonnet"],
    ["negated: adverb between", "Don't ever use Opus here"],
    ["negated: generic name", "do not use the strong model"],
    ["negated tier beside a real one", "don't use haiku, use opus"],
    ["negated, then a sentence that asks", "do not use fable. use sonnet for the tests"],
    ["negated twice, last one real", "never use haiku for this but use opus"],
    ["not a negation: nobody", "nobody use opus"],
    ["not a negation: know", "know use opus"],
    ["not a negation: another word between", "do not just now use opus"],
    ["question", "why does the planner use opus?"],
    ["question: switch", "should we switch to haiku for the lint step?"],
    ["question after an instruction", "Use opus for the migration. Is that too slow?"],
    ["question on an earlier line", "what does the router do when I say\nuse opus"],
    ["question mark inside a url", "see https://example.com/a?b=1 then use opus"],
    ["question mark in a quote", 'he asked "why?" so use opus'],
    ["question dot in a version", "use opus for the v1.2 migration?"],
    ["empty", ""],
    ["null", null],
    ["undefined", undefined],
    ["number", 42],
    ...calibration.CASES.map((c, i) => [`calibration ${i} (${c.want})`, c.prompt]),
  ];
  const detectOverride = prompts.map(([name, prompt]) => ({
    name,
    input: { prompt },
    expected: policy.detectOverride(prompt),
  }));

  const ALL = ["haiku", "sonnet", "opus", "fable"];
  const base = { prompt: "refactor the parser", current: "sonnet", available: ALL, contextTokens: 0 };
  const sure = (choice) => ({ choice, confidence: 0.95 });
  const unsure = (choice) => ({ choice, confidence: 0.2 });
  const decideInputs = [
    ["confident opus", { ...base, jev: sure("opus") }],
    ["override beats jev", { ...base, prompt: "use haiku to fix this typo", jev: sure("opus") }],
    ["handback quote is not an override", { ...base, prompt: handback, current: "opus", jev: sure("sonnet") }],
    ["jev null", { ...base, jev: null }],
    ["invented tier", { ...base, jev: sure("mystery-9") }],
    ["unsure opus from haiku", { ...base, current: "haiku", jev: unsure("opus") }],
    ["unsure haiku", { ...base, jev: unsure("haiku") }],
    ["middling haiku", { ...base, jev: { choice: "haiku", confidence: 0.45 } }],
    ["sound haiku", { ...base, jev: { choice: "haiku", confidence: 0.78 } }],
    ["unsure haiku on opus", { ...base, current: "opus", jev: unsure("haiku") }],
    ["unsure haiku on fable", { ...base, current: "fable", jev: unsure("haiku") }],
    ["confidence missing", { ...base, current: "haiku", jev: { choice: "haiku" } }],
    ["unsure opus on sonnet", { ...base, jev: unsure("opus") }],
    ["unsure fable on sonnet", { ...base, jev: unsure("fable") }],
    ["unsure sonnet on haiku", { ...base, current: "haiku", jev: unsure("sonnet") }],
    ["unsure fable on haiku", { ...base, current: "haiku", jev: unsure("fable") }],
    ["confident fable", { ...base, jev: sure("fable") }],
    ["downgrade refused at 80000", { ...base, current: "opus", jev: sure("haiku"), contextTokens: 80000 }],
    ["downgrade allowed at 20000", { ...base, current: "opus", jev: sure("haiku"), contextTokens: 20000 }],
    ["downgrade refused at 20001", { ...base, current: "opus", jev: sure("haiku"), contextTokens: 20001 }],
    ["downgrade early", { ...base, current: "opus", jev: sure("haiku") }],
    ["contextTokens omitted", { prompt: "x", current: "opus", available: ALL, jev: sure("haiku") }],
    ["upgrade above context guard", { ...base, current: "haiku", jev: sure("opus"), contextTokens: 90000 }],
    ["substitute upward", { ...base, current: "haiku", available: ["haiku", "opus"], jev: sure("sonnet") }],
    ["never up into fable", { ...base, current: "haiku", available: ["haiku", "fable"], jev: sure("opus") }],
    ["fable asked, unavailable", { ...base, current: "haiku", available: ["haiku", "sonnet"], jev: sure("fable") }],
    ["nothing available", { ...base, current: "opus", available: [], jev: sure("haiku") }],
    ["override unavailable", { ...base, prompt: "use fable", available: ["haiku", "sonnet", "opus"], jev: null }],
    ["override no-change", { ...base, prompt: "switch to sonnet", jev: sure("opus") }],
    ["fable off: unsure fable", { ...base, available: ["haiku", "sonnet", "opus"], jev: unsure("fable") }],
    ["current unknown tier", { ...base, current: "mystery", jev: sure("opus") }],
  ];
  // SPEC 3.6: confidence goes through ToNumber, and anything not >= 0.6 is unsure.
  const confidences = [
    ["string 0.9", "0.9"],
    ["array [0.9]", [0.9]],
    ["array ['0.9']", ["0.9"]],
    ["nested [[0.7]]", [[0.7]]],
    ["array [1,2]", [1, 2]],
    ["empty array", []],
    ["array [null]", [null]],
    ["hex with spaces", " 0x1 "],
    ["signed hex", "-0x1"],
    ["null", null],
    ["empty string", ""],
    ["abc", "abc"],
    ["true", true],
    ["false", false],
    ["object", {}],
    ["Infinity string", "Infinity"],
    ["exponent string", "6e-1"],
    ["binary string", "0b1"],
    ["octal string", "0o1"],
    ["exactly 0.6", 0.6],
    ["just under 0.6", 0.5999999999999999],
    ["string 0.6", "0.6"],
    ["NaN", NaN],
    ["nbsp padded", " 0.9 "],
    ["next-line padded", "\u00850.9"],
    ["undefined member", undefined],
  ];
  for (const [label, confidence] of confidences) {
    decideInputs.push([`confidence ${label}`, { ...base, current: "haiku", jev: { choice: "opus", confidence } }]);
  }
  decideInputs.push(["confidence key absent", { ...base, current: "haiku", jev: { choice: "opus" } }]);
  const decide = decideInputs.map(([name, input]) => ({
    name,
    input,
    expected: policy.decide(structuredClone(input)),
  }));

  // SPEC 3.5: trim() strips JSWS (NBSP, BOM, U+3000) but not U+0085 or U+001F.
  const efforts = ["High", " HIGH ", "low", "xhigh", "max", "turbo", "", " max ", "﻿medium", "\u0085high", "\u001fhigh", "Max\n"];
  const effortFloor = [];
  for (const name of ["haiku", "sonnet", "opus", "fable", "nonsense"]) {
    effortFloor.push({ name: `${name} with no env`, input: { name, env: {} }, expected: config.effortFloor(name, {}) });
    for (const value of efforts) {
      const env = { [`JEV_${name.toUpperCase()}_EFFORT`]: value };
      effortFloor.push({ name: `${name} ${JSON.stringify(value)}`, input: { name, env }, expected: config.effortFloor(name, env) });
    }
  }
  effortFloor.push({
    name: "another tier's setting is ignored",
    input: { name: "opus", env: { JEV_SONNET_EFFORT: "low" } },
    expected: config.effortFloor("opus", { JEV_SONNET_EFFORT: "low" }),
  });

  const forcedEnvs = [
    ["none", {}],
    ["global", { JEV_FORCE_EFFORT: "low" }],
    ["global padded", { JEV_FORCE_EFFORT: " Max " }],
    ["per tier beats global", { JEV_FORCE_EFFORT: "low", TIER: "xhigh" }],
    ["invalid per tier falls to global", { JEV_FORCE_EFFORT: "low", TIER: "turbo" }],
    ["invalid both", { JEV_FORCE_EFFORT: "turbo", TIER: "warp" }],
    ["empty per tier falls to global", { JEV_FORCE_EFFORT: "high", TIER: "" }],
    ["nbsp global", { JEV_FORCE_EFFORT: " medium" }],
    ["next-line global is invalid", { JEV_FORCE_EFFORT: "medium\u0085" }],
    ["upper per tier", { TIER: "HIGH" }],
  ];
  const forcedEffort = [];
  for (const name of ["haiku", "sonnet", "opus", "fable", "nonsense"]) {
    for (const [label, template] of forcedEnvs) {
      const env = {};
      for (const [k, v] of Object.entries(template)) env[k === "TIER" ? `JEV_${name.toUpperCase()}_FORCE_EFFORT` : k] = v;
      forcedEffort.push({ name: `${name} ${label}`, input: { name, env }, expected: config.forcedEffort(name, env) });
    }
  }

  const fableValues = ["0", "false", "No", " off ", "OFF", "1", "", "yes", "true", " 0 ", "0\u0085", "﻿off", "falsey", "0 0", "　no", "\u001fno"];
  const fableAllowed = [
    { name: "unset", input: { env: {} }, expected: config.fableAllowed({}) },
    ...fableValues.map((value) => ({
      name: JSON.stringify(value),
      input: { env: { JEV_ALLOW_FABLE: value } },
      expected: config.fableAllowed({ JEV_ALLOW_FABLE: value }),
    })),
  ];

  return {
    "detect-override": detectOverride,
    decide,
    "effort-floor": effortFloor,
    "forced-effort": forcedEffort,
    "fable-allowed": fableAllowed,
  };
}

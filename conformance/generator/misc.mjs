// Cases for update.mjs, launch.mjs's cmd quoting, and Node's env-file parser (SPEC 9.1, 10.2, 14).
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { parseEnv } from "node:util";

export default function miscCases({ root, update, launch }) {
  const versionPairs = [
    ["0.10.0", "0.9.9"], ["0.6.5", "0.6.6"], ["1.0", "1.0.0"], ["0.7.0-beta.1", "0.6.9"], ["0.7.0-beta.1", "0.7.0"],
    ["1.0.0-rc.2", "1.0.0-rc.10"], ["v1.2", "1.2"], ["1.2.3.4", "1.2.3"], ["", "0"], ["01.2", "1.2"], ["1.x.3", "1.0.3"],
    [" 1.2", "1.2"], ["\u00a01.2", "1.2"], ["1e3.0", "1.0"], ["-1.0", "0.0"], ["0x10.1", "0.1"], ["2", "10"],
    ["0.9.2", "0.9.2"], ["0.9.10", "0.9.9"], ["1.2.", "1.2"], ["10.0.0", "9.99.99"], [1.5, "1.5"], [null, "0"],
    [undefined, "0.0.0"], ["3.0.0+build.5", "3.0.0"],
  ];
  const compareVersions = versionPairs.map(([a, b]) => ({
    name: `${JSON.stringify(a) ?? "undefined"} vs ${JSON.stringify(b)}`,
    input: { a, b },
    expected: update.compareVersions(a, b),
  }));

  const notices = [
    ["newer", { available: true, latest: "0.7.0" }, "0.6.5"],
    ["equal", { available: true, latest: "0.7.0" }, "0.7.0"],
    ["older", { available: true, latest: "0.7.0" }, "0.8.0"],
    ["not available", { available: false, latest: "0.7.0" }, "0.6.5"],
    ["null state", null, "0.6.5"],
    ["no current version", { available: true, latest: "0.7.0" }, null],
    ["empty current version", { available: true, latest: "0.7.0" }, ""],
    ["pre-release latest", { available: true, latest: "0.8.0-beta.1" }, "0.7.9"],
    ["pre-release same release", { available: true, latest: "0.8.0-beta" }, "0.8.0"],
    ["truthy available", { available: "yes", latest: "1.0.0" }, "0.9.2"],
    ["empty latest", { available: true, latest: "" }, "0.1.0"],
    ["missing latest", { available: true }, "0.1.0"],
    ["checker output", { checkedAt: "2026-10-04T12:00:00.000Z", available: true, latest: "0.10.0", remote: "abc" }, "0.9.2"],
  ];
  const updateNotice = notices.map(([name, state, currentVersion]) => ({
    name,
    input: { state, currentVersion },
    expected: update.updateNotice(state, currentVersion),
  }));

  // SPEC 14: ports parse only the toISOString form, so every checkedAt here is either exactly that
  // form or a string no date parser accepts.
  const now = Date.parse("2026-10-04T12:00:00.000Z");
  const iso = (ms) => new Date(now - ms).toISOString();
  const every = update.CHECK_EVERY_MS;
  const checks = [
    ["null state", null, now],
    ["empty state", {}, now],
    ["garbage", { checkedAt: "yesterday-ish" }, now],
    ["empty string", { checkedAt: "" }, now],
    ["null checkedAt", { checkedAt: null }, now],
    ["just inside the window", { checkedAt: iso(every - 60_000) }, now],
    ["one ms inside", { checkedAt: iso(every - 1) }, now],
    ["exactly at the window", { checkedAt: iso(every) }, now],
    ["well past", { checkedAt: iso(every * 10) }, now],
    ["future", { checkedAt: iso(-60_000) }, now],
    ["one ms in the future", { checkedAt: iso(-1) }, now],
    ["same instant", { checkedAt: iso(0) }, now],
    ["custom interval inside", { checkedAt: iso(500) }, now, 1000],
    ["custom interval at", { checkedAt: iso(1000) }, now, 1000],
    ["epoch", { checkedAt: new Date(0).toISOString() }, now],
  ];
  const isCheckDue = checks.map(([name, state, at, everyMs]) => ({
    name,
    input: { state, now: at, ...(everyMs === undefined ? {} : { everyMs }) },
    expected: update.isCheckDue(state, at, everyMs),
  }));

  const changeSets = [
    ["docs only", ["README.md", "src/proxy.mjs"]],
    ["version bump", ["package.json"]],
    ["lockfile", ["README.md", "pnpm-lock.yaml"]],
    ["empty", []],
    ["empty diff line", [""]],
    ["nested lockfile", ["node/pnpm-lock.yaml"]],
    ["padded name", ["pnpm-lock.yaml "]],
    ["npm lockfile", ["package-lock.json"]],
  ];
  const needsInstall = changeSets.map(([name, changedFiles]) => ({ name, input: { changedFiles }, expected: update.needsInstall(changedFiles) }));

  const args = [
    'name="Jev Router"', "fix a&b|c", "50% done", 'say "hi"', "plain", "", " ", "a b", "C:\\path\\", "C:\\path with space\\",
    'back\\"slash', 'two\\\\"q', 'end\\\\', "\\\\server\\share", "trailing\\", '"', '""', "^caret^", "!bang!", "(parens)",
    "[brackets]", "%PATH%", "`tick`", "<in>", "out>", "semi;colon", "com,ma", "star*", "what?", "tab\there", "naïve", 42,
    "()[]%!^\"`<>&|;, *?",
  ];
  const quoteForCmd = args.map((arg) => ({ name: JSON.stringify(arg), input: { arg }, expected: launch.quoteForCmd(arg) }));

  // Every branch of conformance/reference/node_dotenv_parse_content.cc (SPEC 9.1).
  const envTexts = [
    ["simple", "A=1\nB=2\n"],
    ["hash in value", "A=a#b"],
    ["hash mid-file", "A=a # comment\nB=b"],
    ["comment line", "# comment\nA=1"],
    ["comment without newline", "A=1\n# trailing comment"],
    ["blank lines", "\n\nA=1\n\n\nB=2\n\n"],
    ["unclosed double quote with following line", 'A="value\nB=2'],
    ["unclosed double quote at end", 'A="value'],
    ["unclosed single quote with following line", "A='value\nB=2"],
    ["unclosed single quote at end", "A='value"],
    ["unclosed backtick at end", "A=`value"],
    ["text after closing double quote", 'A="quoted" trailing\nB=2'],
    ["text after closing single quote", "A='quoted' trailing\nB=2"],
    ["closing quote at end without newline", 'A="quoted"'],
    ["escaped n in double quotes", 'A="line1\\nline2"'],
    ["escaped n in single quotes", "A='line1\\nline2'"],
    ["escaped n in backticks", "A=`line1\\nline2`"],
    ["escaped n unquoted", "A=line1\\nline2"],
    ["real newline inside double quotes", 'A="one\ntwo"\nB=3'],
    ["real newline inside single quotes", "A='one\ntwo'\nB=3"],
    ["backticks", "A=`tick`\nB=2"],
    ["backtick containing quotes", "A=`it's \"x\"`"],
    ["double quote containing single", "A=\"it's\""],
    ["empty double quotes", 'A=""\nB=2'],
    ["export prefix", "export A=1"],
    ["export with spaces", "export    A=1"],
    ["export alone as key", "export=1"],
    ["exported tab is not a prefix", "export\tA=1"],
    ["spaces around equals", "A = 1\n  B  =  two words  \n"],
    ["tabs around equals", "A\t=\t1"],
    ["CRLF", "A=1\r\nB=2\r\n"],
    ["lone CR removed everywhere", "A=a\rb\nB=\"x\ry\""],
    ["empty key", "=a=b\nB=2"],
    ["empty key with empty value", "=\nA=1"],
    ["space-only key", "   =value\nA=1"],
    ["line without equals", "JUSTTEXT\nA=1"],
    ["line without equals at end", "A=1\nJUSTTEXT"],
    ["only text", "no equals here"],
    ["repeated keys keep last", "A=1\nA=2\nA=3"],
    ["trailing key without newline", "A=1\nB=last"],
    ["key with empty value then end", "A="],
    ["key with empty value mid-file", "A=\nB=2"],
    ["value of spaces", "A=   \nB=2"],
    ["value of spaces at end", "A=   "],
    ["equals in value", "A=b=c"],
    ["leading whitespace in file", "   \n\t A=1"],
    ["vertical tab and nbsp are not trimmed", "A=\u000bx\u000b\nB=\u00a0y\u00a0"],
    ["non-ascii value", "A=caf\u00e9 \ud83d\ude80"],
    ["quote not at value start", "A=x\"y\""],
    ["hash right after equals", "A=#notacomment\nB=2"],
    ["hash inside double quotes", 'A="a#b" # c'],
    ["key with dots and dashes", "my.key-name=1"],
    ["lowercase key", "lower=1"],
    ["integer-like key", "1=one\nB=b"],
    ["empty file", ""],
    ["only whitespace", "  \n\t\n"],
    ["double-quoted multi escapes", 'A="a\\nb\\nc\\\\n"'],
    ["unclosed quote followed by valid lines", "A=\"open\nB='closed'\nC=c"],
  ];
  const envExample = readFileSync(join(root, ".env.example"), "utf8");
  envTexts.push([".env.example", envExample]);
  envTexts.push(["hardening test project .env", [
    "JEV_API_KEY=from-project", "JEV_OPUS_EFFORT=low", "JEV_FORCE_EFFORT=max", "JEV_SONNET_FORCE_EFFORT=low",
    "ANTHROPIC_BASE_URL=https://attacker.example", "TYPESAFE_BASE_URL=https://attacker.example",
    "NODE_OPTIONS=--require /tmp/evil.js", "JEV_DUMP=/tmp/loot", "JEV_DEBUG=project",
  ].join("\n")]);
  const parseEnvCases = envTexts.map(([name, text]) => ({ name, input: { text }, expected: { ...parseEnv(text) } }));

  return {
    "compare-versions": compareVersions,
    "update-notice": updateNotice,
    "is-check-due": isCheckDue,
    "needs-install": needsInstall,
    "quote-for-cmd": quoteForCmd,
    "parse-env": parseEnvCases,
  };
}

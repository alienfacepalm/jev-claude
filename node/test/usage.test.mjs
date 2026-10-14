import assert from "node:assert/strict";
import test from "node:test";
import { icons } from "../src/icons.mjs";
import { usageMeters } from "../src/usage.mjs";

const SYMBOLS = icons({ JEV_ICONS: "symbols" }, "darwin");
const TEXT = icons({ JEV_ICONS: "text" }, "darwin");
// biome-ignore lint/suspicious/noControlCharactersInRegex: strips the ANSI codes the meters emit
const plain = (text) => text.replace(/\x1b\[[0-9;]*m/g, "");

// The rate_limits block exactly as Claude Code documents it for the status line
// (https://code.claude.com/docs/en/statusline), spend_limit included.
const FROM_THE_DOCS = {
  five_hour: { used_percentage: 23.5, resets_at: 1738425600 },
  seven_day: { used_percentage: 41.2, resets_at: 1738857600 },
  spend_limit: { used_percentage: 62.8, resets_at: 1740787200, used_usd: 314.12, limit_usd: 500, period: "monthly" },
};

test("the 5-hour and 7-day limits Claude Code reports are drawn as a label, a bar of four, and a percentage", () => {
  assert.equal(plain(usageMeters(FROM_THE_DOCS, SYMBOLS)), " · 5h ▰▱▱▱ 24% · 7d ▰▰▱▱ 41%");
});

test("a console without the bar glyphs gets the percentage alone", () => {
  assert.equal(plain(usageMeters(FROM_THE_DOCS, TEXT)), " · 5h 24% · 7d 41%");
});

test("an account with no limits to report (an API key, or before the first response) adds nothing", () => {
  assert.equal(usageMeters(undefined, SYMBOLS), "");
  assert.equal(usageMeters({}, SYMBOLS), "");
  assert.equal(usageMeters({ five_hour: {} }, SYMBOLS), "");
});

test("a window that has passed its reset is dropped by Claude Code, and the other still shows alone", () => {
  assert.equal(plain(usageMeters({ seven_day: FROM_THE_DOCS.seven_day }, SYMBOLS)), " · 7d ▰▰▱▱ 41%");
  assert.equal(plain(usageMeters({ five_hour: FROM_THE_DOCS.five_hour }, SYMBOLS)), " · 5h ▰▱▱▱ 24%");
});

test("the spend limit and any other window are not drawn", () => {
  assert.equal(
    usageMeters({ spend_limit: FROM_THE_DOCS.spend_limit, seven_day_opus: { used_percentage: 90 } }, SYMBOLS),
    "",
  );
});

test("the value is dim until 70% of a limit is used, then bold", () => {
  const near = usageMeters({ five_hour: { used_percentage: 70 }, seven_day: { used_percentage: 69.4 } }, SYMBOLS);
  assert.ok(near.includes("\x1b[1m▰▰▰▱ 70%"), near);
  assert.ok(near.includes("\x1b[2m▰▰▰▱ 69%"), near);
});

test("a limit that is used up fills the bar, and one that has barely been touched still shows a cell", () => {
  assert.equal(plain(usageMeters({ five_hour: { used_percentage: 100 } }, SYMBOLS)), " · 5h ▰▰▰▰ 100%");
  assert.equal(plain(usageMeters({ five_hour: { used_percentage: 100.4 } }, SYMBOLS)), " · 5h ▰▰▰▰ 100%");
  assert.equal(plain(usageMeters({ five_hour: { used_percentage: 1 } }, SYMBOLS)), " · 5h ▰▱▱▱ 1%");
  assert.equal(plain(usageMeters({ five_hour: { used_percentage: 0 } }, SYMBOLS)), " · 5h ▱▱▱▱ 0%");
});

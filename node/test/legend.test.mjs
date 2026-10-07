import test from "node:test";
import assert from "node:assert/strict";
import { formatLegend } from "../src/legend.mjs";
import { icons } from "../src/icons.mjs";

test("the key explains every mark the status line draws, in the set it is drawing", () => {
  for (const choice of ["symbols", "text"]) {
    const set = icons({ JEV_ICONS: choice }, "darwin");
    const legend = formatLegend(set);
    for (const [name, mark] of Object.entries(set)) assert.ok(legend.includes(mark), `${choice}: ${name}`);
  }
});

test("the symbols are the same ones the status line prints", () => {
  const legend = formatLegend(icons({ JEV_ICONS: "symbols" }, "darwin"));
  assert.match(legend, /^✧✦ +the model/);
  assert.match(legend, /\ue0a0 +the git branch/);
});

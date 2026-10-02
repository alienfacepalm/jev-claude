#!/usr/bin/env node
import { readStatus, mainDecision } from "../src/status.mjs";
import { formatExplanation, formatAgents } from "../src/explain.mjs";

const status = readStatus(process.argv[2] ?? process.env.JEV_CODEX_STATUS_ID);
// The detailed box is about the conversation the user is in, not whichever sub-agent was
// routed last; the agent table below it covers the rest.
const main = mainDecision(status);
const agents = formatAgents(status);

process.stdout.write(
  `${formatExplanation(main && status?.manual ? { ...main, manual: true } : main)}\n${agents ? `${agents}\n` : ""}`,
);

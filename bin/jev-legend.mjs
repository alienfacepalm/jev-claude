#!/usr/bin/env node
import { formatLegend } from "../src/legend.mjs";

process.stdout.write(`Status line key\n\n${formatLegend()}\n`);

#!/usr/bin/env node
// Started detached by jev-claude to look for a newer version, so the launch itself never waits
// on the network. Records what it finds in ~/.jev-router/update.json for the next launch to show.
import { dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { checkForUpdate, writeState } from "../src/update.mjs";

// The repository root (the parent of node/): the clone being updated, and where the release
// version lives in package.json.
const ROOT = dirname(dirname(dirname(fileURLToPath(import.meta.url))));

writeState(await checkForUpdate(ROOT));

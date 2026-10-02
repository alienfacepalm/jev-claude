import { copyFileSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { CODEX_AUTO_MODEL, startCodexProxy } from "./codex-proxy.mjs";
import { loadEnv, childEnv } from "./env.mjs";
import { resolveCommand, launchSpec, spawnSpec } from "./launch.mjs";

export { loadEnv };

const PROVIDER = "jev";
const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const EXPLAIN_SKILL = join(ROOT, "skills", "codex", "jev-explain", "SKILL.md");

export function installCodexSkill(home = homedir()) {
  const target = join(home, ".agents", "skills", "jev-router-explain", "SKILL.md");
  mkdirSync(dirname(target), { recursive: true });
  copyFileSync(EXPLAIN_SKILL, target);
  return target;
}

/**
 * How to launch Codex. npm installs both a `.cmd` and a `.ps1` shim; the `.cmd` comes first
 * because its script can be run directly, while a `.ps1` needs PowerShell and a policy bypass.
 */
export function resolveCodex() {
  const file = resolveCommand("codex", { exts: [".exe", ".cmd", ".bat", ".ps1"] });
  return file ? launchSpec(file) : null;
}

export const codexArgs = (baseURL, args) => [
  ...(args.some((arg) => arg === "--model" || arg === "-m" || arg.startsWith("--model="))
    ? []
    : ["--model", CODEX_AUTO_MODEL]),
  "--config",
  `model_provider="${PROVIDER}"`,
  "--config",
  `model_providers.${PROVIDER}.name="Jev Router"`,
  "--config",
  `model_providers.${PROVIDER}.base_url="${baseURL}"`,
  "--config",
  `model_providers.${PROVIDER}.wire_api="responses"`,
  "--config",
  `model_providers.${PROVIDER}.requires_openai_auth=true`,
  "--config",
  `model_providers.${PROVIDER}.supports_websockets=false`,
  ...args,
];

// Loads configuration, starts the optional routing proxy, and launches the Codex CLI.
export async function runCodex() {
  loadEnv();
  try {
    installCodexSkill();
  } catch (err) {
    process.stderr.write(`[jev] could not install the Codex explanation skill: ${err.message}\n`);
  }
  const command = resolveCodex();
  if (!command) {
    process.stderr.write(
      "[jev] OpenAI Codex is not installed, or `codex` is not on your PATH.\n" +
        "[jev] jev-codex runs the real Codex CLI; install it first:\n" +
        "[jev]   https://developers.openai.com/codex/cli\n",
    );
    process.exitCode = 1;
    return;
  }

  let args = process.argv.slice(2);
  let close = () => {};
  const statusId = `codex-${process.pid}`;
  process.env.JEV_CODEX_STATUS_ID = statusId;
  if (process.env.JEV_API_KEY || process.env.TYPESAFE_API_KEY) {
    const proxy = await startCodexProxy({ statusId });
    close = proxy.close;
    args = codexArgs(`http://127.0.0.1:${proxy.port}`, args);
  } else {
    process.stderr.write(
      "[jev] no JEV_API_KEY found - starting Codex without routing\n" +
        `[jev] add JEV_API_KEY=... to ${join(homedir(), ".jev-router.env")} and restart jev-codex\n`,
    );
  }

  // The Jev key is jev's alone; Codex and every command it runs go without it.
  const child = spawnSpec(command, args, { stdio: "inherit", env: childEnv() });
  child.on("error", (err) => {
    close();
    process.stderr.write(`[jev] could not start Codex: ${err.message}\n`);
    process.exitCode = 1;
  });
  child.on("exit", (code, signal) => {
    close();
    process.exitCode = signal ? 1 : (code ?? 0);
  });
}

import { execFile } from "node:child_process";
import { mkdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { promisify } from "node:util";
import { renameOver } from "./atomic-rename.mjs";

const run = promisify(execFile);

/**
 * What the last update check found. Kept in the home directory next to the first-run marker: the
 * OS clears temp, and re-checking after every reboot would only cost a fetch.
 */
export const UPDATE_FILE = join(homedir(), ".jev-router", "update.json");

/** How long a check stays fresh. Launches inside this window do not touch the network. */
export const CHECK_EVERY_MS = 6 * 60 * 60 * 1000;

// The check runs detached, after Claude Code has started, so a slow remote delays nothing; this
// only stops a hung connection from leaving a process behind.
const FETCH_TIMEOUT_MS = 20_000;
const LOCAL_TIMEOUT_MS = 5_000;

async function git(root, args, timeout = LOCAL_TIMEOUT_MS) {
  const { stdout } = await run("git", ["-C", root, ...args], {
    timeout,
    windowsHide: true,
    // A remote that wants credentials must fail here, not wait on a prompt nobody can see.
    env: { ...process.env, GIT_TERMINAL_PROMPT: "0" },
  });
  return stdout.trim();
}

/** The saved update-check state, or null when it is missing or unreadable. */
export function readState(file = UPDATE_FILE) {
  try {
    const state = JSON.parse(readFileSync(file, "utf8"));
    return state && typeof state === "object" ? state : null;
  } catch {
    return null;
  }
}

/** Saves the update-check state atomically; a failure is ignored, since the check simply runs again. */
export function writeState(state, file = UPDATE_FILE) {
  try {
    mkdirSync(dirname(file), { recursive: true });
    // Renamed into place: a launch reading it while a background check writes never sees half.
    const temp = `${file}.${process.pid}.tmp`;
    writeFileSync(temp, JSON.stringify(state));
    renameOver(temp, file);
  } catch {
    // Unwritable home directory: the check runs again next launch, which is harmless.
  }
}

/** Whether the last check is old enough (or missing, or unreadable) to run another. */
export function isCheckDue(state, now = Date.now(), everyMs = CHECK_EVERY_MS) {
  const at = Date.parse(state?.checkedAt ?? "");
  return !Number.isFinite(at) || now - at >= everyMs || at > now;
}

/** Compares dotted release numbers; negative when `a` is older than `b`. Pre-release tags are ignored. */
export function compareVersions(a, b) {
  const parts = (v) =>
    String(v)
      .split("-")[0]
      .split(".")
      .map((n) => Number.parseInt(n, 10) || 0);
  const [x, y] = [parts(a), parts(b)];
  for (let i = 0; i < Math.max(x.length, y.length); i++) {
    const diff = (x[i] ?? 0) - (y[i] ?? 0);
    if (diff !== 0) return diff;
  }
  return 0;
}

/** The one line printed at launch, or null. Never claims an update the install already has. */
export function updateNotice(state, currentVersion) {
  if (!state?.available || !state.latest || !currentVersion) return null;
  if (compareVersions(state.latest, currentVersion) <= 0) return null;
  return `[jev] Update available: ${currentVersion} -> ${state.latest}. Run \`jev-claude --update\`.`;
}

/** The version in `root`/package.json, or null when it cannot be read. */
export function installedVersion(root) {
  try {
    return JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version ?? null;
  } catch {
    return null;
  }
}

/**
 * Whether `root` is a clone this tool may fast-forward, fetching the remote branch it tracks.
 * Resolves to the facts the caller needs, or `{ ok: false, reason }`; it never throws. Only a
 * clean checkout on a branch whose history the remote extends qualifies: anything else is
 * someone's development copy, and updating it is not this tool's call.
 */
export async function inspectClone(root) {
  try {
    const top = await git(root, ["rev-parse", "--show-toplevel"]);
    // An install inside some other repository must not be mistaken for a clone of its own.
    if (realpathSync(resolve(top)) !== realpathSync(resolve(root))) {
      return { ok: false, reason: "this folder is not a git clone of its own" };
    }
  } catch {
    return { ok: false, reason: "this folder is not a git clone" };
  }
  let branch;
  try {
    branch = await git(root, ["symbolic-ref", "--short", "HEAD"]);
  } catch {
    return { ok: false, reason: "the checkout is not on a branch" };
  }
  try {
    if (await git(root, ["status", "--porcelain", "--untracked-files=no"])) {
      return { ok: false, reason: "there are local changes in the checkout" };
    }
    await git(root, ["fetch", "--quiet", "origin", branch], FETCH_TIMEOUT_MS);
    const head = await git(root, ["rev-parse", "HEAD"]);
    const remote = await git(root, ["rev-parse", "FETCH_HEAD"]);
    if (head === remote) return { ok: true, branch, head, remote, behind: false };
    const isAncestor = (older, newer) =>
      git(root, ["merge-base", "--is-ancestor", older, newer]).then(
        () => true,
        () => false,
      );
    // Ahead of origin (a development copy with nothing new upstream): nothing to fetch, nothing wrong.
    if (await isAncestor("FETCH_HEAD", "HEAD")) return { ok: true, branch, head, remote, behind: false };
    if (!(await isAncestor("HEAD", "FETCH_HEAD"))) {
      return { ok: false, reason: `local ${branch} has commits that origin/${branch} does not` };
    }
    return { ok: true, branch, head, remote, behind: true };
  } catch (error) {
    return { ok: false, reason: `could not reach origin (${String(error.message).split("\n")[0]})` };
  }
}

/** The version in package.json at `ref`, or null. */
async function versionAt(root, ref) {
  try {
    return JSON.parse(await git(root, ["show", `${ref}:package.json`])).version ?? null;
  } catch {
    return null;
  }
}

/**
 * One update check, as the state file records it. A clone that cannot be checked is recorded as
 * having no update, so its notice never lingers from an earlier check.
 */
export async function checkForUpdate(root, now = Date.now()) {
  const checkedAt = new Date(now).toISOString();
  const clone = await inspectClone(root);
  if (!clone.ok || !clone.behind) return { checkedAt, available: false };
  const latest = await versionAt(root, "FETCH_HEAD");
  return { checkedAt, available: Boolean(latest), latest, remote: clone.remote };
}

/**
 * Whether these changed files mean the dependencies need installing again. Only the lockfile
 * counts: every release bumps the version in package.json, and `--frozen-lockfile` insists the
 * two agree, so a real dependency change always shows up in the lockfile.
 */
export const needsInstall = (changedFiles) => changedFiles.includes("pnpm-lock.yaml");

/**
 * Fast-forwards the clone to its remote and, when the dependencies changed, installs them with
 * `install(root)`. Resolves `{ status: "current" | "updated" | "refused" | "failed", ... }`.
 * Nothing is rewritten that a plain `git pull --ff-only` would not rewrite.
 */
export async function applyUpdate(root, { install } = {}) {
  const clone = await inspectClone(root);
  if (!clone.ok) return { status: "refused", reason: clone.reason };
  const from = installedVersion(root);
  if (!clone.behind) return { status: "current", version: from };
  try {
    const changed = (await git(root, ["diff", "--name-only", "HEAD", "FETCH_HEAD"])).split("\n");
    await git(root, ["merge", "--ff-only", "FETCH_HEAD"], FETCH_TIMEOUT_MS);
    const to = installedVersion(root);
    if (needsInstall(changed)) {
      const code = await install?.(root);
      if (code) return { status: "failed", reason: `installing dependencies exited with ${code}`, from, to };
    }
    return { status: "updated", from, to };
  } catch (error) {
    return { status: "failed", reason: String(error.message).split("\n")[0], from };
  }
}

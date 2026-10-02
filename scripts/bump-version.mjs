// Bumps the version in package.json from the commits being released. Run by
// .github/workflows/version-bump.yml on every push to master:
//   node scripts/bump-version.mjs <before-sha> <after-sha>
// prints the new version, after writing it to package.json.
//
// Commit subjects follow Conventional Commits (https://www.conventionalcommits.org):
//   - "BREAKING CHANGE" in a message, or "!" before the colon ("feat!: ...")  -> major
//   - any "feat: ..." / "feat(scope): ..." subject                            -> minor
//   - anything else (fix:, docs:, chore:, or no prefix)                         -> patch
// Release commits made by the workflow itself are ignored.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const RELEASE = /^chore\(release\):/;

/** The bump the messages call for: "major", "minor" or "patch". */
export function bumpLevel(messages) {
  const real = messages.filter((m) => m.trim() && !RELEASE.test(m.trim()));
  if (real.some((m) => /BREAKING[ -]CHANGE/.test(m) || /^\w+(\([^)]*\))?!:/.test(m.trim()))) return "major";
  if (real.some((m) => /^feat(\([^)]*\))?:/.test(m.trim()))) return "minor";
  return "patch";
}

/** `version` raised by `level`, with the lower parts reset. */
export function nextVersion(version, level) {
  const [major, minor, patch] = version.split(/[.+-]/).map(Number);
  if (level === "major") return `${major + 1}.0.0`;
  if (level === "minor") return `${major}.${minor + 1}.0`;
  return `${major}.${minor}.${patch + 1}`;
}

/**
 * Full messages of the commits in `before..after`. A branch's first push has no `before` (git
 * sends all zeros), so only `after` itself counts then.
 */
function messagesBetween(before, after) {
  const range = !before || /^0+$/.test(before) ? [after, "-1"] : [`${before}..${after}`];
  return execFileSync("git", ["log", "--format=%B%x00", ...range], { encoding: "utf8" })
    .split("\0")
    .filter((m) => m.trim());
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [before, after = "HEAD"] = process.argv.slice(2);
  const file = new URL("../package.json", import.meta.url);
  const pkg = JSON.parse(readFileSync(file, "utf8"));
  pkg.version = nextVersion(pkg.version, bumpLevel(messagesBetween(before, after)));
  writeFileSync(file, `${JSON.stringify(pkg, null, 2)}\n`);
  console.log(pkg.version);
}

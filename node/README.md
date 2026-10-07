# jev-claude in Node.js

The reference implementation of jev-claude, and the one the installers set up.
[`../SPEC.md`](../SPEC.md) describes its behaviour for the ports; the golden cases in
[`../conformance/cases`](../conformance/cases) are generated from this code.

**The full guide is [`../doc/NODE.md`](../doc/NODE.md)**: installing Node and pnpm on Windows,
macOS and Linux, putting the commands on `PATH`, configuration, tests, lint and format, the golden
cases, the conformance harness, troubleshooting, and the known divergences and gaps.

## Quick start

Needs Node.js 22.16+ or 24+ (24 LTS recommended), pnpm, git, and Claude Code (`claude` on `PATH`).
From the repository root:

```bash
pnpm install --frozen-lockfile
pnpm add --global "link:$(pwd)"      # Git Bash: "link:$(pwd -W)"; PowerShell: "link:$PWD"
cp .env.example ~/.jev-router.env    # then paste your key after JEV_API_KEY=
jev-claude                           # any claude arguments pass through
```

## Check a change

From the repository root:

```bash
pnpm lint                 # Biome, recommended rules
pnpm run format:check     # Biome formatter
pnpm test                 # node/test, then the conformance harness against Node
```

After a behaviour change, regenerate the golden cases on Node 24.21 with
`node conformance/generate.mjs` and run every port's tests; see
[Golden cases](../doc/NODE.md#golden-cases).

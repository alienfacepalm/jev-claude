const DIM = "\x1b[2m";
const BOLD = "\x1b[1m";
const RESET = "\x1b[0m";

// Claude Code's `rate_limits` windows, in the order the line shows them, with the icon-set name of
// each window's label. Only claude.ai Pro and Max accounts get them, and only after the first
// response, so each one is simply left out when it is missing.
const WINDOWS = [
  ["five_hour", "fiveHour"],
  ["seven_day", "sevenDay"],
];
const CELLS = 4;
// From this share of a limit used, its value is bold instead of dimmed.
const NEAR = 70;

/**
 * Tiny versions of the `/usage` meters: for each limit window Claude Code reports, its label, a bar
 * of four cells when the icon set has one (each cell a quarter of the limit; any use fills at least
 * the first), and the percentage used. Dimmed while there is plenty left, bold from 70%. Returns
 * the items, each led by the line's ` · ` separator, or "" when no window has a usable number.
 */
export function usageMeters(rateLimits, set) {
  let out = "";
  for (const [key, label] of WINDOWS) {
    const used = rateLimits?.[key]?.used_percentage;
    if (typeof used !== "number" || !Number.isFinite(used)) continue;
    const shown = Math.round(Math.min(100, Math.max(0, used)));
    const filled = Math.ceil(shown / (100 / CELLS));
    const bar = set.meterFull ? `${set.meterFull.repeat(filled)}${set.meterEmpty.repeat(CELLS - filled)} ` : "";
    const emphasis = shown >= NEAR ? BOLD : DIM;
    out += ` ${DIM}·${RESET} ${BOLD}${set[label]}${RESET} ${emphasis}${bar}${shown}%${RESET}`;
  }
  return out;
}

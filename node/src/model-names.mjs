/**
 * A model's short display name with its version: `claude-opus-5-5` is "Opus 5.5",
 * `claude-haiku-4-5-20251001` is "Haiku 4.5", `claude-opus-6` is "Opus 6". Read from the id
 * itself rather than from the tier table, so a new release or a new model line names itself the
 * day the router starts using it. Null for anything that is not a Claude model id.
 *
 * Kept free of imports: the status line calls it on every redraw.
 */
export function shortName(model) {
  const m = /claude-([a-z]+)-(\d+)(?:-(\d{1,2})(?!\d))?/.exec(model ?? "");
  if (!m) return null;
  const [, family, major, minor] = m;
  return `${family[0].toUpperCase()}${family.slice(1)} ${minor ? `${major}.${minor}` : major}`;
}

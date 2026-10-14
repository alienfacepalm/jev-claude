//! Tiny versions of the `/usage` meters (SPEC 12; `node/src/usage.mjs`).

use crate::icons::Icons;
use crate::jsjson::Value;
use crate::jsstr::{math_round, number_to_string};
use std::fmt::Write as _;

const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";
const CELLS: usize = 4;
/// From this share of a limit used, its value is bold instead of dimmed.
const NEAR: f64 = 70.0;

/// For each limit window Claude Code reports in `rate_limits` (`five_hour`, then `seven_day`): its
/// label, a bar of four cells when the icon set has one (each cell a quarter of the limit, any use
/// that shows as at least 1% filling the first), and the percentage used. Dimmed while there is
/// plenty left, bold from 70%. Each item is led by the line's ` · ` separator; the result is empty
/// when no window has a usable number.
pub fn usage_meters(rate_limits: &Value, set: &Icons) -> String {
    let mut out = String::new();
    for (key, label) in [("five_hour", set.five_hour), ("seven_day", set.seven_day)] {
        // Only a finite JSON number counts: a string, null, boolean or array is left out.
        let Value::Number(used) = *rate_limits.get(key).get("used_percentage") else { continue };
        if !used.is_finite() {
            continue;
        }
        // `Math.min(100, Math.max(0, used))`, which turns -0 into +0.
        let clamped = if used > 100.0 {
            100.0
        } else if used > 0.0 {
            used
        } else {
            0.0
        };
        let shown = math_round(clamped);
        let filled = (shown / (100.0 / CELLS as f64)).ceil() as usize;
        let bar = if set.meter_full.is_empty() {
            String::new()
        } else {
            format!("{}{} ", set.meter_full.repeat(filled), set.meter_empty.repeat(CELLS - filled))
        };
        let emphasis = if shown >= NEAR { BOLD } else { DIM };
        let _ = write!(
            out,
            " {DIM}\u{00B7}{RESET} {BOLD}{label}{RESET} {emphasis}{bar}{}%{RESET}",
            number_to_string(shown)
        );
    }
    out
}

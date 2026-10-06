//! How a routing decision is said to a person (SPEC 12.4; `node/src/reasons.mjs`).

struct Reason {
    matches: &'static str,
    short: Option<&'static str>,
    long: &'static str,
}

const REASONS: [Reason; 5] = [
    Reason { matches: "override", short: None, long: "you named this model in the prompt" },
    Reason {
        matches: "jev-unavailable",
        short: Some("router offline"),
        long: "the router could not be reached, so the model was left alone",
    },
    Reason {
        matches: "low-confidence-default",
        short: None,
        long: "the router was unsure, so this ran one tier below its pick, and no lower than the default model",
    },
    Reason {
        matches: "downgrade-not-worth-cache-rebuild",
        short: Some("keeping the cache"),
        long: "a cheaper model would have to re-read the whole conversation, which costs more than it saves",
    },
    Reason {
        matches: "unavailable",
        short: Some("nearest available"),
        long: "the chosen tier is not available on this account, so the nearest one was used",
    },
];

fn find(reason: Option<&str>) -> Option<&'static Reason> {
    let reason = reason.filter(|r| !r.is_empty())?;
    REASONS.iter().find(|r| reason.contains(r.matches))
}

/// A few words for the status line, or None when the decision speaks for itself.
pub fn short_reason(reason: Option<&str>) -> Option<&'static str> {
    find(reason).and_then(|r| r.short)
}

/// A sentence for the explanation panel.
pub fn long_reason(reason: Option<&str>) -> &'static str {
    find(reason).map_or("the router's recommendation", |r| r.long)
}

/// Whether a decision left the model where it already was.
pub fn is_no_change(reason: Option<&str>) -> bool {
    reason.is_some_and(|r| r.contains("no-change"))
}

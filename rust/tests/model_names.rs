//! Port of node/test/model-names.test.mjs.

// Each test's doc comment is the Node test title, quoted verbatim so the two suites can be
// compared line by line; Markdown backticks would change the titles.
#![allow(clippy::doc_markdown)]

use jev_router::model_names::short_name;

fn name(s: &str) -> Option<String> {
    short_name(s.as_bytes())
}

/// "a model id reads as its family and version"
#[test]
fn a_model_id_reads_as_its_family_and_version() {
    assert_eq!(name("claude-opus-5-5").as_deref(), Some("Opus 5.5"));
    assert_eq!(name("claude-sonnet-5-5").as_deref(), Some("Sonnet 5.5"));
    assert_eq!(name("claude-fable-5-1").as_deref(), Some("Fable 5.1"));
    assert_eq!(name("claude-opus-6").as_deref(), Some("Opus 6"), "a whole-number release");
    assert_eq!(name("claude-sonnet-5-10").as_deref(), Some("Sonnet 5.10"));
}

/// "a date suffix or a context tag is not part of the version"
#[test]
fn a_date_suffix_or_a_context_tag_is_not_part_of_the_version() {
    assert_eq!(name("claude-haiku-4-5-20251001").as_deref(), Some("Haiku 4.5"));
    assert_eq!(name("claude-opus-4-6[1m]").as_deref(), Some("Opus 4.6"));
}

/// "anything that is not a Claude model id has no short name"
#[test]
fn anything_that_is_not_a_claude_model_id_has_no_short_name() {
    assert_eq!(name("mystery-9"), None);
    assert_eq!(name("jev-router"), None);
    assert_eq!(name(""), None, "undefined becomes the empty string");
}

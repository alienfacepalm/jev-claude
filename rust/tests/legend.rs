//! Port of node/test/legend.test.mjs.

// Each test's doc comment is the Node test title, quoted verbatim so the two suites can be
// compared line by line; Markdown backticks would change the titles.
#![allow(clippy::doc_markdown)]

use jev_router::envx::map;
use jev_router::icons::icons_for;
use jev_router::legend::format_legend;
use regex::Regex;

/// "the key explains every mark the status line draws, in the set it is drawing"
#[test]
fn the_key_explains_every_mark_the_status_line_draws() {
    for choice in ["symbols", "text"] {
        let set = icons_for(&map(&[("JEV_ICONS", choice)]), false);
        let legend = format_legend(&set);
        for (name, mark) in set.entries() {
            assert!(legend.contains(mark), "{choice}: {name}");
        }
    }
}

/// "the symbols are the same ones the status line prints"
#[test]
fn the_symbols_are_the_same_ones_the_status_line_prints() {
    let legend = format_legend(&icons_for(&map(&[("JEV_ICONS", "symbols")]), false));
    assert!(Regex::new("^\u{2727}\u{2726} +the model").unwrap().is_match(&legend));
    assert!(Regex::new("(?m)\u{E0A0} +the git branch").unwrap().is_match(&legend));
}

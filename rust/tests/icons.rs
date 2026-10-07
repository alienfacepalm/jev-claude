//! Port of node/test/icons.test.mjs.

// Each test's doc comment is the Node test title, quoted verbatim so the two suites can be
// compared line by line; Markdown backticks would change the titles.
#![allow(clippy::doc_markdown)]

use jev_router::envx::{EnvMap, map};
use jev_router::icons::{Icons, icons_for};

fn is_text(set: Icons) -> bool {
    set.dir == "dir"
}

/// "symbols everywhere but the legacy Windows console"
#[test]
fn symbols_everywhere_but_the_legacy_windows_console() {
    assert!(!is_text(icons_for(&EnvMap::new(), false)), "darwin and linux");
    assert!(is_text(icons_for(&EnvMap::new(), true)), "conhost fonts lack the glyphs");
}

/// "a Windows terminal that announces itself gets symbols"
#[test]
fn a_windows_terminal_that_announces_itself_gets_symbols() {
    for env in [
        map(&[("WT_SESSION", "1")]),
        map(&[("TERM_PROGRAM", "vscode")]),
        map(&[("TERM_PROGRAM", "mintty")]),
        map(&[("ConEmuPID", "42")]),
    ] {
        assert!(!is_text(icons_for(&env, true)), "{env:?}");
    }
}

/// "JEV_ICONS overrides the guess in both directions"
#[test]
fn jev_icons_overrides_the_guess_in_both_directions() {
    assert!(is_text(icons_for(&map(&[("JEV_ICONS", "text")]), false)));
    assert!(is_text(icons_for(&map(&[("JEV_ICONS", "ASCII")]), false)));
    assert!(!is_text(icons_for(&map(&[("JEV_ICONS", "symbols")]), true)));
}

/// "every item has a symbol and a word"
#[test]
fn every_item_has_a_symbol_and_a_word() {
    let text = icons_for(&map(&[("JEV_ICONS", "text")]), false);
    let symbols = icons_for(&map(&[("JEV_ICONS", "symbols")]), false);
    let keys = |s: Icons| s.entries().map(|(k, _)| k);
    assert_eq!(keys(text), keys(symbols));
    for set in [text, symbols] {
        for (name, label) in set.entries() {
            assert!(!label.is_empty(), "{name}");
        }
    }
}

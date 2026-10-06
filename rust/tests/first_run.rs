//! Port of node/test/first-run.test.mjs.

mod common;

use common::*;
use jev_router::firstrun::{Answer, ask_yes_no, mark_offered, shadows_skill, should_offer, was_offered};
use std::io::{BufRead, Read};

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

/// "the setup check is offered only on a plain interactive first launch"
#[test]
fn the_setup_check_is_offered_only_on_a_plain_interactive_first_launch() {
    assert!(should_offer(&[], true, false, false));
    assert!(!should_offer(&[], true, true, false), "once per user");
    assert!(!should_offer(&[], false, false, false), "nobody to ask");
    for a in [args(&["-p", "fix it"]), args(&["--resume"]), args(&["explain this repo"])] {
        assert!(!should_offer(&a, true, false, false), "{a:?}");
    }
}

/// "the offer is not made where a repository defines its own jev-calibrate skill"
#[test]
fn the_offer_is_not_made_where_a_repository_defines_its_own_skill() {
    let repo = temp_dir("jev-shadow-");
    let router = repo.join("router");
    std::fs::create_dir_all(router.join(".claude").join("skills").join("jev-calibrate")).unwrap();
    let other = repo.join("other");
    std::fs::create_dir_all(other.join(".claude").join("skills").join("jev-calibrate")).unwrap();

    assert!(shadows_skill(&other, Some(&router)), "someone else's skill of the same name");
    assert!(!shadows_skill(&router, Some(&router)), "the router's own skill, in its own repository");
    assert!(!shadows_skill(&repo, Some(&router)), "no such skill here");
    assert!(!should_offer(&[], true, false, true));
    let _ = std::fs::remove_dir_all(&repo);
}

/// "an offer is remembered whatever the answer"
#[test]
fn an_offer_is_remembered_whatever_the_answer() {
    let dir = temp_dir("jev-first-run-");
    let file = dir.join("nested").join("first-run.json");
    assert!(!was_offered(&file));
    mark_offered(false, &file);
    assert!(was_offered(&file), "a no is remembered too, so it is never asked again");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains(r#""accepted":false"#) && text.contains(r#""offeredAt":""#));
    let _ = std::fs::remove_dir_all(&dir);
}

fn answer(text: &str) -> Answer {
    let mut out = Vec::new();
    let a = ask_yes_no("? ", &mut text.as_bytes(), &mut out);
    assert_eq!(out, b"? ", "the question is written out");
    a
}

/// An input that fails on read, like a terminal that goes away.
struct Broken;
impl Read for Broken {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("EIO"))
    }
}

/// "a closed or failing input settles with no answer instead of hanging"
#[test]
fn a_closed_or_failing_input_settles_with_no_answer() {
    assert_eq!(answer(""), Answer::None);
    let mut broken = std::io::BufReader::new(Broken);
    let reader: &mut dyn BufRead = &mut broken;
    assert_eq!(ask_yes_no("? ", reader, &mut Vec::new()), Answer::None);
}

/// "an empty answer or yes accepts, and no declines"
#[test]
fn an_empty_answer_or_yes_accepts_and_no_declines() {
    assert_eq!(answer("\n"), Answer::Yes);
    assert_eq!(answer("y\n"), Answer::Yes);
    assert_eq!(answer("Yes\n"), Answer::Yes);
    assert_eq!(answer("n\n"), Answer::No);
    assert_eq!(answer("NO\n"), Answer::No);
}

//! Replacing a file (SPEC 3.11; `node/test/status.test.mjs`): the rename retries while another
//! process holds the target open, and a rename that never lands removes the temporary file.

mod common;

use common::temp_dir;
use jev_router::fsx::rename_over;
use std::fs;
use std::path::Path;

fn leftovers(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|path| path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("tmp")))
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

#[test]
fn replaces_an_existing_file() {
    let dir = temp_dir("fsx-replace");
    let (file, temp) = (dir.join("a.json"), dir.join("a.json.1.tmp"));
    fs::write(&file, "old").unwrap();
    fs::write(&temp, "new").unwrap();
    rename_over(&temp, &file).unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), "new");
    assert_eq!(leftovers(&dir), Vec::<String>::new());
}

#[test]
fn a_missing_temp_file_is_an_error() {
    let dir = temp_dir("fsx-missing");
    assert!(rename_over(&dir.join("missing.tmp"), &dir.join("a.json")).is_err());
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;
    use std::time::Duration;

    /// Opens `file` with read sharing but no delete sharing, as antivirus, the indexer and a plain
    /// reader do, so nothing can be renamed over it until the handle drops.
    fn hold_open(file: &Path) -> fs::File {
        fs::OpenOptions::new().read(true).share_mode(1).open(file).unwrap()
    }

    #[test]
    fn lands_while_another_handle_briefly_holds_the_file_open() {
        let dir = temp_dir("fsx-held-briefly");
        let (file, temp) = (dir.join("a.json"), dir.join("a.json.1.tmp"));
        fs::write(&file, "old").unwrap();
        let held = hold_open(&file);
        fs::write(&temp, "new").unwrap();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            drop(held);
        });
        rename_over(&temp, &file).expect("the rename was retried until the file was free");
        releaser.join().unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "new");
        assert_eq!(leftovers(&dir), Vec::<String>::new());
    }

    #[test]
    fn a_file_that_stays_held_is_an_error_and_removes_the_temp_file() {
        let dir = temp_dir("fsx-held-long");
        let (file, temp) = (dir.join("a.json"), dir.join("a.json.1.tmp"));
        fs::write(&file, "old").unwrap();
        let held = hold_open(&file);
        fs::write(&temp, "secret prompt").unwrap();
        assert!(rename_over(&temp, &file).is_err(), "the retries run out");
        assert!(leftovers(&dir).is_empty(), "the temp file held prompt text and must be gone");
        drop(held);
        assert_eq!(fs::read_to_string(&file).unwrap(), "old", "the held file kept its previous content");
    }
}

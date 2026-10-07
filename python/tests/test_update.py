"""Port of node/test/update.test.mjs.

These tests drive real git: a bare repository stands in for GitHub, clones are made the way the
installer makes them (including the shallow `--depth 1` one), and the "upstream" moves on by real
commits. What is checked is what the user's checkout looks like afterwards.
"""

import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest
from collections.abc import Mapping
from typing import Any

from jev_router.jsstr import JsObject
from jev_router.log import iso_from_ms
from jev_router.update import (
    CHECK_EVERY_MS,
    apply_update,
    check_for_update,
    compare_versions,
    is_check_due,
    needs_install,
    parse_iso,
    read_state,
    update_notice,
    write_state,
)

from . import as_str, force_rmtree, present


def git(cwd: str, *args: str) -> str:
    out = subprocess.run(
        ["git", "-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false", *args],
        cwd=cwd,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        check=True,
    )
    return out.stdout.decode("utf-8").strip()


def pkg(version: str) -> str:
    return json.dumps({"name": "jev-router", "version": version}, indent=2) + "\n"


def write(path: str, text: str) -> None:
    with open(path, "w", encoding="utf-8", newline="\n") as handle:
        handle.write(text)


def version(directory: str) -> Any:
    with open(os.path.join(directory, "package.json"), encoding="utf-8") as handle:
        return json.load(handle)["version"]


class Fixture:
    """An origin at 0.1.0, a working copy that pushes to it, and a helper to release from it."""

    def __init__(self, test: unittest.TestCase) -> None:
        self.base = tempfile.mkdtemp(prefix="jev-update-")
        test.addCleanup(force_rmtree, self.base)
        self.origin = os.path.join(self.base, "origin.git")
        self.upstream = os.path.join(self.base, "upstream")
        git(self.base, "init", "--bare", "-b", "master", self.origin)
        git(self.base, "clone", self.origin, self.upstream)
        git(self.upstream, "checkout", "-b", "master")
        write(os.path.join(self.upstream, "package.json"), pkg("0.1.0"))
        write(os.path.join(self.upstream, "pnpm-lock.yaml"), "lock: 1\n")
        git(self.upstream, "add", ".")
        git(self.upstream, "commit", "-m", "first")
        git(self.upstream, "push", "-u", "origin", "master")

    def release(self, number: str, files: Mapping[str, str] | None = None) -> None:
        write(os.path.join(self.upstream, "package.json"), pkg(number))
        for name, text in (files or {}).items():
            write(os.path.join(self.upstream, name), text)
        git(self.upstream, "add", ".")
        git(self.upstream, "commit", "-m", f"release {number}")
        git(self.upstream, "push", "origin", "master")

    def clone_to(self, name: str, shallow: bool = False) -> str:
        directory = os.path.join(self.base, name)
        if shallow:
            git(self.base, "clone", "--depth", "1", pathlib.Path(self.origin).as_uri(), directory)
        else:
            git(self.base, "clone", self.origin, directory)
        return directory


class CloneKinds(unittest.TestCase):
    def check_level(self, shallow: bool) -> None:
        fixture = Fixture(self)
        install = fixture.clone_to("install", shallow=shallow)
        found = check_for_update(install)
        self.assertFalse(found["available"])
        self.assertIsNone(update_notice(found, version(install)))

    def check_release(self, shallow: bool) -> None:
        fixture = Fixture(self)
        install = fixture.clone_to("install", shallow=shallow)
        fixture.release("0.2.0", {"new-file.txt": "hello\n"})

        found = check_for_update(install)
        self.assertTrue(found["available"])
        self.assertEqual(found["latest"], "0.2.0")
        self.assertEqual(
            update_notice(found, version(install)), "[jev] Update available: 0.1.0 -> 0.2.0. Run `jev-claude --update`."
        )
        self.assertEqual(version(install), "0.1.0", "looking must not change the install")

        applied = apply_update(install)
        self.assertEqual(applied, {"status": "updated", "from": "0.1.0", "to": "0.2.0"})
        self.assertEqual(version(install), "0.2.0")
        with open(os.path.join(install, "new-file.txt"), encoding="utf-8") as handle:
            self.assertEqual(handle.read(), "hello\n")
        self.assertEqual(git(install, "status", "--porcelain"), "", "a clean checkout afterwards")

        self.assertFalse(check_for_update(install)["available"], "nothing further to find")
        self.assertEqual(apply_update(install), {"status": "current", "version": "0.2.0"})

    def test_an_install_that_is_level_with_origin_has_no_update_a_full_clone(self) -> None:
        """an install that is level with origin has no update (a full clone)"""
        self.check_level(False)

    def test_an_install_that_is_level_with_origin_has_no_update_a_shallow_clone(self) -> None:
        """an install that is level with origin has no update (a shallow clone, as the installer makes)"""
        self.check_level(True)

    def test_a_release_upstream_is_found_announced_and_applied_by_fast_forward_a_full_clone(self) -> None:
        """a release upstream is found, announced, and applied by fast-forward (a full clone)"""
        self.check_release(False)

    def test_a_release_upstream_is_found_announced_and_applied_by_fast_forward_a_shallow_clone(self) -> None:
        """a release upstream is found, announced, and applied by fast-forward (a shallow clone, as the installer makes)"""  # noqa: E501 - the Node test title, verbatim
        self.check_release(True)


class Update(unittest.TestCase):
    def test_changed_dependencies_ask_for_an_install_unchanged_ones_do_not(self) -> None:
        """changed dependencies ask for an install; unchanged ones do not"""
        fixture = Fixture(self)
        install = fixture.clone_to("install")
        fixture.release("0.1.1", {"notes.txt": "docs only\n"})
        calls: list[str] = []

        def recorded(root: str) -> int:
            calls.append(root)
            return 0

        self.assertEqual(apply_update(install, install=recorded)["status"], "updated")
        self.assertEqual(calls, [], "a change that leaves package.json's dependencies alone installs nothing")

        fixture.release("0.2.0", {"pnpm-lock.yaml": "lock: 2\n"})
        self.assertEqual(apply_update(install, install=recorded)["status"], "updated")
        self.assertEqual(calls, [install], "a new lockfile installs once, in the install folder")

        fixture.release("0.3.0", {"pnpm-lock.yaml": "lock: 3\n"})
        failed = apply_update(install, install=lambda root: 1)
        self.assertEqual(failed["status"], "failed", "a failed install is reported, not hidden")
        self.assertRegex(as_str(failed["reason"]), r"exited with 1")

        self.assertFalse(needs_install(["README.md", "src/proxy.mjs"]))
        self.assertFalse(needs_install(["package.json"]), "a version bump alone is not a dependency change")
        self.assertTrue(needs_install(["README.md", "pnpm-lock.yaml"]))

    def test_a_copy_with_local_changes_is_left_exactly_as_it_is(self) -> None:
        """a copy with local changes is left exactly as it is"""
        fixture = Fixture(self)
        install = fixture.clone_to("install")
        write(os.path.join(install, "pnpm-lock.yaml"), "lock: 1\nmy edit\n")
        fixture.release("0.2.0")

        found = check_for_update(install)
        self.assertFalse(found["available"], "no notice for a copy that cannot be updated")
        applied = apply_update(install)
        self.assertEqual(applied["status"], "refused")
        self.assertRegex(as_str(applied["reason"]), r"local changes")
        with open(os.path.join(install, "pnpm-lock.yaml"), encoding="utf-8") as handle:
            self.assertRegex(handle.read(), r"my edit", "the edit survives")
        self.assertEqual(version(install), "0.1.0")

    def test_a_development_clone_ahead_of_origin_is_not_touched(self) -> None:
        """a development clone ahead of origin is not touched"""
        fixture = Fixture(self)
        install = fixture.clone_to("install")
        write(os.path.join(install, "mine.txt"), "work in progress\n")
        git(install, "add", ".")
        git(install, "commit", "-m", "my own commit")
        head = git(install, "rev-parse", "HEAD")

        level = apply_update(install)
        self.assertEqual(level["status"], "current", "ahead of origin with nothing new upstream: nothing to do")
        self.assertEqual(git(install, "rev-parse", "HEAD"), head)

        fixture.release("0.2.0")
        found = check_for_update(install)
        self.assertFalse(found["available"], "diverged: neither announced nor applied")
        applied = apply_update(install)
        self.assertEqual(applied["status"], "refused")
        self.assertRegex(as_str(applied["reason"]), r"commits that origin/master does not")
        self.assertEqual(git(install, "rev-parse", "HEAD"), head, "no merge, no rewrite")

    def test_a_detached_checkout_is_refused(self) -> None:
        """a detached checkout is refused"""
        fixture = Fixture(self)
        install = fixture.clone_to("install")
        git(install, "checkout", "--detach")
        fixture.release("0.2.0")
        applied = apply_update(install)
        self.assertEqual(applied["status"], "refused")
        self.assertRegex(as_str(applied["reason"]), r"not on a branch")

    def test_an_unreachable_origin_is_reported_never_thrown_and_the_install_is_unchanged(self) -> None:
        """an unreachable origin is reported, never thrown, and the install is unchanged"""
        fixture = Fixture(self)
        install = fixture.clone_to("install")
        force_rmtree(fixture.origin)

        self.assertFalse(check_for_update(install)["available"])
        applied = apply_update(install)
        self.assertEqual(applied["status"], "refused")
        self.assertRegex(as_str(applied["reason"]), r"^could not reach origin \(")
        self.assertEqual(version(install), "0.1.0")

    def test_folders_that_are_not_a_clone_of_their_own_are_refused(self) -> None:
        """folders that are not a clone of their own are refused"""
        fixture = Fixture(self)
        plain = os.path.join(fixture.base, "plain")
        os.mkdir(plain)
        refused_plain = apply_update(plain)
        self.assertEqual(refused_plain["status"], "refused")
        self.assertRegex(as_str(refused_plain["reason"]), r"not a git clone")

        outer = fixture.clone_to("outer")
        inner = os.path.join(outer, "vendored", "jev-claude")
        os.makedirs(inner)
        write(os.path.join(inner, "package.json"), pkg("0.0.1"))
        refused_inner = apply_update(inner)
        self.assertEqual(refused_inner["status"], "refused")
        self.assertRegex(as_str(refused_inner["reason"]), r"not a git clone of its own")

    def test_the_check_is_due_when_missing_stale_unreadable_or_from_the_future(self) -> None:
        """the check is due when missing, stale, unreadable, or from the future"""
        now = present(parse_iso("2026-10-04T12:00:00.000Z"))

        def at(ms: float) -> JsObject:
            return {"checkedAt": iso_from_ms(now - ms)}

        self.assertTrue(is_check_due(None, now))
        self.assertTrue(is_check_due({}, now))
        self.assertTrue(is_check_due({"checkedAt": "yesterday-ish"}, now))
        self.assertFalse(is_check_due(at(CHECK_EVERY_MS - 60_000), now), "just inside the window")
        self.assertTrue(is_check_due(at(CHECK_EVERY_MS), now), "exactly at the window")
        self.assertTrue(is_check_due(at(-60_000), now), "a clock that went backwards must not silence checks")

    def test_versions_compare_as_numbers_not_text(self) -> None:
        """versions compare as numbers, not text"""
        self.assertGreater(compare_versions("0.10.0", "0.9.9"), 0, "0.10 is newer than 0.9")
        self.assertLess(compare_versions("0.6.5", "0.6.6"), 0)
        self.assertEqual(compare_versions("1.0", "1.0.0"), 0)
        self.assertGreater(compare_versions("0.7.0-beta.1", "0.6.9"), 0, "a pre-release tag is ignored")

    def test_a_notice_appears_only_for_a_genuinely_newer_version(self) -> None:
        """a notice appears only for a genuinely newer version"""
        found = {"available": True, "latest": "0.7.0"}
        self.assertRegex(present(update_notice(found, "0.6.5")), r"0\.6\.5 -> 0\.7\.0")
        self.assertIsNone(update_notice(found, "0.7.0"), "already installed some other way")
        self.assertIsNone(update_notice(found, "0.8.0"), "a development copy ahead of the release")
        self.assertIsNone(update_notice({"available": False, "latest": "0.7.0"}, "0.6.5"))
        self.assertIsNone(update_notice(None, "0.6.5"))
        self.assertIsNone(update_notice(found, None), "an unreadable local version says nothing")

    def test_the_state_file_round_trips_and_a_damaged_one_reads_as_no_state(self) -> None:
        """the state file round-trips and a damaged one reads as no state"""
        directory = tempfile.mkdtemp(prefix="jev-update-state-")
        self.addCleanup(shutil.rmtree, directory, True)
        file = os.path.join(directory, "nested", "update.json")
        self.assertIsNone(read_state(file), "missing")
        state = {"checkedAt": "2026-10-04T12:00:00.000Z", "available": True, "latest": "0.7.0"}
        write_state(state, file)
        self.assertEqual(read_state(file), state)
        write(file, "{ not json")
        self.assertIsNone(read_state(file), "damaged")
        write(file, "42")
        self.assertIsNone(read_state(file), "valid JSON that is not a state object")

    def test_a_state_write_that_fails_leaves_no_temporary_file_behind(self) -> None:
        """A failed rename (here the target is a directory, on every OS) cleans up its temp file."""
        directory = tempfile.mkdtemp(prefix="jev-update-state-")
        self.addCleanup(shutil.rmtree, directory, True)
        file = os.path.join(directory, "update.json")
        os.mkdir(file)
        write_state({"checkedAt": "2026-10-04T12:00:00.000Z", "available": False}, file)
        self.assertEqual(sorted(os.listdir(directory)), ["update.json"], "no update.json.<pid>.<n>.tmp is left")
        self.assertTrue(os.path.isdir(file), "the failed write changed nothing")


if __name__ == "__main__":
    unittest.main()

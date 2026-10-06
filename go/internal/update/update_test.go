package update

import (
	"errors"
	"fmt"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
	"time"

	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
)

// Ported from node/test/update.test.mjs. These tests drive real git: a bare repository stands
// in for GitHub, clones are made the way the installer makes them (including the shallow
// --depth 1 one), and the "upstream" moves on by real commits.

func sh(t *testing.T, cwd string, args ...string) string {
	t.Helper()
	cmd := exec.Command("git", append([]string{"-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"}, args...)...)
	cmd.Dir = cwd
	out, err := cmd.Output()
	if err != nil {
		var stderr string
		var ee *exec.ExitError
		if errors.As(err, &ee) {
			stderr = string(ee.Stderr)
		}
		t.Fatalf("git %v: %v %s", args, err, stderr)
	}
	return strings.TrimSpace(string(out))
}

func pkg(version string) string {
	return jsjson.Indent(jsjson.Obj("name", "jev-router", "version", version)) + "\n"
}

func write(t *testing.T, file, text string) {
	t.Helper()
	if err := os.WriteFile(file, []byte(text), 0o644); err != nil {
		t.Fatal(err)
	}
}

type fixture struct {
	base, origin, upstream string
	t                      *testing.T
}

// newFixture is an origin at 0.1.0 and a working copy that pushes to it.
func newFixture(t *testing.T) *fixture {
	base := t.TempDir()
	f := &fixture{base: base, origin: filepath.Join(base, "origin.git"), upstream: filepath.Join(base, "upstream"), t: t}
	sh(t, base, "init", "--bare", "-b", "master", f.origin)
	sh(t, base, "clone", f.origin, f.upstream)
	sh(t, f.upstream, "checkout", "-b", "master")
	write(t, filepath.Join(f.upstream, "package.json"), pkg("0.1.0"))
	write(t, filepath.Join(f.upstream, "pnpm-lock.yaml"), "lock: 1\n")
	sh(t, f.upstream, "add", ".")
	sh(t, f.upstream, "commit", "-m", "first")
	sh(t, f.upstream, "push", "-u", "origin", "master")
	return f
}

func (f *fixture) release(version string, files map[string]string) {
	write(f.t, filepath.Join(f.upstream, "package.json"), pkg(version))
	for name, text := range files {
		write(f.t, filepath.Join(f.upstream, name), text)
	}
	sh(f.t, f.upstream, "add", ".")
	sh(f.t, f.upstream, "commit", "-m", "release "+version)
	sh(f.t, f.upstream, "push", "origin", "master")
}

func (f *fixture) cloneTo(name string, shallow bool) string {
	dir := filepath.Join(f.base, name)
	if shallow {
		// pathToFileURL: file:///C:/... on Windows, file:///tmp/... elsewhere.
		u := url.URL{Scheme: "file", Path: "/" + strings.TrimPrefix(filepath.ToSlash(f.origin), "/")}
		sh(f.t, f.base, "clone", "--depth", "1", u.String(), dir)
	} else {
		sh(f.t, f.base, "clone", f.origin, dir)
	}
	return dir
}

func version(t *testing.T, dir string) any {
	data, err := os.ReadFile(filepath.Join(dir, "package.json"))
	if err != nil {
		t.Fatal(err)
	}
	v, _ := jsjson.ParseBytes(data)
	return jsjson.Prop(v, "version")
}

func notice(state any, current any) any {
	s, ok := UpdateNotice(state, current)
	if !ok {
		return nil
	}
	return s
}

func TestUpdate(t *testing.T) {
	for _, shallow := range []bool{false, true} {
		kind := "a full clone"
		if shallow {
			kind = "a shallow clone, as the installer makes"
		}

		t.Run(fmt.Sprintf("an install that is level with origin has no update (%s)", kind), func(t *testing.T) {
			f := newFixture(t)
			install := f.cloneTo("install", shallow)
			found := CheckForUpdate(install, time.Now())
			if found.Value("available") != false || notice(found, version(t, install)) != nil {
				t.Fatalf("%s", jsjson.Stringify(found))
			}
		})

		t.Run(fmt.Sprintf("a release upstream is found, announced, and applied by fast-forward (%s)", kind), func(t *testing.T) {
			f := newFixture(t)
			install := f.cloneTo("install", shallow)
			f.release("0.2.0", map[string]string{"new-file.txt": "hello\n"})

			found := CheckForUpdate(install, time.Now())
			if found.Value("available") != true || found.Value("latest") != "0.2.0" {
				t.Fatalf("%s", jsjson.Stringify(found))
			}
			if got := notice(found, version(t, install)); got != "[jev] Update available: 0.1.0 -> 0.2.0. Run `jev-claude --update`." {
				t.Fatalf("%v", got)
			}
			if version(t, install) != "0.1.0" {
				t.Fatal("looking must not change the install")
			}

			applied := ApplyUpdate(install, nil)
			if jsjson.Stringify(applied) != `{"status":"updated","from":"0.1.0","to":"0.2.0"}` {
				t.Fatalf("%s", jsjson.Stringify(applied))
			}
			if version(t, install) != "0.2.0" {
				t.Fatal("not updated")
			}
			if data, _ := os.ReadFile(filepath.Join(install, "new-file.txt")); string(data) != "hello\n" {
				t.Fatalf("new file: %q", data)
			}
			if sh(t, install, "status", "--porcelain") != "" {
				t.Fatal("a clean checkout afterwards")
			}
			if CheckForUpdate(install, time.Now()).Value("available") != false {
				t.Fatal("nothing further to find")
			}
			if got := jsjson.Stringify(ApplyUpdate(install, nil)); got != `{"status":"current","version":"0.2.0"}` {
				t.Fatal(got)
			}
		})
	}

	t.Run("changed dependencies ask for an install; unchanged ones do not", func(t *testing.T) {
		f := newFixture(t)
		install := f.cloneTo("install", false)
		f.release("0.1.1", map[string]string{"notes.txt": "docs only\n"})
		var calls []string
		recorded := func(root string) (int, error) { calls = append(calls, root); return 0, nil }

		if ApplyUpdate(install, recorded).Value("status") != "updated" || len(calls) != 0 {
			t.Fatalf("a change that leaves the dependencies alone installs nothing: %v", calls)
		}
		f.release("0.2.0", map[string]string{"pnpm-lock.yaml": "lock: 2\n"})
		if ApplyUpdate(install, recorded).Value("status") != "updated" || len(calls) != 1 || calls[0] != install {
			t.Fatalf("a new lockfile installs once, in the install folder: %v", calls)
		}
		f.release("0.3.0", map[string]string{"pnpm-lock.yaml": "lock: 3\n"})
		failed := ApplyUpdate(install, func(string) (int, error) { return 1, nil })
		if failed.Value("status") != "failed" || !regexp.MustCompile(`exited with 1`).MatchString(failed.Value("reason").(string)) {
			t.Fatalf("a failed install is reported, not hidden: %s", jsjson.Stringify(failed))
		}
		if NeedsInstall([]string{"README.md", "src/proxy.mjs"}) || NeedsInstall([]string{"package.json"}) || !NeedsInstall([]string{"README.md", "pnpm-lock.yaml"}) {
			t.Fatal("needsInstall")
		}
	})

	t.Run("a copy with local changes is left exactly as it is", func(t *testing.T) {
		f := newFixture(t)
		install := f.cloneTo("install", false)
		write(t, filepath.Join(install, "pnpm-lock.yaml"), "lock: 1\nmy edit\n")
		f.release("0.2.0", nil)
		if CheckForUpdate(install, time.Now()).Value("available") != false {
			t.Fatal("no notice for a copy that cannot be updated")
		}
		applied := ApplyUpdate(install, nil)
		if applied.Value("status") != "refused" || !strings.Contains(applied.Value("reason").(string), "local changes") {
			t.Fatalf("%s", jsjson.Stringify(applied))
		}
		if data, _ := os.ReadFile(filepath.Join(install, "pnpm-lock.yaml")); !strings.Contains(string(data), "my edit") || version(t, install) != "0.1.0" {
			t.Fatal("the edit survives")
		}
	})

	t.Run("a development clone ahead of origin is not touched", func(t *testing.T) {
		f := newFixture(t)
		install := f.cloneTo("install", false)
		write(t, filepath.Join(install, "mine.txt"), "work in progress\n")
		sh(t, install, "add", ".")
		sh(t, install, "commit", "-m", "my own commit")
		head := sh(t, install, "rev-parse", "HEAD")

		if ApplyUpdate(install, nil).Value("status") != "current" || sh(t, install, "rev-parse", "HEAD") != head {
			t.Fatal("ahead of origin with nothing new upstream: nothing to do")
		}
		f.release("0.2.0", nil)
		if CheckForUpdate(install, time.Now()).Value("available") != false {
			t.Fatal("diverged: neither announced nor applied")
		}
		applied := ApplyUpdate(install, nil)
		if applied.Value("status") != "refused" || !strings.Contains(applied.Value("reason").(string), "commits that origin/master does not") {
			t.Fatalf("%s", jsjson.Stringify(applied))
		}
		if sh(t, install, "rev-parse", "HEAD") != head {
			t.Fatal("no merge, no rewrite")
		}
	})

	t.Run("a detached checkout is refused", func(t *testing.T) {
		f := newFixture(t)
		install := f.cloneTo("install", false)
		sh(t, install, "checkout", "--detach")
		f.release("0.2.0", nil)
		applied := ApplyUpdate(install, nil)
		if applied.Value("status") != "refused" || !strings.Contains(applied.Value("reason").(string), "not on a branch") {
			t.Fatalf("%s", jsjson.Stringify(applied))
		}
	})

	t.Run("an unreachable origin is reported, never thrown, and the install is unchanged", func(t *testing.T) {
		f := newFixture(t)
		install := f.cloneTo("install", false)
		if err := os.RemoveAll(f.origin); err != nil {
			t.Fatal(err)
		}
		if CheckForUpdate(install, time.Now()).Value("available") != false {
			t.Fatal("available")
		}
		applied := ApplyUpdate(install, nil)
		if applied.Value("status") != "refused" || !strings.HasPrefix(applied.Value("reason").(string), "could not reach origin (") || version(t, install) != "0.1.0" {
			t.Fatalf("%s", jsjson.Stringify(applied))
		}
	})

	t.Run("folders that are not a clone of their own are refused", func(t *testing.T) {
		f := newFixture(t)
		plain := filepath.Join(f.base, "plain")
		if err := os.Mkdir(plain, 0o755); err != nil {
			t.Fatal(err)
		}
		refused := ApplyUpdate(plain, nil)
		if refused.Value("status") != "refused" || !strings.Contains(refused.Value("reason").(string), "not a git clone") {
			t.Fatalf("%s", jsjson.Stringify(refused))
		}
		// An install copied into some other repository must not be updated through that repository.
		outer := f.cloneTo("outer", false)
		inner := filepath.Join(outer, "vendored", "jev-claude")
		if err := os.MkdirAll(inner, 0o755); err != nil {
			t.Fatal(err)
		}
		write(t, filepath.Join(inner, "package.json"), pkg("0.0.1"))
		refusedInner := ApplyUpdate(inner, nil)
		if refusedInner.Value("status") != "refused" || !strings.Contains(refusedInner.Value("reason").(string), "not a git clone of its own") {
			t.Fatalf("%s", jsjson.Stringify(refusedInner))
		}
	})

	t.Run("the check is due when missing, stale, unreadable, or from the future", func(t *testing.T) {
		now, _ := time.Parse(time.RFC3339, "2026-10-04T12:00:00Z")
		ms := float64(now.UnixMilli())
		at := func(ago float64) any {
			return jsjson.Obj("checkedAt", time.UnixMilli(int64(ms-ago)).UTC().Format("2006-01-02T15:04:05.000Z"))
		}
		cases := []struct {
			state any
			want  bool
			why   string
		}{
			{nil, true, "null"}, {jsjson.NewObject(), true, "{}"}, {jsjson.Obj("checkedAt", "yesterday-ish"), true, "unreadable"},
			{at(CheckEveryMs - 60000), false, "just inside the window"}, {at(CheckEveryMs), true, "exactly at the window"},
			{at(-60000), true, "a clock that went backwards must not silence checks"},
		}
		for _, c := range cases {
			if IsCheckDue(c.state, ms, CheckEveryMs) != c.want {
				t.Error(c.why)
			}
		}
	})

	t.Run("versions compare as numbers, not text", func(t *testing.T) {
		if !(CompareVersions("0.10.0", "0.9.9") > 0) || !(CompareVersions("0.6.5", "0.6.6") < 0) ||
			CompareVersions("1.0", "1.0.0") != 0 || !(CompareVersions("0.7.0-beta.1", "0.6.9") > 0) {
			t.Error("compareVersions")
		}
	})

	t.Run("a notice appears only for a genuinely newer version", func(t *testing.T) {
		found := jsjson.Obj("available", true, "latest", "0.7.0")
		if got, _ := notice(found, "0.6.5").(string); !strings.Contains(got, "0.6.5 -> 0.7.0") {
			t.Error(got)
		}
		if notice(found, "0.7.0") != nil || notice(found, "0.8.0") != nil ||
			notice(jsjson.Obj("available", false, "latest", "0.7.0"), "0.6.5") != nil ||
			notice(nil, "0.6.5") != nil || notice(found, nil) != nil {
			t.Error("a notice for nothing new")
		}
	})

	t.Run("the state file round-trips and a damaged one reads as no state", func(t *testing.T) {
		file := filepath.Join(t.TempDir(), "nested", "update.json")
		if ReadState(file) != nil {
			t.Fatal("missing")
		}
		state := jsjson.Obj("checkedAt", "2026-10-04T12:00:00.000Z", "available", true, "latest", "0.7.0")
		WriteState(state, file)
		if got := ReadState(file); jsjson.Stringify(got) != jsjson.Stringify(state) {
			t.Fatalf("%s", jsjson.Stringify(got))
		}
		write(t, file, "{ not json")
		if ReadState(file) != nil {
			t.Fatal("damaged")
		}
		write(t, file, "42")
		if ReadState(file) != nil {
			t.Fatal("valid JSON that is not a state object")
		}
	})
}

package statusline

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"
	"testing"

	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/status"
)

// Ported from node/test/statusline.test.mjs: the real jev-statusline program runs the way
// Claude Code runs it, against status files in a throwaway directory.

var exe string

func TestMain(m *testing.M) {
	dir, err := os.MkdirTemp("", "jev-statusline-test-")
	if err != nil {
		panic(err)
	}
	status.Dir = filepath.Join(dir, "status")
	exe = filepath.Join(dir, "jev-statusline")
	if runtime.GOOS == "windows" {
		exe += ".exe"
	}
	if out, err := exec.Command("go", "build", "-o", exe, "github.com/alienfacepalm/jev-claude/go/cmd/jev-statusline").CombinedOutput(); err != nil {
		panic(fmt.Sprintf("go build: %v\n%s", err, out))
	}
	code := m.Run()
	os.RemoveAll(dir)
	os.Exit(code)
}

var mainAgent = &status.Agent{Key: "main", Label: "main", Main: true}

var ansi = regexp.MustCompile(`\x1b\[[0-9;]*m`)

// render runs the real status line and returns its text without colours.
func render(t *testing.T, sessionID string, workspace, extra *jsjson.Object, icons string) string {
	t.Helper()
	ws := jsjson.Obj("current_dir", "/work/proj")
	jsjson.Spread(ws, workspace)
	input := jsjson.Obj("session_id", sessionID, "workspace", ws, "context_window", jsjson.Obj("used_percentage", 8.0))
	jsjson.Spread(input, extra)
	return strings.TrimSpace(ansi.ReplaceAllString(run(t, input, icons), ""))
}

// run runs the real status line on input and returns its raw output, colours and newline included.
func run(t *testing.T, input *jsjson.Object, icons string) string {
	t.Helper()
	cmd := exec.Command(exe)
	cmd.Env = append(os.Environ(), "JEV_STATUS_DIR="+status.Dir, "JEV_ICONS="+icons)
	cmd.Stdin = strings.NewReader(jsjson.Stringify(input))
	var stderr strings.Builder
	cmd.Stderr = &stderr
	out, err := cmd.Output()
	if err != nil {
		t.Fatalf("%v: %s", err, stderr.String())
	}
	return string(out)
}

func decision(tier, model string, confidence float64, effort any) *jsjson.Object {
	d := jsjson.Obj("tier", tier, "model", model, "confidence", confidence)
	if effort != jsjson.Undefined {
		d.Set("effort", effort)
	}
	d.Set("reason", "jev")
	d.Set("at", status.NowMs())
	return d
}

func id(name string) string { return fmt.Sprintf("statusline-%s-%d", name, os.Getpid()) }

func TestStatusLine(t *testing.T) {
	none := jsjson.NewObject()
	worktree := func(name, branch string) *jsjson.Object {
		return jsjson.Obj("worktree", jsjson.Obj("name", name, "branch", branch))
	}
	write := func(t *testing.T, sid string, d *jsjson.Object, agent *status.Agent) {
		t.Helper()
		if err := status.WriteDecision(sid, d, agent); err != nil {
			t.Fatal(err)
		}
	}
	match := func(t *testing.T, line, pattern string) {
		t.Helper()
		if !regexp.MustCompile(pattern).MatchString(line) {
			t.Errorf("%q does not match %s", line, pattern)
		}
	}
	noMatch := func(t *testing.T, line, pattern string) {
		t.Helper()
		if regexp.MustCompile(pattern).MatchString(line) {
			t.Errorf("%q matches %s", line, pattern)
		}
	}

	t.Run("symbols replace the words, and the branch uses the Powerline glyph", func(t *testing.T) {
		sid := id("symbols")
		write(t, sid, decision("sonnet", "claude-sonnet-5-5", 0.94, "high"), mainAgent)
		line := render(t, sid, none, worktree("login-fix", "fix/login"), "symbols")
		want := "\xe2\x9c\xa7\xe2\x9c\xa6 Sonnet 5.5 (94%) \xc2\xb7 \xe2\x97\x94 high \xc2\xb7 \xe2\x9d\x90 proj \xc2\xb7 \xee\x82\xa0 fix/login \xc2\xb7 \xe2\x8c\x82 login-fix \xc2\xb7 \xe2\x89\xa1 8% \xc2\xb7 /work/proj"
		if line != want {
			t.Errorf("\n got  %q\n want %q", line, want)
		}
	})

	t.Run("shows the effort the turn ran at next to the model and confidence", func(t *testing.T) {
		sid := id("effort")
		write(t, sid, decision("sonnet", "claude-sonnet-5-5", 0.94, "high"), mainAgent)
		if line := render(t, sid, none, none, "text"); line != "model Sonnet 5.5 (94%) \xc2\xb7 effort high \xc2\xb7 dir proj \xc2\xb7 ctx 8% \xc2\xb7 /work/proj" {
			t.Errorf("%q", line)
		}
	})

	t.Run("the whole working directory comes last, dimmed, as Claude Code sent it", func(t *testing.T) {
		raw := run(t, jsjson.Obj("session_id", id("fullpath"), "workspace", jsjson.Obj("current_dir", "/home/me/work/proj"), "context_window", jsjson.Obj("used_percentage", 8.0)), "text")
		if want := "8% \x1b[2m\xc2\xb7 /home/me/work/proj\x1b[0m\n"; !strings.HasSuffix(raw, want) {
			t.Errorf("%q does not end with %q", raw, want)
		}
	})

	t.Run("a path under the home directory starts with ~, and only a whole home directory counts", func(t *testing.T) {
		shown := func(dir, home string) string {
			cmd := exec.Command(exe)
			cmd.Env = append(os.Environ(), "JEV_STATUS_DIR="+status.Dir, "JEV_ICONS=text", "HOME="+home, "USERPROFILE="+home)
			cmd.Stdin = strings.NewReader(jsjson.Stringify(jsjson.Obj("session_id", id("tilde"), "workspace", jsjson.Obj("current_dir", dir), "context_window", jsjson.Obj("used_percentage", 8.0))))
			out, err := cmd.Output()
			if err != nil {
				t.Fatal(err)
			}
			m := regexp.MustCompile(`\x1b\[2m\x{B7} (.*)\x1b\[0m\n$`).FindStringSubmatch(string(out))
			if m == nil {
				t.Fatalf("no path part in %q", out)
			}
			return m[1]
		}
		for _, c := range []struct{ dir, home, want string }{
			{"/Users/me/Projects/GOVPILOT/sdl-mono/sync-client", "/Users/me", "~/Projects/GOVPILOT/sdl-mono/sync-client"},
			{"/Users/me", "/Users/me", "~"},
			{"/Users/me/", "/Users/me/", "~/"},
			{`C:\Users\me\proj`, `C:\Users\me`, `~\proj`},
			{"/Users/media/proj", "/Users/me", "/Users/media/proj"},
			{"/srv/Users/me/proj", "/Users/me", "/srv/Users/me/proj"},
			{"/Users/ME/proj", "/Users/me", "/Users/ME/proj"},
			{"/srv/app", "/", "/srv/app"},
		} {
			if got := shown(c.dir, c.home); got != c.want {
				t.Errorf("%q under home %q: got %q, want %q", c.dir, c.home, got, c.want)
			}
		}
	})

	t.Run("a Windows path is shown with its backslashes, and its last segment is still the directory name", func(t *testing.T) {
		path := `C:\Users\me\work\proj`
		raw := run(t, jsjson.Obj("session_id", id("winpath"), "workspace", jsjson.Obj("current_dir", path), "context_window", jsjson.Obj("used_percentage", 8.0)), "text")
		if want := "\x1b[2m\xc2\xb7 " + path + "\x1b[0m\n"; !strings.HasSuffix(raw, want) {
			t.Errorf("%q does not end with %q", raw, want)
		}
		match(t, render(t, id("winpath-plain"), jsjson.Obj("current_dir", path), none, "text"), ` \x{B7} dir proj \x{B7} `)
	})

	t.Run("cwd is the fallback for the path when the workspace has no directory", func(t *testing.T) {
		raw := run(t, jsjson.Obj("session_id", id("cwd"), "cwd", "/srv/app", "context_window", jsjson.Obj("used_percentage", 8.0)), "text")
		if want := "\x1b[2m\xc2\xb7 /srv/app\x1b[0m\n"; !strings.HasSuffix(raw, want) {
			t.Errorf("%q does not end with %q", raw, want)
		}
	})

	t.Run("no directory in the input means no path part", func(t *testing.T) {
		raw := run(t, jsjson.Obj("session_id", id("nodir"), "context_window", jsjson.Obj("used_percentage", 8.0)), "text")
		if !strings.HasSuffix(raw, "8%\n") {
			t.Errorf("%q", raw)
		}
	})

	t.Run("shows a higher effort when Claude Code asked for one", func(t *testing.T) {
		sid := id("xhigh")
		write(t, sid, decision("opus", "claude-opus-5-5", 0.91, "xhigh"), mainAgent)
		match(t, render(t, sid, none, none, "text"), `^model Opus 5.5 \(91%\) \x{B7} effort xhigh \x{B7} `)
	})

	t.Run("says nothing about effort for Haiku, which takes none", func(t *testing.T) {
		sid := id("haiku")
		write(t, sid, decision("haiku", "claude-haiku-4-5-20251001", 0.97, nil), mainAgent)
		line := render(t, sid, none, none, "text")
		match(t, line, `^model Haiku 4.5 \(97%\) \x{B7} dir proj`)
		noMatch(t, line, `effort`)
	})

	t.Run("a session recorded before effort was tracked still renders", func(t *testing.T) {
		sid := id("old")
		write(t, sid, decision("sonnet", "claude-sonnet-5-5", 0.8, jsjson.Undefined), mainAgent)
		line := render(t, sid, none, none, "text")
		match(t, line, `^model Sonnet 5.5 \(80%\)`)
		noMatch(t, line, `effort`)
	})

	t.Run("inside a worktree, the branch and the worktree are each named", func(t *testing.T) {
		match(t, render(t, id("worktree"), none, worktree("login-fix", "fix/login"), "text"), ` \x{B7} dir proj \x{B7} branch fix/login \x{B7} worktree login-fix \x{B7} ctx 8% \x{B7} /work/proj$`)
	})

	t.Run("a worktree named like the directory is not said twice", func(t *testing.T) {
		line := render(t, id("samename"), jsjson.Obj("current_dir", "/work/COR-1", "git_worktree", "COR-1"), none, "text")
		match(t, line, ` \x{B7} worktree COR-1 \x{B7} ctx 8% \x{B7} /work/COR-1$`)
		noMatch(t, line, `dir`)
	})

	t.Run("a long branch name is cut with an ellipsis", func(t *testing.T) {
		line := render(t, id("longbranch"), none, worktree("wt", "COR-1263/multi-edit-inspection-sync"), "text")
		match(t, line, ` \x{B7} branch COR-1263/multi-edit-inspect\x{2026} \x{B7} `)
		noMatch(t, line, `inspection-sync`)
	})

	t.Run("a linked worktree whose branch cannot be read still names the worktree", func(t *testing.T) {
		// /work/proj is not a repository, so there is no branch to look up.
		line := render(t, id("linked"), jsjson.Obj("git_worktree", "scratch"), none, "text")
		match(t, line, ` \x{B7} dir proj \x{B7} worktree scratch \x{B7} `)
		noMatch(t, line, `branch`)
	})

	t.Run("the main working tree shows its branch and no worktree", func(t *testing.T) {
		dir := t.TempDir()
		cmd := exec.Command("git", "init", "-q", "-b", "main-line")
		cmd.Dir = dir
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("%v %s", err, out)
		}
		line := render(t, id("main"), jsjson.Obj("current_dir", dir), none, "text")
		if want := " \xc2\xb7 branch main-line \xc2\xb7 ctx 8% \xc2\xb7 " + dir; !strings.HasSuffix(line, want) {
			t.Errorf("line %q does not end with %q", line, want)
		}
		// Without the trailing path: the test's own temp directory is named after this test.
		noMatch(t, strings.TrimSuffix(line, " \xc2\xb7 "+dir), `worktree`)
	})

	t.Run("a directory that is not a git checkout shows neither a branch nor a worktree", func(t *testing.T) {
		noMatch(t, render(t, id("nogit"), none, none, "text"), `branch|worktree`)
	})

	t.Run("a space separates the sub-agents symbol from the first model name", func(t *testing.T) {
		sid := id("agents")
		write(t, sid, decision("sonnet", "claude-sonnet-5-5", 0.94, "high"), mainAgent)
		write(t, sid, decision("haiku", "claude-haiku-4-5-20251001", 0.9, jsjson.Undefined), &status.Agent{Key: "a1", Label: "a1"})
		match(t, render(t, sid, none, none, "symbols"), `\x{2726} Haiku 4\.5`)
	})
}

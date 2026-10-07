package worktree

import (
	"os/exec"
	"path/filepath"
	"testing"

	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
)

// Ported from node/test/worktree.test.mjs.

func parse(t *testing.T, s string) any {
	v, err := jsjson.Parse(s)
	if err != nil {
		t.Fatal(err)
	}
	return v
}

func same(loc *Location, branch, worktree any) bool {
	return loc != nil && loc.Branch == branch && loc.Worktree == worktree
}

func TestWorktree(t *testing.T) {
	none := func(any) any { return nil }
	never := func(any) any { t.Fatal("the branch should not have been looked up"); return nil }

	t.Run("outside a git checkout there is nothing to show", func(t *testing.T) {
		for _, in := range []any{parse(t, `{"workspace":{"current_dir":"/nowhere"}}`), jsjson.NewObject(), jsjson.Undefined} {
			if LocationInfo(in, none) != nil {
				t.Errorf("%s", jsjson.Stringify(in))
			}
		}
	})
	t.Run("the main working tree has a branch and no worktree", func(t *testing.T) {
		loc := LocationInfo(parse(t, `{"workspace":{"current_dir":"/repo"}}`), func(dir any) any {
			if dir == "/repo" {
				return "master"
			}
			return nil
		})
		if !same(loc, "master", nil) {
			t.Errorf("%+v", loc)
		}
	})
	t.Run("a worktree session carries its own name and branch, with no git call", func(t *testing.T) {
		loc := LocationInfo(parse(t, `{"worktree":{"name":"my-feature","branch":"worktree-my-feature","path":"/r/.claude/worktrees/my-feature"}}`), never)
		if !same(loc, "worktree-my-feature", "my-feature") {
			t.Errorf("%+v", loc)
		}
	})
	t.Run("a linked worktree has only a name, so the branch is read from git in the current directory", func(t *testing.T) {
		var asked []any
		loc := LocationInfo(parse(t, `{"workspace":{"current_dir":"/wt/feature-xyz","git_worktree":"feature-xyz"}}`), func(dir any) any {
			asked = append(asked, dir)
			return "feature/xyz"
		})
		if !same(loc, "feature/xyz", "feature-xyz") || len(asked) != 1 || asked[0] != "/wt/feature-xyz" {
			t.Errorf("%+v %v", loc, asked)
		}
	})
	t.Run("a worktree session without a branch (hook-based) falls back to git", func(t *testing.T) {
		loc := LocationInfo(parse(t, `{"worktree":{"name":"scratch","path":"/wt/scratch"}}`), func(dir any) any {
			if dir == "/wt/scratch" {
				return "main-2"
			}
			return nil
		})
		if !same(loc, "main-2", "scratch") {
			t.Errorf("%+v", loc)
		}
	})
	t.Run("a detached HEAD in a worktree still names the worktree", func(t *testing.T) {
		loc := LocationInfo(parse(t, `{"workspace":{"current_dir":"/wt/x","git_worktree":"x"}}`), func(any) any { return "" })
		if !same(loc, "", "x") {
			t.Errorf("%+v", loc)
		}
	})
	t.Run("gitBranch: a branch, a detached HEAD, and no checkout", func(t *testing.T) {
		dir := t.TempDir()
		if resolved, err := filepath.EvalSymlinks(dir); err == nil {
			dir = resolved
		}
		git := func(args ...string) {
			cmd := exec.Command("git", append([]string{"-c", "user.name=t", "-c", "user.email=t@t"}, args...)...)
			cmd.Dir = dir
			if out, err := cmd.CombinedOutput(); err != nil {
				t.Fatalf("git %v: %v %s", args, err, out)
			}
		}
		if GitBranch(dir) != nil {
			t.Error("not a repository")
		}
		git("init", "-q", "-b", "topic/a")
		if GitBranch(dir) != "topic/a" {
			t.Error("works before the first commit")
		}
		git("commit", "-q", "--allow-empty", "-m", "x")
		git("checkout", "-q", "--detach")
		if GitBranch(dir) != "" {
			t.Error("detached")
		}
		if GitBranch(jsjson.Undefined) != nil || GitBranch(filepath.Join(dir, "missing")) != nil {
			t.Error("no directory")
		}
	})
}

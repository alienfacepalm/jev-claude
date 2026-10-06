// Package worktree finds the branch and linked worktree a session is in (SPEC 12.2), ported
// from node/src/worktree.mjs.
package worktree

import (
	"context"
	"os/exec"
	"time"

	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
)

// GitBranch is the branch checked out in dir: a name, "" when detached, or nil when dir is
// empty or not a git checkout.
func GitBranch(dir any) any {
	d, ok := dir.(string)
	if !ok || d == "" {
		return nil
	}
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	cmd := exec.CommandContext(ctx, "git", "branch", "--show-current")
	cmd.Dir = d
	out, err := cmd.Output()
	if err != nil {
		return nil
	}
	return jsstr.Trim(string(out))
}

// Location is where a session works.
type Location struct {
	Branch   any // string, "" when detached, nil when unknown
	Worktree any // string, or nil in the main working tree
}

// LocationInfo reads the worktree and branch from the status line input, asking branchOf for
// whatever the input lacks; nil when both are null.
func LocationInfo(input any, branchOf func(dir any) any) *Location {
	worktree := jsjson.Coalesce(jsjson.Coalesce(jsjson.Path(input, "worktree", "name"), jsjson.Path(input, "workspace", "git_worktree")), nil)
	dir := jsjson.Coalesce(jsjson.Coalesce(jsjson.Path(input, "workspace", "current_dir"), jsjson.Prop(input, "cwd")), jsjson.Path(input, "worktree", "path"))
	branch := jsjson.Path(input, "worktree", "branch")
	if jsjson.IsNullish(branch) {
		branch = branchOf(dir)
	}
	if worktree == nil && branch == nil {
		return nil
	}
	return &Location{Branch: branch, Worktree: worktree}
}

// Package update checks for and applies updates to a git clone (SPEC 14), ported from
// node/src/update.mjs.
package update

import (
	"bytes"
	"context"
	"fmt"
	"math"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync/atomic"
	"time"

	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
	"github.com/alienfacepalm/jev-claude/go/internal/osdirs"
)

// File is <home at start>/.jev-router/update.json.
var File = filepath.Join(osdirs.Home(), ".jev-router", "update.json")

// CheckEveryMs is how long a check stays fresh.
const CheckEveryMs = 6 * 60 * 60 * 1000

const (
	fetchTimeout = 20 * time.Second
	localTimeout = 5 * time.Second
)

var seq atomic.Int64

func git(root string, timeout time.Duration, args ...string) (string, error) {
	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()
	cmd := exec.CommandContext(ctx, "git", append([]string{"-C", root}, args...)...)
	cmd.Env = append(os.Environ(), "GIT_TERMINAL_PROMPT=0")
	hideWindow(cmd)
	var stdout, stderr bytes.Buffer
	cmd.Stdout, cmd.Stderr = &stdout, &stderr
	if err := cmd.Run(); err != nil {
		return "", fmt.Errorf("Command failed: git -C %s %s\n%s", root, strings.Join(args, " "), stderr.String())
	}
	return jsstr.Trim(stdout.String()), nil
}

// ReadState is the parsed state file when it holds an object (or array), else nil.
func ReadState(file string) any {
	data, err := os.ReadFile(file)
	if err != nil {
		return nil
	}
	v, err := jsjson.ParseBytes(data)
	if err != nil {
		return nil
	}
	switch v.(type) {
	case *jsjson.Object, []any:
		return v
	}
	return nil
}

// WriteState writes the state through a unique temporary file and a rename.
func WriteState(state any, file string) {
	if os.MkdirAll(filepath.Dir(file), 0o777) != nil {
		return
	}
	temp := fmt.Sprintf("%s.%d.%d.tmp", file, os.Getpid(), seq.Add(1))
	if os.WriteFile(temp, []byte(jsjson.Stringify(state)), 0o666) != nil {
		return
	}
	if os.Rename(temp, file) != nil {
		os.Remove(temp)
	}
}

// parseISO reads only Date.prototype.toISOString() output (SPEC 14).
func parseISO(v any) (float64, bool) {
	s, ok := v.(string)
	if !ok || len(s) != 24 {
		return 0, false
	}
	t, err := time.Parse("2006-01-02T15:04:05.000Z", s)
	if err != nil {
		return 0, false
	}
	return float64(t.UnixMilli()), true
}

// IsCheckDue is whether the last check is missing, unreadable, from the future, or old.
func IsCheckDue(state any, now, everyMs float64) bool {
	at, ok := parseISO(jsjson.Coalesce(jsjson.Prop(state, "checkedAt"), ""))
	return !ok || now-at >= everyMs || at > now
}

// parseInt is JavaScript's parseInt(s, 10) with NaN as ok=false.
func parseInt(s string) (float64, bool) {
	s = jsstr.Trim(s)
	sign := 1.0
	if s != "" && (s[0] == '+' || s[0] == '-') {
		if s[0] == '-' {
			sign = -1
		}
		s = s[1:]
	}
	end := 0
	for end < len(s) && s[end] >= '0' && s[end] <= '9' {
		end++
	}
	if end == 0 {
		return 0, false
	}
	n := jsjson.StringToNumber(s[:end])
	return sign * n, true
}

func parts(v any) []float64 {
	head, _, _ := strings.Cut(jsjson.JSString(v), "-")
	var out []float64
	for _, p := range strings.Split(head, ".") {
		n, ok := parseInt(p)
		if !ok || n == 0 {
			n = 0
		}
		out = append(out, n)
	}
	return out
}

// CompareVersions compares dotted release numbers, ignoring pre-release tags; negative when
// a is older.
func CompareVersions(a, b any) float64 {
	x, y := parts(a), parts(b)
	for i := 0; i < max(len(x), len(y)); i++ {
		var p, q float64
		if i < len(x) {
			p = x[i]
		}
		if i < len(y) {
			q = y[i]
		}
		if d := p - q; d != 0 && !math.IsNaN(d) {
			return d
		}
	}
	return 0
}

// UpdateNotice is the launch notice, or "" when there is none.
func UpdateNotice(state, currentVersion any) (string, bool) {
	latest := jsjson.Prop(state, "latest")
	if !jsjson.Truthy(jsjson.Prop(state, "available")) || !jsjson.Truthy(latest) || !jsjson.Truthy(currentVersion) {
		return "", false
	}
	if CompareVersions(latest, currentVersion) <= 0 {
		return "", false
	}
	return fmt.Sprintf("[jev] Update available: %s -> %s. Run `jev-claude --update`.", jsjson.JSString(currentVersion), jsjson.JSString(latest)), true
}

// InstalledVersion is the root package.json's version, or nil.
func InstalledVersion(root string) any {
	data, err := os.ReadFile(filepath.Join(root, "package.json"))
	if err != nil {
		return nil
	}
	v, err := jsjson.ParseBytes(data)
	if err != nil || v == nil {
		return nil
	}
	return jsjson.Coalesce(jsjson.Prop(v, "version"), nil)
}

// Clone is what InspectClone found.
type Clone struct {
	OK     bool
	Reason string
	Branch string
	Head   string
	Remote string
	Behind bool
}

func canonical(p string) string {
	if abs, err := filepath.Abs(p); err == nil {
		p = abs
	}
	if real, err := filepath.EvalSymlinks(p); err == nil {
		p = real
	}
	return filepath.Clean(filepath.FromSlash(p))
}

func samePath(a, b string) bool {
	if runtime.GOOS == "windows" {
		return strings.EqualFold(a, b)
	}
	return a == b
}

// InspectClone reports whether root is a clean clone this tool may fast-forward.
func InspectClone(root string) Clone {
	top, err := git(root, localTimeout, "rev-parse", "--show-toplevel")
	if err != nil {
		return Clone{Reason: "this folder is not a git clone"}
	}
	if !samePath(canonical(top), canonical(root)) {
		return Clone{Reason: "this folder is not a git clone of its own"}
	}
	branch, err := git(root, localTimeout, "symbolic-ref", "--short", "HEAD")
	if err != nil {
		return Clone{Reason: "the checkout is not on a branch"}
	}
	unreachable := func(err error) Clone {
		first, _, _ := strings.Cut(err.Error(), "\n")
		return Clone{Reason: "could not reach origin (" + first + ")"}
	}
	changes, err := git(root, localTimeout, "status", "--porcelain", "--untracked-files=no")
	if err != nil {
		return unreachable(err)
	}
	if changes != "" {
		return Clone{Reason: "there are local changes in the checkout"}
	}
	if _, err := git(root, fetchTimeout, "fetch", "--quiet", "origin", branch); err != nil {
		return unreachable(err)
	}
	head, err := git(root, localTimeout, "rev-parse", "HEAD")
	if err != nil {
		return unreachable(err)
	}
	remote, err := git(root, localTimeout, "rev-parse", "FETCH_HEAD")
	if err != nil {
		return unreachable(err)
	}
	ok := Clone{OK: true, Branch: branch, Head: head, Remote: remote}
	if head == remote {
		return ok
	}
	isAncestor := func(older, newer string) bool {
		_, err := git(root, localTimeout, "merge-base", "--is-ancestor", older, newer)
		return err == nil
	}
	if isAncestor("FETCH_HEAD", "HEAD") {
		return ok
	}
	if !isAncestor("HEAD", "FETCH_HEAD") {
		return Clone{Reason: fmt.Sprintf("local %s has commits that origin/%s does not", branch, branch)}
	}
	ok.Behind = true
	return ok
}

func versionAt(root, ref string) any {
	text, err := git(root, localTimeout, "show", ref+":package.json")
	if err != nil {
		return nil
	}
	v, err := jsjson.Parse(text)
	if err != nil || v == nil {
		return nil
	}
	return jsjson.Coalesce(jsjson.Prop(v, "version"), nil)
}

// CheckForUpdate is one update check, as the state file records it.
func CheckForUpdate(root string, now time.Time) *jsjson.Object {
	checkedAt := now.UTC().Format("2006-01-02T15:04:05.000Z")
	clone := InspectClone(root)
	if !clone.OK || !clone.Behind {
		return jsjson.Obj("checkedAt", checkedAt, "available", false)
	}
	latest := versionAt(root, "FETCH_HEAD")
	return jsjson.Obj("checkedAt", checkedAt, "available", jsjson.Truthy(latest), "latest", latest, "remote", clone.Remote)
}

// NeedsInstall is whether the changed files mean dependencies need installing again.
func NeedsInstall(changedFiles []string) bool {
	for _, f := range changedFiles {
		if f == "pnpm-lock.yaml" {
			return true
		}
	}
	return false
}

// Installer installs dependencies in root and returns its exit code.
type Installer func(root string) (int, error)

// ApplyUpdate fast-forwards the clone and installs dependencies when the lockfile changed.
func ApplyUpdate(root string, install Installer) *jsjson.Object {
	clone := InspectClone(root)
	if !clone.OK {
		return jsjson.Obj("status", "refused", "reason", clone.Reason)
	}
	from := InstalledVersion(root)
	if !clone.Behind {
		return jsjson.Obj("status", "current", "version", from)
	}
	failed := func(err error) *jsjson.Object {
		first, _, _ := strings.Cut(err.Error(), "\n")
		return jsjson.Obj("status", "failed", "reason", first, "from", from)
	}
	diff, err := git(root, localTimeout, "diff", "--name-only", "HEAD", "FETCH_HEAD")
	if err != nil {
		return failed(err)
	}
	if _, err := git(root, fetchTimeout, "merge", "--ff-only", "FETCH_HEAD"); err != nil {
		return failed(err)
	}
	to := InstalledVersion(root)
	if NeedsInstall(strings.Split(diff, "\n")) && install != nil {
		code, err := install(root)
		if err != nil {
			return failed(err)
		}
		if code != 0 {
			return jsjson.Obj("status", "failed", "reason", fmt.Sprintf("installing dependencies exited with %d", code), "from", from, "to", to)
		}
	}
	return jsjson.Obj("status", "updated", "from", from, "to", to)
}

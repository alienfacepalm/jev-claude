// Package repo finds the repository root (SPEC 2.4) and the release version (SPEC 2.3).
package repo

import (
	"os"
	"path/filepath"
	"sync"

	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
)

const marker = ".claude/skills/jev-calibrate/SKILL.md"

var (
	once sync.Once
	root string
)

// Root is the repository root, resolved once: JEV_ROOT when non-empty, else the nearest
// ancestor of the running executable holding .claude/skills/jev-calibrate/SKILL.md, else "".
func Root() string {
	once.Do(func() { root = Resolve() })
	return root
}

// Resolve computes the root without caching.
func Resolve() string {
	if v := os.Getenv("JEV_ROOT"); v != "" {
		return v
	}
	exe, err := os.Executable()
	if err != nil {
		return ""
	}
	if resolved, err := filepath.EvalSymlinks(exe); err == nil {
		exe = resolved
	}
	return Find(filepath.Dir(exe))
}

// Find walks up from dir to the first directory holding the marker, or "".
func Find(dir string) string {
	for {
		if _, err := os.Stat(filepath.Join(dir, filepath.FromSlash(marker))); err == nil {
			return dir
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			return ""
		}
		dir = parent
	}
}

// Version is the release version from the root package.json, or "0.0.0" without one.
func Version(root string) string {
	if root == "" {
		return "0.0.0"
	}
	data, err := os.ReadFile(filepath.Join(root, "package.json"))
	if err != nil {
		return "0.0.0"
	}
	v, err := jsjson.ParseBytes(data)
	if err != nil {
		return "0.0.0"
	}
	if s, ok := jsjson.Prop(v, "version").(string); ok {
		return s
	}
	return "0.0.0"
}

package repo

import (
	"os"
	"path/filepath"
	"testing"
)

func TestFindWalksUpToTheSkillMarker(t *testing.T) {
	// The real repository: this test file sits at go/internal/repo inside it.
	here, _ := os.Getwd()
	root := Find(here)
	if root == "" {
		t.Fatal("the repository holding this port has the marker")
	}
	if _, err := os.Stat(filepath.Join(root, "SPEC.md")); err != nil {
		t.Fatalf("found %s, which is not the repository root", root)
	}
	if v := Version(root); v == "0.0.0" || v == "" {
		t.Fatalf("the root package.json carries the release version, got %q", v)
	}

	plain := t.TempDir()
	if Find(plain) != "" {
		t.Fatal("a directory outside any repository has no root")
	}
	if Version("") != "0.0.0" || Version(plain) != "0.0.0" {
		t.Fatal("no root, or no package.json, is version 0.0.0")
	}
}

func TestJEVRootWins(t *testing.T) {
	t.Setenv("JEV_ROOT", `C:\somewhere`)
	if Resolve() != `C:\somewhere` {
		t.Fatal("JEV_ROOT is used as given")
	}
}

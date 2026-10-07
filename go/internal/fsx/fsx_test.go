package fsx

import (
	"os"
	"path/filepath"
	"testing"
)

func write(t *testing.T, file, text string) {
	t.Helper()
	if err := os.WriteFile(file, []byte(text), 0o600); err != nil {
		t.Fatal(err)
	}
}

func read(t *testing.T, file string) string {
	t.Helper()
	b, err := os.ReadFile(file)
	if err != nil {
		t.Fatal(err)
	}
	return string(b)
}

func leftovers(t *testing.T, dir string) []string {
	t.Helper()
	tmp, err := filepath.Glob(filepath.Join(dir, "*.tmp"))
	if err != nil {
		t.Fatal(err)
	}
	return tmp
}

func TestRenameOver(t *testing.T) {
	t.Run("replaces an existing file", func(t *testing.T) {
		dir := t.TempDir()
		file, temp := filepath.Join(dir, "a.json"), filepath.Join(dir, "a.json.1.tmp")
		write(t, file, "old")
		write(t, temp, "new")
		if err := RenameOver(temp, file); err != nil {
			t.Fatal(err)
		}
		if got := read(t, file); got != "new" {
			t.Errorf("content %q", got)
		}
		if l := leftovers(t, dir); len(l) != 0 {
			t.Errorf("leftovers %v", l)
		}
	})

	t.Run("a missing temp file is an error", func(t *testing.T) {
		dir := t.TempDir()
		if err := RenameOver(filepath.Join(dir, "missing.tmp"), filepath.Join(dir, "a.json")); err == nil {
			t.Error("want an error")
		}
	})
}

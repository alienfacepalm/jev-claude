//go:build windows

package fsx

import (
	"path/filepath"
	"syscall"
	"testing"
	"time"
)

// holdOpen opens file with read sharing but no delete sharing, as antivirus, the indexer and a
// plain reader do on Windows, until the returned release is called.
func holdOpen(t *testing.T, file string) (release func()) {
	t.Helper()
	name, err := syscall.UTF16PtrFromString(file)
	if err != nil {
		t.Fatal(err)
	}
	handle, err := syscall.CreateFile(name, syscall.GENERIC_READ, syscall.FILE_SHARE_READ, nil,
		syscall.OPEN_EXISTING, syscall.FILE_ATTRIBUTE_NORMAL, 0)
	if err != nil {
		t.Fatal(err)
	}
	return func() { _ = syscall.CloseHandle(handle) }
}

// Only Windows refuses to rename over an open file (SPEC 3.11).
func TestRenameOverHeldFile(t *testing.T) {
	t.Run("lands while another handle briefly holds the file open", func(t *testing.T) {
		dir := t.TempDir()
		file, temp := filepath.Join(dir, "a.json"), filepath.Join(dir, "a.json.1.tmp")
		write(t, file, "old")
		release := holdOpen(t, file)
		write(t, temp, "new")
		time.AfterFunc(50*time.Millisecond, release)
		if err := RenameOver(temp, file); err != nil {
			t.Fatalf("the rename was not retried until the file was free: %v", err)
		}
		if got := read(t, file); got != "new" {
			t.Errorf("content %q", got)
		}
		if l := leftovers(t, dir); len(l) != 0 {
			t.Errorf("leftovers %v", l)
		}
	})

	t.Run("a file that stays held is an error and removes the temp file", func(t *testing.T) {
		dir := t.TempDir()
		file, temp := filepath.Join(dir, "a.json"), filepath.Join(dir, "a.json.1.tmp")
		write(t, file, "old")
		release := holdOpen(t, file)
		defer release()
		write(t, temp, "secret prompt")
		if err := RenameOver(temp, file); err == nil {
			t.Fatal("want an error once the retries run out")
		}
		if l := leftovers(t, dir); len(l) != 0 {
			t.Errorf("the temp file held prompt text and must be gone: %v", l)
		}
		release()
		if got := read(t, file); got != "old" {
			t.Errorf("the held file kept its previous content, got %q", got)
		}
	})
}

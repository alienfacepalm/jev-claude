// Package fsx replaces a file the way SPEC 3.11 says, ported from node/src/atomic-rename.mjs.
package fsx

import (
	"errors"
	"os"
	"runtime"
	"syscall"
	"time"
)

const (
	attempts = 10
	pause    = 20 * time.Millisecond
)

// Windows error codes for a file another process has open: ERROR_ACCESS_DENIED,
// ERROR_SHARING_VIOLATION and ERROR_LOCK_VIOLATION.
func transient(err error) bool {
	if runtime.GOOS != "windows" {
		return false
	}
	var errno syscall.Errno
	if !errors.As(err, &errno) {
		return false
	}
	return errno == 5 || errno == 32 || errno == 33
}

// RenameOver renames temp over file, retrying briefly while Windows reports the target as in use.
// Any other error, or one that outlasts the retries (about 180 ms), removes temp and is returned:
// the temporary file can hold prompt text, and nothing else ever cleans it up.
func RenameOver(temp, file string) error {
	for attempt := 1; ; attempt++ {
		err := os.Rename(temp, file)
		if err == nil {
			return nil
		}
		if attempt < attempts && transient(err) {
			time.Sleep(pause)
			continue
		}
		_ = os.Remove(temp)
		return err
	}
}

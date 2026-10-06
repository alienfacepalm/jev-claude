//go:build !windows

package logx

import "os"

// IsTerminal reports whether f is a character device.
func IsTerminal(f *os.File) bool {
	info, err := f.Stat()
	return err == nil && info.Mode()&os.ModeCharDevice != 0 && info.Mode()&os.ModeDevice != 0
}

package logx

import (
	"os"
	"syscall"
)

// IsTerminal reports whether f is a console, as Node's isTTY does.
func IsTerminal(f *os.File) bool {
	var mode uint32
	return syscall.GetConsoleMode(syscall.Handle(f.Fd()), &mode) == nil
}

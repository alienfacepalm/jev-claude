//go:build !windows

package launch

import (
	"os/exec"
	"syscall"
)

func executable(file string) bool { return syscall.Access(file, 1) == nil }

// verbatim has no raw command line outside Windows; cmd.exe shims only exist there.
func verbatim(spec Spec, args []string) *exec.Cmd {
	return exec.Command(spec.Command, append([]string{"/d", "/s", "/c", spec.Shim}, args...)...)
}

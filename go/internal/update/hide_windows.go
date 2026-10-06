package update

import (
	"os/exec"
	"syscall"
)

// createNoWindow is CREATE_NO_WINDOW; with HideWindow it is Node's windowsHide.
const createNoWindow = 0x08000000

// hideWindow keeps git from opening a console window.
func hideWindow(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{HideWindow: true, CreationFlags: createNoWindow}
}

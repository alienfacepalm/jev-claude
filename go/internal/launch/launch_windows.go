package launch

import (
	"os"
	"os/exec"
	"syscall"
)

func executable(file string) bool {
	_, err := os.Stat(file)
	return err == nil
}

func verbatim(spec Spec, args []string) *exec.Cmd {
	c := exec.Command(spec.Command)
	c.SysProcAttr = &syscall.SysProcAttr{CmdLine: CmdLine(spec, args)}
	return c
}

// Command jev-legend prints the status line key (SPEC 13).
package main

import (
	"os"
	"runtime"

	"github.com/alienfacepalm/jev-claude/go/internal/config"
	"github.com/alienfacepalm/jev-claude/go/internal/icons"
	"github.com/alienfacepalm/jev-claude/go/internal/legend"
)

func main() {
	os.Stdout.WriteString("Status line key\n\n" + legend.Format(icons.Icons(config.ProcessEnv, runtime.GOOS)) + "\n")
}

// Command jev-statusline is the status line Claude Code runs (SPEC 12).
package main

import (
	"io"
	"os"

	"github.com/alienfacepalm/jev-claude/go/internal/config"
	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
	"github.com/alienfacepalm/jev-claude/go/internal/statusline"
)

func main() {
	stdin, _ := io.ReadAll(os.Stdin)
	line, err := statusline.Render(stdin, config.ProcessEnv)
	if err != nil {
		os.Stderr.WriteString(err.Error() + "\n")
		os.Exit(1)
	}
	os.Stdout.WriteString(jsstr.ToUTF8(line))
}

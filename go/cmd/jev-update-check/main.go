// Command jev-update-check looks for a newer version in the background (SPEC 14).
package main

import (
	"time"

	"github.com/alienfacepalm/jev-claude/go/internal/repo"
	"github.com/alienfacepalm/jev-claude/go/internal/update"
)

func main() {
	root := repo.Root()
	if root == "" {
		return // no repository to check (SPEC 2.4)
	}
	update.WriteState(update.CheckForUpdate(root, time.Now()), update.File)
}

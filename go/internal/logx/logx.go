// Package logx is the debug log (SPEC 15), ported from node/src/log.mjs.
package logx

import (
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
	"github.com/alienfacepalm/jev-claude/go/internal/osdirs"
)

// File is <home at start>/.jev-claude.log.
var File = filepath.Join(osdirs.Home(), ".jev-claude.log")

// Interactive is whether stdout was a terminal at start (SPEC 3.9).
var Interactive = IsTerminal(os.Stdout)

var (
	mu        sync.Mutex
	tightened bool
)

// ISOTime is Date.prototype.toISOString() for a time.
func ISOTime(t time.Time) string { return t.UTC().Format("2006-01-02T15:04:05.000Z") }

// Log writes one line: to stderr when stdout is not a terminal, else to the log file.
func Log(line string) {
	text := "[jev] " + jsstr.ToUTF8(line) + "\n"
	mu.Lock()
	defer mu.Unlock()
	if !Interactive {
		os.Stderr.WriteString(text)
		return
	}
	f, err := os.OpenFile(File, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o600)
	if err != nil {
		return
	}
	// Logging never fails the caller: write and chmod errors are swallowed, as in Node.
	_, _ = f.WriteString(ISOTime(time.Now()) + " " + text)
	f.Close()
	if !tightened {
		tightened = true
		_ = os.Chmod(File, 0o600)
	}
}

// Debug logs only while JEV_DEBUG is set to a non-empty value.
func Debug(line func() string) {
	if os.Getenv("JEV_DEBUG") != "" {
		Log(line())
	}
}

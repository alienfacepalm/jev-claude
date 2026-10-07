// Package firstrun offers the setup check once per user (SPEC 11), ported from
// node/src/first-run.mjs.
package firstrun

import (
	"bufio"
	"io"
	"os"
	"os/signal"
	"path/filepath"
	"regexp"
	"strings"
	"time"

	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
	"github.com/alienfacepalm/jev-claude/go/internal/logx"
	"github.com/alienfacepalm/jev-claude/go/internal/osdirs"
)

// File is the marker, from the home directory at start.
var File = filepath.Join(osdirs.Home(), ".jev-router", "first-run.json")

// WasOffered is whether the marker can be read.
func WasOffered(file string) bool {
	_, err := os.ReadFile(file)
	return err == nil
}

// MarkOffered records the offer; errors are swallowed.
func MarkOffered(accepted bool, file string) {
	if os.MkdirAll(filepath.Dir(file), 0o777) != nil {
		return
	}
	_ = os.WriteFile(file, []byte(jsjson.Stringify(jsjson.Obj("offeredAt", logx.ISOTime(time.Now()), "accepted", accepted))), 0o666)
}

// ShouldOffer is whether this launch may offer the check.
func ShouldOffer(args []string, interactive, offered, shadowed bool) bool {
	return interactive && !offered && !shadowed && len(args) == 0
}

// ShadowsSkill is whether cwd defines its own jev-calibrate skill and is not the router's
// repository. With no root there is nothing to shadow.
func ShadowsSkill(cwd, root string) bool {
	if root == "" {
		return false
	}
	a, err1 := filepath.Abs(cwd)
	b, err2 := filepath.Abs(root)
	if err1 != nil || err2 != nil || a == b {
		return false
	}
	_, err := os.Stat(filepath.Join(cwd, ".claude", "skills", "jev-calibrate"))
	return err == nil
}

// Answer is the outcome of AskYesNo.
type Answer int

// Answers.
const (
	None Answer = iota // input closed or failed without an answer
	Yes
	No
	Interrupt // Ctrl+C
)

var no = regexp.MustCompile(`^(n|no)$`)

// AskYesNo writes question to out and reads one line from in. Anything but n/no is yes, so an
// empty line is yes; end of input or an error is None; Ctrl+C (SIGINT) is Interrupt. It
// always settles. interrupts may be nil.
func AskYesNo(question string, in io.Reader, out io.Writer, interrupts <-chan os.Signal) Answer {
	_, _ = io.WriteString(out, question)
	lines := make(chan Answer, 1)
	go func() {
		line, err := bufio.NewReader(in).ReadString('\n')
		if err != nil && line == "" {
			lines <- None
			return
		}
		line = strings.TrimSuffix(line, "\n")
		line = strings.TrimSuffix(line, "\r")
		if no.MatchString(jsstr.ASCIILower(jsstr.Trim(line))) {
			lines <- No
		} else {
			lines <- Yes
		}
	}()
	select {
	case a := <-lines:
		return a
	case <-interrupts:
		return Interrupt
	}
}

// AskTerminal asks on stdin/stderr, turning Ctrl+C into Interrupt.
func AskTerminal(question string) Answer {
	sig := make(chan os.Signal, 1)
	signal.Notify(sig, os.Interrupt)
	defer signal.Stop(sig)
	return AskYesNo(question, os.Stdin, os.Stderr, sig)
}

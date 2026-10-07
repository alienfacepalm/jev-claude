// Package launch resolves and starts the real claude CLI (SPEC 10.2), ported from
// node/src/launch.mjs.
package launch

import (
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"

	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
)

// Windows is whether this process runs on Windows.
var Windows = runtime.GOOS == "windows"

// ResolveCommand finds name on path. On Windows (win) it tries each suffix of exts (nil:
// PATHEXT, else .COM;.EXE;.BAT;.CMD) in each ;-separated entry and accepts the first path that
// exists; elsewhere it accepts name in a :-separated entry when it is executable.
func ResolveCommand(name string, exts []string, path string, win bool) string {
	suffixes := []string{""}
	sep := ":"
	if win {
		sep = ";"
		suffixes = exts
		if suffixes == nil {
			pathext, ok := os.LookupEnv("PATHEXT")
			if !ok {
				pathext = ".COM;.EXE;.BAT;.CMD"
			}
			suffixes = strings.Split(pathext, ";")
		}
	}
	for _, dir := range strings.Split(path, sep) {
		if dir == "" {
			continue
		}
		dir = strings.TrimPrefix(dir, `"`)
		dir = strings.TrimSuffix(dir, `"`)
		for _, ext := range suffixes {
			file := filepath.Join(dir, name+ext)
			if win {
				if _, err := os.Stat(file); err == nil {
					return file
				}
			} else if executable(file) {
				return file
			}
		}
	}
	return ""
}

var shimPattern = regexp.MustCompile(`"%~?dp0%?\\([^"]+?\.[cm]?js)"`)

// ShimScript is the Node script an npm .cmd shim launches, or "".
func ShimScript(file string) string {
	data, err := os.ReadFile(file)
	if err != nil {
		return ""
	}
	text := string(data)
	m := shimPattern.FindStringSubmatchIndex(jsstr.ASCIILower(text))
	if m == nil {
		return ""
	}
	parts := append([]string{filepath.Dir(file)}, strings.Split(text[m[2]:m[3]], `\`)...)
	script := filepath.Join(parts...)
	if _, err := os.Stat(script); err != nil {
		return ""
	}
	return script
}

var meta = regexp.MustCompile("([()\\][%!^\"`<>&|;, *?])")

func caret(s string) string { return meta.ReplaceAllString(s, "^$1") }

var (
	backslashQuote = regexp.MustCompile(`(\\*)"`)
	trailingSlash  = regexp.MustCompile(`(\\*)$`)
)

// QuoteForCmd quotes one argument for cmd.exe followed by a batch file's own parse.
func QuoteForCmd(arg any) string {
	quoted := jsjson.JSString(arg)
	quoted = backslashQuote.ReplaceAllString(quoted, `$1$1\"`)
	quoted = trailingSlash.ReplaceAllString(quoted, "$1$1")
	return caret(caret(`"` + quoted + `"`))
}

// Spec is how to start a resolved executable.
type Spec struct {
	Command string
	Prefix  []string
	Shim    string // a .cmd/.bat run through cmd.exe with a verbatim command line
}

var (
	ps1 = regexp.MustCompile(`\.ps1$`)
	cmd = regexp.MustCompile(`\.(cmd|bat)$`)
)

// SpecFor decides how to start file (Node's launchSpec).
func SpecFor(file string) Spec {
	lower := jsstr.ASCIILower(file)
	if ps1.MatchString(lower) {
		return Spec{Command: "powershell.exe", Prefix: []string{"-NoProfile", "-ExecutionPolicy", "Bypass", "-File", file}}
	}
	if cmd.MatchString(lower) {
		if script := ShimScript(file); script != "" {
			if node := ResolveCommand("node", nil, os.Getenv("PATH"), Windows); node != "" {
				return Spec{Command: node, Prefix: []string{script}}
			}
		}
		comspec, ok := os.LookupEnv("ComSpec")
		if !ok {
			comspec = "cmd.exe"
		}
		return Spec{Command: comspec, Shim: file}
	}
	return Spec{Command: file}
}

// CmdLine is the verbatim command line for a shim spec.
func CmdLine(spec Spec, args []string) string {
	parts := []string{caret(spec.Shim)}
	for _, a := range args {
		parts = append(parts, QuoteForCmd(a))
	}
	return spec.Command + ` /d /s /c "` + strings.Join(parts, " ") + `"`
}

// Command builds the process for spec and args, never through an implicit shell.
func Command(spec Spec, args []string) *exec.Cmd {
	if spec.Shim == "" {
		return exec.Command(spec.Command, append(append([]string{}, spec.Prefix...), args...)...)
	}
	return verbatim(spec, args)
}

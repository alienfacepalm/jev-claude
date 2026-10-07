package main

import (
	"bytes"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/alienfacepalm/jev-claude/go/internal/repo"
)

// What the stand-in prints: the arguments and the variables that say whether a session was set up.
const fakeClaude = `process.stdout.write(JSON.stringify({
  args: process.argv.slice(2),
  env: Object.fromEntries(
    ["ANTHROPIC_BASE_URL", "ANTHROPIC_MODEL", "JEV_API_KEY"].map((k) => [k, process.env[k] ?? null]),
  ),
}));
`

type seenByClaude struct {
	Args []string          `json:"args"`
	Env  map[string]string `json:"env"`
}

var (
	buildOnce  sync.Once
	launcher   string
	buildError error
	buildDir   string
)

func TestMain(m *testing.M) {
	code := m.Run()
	if buildDir != "" {
		_ = os.RemoveAll(buildDir)
	}
	os.Exit(code)
}

// launcherBinary builds the real jev-claude program once.
func launcherBinary(t *testing.T) string {
	t.Helper()
	buildOnce.Do(func() {
		dir, err := os.MkdirTemp("", "jev-launcher-build-")
		if err != nil {
			buildError = err
			return
		}
		buildDir = dir
		name := "jev-claude"
		if runtime.GOOS == "windows" {
			name += ".exe"
		}
		out, err := exec.Command("go", "build", "-o", filepath.Join(dir, name), ".").CombinedOutput()
		if err != nil {
			buildError = &buildFailure{err: err, output: string(out)}
			return
		}
		launcher = filepath.Join(dir, name)
	})
	if buildError != nil {
		t.Fatalf("building the launcher: %v", buildError)
	}
	return launcher
}

type buildFailure struct {
	err    error
	output string
}

func (f *buildFailure) Error() string { return f.err.Error() + "\n" + f.output }

// repoRoot is the repository this test file sits in.
func repoRoot(t *testing.T) string {
	t.Helper()
	here, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	root := repo.Find(here)
	if root == "" {
		t.Fatal("the repository holding this port has the marker")
	}
	return root
}

// installFakeClaude puts a `claude` the launcher finds on PATH into dir: an npm-style `.cmd` shim
// on Windows (the form the launcher runs directly through node), an executable script elsewhere.
func installFakeClaude(t *testing.T, dir string) {
	t.Helper()
	if runtime.GOOS == "windows" {
		if err := os.WriteFile(filepath.Join(dir, "claude-fake.mjs"), []byte(fakeClaude), 0o644); err != nil {
			t.Fatal(err)
		}
		shim := "@ECHO off\r\n\"node\"  \"%~dp0\\claude-fake.mjs\" %*\r\n"
		if err := os.WriteFile(filepath.Join(dir, "claude.cmd"), []byte(shim), 0o644); err != nil {
			t.Fatal(err)
		}
		return
	}
	if err := os.WriteFile(filepath.Join(dir, "claude"), []byte("#!/usr/bin/env node\n"+fakeClaude), 0o755); err != nil {
		t.Fatal(err)
	}
}

// cleanEnv is the test process's environment without the named variables.
func cleanEnv(drop ...string) []string {
	var kept []string
	for _, kv := range os.Environ() {
		key, _, _ := strings.Cut(kv, "=")
		skip := false
		for _, name := range drop {
			if key == name || (runtime.GOOS == "windows" && strings.EqualFold(key, name)) {
				skip = true
			}
		}
		if !skip {
			kept = append(kept, kv)
		}
	}
	return kept
}

// run starts the real launcher with the given environment and returns what the stand-in saw and
// what the launcher wrote to stderr.
func run(t *testing.T, cwd string, env []string, args ...string) (seenByClaude, string) {
	t.Helper()
	cmd := exec.Command(launcherBinary(t), args...)
	cmd.Dir = cwd
	cmd.Env = env
	var stdout, stderr bytes.Buffer
	cmd.Stdout, cmd.Stderr = &stdout, &stderr
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	done := make(chan error, 1)
	go func() { done <- cmd.Wait() }()
	select {
	case err := <-done:
		if err != nil {
			t.Fatalf("launcher failed: %v\nstdout: %s\nstderr: %s", err, stdout.String(), stderr.String())
		}
	case <-time.After(30 * time.Second):
		_ = cmd.Process.Kill()
		t.Fatalf("launcher timed out\nstdout: %s\nstderr: %s", stdout.String(), stderr.String())
	}
	var seen seenByClaude
	if err := json.Unmarshal(stdout.Bytes(), &seen); err != nil {
		t.Fatalf("stand-in output %q: %v\nstderr: %s", stdout.String(), err, stderr.String())
	}
	return seen, stderr.String()
}

// launch runs the real launcher in an empty home and working directory, with a Jev key set.
func runSession(t *testing.T, args ...string) seenByClaude {
	t.Helper()
	base := t.TempDir()
	bin, home, cwd := filepath.Join(base, "bin"), filepath.Join(base, "home"), filepath.Join(base, "cwd")
	for _, dir := range []string{bin, home, cwd} {
		if err := os.Mkdir(dir, 0o755); err != nil {
			t.Fatal(err)
		}
	}
	installFakeClaude(t, bin)
	env := cleanEnv("PATH", "HOME", "USERPROFILE", "JEV_API_KEY", "JEV_STATUS_DIR", "JEV_ROOT",
		"ANTHROPIC_BASE_URL", "ANTHROPIC_MODEL", "TYPESAFE_API_KEY", "JEV_NO_STATUSLINE")
	env = append(env,
		"PATH="+bin+string(os.PathListSeparator)+os.Getenv("PATH"),
		"HOME="+home, "USERPROFILE="+home,
		"JEV_API_KEY=test-key",
		"JEV_STATUS_DIR="+filepath.Join(base, "status"),
		"JEV_ROOT="+repoRoot(t),
	)
	seen, _ := run(t, cwd, env, args...)
	return seen
}

func TestSessionLaunchGetsAddDirSettingsAndTheProxy(t *testing.T) {
	seen := runSession(t)
	n := len(seen.Args)
	if n < 4 || seen.Args[n-4] != "--add-dir" || seen.Args[n-3] != repoRoot(t) || seen.Args[n-2] != "--settings" {
		t.Fatalf("args = %q, want ... --add-dir <root> --settings <file>", seen.Args)
	}
	if !regexp.MustCompile(`^http://127\.0\.0\.1:\d+$`).MatchString(seen.Env["ANTHROPIC_BASE_URL"]) {
		t.Fatalf("ANTHROPIC_BASE_URL = %q", seen.Env["ANTHROPIC_BASE_URL"])
	}
	if seen.Env["ANTHROPIC_MODEL"] != "jev-router" {
		t.Fatalf("ANTHROPIC_MODEL = %q", seen.Env["ANTHROPIC_MODEL"])
	}
	if seen.Env["JEV_API_KEY"] != "" {
		t.Fatal("the key is the launcher's alone")
	}
}

func TestPromptThatStartsWithASubcommandNameIsStillASession(t *testing.T) {
	seen := runSession(t, "update the docs")
	if len(seen.Args) == 0 || seen.Args[0] != "update the docs" {
		t.Fatalf("args = %q", seen.Args)
	}
	found := false
	for _, a := range seen.Args {
		found = found || a == "--add-dir"
	}
	if !found {
		t.Fatalf("a prompt is a session and gets --add-dir, args = %q", seen.Args)
	}
	if !strings.HasPrefix(seen.Env["ANTHROPIC_BASE_URL"], "http:") {
		t.Fatalf("ANTHROPIC_BASE_URL = %q", seen.Env["ANTHROPIC_BASE_URL"])
	}
}

func TestClaudeSubcommandsRunUntouched(t *testing.T) {
	for _, args := range [][]string{
		{"mcp", "list"},
		{"plugin", "install", "x", "--scope", "user"},
		{"doctor"},
		{"update"},
	} {
		name := strings.Join(args, " ")
		seen := runSession(t, args...)
		if strings.Join(seen.Args, "\x00") != strings.Join(args, "\x00") {
			t.Errorf("%s: args = %q, want exactly %q", name, seen.Args, args)
		}
		if v := seen.Env["ANTHROPIC_BASE_URL"]; v != "" {
			t.Errorf("%s: ANTHROPIC_BASE_URL = %q, want unset", name, v)
		}
		if v := seen.Env["ANTHROPIC_MODEL"]; v != "" {
			t.Errorf("%s: ANTHROPIC_MODEL = %q, want unset", name, v)
		}
		if v := seen.Env["JEV_API_KEY"]; v != "" {
			t.Errorf("%s: the key must be stripped from the subcommand's environment too", name)
		}
	}
}

func TestSubcommandWithoutAKeyDoesNotAnnounceThatRoutingIsOff(t *testing.T) {
	// No key: a subcommand is not a session, so there is nothing to say about routing.
	base := t.TempDir()
	bin := filepath.Join(base, "bin")
	if err := os.Mkdir(bin, 0o755); err != nil {
		t.Fatal(err)
	}
	installFakeClaude(t, bin)
	env := cleanEnv("PATH", "HOME", "USERPROFILE", "JEV_API_KEY", "TYPESAFE_API_KEY", "JEV_ROOT",
		"ANTHROPIC_BASE_URL", "ANTHROPIC_MODEL")
	env = append(env, "PATH="+bin+string(os.PathListSeparator)+os.Getenv("PATH"), "HOME="+base, "USERPROFILE="+base)
	seen, stderr := run(t, base, env, "mcp", "list")
	if strings.Contains(stderr, "no JEV_API_KEY") {
		t.Fatalf("stderr announces routing is off: %q", stderr)
	}
	if strings.Join(seen.Args, " ") != "mcp list" {
		t.Fatalf("args = %q", seen.Args)
	}
}

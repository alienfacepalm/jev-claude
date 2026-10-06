// Command jev-claude starts the routing proxy and launches the real Claude Code CLI (SPEC 10).
package main

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"syscall"
	"time"

	"github.com/alienfacepalm/jev-claude/go/internal/config"
	"github.com/alienfacepalm/jev-claude/go/internal/env"
	"github.com/alienfacepalm/jev-claude/go/internal/firstrun"
	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/launch"
	"github.com/alienfacepalm/jev-claude/go/internal/logx"
	"github.com/alienfacepalm/jev-claude/go/internal/osdirs"
	"github.com/alienfacepalm/jev-claude/go/internal/proxy"
	"github.com/alienfacepalm/jev-claude/go/internal/repo"
	"github.com/alienfacepalm/jev-claude/go/internal/settings"
	"github.com/alienfacepalm/jev-claude/go/internal/status"
)

var (
	cleanupOnce sync.Once
	cleanups    []func()
)

// exit runs the cleanup (once) on every exit path, then exits.
func exit(code int) {
	cleanupOnce.Do(func() {
		for _, f := range cleanups {
			f()
		}
	})
	os.Exit(code)
}

// setEnv sets key in a "KEY=value" list, replacing an existing entry.
func setEnv(list []string, key, value string) []string {
	for i, kv := range list {
		k, _, _ := strings.Cut(kv, "=")
		if k == key || (runtime.GOOS == "windows" && strings.EqualFold(k, key)) {
			list[i] = key + "=" + value
			return list
		}
	}
	return append(list, key+"="+value)
}

// statusLineArgs points Claude Code at this port's status line unless the user has their own.
func statusLineArgs(cwd string) []string {
	if os.Getenv("JEV_NO_STATUSLINE") != "" {
		return nil
	}
	for _, dir := range []string{filepath.Join(cwd, ".claude"), filepath.Join(osdirs.Home(), ".claude")} {
		data, err := os.ReadFile(filepath.Join(dir, "settings.json"))
		if err != nil {
			continue
		}
		if v, err := jsjson.ParseBytes(data); err == nil && v != nil && jsjson.Truthy(jsjson.Prop(v, "statusLine")) {
			return nil
		}
	}
	exe, err := os.Executable()
	if err != nil {
		return nil
	}
	name := "jev-statusline"
	if runtime.GOOS == "windows" {
		name += ".exe"
	}
	command := `"` + filepath.Join(filepath.Dir(exe), name) + `"`
	file := status.SettingsFile()
	if status.WritePrivate(file, jsjson.Stringify(jsjson.Obj("statusLine", jsjson.Obj("type", "command", "command", command)))) != nil {
		return nil
	}
	return []string{"--settings", file}
}

func main() {
	savedModelBefore := settings.ReadSavedModel(settings.UserSettings, "")
	cwd, _ := os.Getwd()
	env.Load(cwd, osdirs.Home(), env.Process{})

	root := repo.Root()
	userArgs := os.Args[1:]
	args := append([]string{}, userArgs...)
	if root != "" {
		args = append(args, "--add-dir", root)
	}
	childEnv := env.ChildEnv(os.Environ())

	claude := launch.ResolveCommand("claude", nil, os.Getenv("PATH"), launch.Windows)
	if claude == "" {
		os.Stderr.WriteString("[jev] Claude Code is not installed, or `claude` is not on your PATH.\n" +
			"[jev] jev-claude runs the real Claude Code CLI; install it first:\n" +
			"[jev]   https://code.claude.com/docs/en/setup\n")
		exit(1)
	}

	interactive := logx.IsTerminal(os.Stdin) && logx.IsTerminal(os.Stdout)
	if firstrun.ShouldOffer(userArgs, interactive, firstrun.WasOffered(firstrun.File), firstrun.ShadowsSkill(cwd, root)) {
		answer := firstrun.AskTerminal("[jev] First run: check your Jev Router setup now with /jev-calibrate?\n" +
			"[jev] It only reads your setup and changes nothing, using a little of your Claude usage. [Y/n] ")
		if answer == firstrun.Interrupt {
			exit(130)
		}
		if answer != firstrun.None {
			firstrun.MarkOffered(answer == firstrun.Yes, firstrun.File)
		}
		if answer == firstrun.Yes {
			args = append([]string{"/jev-calibrate check"}, args...)
		}
	}

	if os.Getenv("JEV_API_KEY") != "" || os.Getenv("TYPESAFE_API_KEY") != "" {
		inherited := os.Getenv("ANTHROPIC_BASE_URL")
		p, err := proxy.Start(proxy.Options{UpstreamURL: inherited})
		if err != nil {
			fmt.Fprintf(os.Stderr, "[jev] could not start the routing proxy: %v\n", err)
			exit(1)
		}
		if inherited != "" && os.Getenv("JEV_DEBUG") != "" {
			fmt.Fprintf(os.Stderr, "[jev] upstream %s\n", inherited)
		}
		for _, kv := range [][2]string{
			{"ANTHROPIC_BASE_URL", fmt.Sprintf("http://127.0.0.1:%d", p.Port)},
			{"CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY", "1"},
			{"ANTHROPIC_CUSTOM_MODEL_OPTION", config.AutoModel},
			{"ANTHROPIC_CUSTOM_MODEL_OPTION_NAME", "Jev Router"},
			{"ANTHROPIC_CUSTOM_MODEL_OPTION_DESCRIPTION", "Route each turn to the cheapest model that can do it"},
			{"ANTHROPIC_CUSTOM_MODEL_OPTION_SUPPORTED_CAPABILITIES", "thinking,adaptive_thinking,interleaved_thinking,effort,max_effort"},
			{"CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT", "1"},
		} {
			childEnv = setEnv(childEnv, kv[0], kv[1])
		}
		if os.Getenv("ANTHROPIC_MODEL") == "" {
			childEnv = setEnv(childEnv, "ANTHROPIC_MODEL", config.AutoModel)
		}
		cleanups = append(cleanups, func() {
			p.Close()
			settings.RestoreSavedModel(savedModelBefore, settings.UserSettings)
		})
		args = append(args, statusLineArgs(cwd)...)
		if os.Getenv("JEV_DEBUG") != "" && logx.IsTerminal(os.Stdout) {
			fmt.Fprintf(os.Stderr, "[jev] routing decisions -> %s\n", logx.File)
		}
	} else {
		fmt.Fprintf(os.Stderr, "[jev] no JEV_API_KEY found - starting Claude Code without routing\n"+
			"[jev] set it in %s to enable routing\n", filepath.Join(osdirs.Home(), ".jev-router.env"))
	}

	// Ctrl+C reaches Claude Code too, and it decides whether the session ends. A handler (not
	// SIG_IGN) keeps the child's own default behaviour.
	interrupts := make(chan os.Signal, 1)
	signal.Notify(interrupts, os.Interrupt)
	go func() {
		for range interrupts {
		}
	}()
	stops := make(chan os.Signal, 1)
	signal.Notify(stops, syscall.SIGTERM, syscall.SIGHUP)

	cmd := launch.Command(launch.LaunchSpec(claude), args)
	cmd.Stdin, cmd.Stdout, cmd.Stderr = os.Stdin, os.Stdout, os.Stderr
	cmd.Env = childEnv
	if err := cmd.Start(); err != nil {
		fmt.Fprintf(os.Stderr, "[jev] could not start Claude Code: %v\n", err)
		exit(1)
	}

	go func() {
		sig := <-stops
		// Windows cannot deliver a signal to another process; Node's child.kill terminates it too.
		if runtime.GOOS == "windows" {
			cmd.Process.Kill()
		} else {
			cmd.Process.Signal(sig)
		}
		time.Sleep(5 * time.Second)
		exit(1)
	}()

	err := cmd.Wait()
	var exitErr *exec.ExitError
	switch {
	case err == nil:
		exit(0)
	case errors.As(err, &exitErr):
		if code := exitErr.ExitCode(); code >= 0 {
			exit(code)
		}
		exit(1) // killed by a signal
	default:
		exit(1)
	}
}

package proxy

import (
	"bufio"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/alienfacepalm/jev-claude/go/internal/config"
	"github.com/alienfacepalm/jev-claude/go/internal/env"
	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/launch"
	"github.com/alienfacepalm/jev-claude/go/internal/router"
	"github.com/alienfacepalm/jev-claude/go/internal/status"
)

// Ported from node/test/hardening.test.mjs.

var (
	haiku  = config.IDOf("haiku")
	sonnet = config.IDOf("sonnet")
)

func sure(choice string) Route { return answer(choice, 0.97) }

var (
	toolResult = `{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}`
	toolUse    = `{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}}]}`
)

func TestHardeningRouting(t *testing.T) {
	bash := func() any { return parse(t, `[{"name":"Bash"}]`) }

	t.Run("the main thread keeps its tier after a session has run 50 sub-agents", func(t *testing.T) {
		rec, url := recordingUpstream(t, nil)
		base := startProxy(t, Options{UpstreamURL: url, Route: sure(haiku)})
		session := fmt.Sprintf("lru-%d", os.Getpid())
		opening := parse(t, `{"role":"user","content":"rename the config loader"}`)
		send := func(messages []any) {
			post(t, base+"/v1/messages", jsjson.Obj("model", "jev-router", "tools", bash(), "metadata", metadata(session), "messages", messages), nil)
		}
		send([]any{opening})
		for i := 0; i < 51; i++ {
			send([]any{jsjson.Obj("role", "user", "content", fmt.Sprintf("sub-agent %d", i))})
		}
		// A tool continuation of the main thread's turn, which must not change model mid-task.
		send([]any{opening, parse(t, toolUse), parse(t, toolResult)})
		all := rec.all()
		if modelOf(all[len(all)-1]) != haiku {
			t.Errorf("%v", modelOf(all[len(all)-1]))
		}
	})

	t.Run("a conversation the proxy has not routed yet may still downgrade", func(t *testing.T) {
		rec, url := recordingUpstream(t, nil)
		base := startProxy(t, Options{UpstreamURL: url, Route: sure(haiku)})
		// A resumed session: a long history, but nothing cached on any model by this process.
		history := strings.Repeat("x", 120000)
		post(t, base+"/v1/messages", jsjson.Obj("model", "jev-router", "tools", bash(), "messages", []any{
			jsjson.Obj("role", "user", "content", history), parse(t, toolUse), parse(t, toolResult), parse(t, `{"role":"user","content":"fix the typo"}`),
		}), nil)
		if modelOf(rec.all()[0]) != haiku {
			t.Errorf("%v", modelOf(rec.all()[0]))
		}
	})

	t.Run("a routing failure never forwards the sentinel", func(t *testing.T) {
		rec, url := recordingUpstream(t, nil)
		base := startProxy(t, Options{UpstreamURL: url, Route: func(router.Args) (*jsjson.Object, error) {
			return nil, errors.New("router blew up")
		}})
		post(t, base+"/v1/messages", jsjson.Obj("model", "jev-router", "tools", bash(), "messages", parse(t, `[{"role":"user","content":"hello"}]`)), nil)
		if modelOf(rec.all()[0]) != sonnet {
			t.Errorf("a failure lands on the default tier, never the sentinel: %v", modelOf(rec.all()[0]))
		}
	})

	t.Run("print mode keeps the conversation's tier when the session id appears later", func(t *testing.T) {
		rec, url := recordingUpstream(t, nil)
		base := startProxy(t, Options{UpstreamURL: url, Route: sure(haiku)})
		opening := jsjson.Obj("role", "user", "content", fmt.Sprintf("print-mode %d", os.Getpid()))
		// `claude -p` sends its first request without metadata.
		post(t, base+"/v1/messages", jsjson.Obj("model", "jev-router", "tools", bash(), "messages", []any{opening}), nil)
		post(t, base+"/v1/messages", jsjson.Obj("model", "jev-router", "tools", bash(), "metadata", metadata(fmt.Sprintf("late-%d", os.Getpid())),
			"messages", []any{opening, parse(t, toolUse), parse(t, toolResult)}), nil)
		all := rec.all()
		if modelOf(all[0]) != haiku || modelOf(all[1]) != haiku {
			t.Errorf("%v %v", modelOf(all[0]), modelOf(all[1]))
		}
	})

	t.Run("a client that leaves stops the upstream response", func(t *testing.T) {
		gaveUp := make(chan bool, 1)
		upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			io.ReadAll(r.Body)
			w.Header().Set("content-type", "text/event-stream")
			w.WriteHeader(200)
			sent := 0
			ticker := time.NewTicker(20 * time.Millisecond)
			defer ticker.Stop()
			for sent < 100 {
				select {
				case <-r.Context().Done():
					gaveUp <- true
					return
				case <-ticker.C:
					if _, err := fmt.Fprintf(w, "data: {\"n\":%d}\n\n", sent); err != nil {
						gaveUp <- true
						return
					}
					w.(http.Flusher).Flush()
					sent++
				}
			}
			gaveUp <- false
		}))
		t.Cleanup(upstream.Close)
		base := startProxy(t, Options{UpstreamURL: upstream.URL, Route: sure(haiku)})

		conn, err := net.Dial("tcp", strings.TrimPrefix(base, "http://"))
		if err != nil {
			t.Fatal(err)
		}
		body := `{"model":"jev-router","tools":[{"name":"Bash"}],"messages":[{"role":"user","content":"go"}]}`
		fmt.Fprintf(conn, "POST /v1/messages HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: %d\r\n\r\n%s", len(body), body)
		res, err := http.ReadResponse(bufio.NewReader(conn), nil)
		if err != nil {
			t.Fatal(err)
		}
		buf := make([]byte, 1)
		res.Body.Read(buf) // the first data arrives
		conn.Close()

		select {
		case cut := <-gaveUp:
			if !cut {
				t.Fatal("the upstream stream ran to the end")
			}
		case <-time.After(5 * time.Second):
			t.Fatal("the upstream never noticed")
		}
	})

	t.Run("an upstream that drops mid-stream fails the client instead of hanging it", func(t *testing.T) {
		// A raw upstream: one chunk of an event stream, then the socket is destroyed.
		ln, err := net.Listen("tcp", "127.0.0.1:0")
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { ln.Close() })
		go func() {
			for {
				c, err := ln.Accept()
				if err != nil {
					return
				}
				go func(c net.Conn) {
					req, err := http.ReadRequest(bufio.NewReader(c))
					if err != nil {
						c.Close()
						return
					}
					io.ReadAll(req.Body)
					io.WriteString(c, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n")
					chunk := "data: {\"n\":0}\n\n"
					fmt.Fprintf(c, "%x\r\n%s\r\n", len(chunk), chunk)
					time.Sleep(50 * time.Millisecond)
					c.Close()
				}(c)
			}
		}()
		base := startProxy(t, Options{UpstreamURL: "http://" + ln.Addr().String(), Route: sure(haiku)})

		done := make(chan error, 1)
		go func() {
			res, err := http.Post(base+"/v1/messages", "application/json", strings.NewReader(`{"model":"jev-router","tools":[{"name":"Bash"}],"messages":[{"role":"user","content":"go"}]}`))
			if err != nil {
				done <- err
				return
			}
			_, err = io.ReadAll(res.Body)
			res.Body.Close()
			done <- err
		}()
		select {
		case err := <-done:
			if err == nil {
				t.Fatal("the client sees an incomplete response, not a clean end")
			}
		case <-time.After(3 * time.Second):
			t.Fatal("client still waiting after 3s")
		}
	})
}

func TestHardeningEnv(t *testing.T) {
	t.Run("a project's .env may only set jev's own keys", func(t *testing.T) {
		cwd, home := t.TempDir(), t.TempDir()
		os.WriteFile(filepath.Join(cwd, ".env"), []byte(strings.Join([]string{
			"JEV_API_KEY=from-project",
			"JEV_OPUS_EFFORT=low",
			"JEV_FORCE_EFFORT=max",
			"JEV_SONNET_FORCE_EFFORT=low",
			"ANTHROPIC_BASE_URL=https://attacker.example",
			"TYPESAFE_BASE_URL=https://attacker.example",
			"NODE_OPTIONS=--require /tmp/evil.js",
			"JEV_DUMP=/tmp/loot",
			"JEV_DEBUG=project",
		}, "\n")), 0o644)
		os.WriteFile(filepath.Join(home, ".jev-router.env"), []byte("JEV_DEBUG=home\nTYPESAFE_BASE_URL=https://jev.example\n"), 0o644)

		e := env.Map{"JEV_ALLOW_FABLE": "1"}
		env.Load(cwd, home, e)
		want := map[string]string{
			"JEV_API_KEY": "from-project", "JEV_OPUS_EFFORT": "low", "JEV_FORCE_EFFORT": "max", "JEV_SONNET_FORCE_EFFORT": "low",
			"JEV_DEBUG":         "project",             // the project file still outranks the home file
			"TYPESAFE_BASE_URL": "https://jev.example", // only the user's own file may move Jev
			"JEV_ALLOW_FABLE":   "1",                   // the real environment wins
		}
		for k, v := range want {
			if e[k] != v {
				t.Errorf("%s = %q, want %q", k, e[k], v)
			}
		}
		for _, k := range []string{"ANTHROPIC_BASE_URL", "NODE_OPTIONS", "JEV_DUMP"} {
			if _, ok := e[k]; ok {
				t.Errorf("%s set from a project file", k)
			}
		}
		list := []string{"PATH=/bin", "TYPESAFE_API_KEY=k"}
		for k, v := range e {
			list = append(list, k+"="+v)
		}
		child := strings.Join(env.ChildEnv(list), "\n")
		if strings.Contains(child, "JEV_API_KEY=") || strings.Contains(child, "TYPESAFE_API_KEY=") || !strings.Contains(child, "PATH=/bin") {
			t.Errorf("child env: %s", child)
		}
	})

	t.Run("a Claude API key in the user's own file reaches Claude Code, but never from a project's .env", func(t *testing.T) {
		cwd, home := t.TempDir(), t.TempDir()
		// A repository's .env must not be able to send your prompts to someone else's account.
		os.WriteFile(filepath.Join(cwd, ".env"), []byte("ANTHROPIC_API_KEY=sk-ant-someone-else\n"), 0o644)
		os.WriteFile(filepath.Join(home, ".jev-router.env"), []byte("JEV_API_KEY=jev\nANTHROPIC_API_KEY=sk-ant-mine\n"), 0o644)
		e := env.Map{}
		env.Load(cwd, home, e)
		if e["ANTHROPIC_API_KEY"] != "sk-ant-mine" {
			t.Errorf("%v", e)
		}
		child := strings.Join(env.ChildEnv([]string{"ANTHROPIC_API_KEY=" + e["ANTHROPIC_API_KEY"], "JEV_API_KEY=" + e["JEV_API_KEY"]}), "\n")
		if child != "ANTHROPIC_API_KEY=sk-ant-mine" {
			t.Errorf("Claude Code needs it; only the Jev key is withheld: %q", child)
		}
		fromProjectOnly := env.Map{}
		env.Load(cwd, t.TempDir(), fromProjectOnly)
		if _, ok := fromProjectOnly["ANTHROPIC_API_KEY"]; ok {
			t.Error("a project .env set ANTHROPIC_API_KEY")
		}
	})

	t.Run("a blank key in a copied .env.example does not hide the real one", func(t *testing.T) {
		cwd, home := t.TempDir(), t.TempDir()
		example, err := os.ReadFile(filepath.Join("..", "..", "..", ".env.example"))
		if err != nil {
			t.Fatal(err)
		}
		os.WriteFile(filepath.Join(cwd, ".env"), example, 0o644)
		os.WriteFile(filepath.Join(home, ".jev-router.env"), []byte("JEV_API_KEY=from-home\n"), 0o644)
		e := env.Map{}
		env.Load(cwd, home, e)
		if e["JEV_API_KEY"] != "from-home" {
			t.Errorf("%q", e["JEV_API_KEY"])
		}
		if _, ok := e["JEV_ALLOW_FABLE"]; ok {
			t.Error("commented-out settings stay unset")
		}
	})
}

// shimDir is a directory holding an npm-style name.cmd shim, its script, and a .ps1 beside it.
func shimDir(t *testing.T, name string, withScript bool) (string, string) {
	dir := t.TempDir()
	script := filepath.Join(dir, "node_modules", "pkg", "cli.js")
	os.MkdirAll(filepath.Dir(script), 0o755)
	os.WriteFile(script, []byte("process.stdout.write(JSON.stringify(process.argv.slice(2)));\n"), 0o644)
	target := `"%dp0%\node_modules\pkg\cli.js"`
	if !withScript {
		target = `"` + script + `"`
	}
	os.WriteFile(filepath.Join(dir, name+".cmd"), []byte("@ECHO off\r\n\"node\"  "+target+" %*\r\n"), 0o644)
	os.WriteFile(filepath.Join(dir, name+".ps1"), []byte("#!/usr/bin/env pwsh\n"), 0o644)
	return dir, script
}

// Values whose quoting the old shell path broke: embedded quotes, a space, and cmd metacharacters.
var awkward = []string{`name="Jev Router"`, "fix a&b|c", "50% done", `say "hi"`, "plain"}

func runSpec(t *testing.T, spec launch.Spec, args []string) []string {
	t.Helper()
	cmd := launch.Command(spec, args)
	cmd.Stderr = os.Stderr
	out, err := cmd.Output()
	if err != nil {
		t.Fatalf("%v", err)
	}
	v, err := jsjson.Parse(string(out))
	if err != nil {
		t.Fatalf("output %q: %v", out, err)
	}
	var got []string
	for _, e := range v.([]any) {
		got = append(got, e.(string))
	}
	return got
}

func TestHardeningLaunch(t *testing.T) {
	if launch.ResolveCommand("node", nil, os.Getenv("PATH"), launch.Windows) == "" {
		t.Skip("node is not on PATH; the shim scripts are Node programs")
	}
	t.Run("prefers Claude's .cmd shim over its .ps1 and runs the script behind it directly", func(t *testing.T) {
		dir, script := shimDir(t, "claude", true)
		file := launch.ResolveCommand("claude", []string{".exe", ".cmd", ".bat", ".ps1"}, dir, true)
		if file != filepath.Join(dir, "claude.cmd") {
			t.Fatalf("resolved %s", file)
		}
		if launch.ShimScript(file) != script {
			t.Fatalf("shim script %q", launch.ShimScript(file))
		}
		spec := launch.LaunchSpec(file)
		if spec.Shim != "" || len(spec.Prefix) != 1 || spec.Prefix[0] != script || !regexp.MustCompile(`(?i)node(\.exe)?$`).MatchString(spec.Command) {
			t.Fatalf("%+v", spec)
		}
		if got := runSpec(t, spec, awkward); strings.Join(got, "\x00") != strings.Join(awkward, "\x00") {
			t.Fatalf("%q", got)
		}
	})

	t.Run("a PowerShell shim is run past the default execution policy", func(t *testing.T) {
		spec := launch.LaunchSpec(`C:\bin\claude.ps1`)
		if strings.Join(spec.Prefix[:4], " ") != "-NoProfile -ExecutionPolicy Bypass -File" {
			t.Fatalf("%+v", spec)
		}
	})

	t.Run("a shim with no script to run directly is quoted for cmd.exe", func(t *testing.T) {
		if runtime.GOOS != "windows" {
			t.Skip("cmd.exe shims exist only on Windows")
		}
		dir, _ := shimDir(t, "opaque", false)
		spec := launch.LaunchSpec(filepath.Join(dir, "opaque.cmd"))
		if spec.Shim == "" {
			t.Fatal("falls back to cmd.exe")
		}
		if got := runSpec(t, spec, awkward); strings.Join(got, "\x00") != strings.Join(awkward, "\x00") {
			t.Fatalf("%q", got)
		}
	})

	t.Run("cmd quoting escapes metacharacters", func(t *testing.T) {
		if regexp.MustCompile(`[^^][&|]`).MatchString(launch.QuoteForCmd("a&b|c")) {
			t.Error("every & and | is caret-escaped")
		}
		if regexp.MustCompile(`[^^]%`).MatchString(launch.QuoteForCmd("50%")) {
			t.Error("every % is caret-escaped")
		}
	})
}

func TestHardeningDump(t *testing.T) {
	t.Run("JEV_DUMP=1 writes owner-only dumps into the status directory, never over each other", func(t *testing.T) {
		first := status.DumpBody(jsjson.Obj("a", 1.0), "1")
		second := status.DumpBody(jsjson.Obj("a", 2.0), "1")
		defer os.Remove(first)
		defer os.Remove(second)
		if !strings.HasPrefix(first, status.Dir) || first == second {
			t.Fatalf("%s %s", first, second)
		}
		data, _ := os.ReadFile(second)
		if v, _ := jsjson.ParseBytes(data); jsjson.Stringify(v) != `{"a":2}` {
			t.Fatalf("%s", data)
		}
		if runtime.GOOS != "windows" {
			if info, _ := os.Stat(first); info.Mode().Perm() != 0o600 {
				t.Errorf("%v", info.Mode())
			}
		}
	})
}

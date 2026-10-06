package firstrun

import (
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// Ported from node/test/first-run.test.mjs.

func TestFirstRun(t *testing.T) {
	t.Run("the setup check is offered only on a plain interactive first launch", func(t *testing.T) {
		if !ShouldOffer(nil, true, false, false) {
			t.Error("a plain first launch")
		}
		if ShouldOffer(nil, true, true, false) {
			t.Error("once per user")
		}
		if ShouldOffer(nil, false, false, false) {
			t.Error("nobody to ask")
		}
		for _, args := range [][]string{{"-p", "fix it"}, {"--resume"}, {"explain this repo"}} {
			if ShouldOffer(args, true, false, false) {
				t.Error(strings.Join(args, " "))
			}
		}
	})

	t.Run("the offer is not made where a repository defines its own jev-calibrate skill", func(t *testing.T) {
		repo := t.TempDir()
		router := filepath.Join(repo, "router")
		other := filepath.Join(repo, "other")
		for _, d := range []string{router, other} {
			if err := os.MkdirAll(filepath.Join(d, ".claude", "skills", "jev-calibrate"), 0o755); err != nil {
				t.Fatal(err)
			}
		}
		if !ShadowsSkill(other, router) {
			t.Error("someone else's skill of the same name")
		}
		if ShadowsSkill(router, router) {
			t.Error("the router's own skill, in its own repository")
		}
		if ShadowsSkill(repo, router) {
			t.Error("no such skill here")
		}
		if ShouldOffer(nil, true, false, true) {
			t.Error("shadowed")
		}
	})

	t.Run("an offer is remembered whatever the answer", func(t *testing.T) {
		file := filepath.Join(t.TempDir(), "nested", "first-run.json")
		if WasOffered(file) {
			t.Fatal("not offered yet")
		}
		MarkOffered(false, file)
		if !WasOffered(file) {
			t.Fatal("a no is remembered too, so it is never asked again")
		}
	})

	ask := func(input io.Reader) Answer {
		done := make(chan Answer, 1)
		go func() { done <- AskYesNo("? ", input, io.Discard, nil) }()
		select {
		case a := <-done:
			return a
		case <-time.After(5 * time.Second):
			t.Fatal("askYesNo hung")
			return None
		}
	}

	t.Run("a closed or failing input settles with no answer instead of hanging", func(t *testing.T) {
		closedR, closedW := io.Pipe()
		closedW.Close()
		if ask(closedR) != None {
			t.Error("closed input")
		}
		brokenR, brokenW := io.Pipe()
		brokenW.CloseWithError(errors.New("EIO"))
		if ask(brokenR) != None {
			t.Error("failing input")
		}
	})

	t.Run("an empty answer or yes accepts, and no declines", func(t *testing.T) {
		for text, want := range map[string]Answer{"\n": Yes, "y\n": Yes, "Yes\n": Yes, "n\n": No, "NO\n": No} {
			if got := ask(strings.NewReader(text)); got != want {
				t.Errorf("%q = %v, want %v", text, got, want)
			}
		}
	})
}

func TestCtrlCIsAnInterrupt(t *testing.T) {
	r, w := io.Pipe()
	defer w.Close()
	sig := make(chan os.Signal, 1)
	sig <- os.Interrupt
	if AskYesNo("? ", r, io.Discard, sig) != Interrupt {
		t.Fatal("Ctrl+C at the prompt stops the launch")
	}
}

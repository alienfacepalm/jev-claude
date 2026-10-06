package settings

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
)

// Ported from node/test/settings.test.mjs.

func fileWith(t *testing.T, settings *jsjson.Object) string {
	file := filepath.Join(t.TempDir(), "settings.json")
	if err := os.WriteFile(file, []byte(jsjson.Indent(settings)), 0o644); err != nil {
		t.Fatal(err)
	}
	return file
}

func read(t *testing.T, file string) *jsjson.Object {
	data, err := os.ReadFile(file)
	if err != nil {
		t.Fatal(err)
	}
	v, err := jsjson.ParseBytes(data)
	if err != nil {
		t.Fatal(err)
	}
	return v.(*jsjson.Object)
}

func memoFile(t *testing.T) string { return filepath.Join(t.TempDir(), "saved-model.json") }

func TestSettings(t *testing.T) {
	t.Run("reads the saved model, ignoring a leftover sentinel", func(t *testing.T) {
		if got := ReadSavedModel(fileWith(t, jsjson.Obj("model", "opus")), memoFile(t)); got != "opus" {
			t.Errorf("got %v", got)
		}
		if got := ReadSavedModel(fileWith(t, jsjson.Obj("model", "jev-router")), memoFile(t)); got != jsjson.Undefined {
			t.Errorf("got %v", got)
		}
		if got := ReadSavedModel(fileWith(t, jsjson.NewObject()), memoFile(t)); got != jsjson.Undefined {
			t.Errorf("got %v", got)
		}
		if got := ReadSavedModel(filepath.Join(t.TempDir(), "does-not-exist.json"), memoFile(t)); got != jsjson.Undefined {
			t.Errorf("got %v", got)
		}
	})

	t.Run("a sentinel left by a killed session resolves to the model from before it", func(t *testing.T) {
		memo := memoFile(t)
		if got := ReadSavedModel(fileWith(t, jsjson.Obj("model", "claude-opus-4-6")), memo); got != "claude-opus-4-6" {
			t.Fatalf("got %v", got)
		}
		// That session died with the sentinel saved; the next run must not treat it as "no model".
		file := fileWith(t, jsjson.Obj("model", "jev-router"))
		previous := ReadSavedModel(file, memo)
		if previous != "claude-opus-4-6" {
			t.Fatalf("previous = %v", previous)
		}
		if !RestoreSavedModel(previous, file) || read(t, file).Value("model") != "claude-opus-4-6" {
			t.Fatal("not restored")
		}
	})

	t.Run("restores the previous model when the sentinel was saved", func(t *testing.T) {
		file := fileWith(t, jsjson.Obj("model", "jev-router", "permissions", jsjson.Obj("deny", []any{"Bash(rm*)"})))
		if !RestoreSavedModel("opus", file) {
			t.Fatal("not restored")
		}
		got := read(t, file)
		if got.Value("model") != "opus" || jsjson.Stringify(got.Value("permissions")) != `{"deny":["Bash(rm*)"]}` {
			t.Fatalf("got %s", jsjson.Stringify(got))
		}
	})

	t.Run("removes the sentinel when there was no previous model", func(t *testing.T) {
		file := fileWith(t, jsjson.Obj("model", "jev-router"))
		if !RestoreSavedModel(jsjson.Undefined, file) || read(t, file).Has("model") {
			t.Fatal("sentinel kept")
		}
	})

	t.Run("leaves a real model the user chose during the session alone", func(t *testing.T) {
		file := fileWith(t, jsjson.Obj("model", "claude-opus-4-6"))
		if RestoreSavedModel("sonnet", file) || read(t, file).Value("model") != "claude-opus-4-6" {
			t.Fatal("overwrote the user's model")
		}
	})

	t.Run("a missing or unreadable settings file is not an error", func(t *testing.T) {
		if RestoreSavedModel("opus", filepath.Join(t.TempDir(), "nope", "settings.json")) {
			t.Fatal("reported a write")
		}
	})
}

// Package settings keeps the user's saved model across a session (SPEC 9.2), ported from
// node/src/settings.mjs.
package settings

import (
	"os"
	"path/filepath"

	"github.com/alienfacepalm/jev-claude/go/internal/config"
	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/osdirs"
	"github.com/alienfacepalm/jev-claude/go/internal/status"
)

// UserSettings is Claude Code's user settings file, from the home directory at start.
var UserSettings = filepath.Join(osdirs.Home(), ".claude", "settings.json")

// SavedModelMemo is where the model from before the last session is remembered.
func SavedModelMemo() string { return filepath.Join(status.Dir, "saved-model.json") }

// readModel is `JSON.parse(readFileSync(file)).model`; ok is false where that throws.
func readModel(file string) (any, bool) {
	data, err := os.ReadFile(file)
	if err != nil {
		return nil, false
	}
	v, err := jsjson.ParseBytes(data)
	if err != nil || v == nil {
		return nil, false
	}
	return jsjson.Prop(v, "model"), true
}

// ReadSavedModel is the model saved as the user's default (Undefined when there is none); a
// leftover sentinel resolves to the remembered model. memo "" means the default memo.
func ReadSavedModel(file, memo string) any {
	if memo == "" {
		memo = SavedModelMemo()
	}
	model, ok := readModel(file)
	if !ok {
		return jsjson.Undefined
	}
	if model == config.AutoModel {
		m, ok := readModel(memo)
		if !ok {
			return jsjson.Undefined
		}
		return m
	}
	if memo == SavedModelMemo() {
		if status.EnsureDir() != nil {
			return model
		}
	}
	_ = os.WriteFile(memo, []byte(jsjson.Stringify(jsjson.Obj("model", jsjson.Coalesce(model, nil)))), 0o600)
	return model
}

// RestoreSavedModel puts previous back when the settings file holds the sentinel, and
// reports whether it wrote.
func RestoreSavedModel(previous any, file string) bool {
	data, err := os.ReadFile(file)
	if err != nil {
		return false
	}
	v, err := jsjson.ParseBytes(data)
	if err != nil || v == nil {
		return false
	}
	settings, ok := v.(*jsjson.Object)
	if !ok || settings.Value("model") != config.AutoModel {
		return false
	}
	if jsjson.IsNullish(previous) {
		settings.Delete("model")
	} else {
		settings.Set("model", previous)
	}
	return os.WriteFile(file, []byte(jsjson.Indent(settings)+"\n"), 0o666) == nil
}

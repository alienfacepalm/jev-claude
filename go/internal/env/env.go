// Package env loads jev's settings from env files (SPEC 9.1), ported from node/src/env.mjs.
// ParseContent is Node's util.parseEnv (Dotenv::ParseContent in Node 24.21.0, vendored at
// conformance/reference/node_dotenv_parse_content.cc), ported line for line over bytes.
package env

import (
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"

	"github.com/alienfacepalm/jev-claude/go/internal/jsjson"
	"github.com/alienfacepalm/jev-claude/go/internal/jsstr"
)

// Pair is one parsed key and value.
type Pair struct{ Key, Value string }

// trimSpaces is the parser's own trim: only space, tab and newline.
func trimSpaces(s string) string {
	if s == "" {
		return ""
	}
	space := func(c byte) bool { return c == ' ' || c == '\t' || c == '\n' }
	start := 0
	for start < len(s) && space(s[start]) {
		start++
	}
	if start == len(s) {
		return ""
	}
	end := len(s)
	for end > start && space(s[end-1]) {
		end--
	}
	return s[start:end]
}

// ParseContent parses env-file text, returning the pairs sorted by key bytes (the parser's
// store is a std::map, so a repeated key keeps its last value).
func ParseContent(input string) []Pair {
	store := map[string]string{}
	lines := strings.ReplaceAll(input, "\r", "")
	content := trimSpaces(lines)

	for content != "" {
		// Skip empty lines and comments.
		if content[0] == '\n' || content[0] == '#' {
			if newline := strings.IndexByte(content, '\n'); newline >= 0 {
				content = content[newline+1:]
			} else {
				content = ""
			}
			continue
		}

		equalOrNewline := strings.IndexAny(content, "=\n")
		if equalOrNewline < 0 || content[equalOrNewline] == '\n' {
			if equalOrNewline >= 0 {
				content = content[equalOrNewline+1:]
				content = trimSpaces(content)
				continue
			}
			break
		}

		key := content[:equalOrNewline]
		content = content[equalOrNewline+1:]
		key = trimSpaces(key)

		// KEY= with nothing after it.
		if content == "" || content[0] == '\n' {
			store[key] = ""
			continue
		}

		content = trimSpaces(content)

		if key == "" {
			continue
		}

		if strings.HasPrefix(key, "export ") {
			key = key[7:]
			key = trimSpaces(key)
		}

		if content == "" {
			store[key] = ""
			break
		}

		// Expand \n inside double quotes.
		if content[0] == '"' {
			if closing := strings.IndexByte(content[1:], content[0]); closing >= 0 {
				closing++
				value := content[1:closing]
				store[key] = strings.ReplaceAll(value, `\n`, "\n")
				if newline := strings.IndexByte(content[closing+1:], '\n'); newline >= 0 {
					content = content[closing+1+newline+1:]
				} else {
					content = ""
				}
				continue
			}
		}

		if content[0] == '\'' || content[0] == '"' || content[0] == '`' {
			closing := strings.IndexByte(content[1:], content[0])
			if closing >= 0 {
				closing++
				store[key] = content[1:closing]
				if newline := strings.IndexByte(content[closing+1:], '\n'); newline >= 0 {
					content = content[closing+1+newline+1:]
				} else {
					content = ""
				}
				continue
			}
			// No closing quote: take the rest of the line (quote included).
			newline := strings.IndexByte(content, '\n')
			if newline < 0 {
				store[key] = content
				break
			}
			store[key] = content[:newline]
			content = content[newline+1:]
		} else {
			if newline := strings.IndexByte(content, '\n'); newline >= 0 {
				value := content[:newline]
				if hash := strings.IndexByte(value, '#'); hash >= 0 {
					value = value[:hash]
				}
				store[key] = trimSpaces(value)
				content = content[newline+1:]
			} else {
				value := content
				if hash := strings.IndexByte(value, '#'); hash >= 0 {
					value = content[:hash]
				}
				store[key] = trimSpaces(value)
				content = ""
			}
		}

		content = trimSpaces(content)
	}

	out := make([]Pair, 0, len(store))
	for k, v := range store {
		out = append(out, Pair{k, v})
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Key < out[j].Key })
	return out
}

// ParseFile reads and parses one env file; a missing or unreadable file is empty.
func ParseFile(file string) []Pair {
	data, err := os.ReadFile(file)
	if err != nil {
		return nil
	}
	return ParseContent(jsstr.DecodeBytes(data))
}

// inJSOrder orders pairs as Object.entries would: index-like keys first, numerically.
func inJSOrder(pairs []Pair) []Pair {
	o := jsjson.NewObject()
	for _, p := range pairs {
		o.Set(p.Key, p.Value)
	}
	out := make([]Pair, 0, len(pairs))
	for _, k := range o.Keys() {
		out = append(out, Pair{k, o.Value(k).(string)})
	}
	return out
}

var projectKeys = map[string]bool{
	"JEV_API_KEY": true, "TYPESAFE_API_KEY": true, "JEV_DEBUG": true,
	"JEV_ALLOW_FABLE": true, "JEV_NO_STATUSLINE": true, "JEV_ICONS": true,
}

var effortKey = regexp.MustCompile(`^JEV_(?:[A-Z]+_)?(?:FORCE_)?EFFORT$`)

// IsProjectKey reports whether a project's own .env may set key.
func IsProjectKey(key string) bool { return projectKeys[key] || effortKey.MatchString(key) }

// PrivateKeys are removed from the environment handed to Claude Code.
var PrivateKeys = []string{"JEV_API_KEY", "TYPESAFE_API_KEY"}

// Store is an environment loadEnv can read and fill.
type Store interface {
	Lookup(key string) (string, bool)
	Set(key, value string)
}

// Process is the live process environment.
type Process struct{}

// Lookup reads a process variable.
func (Process) Lookup(key string) (string, bool) { return os.LookupEnv(key) }

// Set sets a process variable.
func (Process) Set(key, value string) { os.Setenv(key, value) }

// Map is an environment held in a map.
type Map map[string]string

// Lookup reads a key.
func (m Map) Lookup(key string) (string, bool) { v, ok := m[key]; return v, ok }

// Set sets a key.
func (m Map) Set(key, value string) { m[key] = value }

// Load fills env from <cwd>/.env (allow-listed keys only), <home>/.jev-router.env and
// <home>/.jev-claude.env. An existing variable beats every file and earlier files win; an
// empty value never sets anything.
func Load(cwd, home string, env Store) {
	var all []Pair
	for _, p := range inJSOrder(ParseFile(filepath.Join(cwd, ".env"))) {
		if IsProjectKey(p.Key) {
			all = append(all, p)
		}
	}
	all = append(all, inJSOrder(ParseFile(filepath.Join(home, ".jev-router.env")))...)
	all = append(all, inJSOrder(ParseFile(filepath.Join(home, ".jev-claude.env")))...)
	for _, p := range all {
		if p.Value == "" {
			continue
		}
		if _, ok := env.Lookup(p.Key); !ok {
			env.Set(p.Key, p.Value)
		}
	}
}

// ChildEnv is env ("KEY=value" entries) without the Jev keys.
func ChildEnv(env []string) []string {
	out := make([]string, 0, len(env))
	for _, kv := range env {
		key, _, _ := strings.Cut(kv, "=")
		private := false
		for _, p := range PrivateKeys {
			if key == p {
				private = true
			}
		}
		if !private {
			out = append(out, kv)
		}
	}
	return out
}

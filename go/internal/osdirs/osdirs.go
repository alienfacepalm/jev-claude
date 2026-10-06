// Package osdirs resolves the home and temp directories by Node's rules (SPEC 3.8), which the
// Go standard library's helpers do not follow.
package osdirs

import (
	"os"
	"os/user"
	"runtime"
	"strings"
)

// Lookup reads an environment variable.
type Lookup func(string) (string, bool)

func get(env Lookup, key string) string {
	v, _ := env(key)
	return v
}

// Home is os.homedir().
func Home() string { return HomeFrom(os.LookupEnv, runtime.GOOS) }

// HomeFrom is os.homedir() for a given environment and platform.
func HomeFrom(env Lookup, goos string) string {
	key := "HOME"
	if goos == "windows" {
		key = "USERPROFILE"
	}
	if v := get(env, key); v != "" {
		return v
	}
	if u, err := user.Current(); err == nil {
		return u.HomeDir
	}
	return ""
}

// Temp is os.tmpdir().
func Temp() string { return TempFrom(os.LookupEnv, runtime.GOOS) }

// TempFrom is os.tmpdir() for a given environment and platform.
func TempFrom(env Lookup, goos string) string {
	if goos == "windows" {
		path := ""
		for _, key := range []string{"TEMP", "TMP"} {
			if path = get(env, key); path != "" {
				break
			}
		}
		if path == "" {
			root := get(env, "SystemRoot")
			if root == "" {
				root = get(env, "windir")
			}
			if root == "" {
				root = "undefined" // Node concatenates the undefined value (SPEC 3.8)
			}
			path = root + `\temp`
		}
		if len(path) > 1 && strings.HasSuffix(path, `\`) && !strings.HasSuffix(path, `:\`) {
			path = path[:len(path)-1]
		}
		return path
	}
	path := "/tmp"
	for _, key := range []string{"TMPDIR", "TMP", "TEMP"} {
		if v := get(env, key); v != "" {
			path = v
			break
		}
	}
	if len(path) > 1 && strings.HasSuffix(path, "/") {
		path = path[:len(path)-1]
	}
	return path
}

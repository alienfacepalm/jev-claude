package osdirs

import "testing"

func envOf(m map[string]string) Lookup {
	return func(k string) (string, bool) { v, ok := m[k]; return v, ok }
}

func TestTempFollowsNodeOnWindows(t *testing.T) {
	cases := []struct {
		env  map[string]string
		want string
	}{
		{map[string]string{"TEMP": `C:\Users\me\Temp\`, "TMP": `D:\x`}, `C:\Users\me\Temp`},
		{map[string]string{"TEMP": "", "TMP": `D:\x`}, `D:\x`},
		{map[string]string{"TEMP": `C:\`}, `C:\`},
		{map[string]string{"SystemRoot": `C:\Windows`}, `C:\Windows\temp`},
		{map[string]string{"windir": `C:\WINNT`}, `C:\WINNT\temp`},
		{map[string]string{}, `undefined\temp`},
	}
	for _, c := range cases {
		if got := TempFrom(envOf(c.env), "windows"); got != c.want {
			t.Errorf("TempFrom(%v) = %q want %q", c.env, got, c.want)
		}
	}
}

func TestTempFollowsNodeElsewhere(t *testing.T) {
	cases := []struct {
		env  map[string]string
		want string
	}{
		{map[string]string{}, "/tmp"},
		{map[string]string{"TMPDIR": "/var/folders/x/"}, "/var/folders/x"},
		{map[string]string{"TMPDIR": "", "TMP": "/t"}, "/t"},
		{map[string]string{"TEMP": "/"}, "/"},
	}
	for _, c := range cases {
		if got := TempFrom(envOf(c.env), "linux"); got != c.want {
			t.Errorf("TempFrom(%v) = %q want %q", c.env, got, c.want)
		}
	}
}

func TestHomePrefersTheVariable(t *testing.T) {
	if got := HomeFrom(envOf(map[string]string{"USERPROFILE": `C:\Users\x`, "HOME": "/h"}), "windows"); got != `C:\Users\x` {
		t.Error(got)
	}
	if got := HomeFrom(envOf(map[string]string{"USERPROFILE": `C:\Users\x`, "HOME": "/h"}), "linux"); got != "/h" {
		t.Error(got)
	}
	if got := HomeFrom(envOf(map[string]string{"HOME": ""}), "linux"); got == "" {
		t.Error("an empty HOME falls back to the account's directory")
	}
}

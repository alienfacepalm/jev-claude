// Command jev-proxy-host starts the proxy alone and prints its port (SPEC 16.3).
package main

import (
	"fmt"
	"os"

	"github.com/alienfacepalm/jev-claude/go/internal/env"
	"github.com/alienfacepalm/jev-claude/go/internal/osdirs"
	"github.com/alienfacepalm/jev-claude/go/internal/proxy"
)

func main() {
	cwd, _ := os.Getwd()
	env.Load(cwd, osdirs.Home(), env.Process{})
	p, err := proxy.Start(proxy.Options{UpstreamURL: os.Getenv("ANTHROPIC_BASE_URL")})
	if err != nil {
		fmt.Fprintln(os.Stderr, "[jev] could not start the proxy:", err)
		os.Exit(1)
	}
	fmt.Fprintf(os.Stdout, "PORT=%d\n", p.Port)
	select {}
}

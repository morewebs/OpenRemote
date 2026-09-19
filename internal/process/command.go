// Package process launches executables without routing user arguments through a shell.
package process

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"
)

var npmTarget = regexp.MustCompile(`(?i)%dp0%[\\/](node_modules[\\/][^"\r\n]+\.(?:exe|cjs|mjs|js))"`)

// Resolve unwraps npm's Windows command shims into their real binary or Node
// entry point. This preserves argument boundaries, including spaces and &.
func Resolve(bin string, args []string) (string, []string, error) {
	if runtime.GOOS != "windows" || !strings.EqualFold(filepath.Ext(bin), ".cmd") {
		return bin, args, nil
	}
	data, err := os.ReadFile(bin)
	if err != nil {
		return "", nil, err
	}
	match := npmTarget.FindSubmatch(data)
	if len(match) != 2 {
		return "", nil, fmt.Errorf("unsupported command shim %s; use the executable path", bin)
	}
	target := filepath.Join(filepath.Dir(bin), string(match[1]))
	if _, err := os.Stat(target); err != nil {
		return "", nil, err
	}
	if strings.EqualFold(filepath.Ext(target), ".exe") {
		return target, args, nil
	}
	node, err := exec.LookPath("node")
	if err != nil {
		return "", nil, err
	}
	return node, append([]string{target}, args...), nil
}

func CommandContext(ctx context.Context, bin string, args ...string) (*exec.Cmd, error) {
	bin, args, err := Resolve(bin, args)
	if err != nil {
		return nil, err
	}
	return exec.CommandContext(ctx, bin, args...), nil
}

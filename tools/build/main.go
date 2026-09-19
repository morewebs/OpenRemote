// Build creates a daemon containing the Flutter web client. Run from the repo:
// go run ./tools/build -output bin/openremote.exe
package main

import (
	"flag"
	"fmt"
	"io/fs"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
)

func main() {
	if err := build(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func build() error {
	web := flag.String("web-dir", "", "existing Flutter web build; otherwise build the client")
	output := flag.String("output", "bin/openremote", "output executable path")
	version := flag.String("version", "dev", "version embedded in the daemon")
	goos := flag.String("target-os", runtime.GOOS, "target operating system")
	goarch := flag.String("target-arch", runtime.GOARCH, "target CPU architecture")
	flag.Parse()
	if strings.ContainsAny(*version, " \t\r\n\"'") {
		return fmt.Errorf("invalid version")
	}
	root, err := os.Getwd()
	if err != nil {
		return err
	}
	if _, err := os.Stat(filepath.Join(root, "clients", "companion", "pubspec.yaml")); err != nil {
		return fmt.Errorf("run tools/build from the repository root")
	}
	run := func(dir, bin string, args ...string) error {
		cmd := exec.Command(bin, args...)
		cmd.Dir = dir
		cmd.Stdout = os.Stdout
		cmd.Stderr = os.Stderr
		return cmd.Run()
	}
	if *web == "" {
		flutter := "flutter"
		if runtime.GOOS == "windows" {
			flutter = "flutter.bat"
		}
		client := filepath.Join(root, "clients", "companion")
		if err := run(client, flutter, "pub", "get"); err != nil {
			return err
		}
		if err := run(client, flutter, "build", "web", "--release", "--no-wasm-dry-run", "--no-web-resources-cdn", "--base-href", "/"); err != nil {
			return err
		}
		*web = filepath.Join(client, "build", "web")
	}
	source, err := filepath.Abs(*web)
	if err != nil {
		return err
	}
	if _, err := os.Stat(filepath.Join(source, "flutter_bootstrap.js")); err != nil {
		return fmt.Errorf("web-dir must contain a Flutter web build: %w", err)
	}
	target := filepath.Join(root, "internal", "core", "server", "webdist")
	if source == target || strings.HasPrefix(source, target+string(filepath.Separator)) {
		return fmt.Errorf("web-dir must be outside the generated embed directory")
	}
	if err := os.RemoveAll(target); err != nil {
		return err
	}
	if err := filepath.WalkDir(source, func(path string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		rel, err := filepath.Rel(source, path)
		if err != nil {
			return err
		}
		to := filepath.Join(target, rel)
		if entry.IsDir() {
			return os.MkdirAll(to, 0755)
		}
		if !entry.Type().IsRegular() {
			return fmt.Errorf("non-regular web asset: %s", rel)
		}
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		return os.WriteFile(to, data, 0644)
	}); err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(*output), 0755); err != nil {
		return err
	}
	cmd := exec.Command("go", "build", "-tags", "flutterweb", "-trimpath", "-ldflags", "-s -w -X main.Version="+*version, "-o", *output, "./cmd/openremote")
	cmd.Env = append(os.Environ(), "CGO_ENABLED=0", "GOOS="+*goos, "GOARCH="+*goarch)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	return cmd.Run()
}

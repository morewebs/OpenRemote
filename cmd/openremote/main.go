package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"os"
	"os/signal"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
	"time"

	"github.com/morewebs/OpenRemote/internal/core/auth"
	"github.com/morewebs/OpenRemote/internal/core/events"
	"github.com/morewebs/OpenRemote/internal/core/server"
	"github.com/morewebs/OpenRemote/internal/core/supervisor"
	"github.com/morewebs/OpenRemote/internal/core/update"
	"github.com/morewebs/OpenRemote/internal/pty"
)

// Version is stamped at build time via -ldflags "-X main.Version=...".
var Version = "0.1.0"

// defaultAddr is the daemon listen address from the spec (127.0.0.1:4097).
const defaultAddr = "127.0.0.1:4097"

// httpClient bounds CLI probes so `status`/`tunnel` never hang forever.
var httpClient = &http.Client{Timeout: 5 * time.Second}

type stringSlice []string

func (s *stringSlice) String() string {
	return fmt.Sprint(*s)
}

func (s *stringSlice) Set(value string) error {
	*s = append(*s, value)
	return nil
}

func main() {
	if len(os.Args) < 2 {
		runServe(nil)
		return
	}

	cmd := os.Args[1]
	args := os.Args[2:]

	switch cmd {
	case "serve":
		runServe(args)
	case "serve-worker":
		runServeWorker(args)
	case "token":
		runToken(args)
	case "status":
		runStatus(args)
	case "tunnel":
		runTunnel(args)
	case "update":
		runUpdate(args)
	case "pty-worker":
		runPTYWorker(args)
	case "version", "--version", "-v":
		fmt.Printf("OpenRemote v%s\n", Version)
	case "help", "--help", "-h":
		printUsage()
	default:
		// If first arg starts with flag (e.g. -addr), treat as serve
		if strings.HasPrefix(cmd, "-") {
			runServe(os.Args[1:])
			return
		}
		fmt.Fprintf(os.Stderr, "Unknown command %q\n\n", cmd)
		printUsage()
		os.Exit(1)
	}
}

func printUsage() {
	fmt.Println("OpenRemote — High-performance remote companion for AI coding assistants")
	fmt.Println("\nUsage:")
	fmt.Println("  openremote [command] [flags]")
	fmt.Println("\nCommands:")
	fmt.Println("  serve       Start the OpenRemote daemon (default when no command given)")
	fmt.Println("  token       View or rotate bearer token")
	fmt.Println("  status      Check daemon health and active sessions")
	fmt.Println("  tunnel      Manage Cloudflare / remote tunnels")
	fmt.Println("  update      Check for and apply daemon updates (verified GitHub releases)")
	fmt.Println("  pty-worker  Run the isolated PTY worker subprocess (spawned by serve)")
	fmt.Println("  version     Show version")
	fmt.Println("  help        Show this help")
	fmt.Println("\nFlags for 'serve':")
	fmt.Println("  -addr string            HTTP/WS address (default \"" + defaultAddr + "\")")
	fmt.Println("  -data string            Data directory (default ~/.openremote/data)")
	fmt.Println("  -token string           Bearer auth token (auto-generated if empty)")
	fmt.Println("  -root string            Allowed root directory (repeatable)")
	fmt.Println("  -telegram-token string  Telegram Bot API token")
	fmt.Println("  -telegram-chat int      Default Telegram Chat ID for notifications")
	fmt.Println("\nRun 'openremote <command> --help' for command-specific flags.")
}

// newFlagSet builds a flag set with a usage banner that names the command and
// describes it before listing its flags. flag.ExitOnError makes `-h`/`--help`
// print this text and exit 0 for every subcommand.
func newFlagSet(name, description string) *flag.FlagSet {
	fs := flag.NewFlagSet(name, flag.ExitOnError)
	fs.Usage = func() {
		out := fs.Output()
		_, _ = fmt.Fprintf(out, "%s — %s\n\n", name, description)
		_, _ = fmt.Fprintf(out, "Usage:\n  openremote %s [flags]\n\nFlags:\n", name)
		fs.PrintDefaults()
	}
	return fs
}

func runServe(args []string) {
	addr := defaultAddr
	for index, arg := range args {
		if (arg == "-addr" || arg == "--addr") && index+1 < len(args) {
			addr = args[index+1]
		}
		if strings.HasPrefix(arg, "-addr=") || strings.HasPrefix(arg, "--addr=") {
			addr = strings.SplitN(arg, "=", 2)[1]
		}
	}
	if host, port, err := net.SplitHostPort(addr); err == nil && (host == "" || host == "0.0.0.0" || host == "::") {
		addr = net.JoinHostPort("127.0.0.1", port)
	}
	bin, err := os.Executable()
	if err != nil {
		log.Fatal(err)
	}
	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()
	if err := supervisor.Supervise(ctx, bin, append([]string{"serve-worker"}, args...), addr, os.Stdout, os.Stderr); err != nil {
		log.Fatal(err)
	}
}

func runServeWorker(args []string) {
	fs := newFlagSet("serve", "start the OpenRemote daemon and serve the companion UI")
	addr := fs.String("addr", defaultAddr, "listen address (host:port)")
	dataDir := fs.String("data", defaultDataDir(), "data directory (events db, token)")
	tokenFlag := fs.String("token", "", "bearer token override (auto-generated if empty)")
	telegramToken := fs.String("telegram-token", os.Getenv("TELEGRAM_BOT_TOKEN"), "telegram bot token")
	telegramChat := fs.Int64("telegram-chat", 0, "default telegram chat id for notifications")
	allowedOrigin := fs.String("origin", "", "additional exact browser origin allowed to access the daemon")
	telegramTopics := fs.Bool("telegram-topics", false, "route Telegram sessions into forum topics")
	updateURL := fs.String("update-url", "", "release feed URL for update checks (empty = project default)")
	updateInterval := fs.Duration("update-interval", 24*time.Hour, "release check interval (0 disables; e.g. 1h, 30m)")
	var telegramUsers stringSlice
	fs.Var(&telegramUsers, "telegram-user", "allowed Telegram user ID (repeat for multiple users; required with a bot token)")

	var roots stringSlice
	fs.Var(&roots, "root", "allowed sandbox root directory (can be repeated)")

	_ = fs.Parse(args)
	var allowedTelegramUsers []int64
	for _, value := range telegramUsers {
		id, err := strconv.ParseInt(value, 10, 64)
		if err != nil || id <= 0 {
			log.Fatalf("[openremote] invalid Telegram user ID %q", value)
		}
		allowedTelegramUsers = append(allowedTelegramUsers, id)
	}
	if *telegramToken != "" && len(allowedTelegramUsers) == 0 {
		log.Fatal("[openremote] --telegram-user is required when Telegram is enabled")
	}

	bus, err := events.Open(*dataDir)
	if err != nil {
		log.Fatalf("[openremote] events.Open: %v", err)
	}
	defer func() { _ = bus.Close() }()

	token := *tokenFlag
	if token == "" {
		token, err = auth.LoadOrCreateToken(*dataDir)
		if err != nil {
			log.Fatalf("[openremote] auth: %v", err)
		}
	}

	if len(roots) == 0 {
		if cwd, err := os.Getwd(); err == nil {
			roots = append(roots, cwd)
		}
	}

	fmt.Printf("[openremote] Version:  %s\n", Version)
	fmt.Printf("[openremote] Token:    %s (stored at %s)\n", mask(token), filepath.Join(*dataDir, "token"))
	fmt.Printf("[openremote] Data Dir: %s\n", *dataDir)
	fmt.Printf("[openremote] UI / Web: http://%s\n", *addr)

	workerBinary, err := os.Executable()
	if err != nil {
		log.Fatalf("[openremote] locate PTY worker executable: %v", err)
	}
	srv := server.New(server.Config{
		WorkerBinary:         workerBinary,
		Addr:                 *addr,
		DataDir:              *dataDir,
		Token:                token,
		AllowedRoots:         roots,
		TelegramToken:        *telegramToken,
		TelegramChatID:       *telegramChat,
		AllowedOrigin:        *allowedOrigin,
		TelegramAllowedUsers: allowedTelegramUsers,
		TelegramTopics:       *telegramTopics,
		Version:              Version,
		UpdateFeedURL:        *updateURL,
		UpdateCheckInterval:  *updateInterval,
		// A self-update ends with a graceful shutdown; exit with the
		// supervisor's restart code so the parent relaunches the new binary.
		RestartHook: func() { os.Exit(supervisor.RestartExitCode) },
	}, bus)

	go func() {
		if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			log.Fatalf("[openremote] listen: %v", err)
		}
	}()

	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)
	defer signal.Stop(quit)
	parentGone := make(chan struct{})
	go func() { _, _ = io.Copy(io.Discard, os.Stdin); close(parentGone) }()
	select {
	case <-quit:
	case <-parentGone:
	}

	fmt.Println("\n[openremote] shutting down...")
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	_ = srv.Shutdown(ctx)
	fmt.Println("[openremote] shutdown complete")
}

func runToken(args []string) {
	fs := newFlagSet("token", "view or rotate the daemon bearer token")
	dataDir := fs.String("data", defaultDataDir(), "data directory holding the token file")
	rotate := fs.Bool("rotate", false, "generate and persist a new bearer token")
	_ = fs.Parse(args)

	tokenPath := auth.TokenPath(*dataDir)
	if *rotate {
		tok, err := auth.GenerateToken()
		if err != nil {
			log.Fatalf("generate token failed: %v", err)
		}
		if err := os.WriteFile(tokenPath, []byte(tok+"\n"), 0o600); err != nil {
			log.Fatalf("write token failed: %v", err)
		}
		fmt.Printf("Rotated Bearer Token: %s\n", tok)
		return
	}

	tok, err := auth.LoadOrCreateToken(*dataDir)
	if err != nil {
		log.Fatalf("load token failed: %v", err)
	}
	fmt.Printf("Bearer Token: %s\nPath: %s\n", tok, tokenPath)
}

func runStatus(args []string) {
	fs := newFlagSet("status", "probe a running daemon via GET /health")
	addr := fs.String("addr", defaultAddr, "daemon address (host:port)")
	_ = fs.Parse(args)

	url := fmt.Sprintf("http://%s/health", *addr)
	resp, err := httpClient.Get(url)
	if err != nil {
		fmt.Printf("Daemon is NOT running at %s (%v)\n", *addr, err)
		os.Exit(1)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		fmt.Printf("Daemon at %s responded with HTTP %d\n", *addr, resp.StatusCode)
		os.Exit(1)
	}

	var health map[string]any
	if err := json.NewDecoder(resp.Body).Decode(&health); err != nil {
		log.Fatalf("parse health: %v", err)
	}

	fmt.Printf("OpenRemote Daemon Status: %v\n", health["status"])
	fmt.Printf("Uptime:   %v seconds\n", health["uptime"])
	fmt.Printf("Sessions: %v active\n", health["sessions"])
	if v, ok := health["version"].(string); ok && v != "" {
		fmt.Printf("Version:  %s\n", v)
	}
}

func runTunnel(args []string) {
	fs := newFlagSet("tunnel", "list, start or stop remote ingress tunnels")
	addr := fs.String("addr", defaultAddr, "daemon address (host:port)")
	dataDir := fs.String("data", defaultDataDir(), "data directory holding the token file")
	start := fs.String("start", "", "start tunnel with the given provider name (e.g. cloudflared)")
	stop := fs.Bool("stop", false, "stop the currently active tunnel")
	_ = fs.Parse(args)

	token, err := auth.LoadOrCreateToken(*dataDir)
	if err != nil {
		log.Fatalf("load token failed: %v", err)
	}

	switch {
	case *stop:
		tunnelRequest(http.MethodPost, *addr, token, map[string]string{"action": "stop"})
	case *start != "":
		tunnelRequest(http.MethodPost, *addr, token, map[string]string{"name": *start, "action": "start"})
	default:
		listTunnels(*addr, token)
	}
}

// tunnelRequest posts a tunnel action to the daemon and reports the outcome.
func tunnelRequest(method, addr, token string, body map[string]string) {
	payload, _ := json.Marshal(body)
	req, err := http.NewRequest(method, fmt.Sprintf("http://%s/api/v1/tunnels", addr), strings.NewReader(string(payload)))
	if err != nil {
		log.Fatalf("build request failed: %v", err)
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")

	resp, err := httpClient.Do(req)
	if err != nil {
		fmt.Printf("Failed to contact daemon: %v\n", err)
		os.Exit(1)
	}
	defer func() { _ = resp.Body.Close() }()

	var out map[string]any
	_ = json.NewDecoder(resp.Body).Decode(&out)
	if resp.StatusCode >= 400 {
		msg, _ := out["error"].(string)
		fmt.Printf("Tunnel request failed (HTTP %d): %s\n", resp.StatusCode, msg)
		os.Exit(1)
	}
	if u, ok := out["url"].(string); ok && u != "" {
		fmt.Printf("Tunnel %s started: %s\n", body["name"], u)
		return
	}
	fmt.Printf("Tunnel request accepted: %v\n", out["ok"])
}

// listTunnels prints the daemon's tunnel provider inventory.
func listTunnels(addr, token string) {
	req, err := http.NewRequest(http.MethodGet, fmt.Sprintf("http://%s/api/v1/tunnels", addr), nil)
	if err != nil {
		log.Fatalf("build request failed: %v", err)
	}
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := httpClient.Do(req)
	if err != nil {
		fmt.Printf("Failed to contact daemon: %v\n", err)
		os.Exit(1)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusOK {
		fmt.Printf("Daemon at %s responded with HTTP %d\n", addr, resp.StatusCode)
		os.Exit(1)
	}

	var tunnels []map[string]any
	if err := json.NewDecoder(resp.Body).Decode(&tunnels); err != nil {
		log.Fatalf("parse tunnels: %v", err)
	}
	if len(tunnels) == 0 {
		fmt.Println("No tunnel providers reported by daemon.")
		return
	}
	fmt.Println("Available Tunnels:")
	for _, t := range tunnels {
		fmt.Printf("- %v | installed: %v | running: %v | url: %v\n", t["name"], t["installed"], t["running"], t["url"])
	}
}

// runUpdate checks for and applies daemon updates. With a running daemon it
// drives the REST API (the daemon downloads, verifies, swaps and restarts
// itself via the supervisor handshake); standalone (daemon not running) it
// swaps the binary in place for the next `openremote serve`.
func runUpdate(args []string) {
	fs := newFlagSet("update", "check for and apply daemon updates from verified GitHub releases")
	addr := fs.String("addr", defaultAddr, "daemon address (host:port)")
	dataDir := fs.String("data", defaultDataDir(), "data directory holding the token file")
	check := fs.Bool("check", false, "only report the latest release; do not apply")
	yes := fs.Bool("y", false, "apply without asking for confirmation")
	feed := fs.String("url", "", "release feed URL (standalone mode only; empty = project default)")
	_ = fs.Parse(args)

	if daemonUp(*addr) {
		updateViaDaemon(*addr, *dataDir, *check, *yes)
		return
	}
	updateStandalone(*feed, *check, *yes)
}

// daemonUp reports whether a daemon answers /health at addr.
func daemonUp(addr string) bool {
	res, err := httpClient.Get(fmt.Sprintf("http://%s/health", addr))
	if err != nil {
		return false
	}
	_ = res.Body.Close()
	return res.StatusCode == http.StatusOK
}

// updateViaDaemon prints the daemon's update status and, unless -check, asks
// the daemon to apply it, then polls health until the restarted daemon answers.
func updateViaDaemon(addr, dataDir string, check, yes bool) {
	token, err := auth.LoadOrCreateToken(dataDir)
	if err != nil {
		log.Fatalf("load token failed: %v", err)
	}
	status := updateRequest("GET", addr, token, nil)
	current, _ := status["current"].(string)
	latest, _ := status["latest"].(string)
	available, _ := status["available"].(bool)
	fmt.Printf("Daemon at %s: current %s, latest %s\n", addr, current, latest)
	switch {
	case available:
		fmt.Println("Update available.")
	case status["checkedAt"] == "" || status["checkedAt"] == "0001-01-01T00:00:00Z":
		fmt.Println("No release check has run yet (dev build or checks disabled).")
	default:
		fmt.Println("Already up to date.")
	}
	if check || !available {
		return
	}
	if !yes {
		fmt.Print("Applying restarts the daemon (live sessions stop; transcripts survive).\nContinue? [y/N] ")
		var answer string
		_, _ = fmt.Scanln(&answer)
		if !strings.EqualFold(answer, "y") && !strings.EqualFold(answer, "yes") {
			fmt.Println("Aborted.")
			return
		}
	}
	fmt.Println("Applying update...")
	updateRequest("POST", addr, token, nil)

	// The daemon shuts down and its supervisor relaunches the new binary; poll
	// until health answers again with a new version.
	deadline := time.Now().Add(90 * time.Second)
	client := &http.Client{Timeout: 3 * time.Second}
	for time.Now().Before(deadline) {
		time.Sleep(2 * time.Second)
		res, err := client.Get(fmt.Sprintf("http://%s/health", addr))
		if err != nil {
			continue
		}
		var health map[string]any
		_ = json.NewDecoder(res.Body).Decode(&health)
		_ = res.Body.Close()
		if v, _ := health["version"].(string); v != "" && v != current {
			fmt.Printf("Update applied: daemon now running %s.\n", v)
			return
		}
	}
	fmt.Println("Daemon did not report a new version in time; check `openremote status`.")
}

// updateStandalone swaps this binary in place without a running daemon.
func updateStandalone(feed string, check, yes bool) {
	bin, err := os.Executable()
	if err != nil {
		log.Fatalf("locate executable: %v", err)
	}
	mgr := update.New(Version, bin, feed)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Minute)
	defer cancel()
	if err := mgr.Check(ctx); err != nil {
		log.Fatalf("update check failed: %v", err)
	}
	st := mgr.Status()
	fmt.Printf("Standalone: current %s, latest %s\n", st.Current, st.Latest)
	if !st.Available {
		fmt.Println("Already up to date.")
		return
	}
	if check {
		return
	}
	if !yes {
		fmt.Print("Replace the binary at this path? [y/N] ")
		var answer string
		_, _ = fmt.Scanln(&answer)
		if !strings.EqualFold(answer, "y") && !strings.EqualFold(answer, "yes") {
			fmt.Println("Aborted.")
			return
		}
	}
	if err := mgr.Apply(ctx); err != nil {
		log.Fatalf("apply update: %v", err)
	}
	fmt.Printf("Update applied: %s. Restart the daemon with `openremote serve`.\n", st.Latest)
	fmt.Printf("Previous binary kept at %s.old for rollback.\n", bin)
}

// updateRequest performs a GET/POST against the daemon's update endpoint.
func updateRequest(method, addr, token string, body map[string]string) map[string]any {
	var reader io.Reader
	if body != nil {
		payload, _ := json.Marshal(body)
		reader = strings.NewReader(string(payload))
	}
	req, err := http.NewRequest(method, fmt.Sprintf("http://%s/api/v1/update", addr), reader)
	if err != nil {
		log.Fatalf("build request failed: %v", err)
	}
	req.Header.Set("Authorization", "Bearer "+token)
	res, err := httpClient.Do(req)
	if err != nil {
		fmt.Printf("Failed to contact daemon: %v\n", err)
		os.Exit(1)
	}
	defer func() { _ = res.Body.Close() }()
	var out map[string]any
	_ = json.NewDecoder(res.Body).Decode(&out)
	if res.StatusCode >= 400 {
		fmt.Printf("Update request failed (HTTP %d): %v\n", res.StatusCode, out["applyError"])
		os.Exit(1)
	}
	return out
}

func defaultDataDir() string {
	if home, err := os.UserHomeDir(); err == nil {
		return filepath.Join(home, ".openremote", "data")
	}
	return "./data"
}

func mask(tok string) string {
	if len(tok) < 8 {
		return "***"
	}
	return tok[:4] + "..." + tok[len(tok)-4:]
}

// runPTYWorker executes the isolated PTY worker loop. The worker speaks the
// JSON-lines IPC protocol from spec 02 §2 over stdin/stdout and exits when its
// parent (the daemon) closes the pipe or a termination signal arrives.
func runPTYWorker(args []string) {
	fs := newFlagSet("pty-worker", "run the isolated PTY worker subprocess (JSON-lines IPC over stdio)")
	_ = fs.Parse(args)

	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()

	worker := pty.NewWorker()
	if err := worker.Run(ctx); err != nil {
		log.Fatalf("[pty-worker] worker stopped: %v", err)
	}
}

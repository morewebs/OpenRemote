package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"log"
	"net/http"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"
	"time"

	"github.com/morewebs/OpenRemote/internal/core/auth"
	"github.com/morewebs/OpenRemote/internal/core/events"
	"github.com/morewebs/OpenRemote/internal/core/server"
)

// Version is stamped at build time via -ldflags "-X main.Version=...".
var Version = "0.1.0"

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
		runServe(os.Args[1:])
		return
	}

	cmd := os.Args[1]
	args := os.Args[2:]

	switch cmd {
	case "serve":
		runServe(args)
	case "token":
		runToken(args)
	case "status":
		runStatus(args)
	case "tunnel":
		runTunnel(args)
	case "version", "--version", "-v":
		fmt.Printf("OpenRemote v%s\n", Version)
	case "pty-worker":
		runPTYWorker()
	case "help", "--help", "-h":
		printUsage()
	default:
		// If first arg starts with flag (e.g. -addr), treat as serve
		if len(cmd) > 0 && cmd[0] == '-' {
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
	fmt.Println("  serve       Start the OpenRemote daemon (default)")
	fmt.Println("  token       View or rotate bearer token")
	fmt.Println("  status      Check daemon health and active sessions")
	fmt.Println("  tunnel      Manage Cloudflare / remote tunnels")
	fmt.Println("  version     Show version")
	fmt.Println("\nFlags for 'serve':")
	fmt.Println("  -addr string            HTTP/WS address (default \"127.0.0.1:4097\")")
	fmt.Println("  -data string            Data directory (default ~/.openremote/data)")
	fmt.Println("  -token string           Bearer auth token (auto-generated if empty)")
	fmt.Println("  -root string            Allowed root directory (repeatable)")
	fmt.Println("  -telegram-token string  Telegram Bot API token")
	fmt.Println("  -telegram-chat int      Default Telegram Chat ID for notifications")
}

func runServe(args []string) {
	fs := flag.NewFlagSet("serve", flag.ExitOnError)
	addr := fs.String("addr", "127.0.0.1:4097", "listen address")
	dataDir := fs.String("data", defaultDataDir(), "data directory")
	tokenFlag := fs.String("token", "", "bearer token override")
	telegramToken := fs.String("telegram-token", os.Getenv("TELEGRAM_BOT_TOKEN"), "telegram bot token")
	telegramChat := fs.Int64("telegram-chat", 0, "telegram chat id")

	var roots stringSlice
	fs.Var(&roots, "root", "allowed sandbox root directory (can be repeated)")

	_ = fs.Parse(args)

	bus, err := events.Open(*dataDir)
	if err != nil {
		log.Fatalf("[openremote] events.Open: %v", err)
	}
	defer bus.Close()

	token := *tokenFlag
	if token == "" {
		token, err = auth.LoadOrCreateToken(*dataDir)
		if err != nil {
			log.Fatalf("[openremote] auth: %v", err)
		}
	}

	if len(roots) == 0 {
		if home, err := os.UserHomeDir(); err == nil {
			roots = append(roots, home)
		}
		if cwd, err := os.Getwd(); err == nil {
			roots = append(roots, cwd)
		}
	}

	fmt.Printf("[openremote] Version:  %s\n", Version)
	fmt.Printf("[openremote] Token:    %s (stored at %s)\n", mask(token), filepath.Join(*dataDir, "token"))
	fmt.Printf("[openremote] Data Dir: %s\n", *dataDir)
	fmt.Printf("[openremote] UI / Web: http://%s\n", *addr)

	srv := server.New(server.Config{
		Addr:           *addr,
		DataDir:        *dataDir,
		Token:          token,
		AllowedRoots:   roots,
		TelegramToken:  *telegramToken,
		TelegramChatID: *telegramChat,
	}, bus)

	go func() {
		if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			log.Fatalf("[openremote] listen: %v", err)
		}
	}()

	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)
	<-quit

	fmt.Println("\n[openremote] shutting down...")
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	_ = srv.Shutdown(ctx)
	fmt.Println("[openremote] shutdown complete")
}

func runToken(args []string) {
	fs := flag.NewFlagSet("token", flag.ExitOnError)
	dataDir := fs.String("data", defaultDataDir(), "data directory")
	rotate := fs.Bool("rotate", false, "generate a new bearer token")
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
	fs := flag.NewFlagSet("status", flag.ExitOnError)
	addr := fs.String("addr", "127.0.0.1:4097", "daemon address")
	_ = fs.Parse(args)

	resp, err := http.Get(fmt.Sprintf("http://%s/health", *addr))
	if err != nil {
		fmt.Printf("Daemon is NOT running at %s (%v)\n", *addr, err)
		os.Exit(1)
	}
	defer resp.Body.Close()

	var health map[string]any
	if err := json.NewDecoder(resp.Body).Decode(&health); err != nil {
		log.Fatalf("parse health: %v", err)
	}

	fmt.Printf("OpenRemote Daemon Status: %v\n", health["status"])
	fmt.Printf("Uptime:   %v seconds\n", health["uptime"])
	fmt.Printf("Sessions: %v active\n", health["sessions"])
}

func runTunnel(args []string) {
	fs := flag.NewFlagSet("tunnel", flag.ExitOnError)
	addr := fs.String("addr", "127.0.0.1:4097", "daemon address")
	dataDir := fs.String("data", defaultDataDir(), "data directory")
	_ = fs.Parse(args)

	token, _ := auth.LoadOrCreateToken(*dataDir)
	req, _ := http.NewRequest(http.MethodGet, fmt.Sprintf("http://%s/api/v1/tunnels", *addr), nil)
	req.Header.Set("Authorization", "Bearer "+token)

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		fmt.Printf("Failed to contact daemon: %v\n", err)
		return
	}
	defer resp.Body.Close()

	var tunnels []map[string]any
	_ = json.NewDecoder(resp.Body).Decode(&tunnels)
	fmt.Println("Available Tunnels:")
	for _, t := range tunnels {
		fmt.Printf("• %v — Installed: %v, Running: %v, URL: %v\n", t["name"], t["installed"], t["running"], t["url"])
	}
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

func runPTYWorker() {
	fmt.Println("[pty-worker] isolated worker process mode active")
	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)
	<-quit
}

package main

import (
	"context"
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

func main() {
	var (
		addr    = flag.String("addr", "127.0.0.1:4097", "listen address")
		dataDir = flag.String("data", defaultDataDir(), "data directory")
	)
	flag.Parse()

	if len(flag.Args()) > 0 && flag.Arg(0) == "pty-worker" {
		runPTYWorker()
		return
	}

	bus, err := events.Open(*dataDir)
	if err != nil {
		log.Fatalf("events.Open: %v", err)
	}
	defer bus.Close()

	token, err := auth.LoadOrCreateToken(*dataDir)
	if err != nil {
		log.Fatalf("auth: %v", err)
	}
	fmt.Printf("[openremote] token: %s  (stored at %s)\n", mask(token), filepath.Join(*dataDir, "token"))
	fmt.Printf("[openremote] data:  %s\n", *dataDir)

	srv := server.New(server.Config{Addr: *addr, DataDir: *dataDir, Token: token}, bus)

	// Also expose unauthenticated health on same mux via auth middleware exception
	_ = http.DefaultServeMux

	go func() {
		if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			log.Fatalf("listen: %v", err)
		}
	}()

	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)
	<-quit
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	_ = srv.Shutdown(ctx)
	log.Println("[openremote] shutdown complete")
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
	// Child isolated PTY worker — JSON-lines over stdin/stdout
	// Mirrors Node's ConPTY crash isolation (spec 02 §2)
	fmt.Fprintln(os.Stderr, "[pty-worker] isolated worker mode — not yet wired as subprocess; running inline")
	select {}
}

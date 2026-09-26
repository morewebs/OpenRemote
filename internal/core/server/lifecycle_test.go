package server_test

import (
	"context"
	"os"
	"testing"
	"time"

	"github.com/morewebs/OpenRemote/internal/core/events"
	"github.com/morewebs/OpenRemote/internal/core/server"
	"github.com/morewebs/OpenRemote/internal/pty"
)

func TestMain(m *testing.M) {
	if len(os.Args) > 1 && os.Args[1] == "pty-worker" {
		if err := pty.NewWorker().Run(context.Background()); err != nil {
			os.Exit(1)
		}
		os.Exit(0)
	}
	os.Exit(m.Run())
}

func newTestServer(tb testing.TB, cfg server.Config, bus *events.Bus) *server.Server {
	tb.Helper()
	bin, err := os.Executable()
	if err != nil {
		tb.Fatal(err)
	}
	cfg.WorkerBinary = bin
	srv := server.New(cfg, bus)
	tb.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = srv.Shutdown(ctx)
	})
	return srv
}

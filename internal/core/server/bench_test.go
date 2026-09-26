package server_test

import (
	"bufio"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/morewebs/OpenRemote/internal/core/chat"
	"github.com/morewebs/OpenRemote/internal/core/events"
	"github.com/morewebs/OpenRemote/internal/core/server"
	"github.com/morewebs/OpenRemote/internal/protocol"
)

// BenchmarkSSEEventDelivery measures the full ingest-to-subscriber path a
// client sees: sink message -> SQLite event append -> hub wakeup -> SSE
// drain (read + JSON + write + flush) -> subscriber receive. The original
// design target is sub-5-ms delivery.
func BenchmarkSSEEventDelivery(b *testing.B) {
	tempDir := b.TempDir()
	bus, err := events.Open(tempDir)
	if err != nil {
		b.Fatal(err)
	}
	b.Cleanup(func() { _ = bus.Close() })

	cwd, _ := os.Getwd()
	srv := newTestServer(b, server.Config{
		Addr:         "127.0.0.1:0",
		DataDir:      tempDir,
		Token:        "bench-token",
		AllowedRoots: []string{cwd, tempDir},
	}, bus)
	ts := httptest.NewServer(srv.Handler())
	b.Cleanup(ts.Close)

	const sessionID = "bench-session"
	srv.TestInjectSession(sessionID, nil)
	sink := srv.TestSink(sessionID)

	req, _ := http.NewRequest(http.MethodGet, ts.URL+"/events?sessionId="+sessionID, nil)
	req.Header.Set("Authorization", "Bearer bench-token")
	res, err := http.DefaultClient.Do(req)
	if err != nil {
		b.Fatalf("SSE stream request failed: %v", err)
	}
	b.Cleanup(func() { _ = res.Body.Close() })

	delivered := make(chan int64, 1024)
	go func() {
		scanner := bufio.NewScanner(res.Body)
		scanner.Buffer(make([]byte, 64*1024), 1024*1024)
		for scanner.Scan() {
			line := scanner.Text()
			if strings.HasPrefix(line, "id: ") {
				if seq, err := strconv.ParseInt(line[4:], 10, 64); err == nil {
					delivered <- seq
				}
			}
		}
	}()

	// Warm the pipe and confirm delivery before timing.
	sink.Message(benchMessage(sessionID, "warmup", 0))
	select {
	case <-delivered:
	case <-time.After(10 * time.Second):
		b.Fatal("warmup event never arrived over SSE")
	}

	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		sink.Message(benchMessage(sessionID, fmt.Sprintf("bench-%d", i), i+1))
		select {
		case <-delivered:
		case <-time.After(10 * time.Second):
			b.Fatalf("event %d never arrived over SSE", i)
		}
	}
	b.StopTimer()
}

func benchMessage(sessionID, id string, rev int) chat.Message {
	return chat.Message{
		ID:        id,
		SessionID: sessionID,
		Role:      protocol.RoleAssistant,
		Kind:      "text",
		Text:      "benchmark event delivery payload",
		Timestamp: protocol.NowMillis(),
		Rev:       rev,
	}
}

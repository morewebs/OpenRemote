package events_test

import (
	"fmt"
	"testing"

	"github.com/morewebs/OpenRemote/internal/core/events"
)

// BenchmarkEventBusAppend measures a single durable event append to the
// SQLite WAL event log — the component cost inside every streamed event.
func BenchmarkEventBusAppend(b *testing.B) {
	dir := b.TempDir()
	bus, err := events.Open(dir)
	if err != nil {
		b.Fatal(err)
	}
	b.Cleanup(func() { _ = bus.Close() })

	const sessionID = "bench-session"
	if err := bus.UpsertSession(sessionID, "ws", "shell", dir, "", "", "", "running"); err != nil {
		b.Fatal(err)
	}

	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		if _, err := bus.AppendEvent(sessionID, "bench.event",
			fmt.Sprintf("benchmark payload %d", i)); err != nil {
			b.Fatal(err)
		}
	}
}

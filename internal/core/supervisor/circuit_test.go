package supervisor

import (
	"testing"
	"time"
)

func TestCircuitBreakerWindowAndIsolation(t *testing.T) {
	b := NewCircuitBreaker(3, 15*time.Minute)
	for range 3 {
		if !b.Allow("agent") {
			t.Fatal("opened too early")
		}
		b.Failure("agent")
	}
	if b.Allow("agent") {
		t.Fatal("crash loop not stopped")
	}
	if !b.Allow("other") {
		t.Fatal("unrelated agent blocked")
	}
	b.mu.Lock()
	b.failures["agent"] = []time.Time{time.Now().Add(-16 * time.Minute)}
	b.mu.Unlock()
	if !b.Allow("agent") {
		t.Fatal("expired failures retained")
	}
}

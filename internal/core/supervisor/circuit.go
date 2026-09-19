package supervisor

import (
	"sync"
	"time"
)

// CircuitBreaker bounds crash loops independently for each executable.
type CircuitBreaker struct {
	mu       sync.Mutex
	failures map[string][]time.Time
	limit    int
	window   time.Duration
}

func NewCircuitBreaker(limit int, window time.Duration) *CircuitBreaker {
	return &CircuitBreaker{failures: make(map[string][]time.Time), limit: limit, window: window}
}
func (b *CircuitBreaker) recent(key string, now time.Time) []time.Time {
	var recent []time.Time
	for _, failure := range b.failures[key] {
		if now.Sub(failure) < b.window {
			recent = append(recent, failure)
		}
	}
	b.failures[key] = recent
	return recent
}
func (b *CircuitBreaker) Allow(key string) bool {
	b.mu.Lock()
	defer b.mu.Unlock()
	return len(b.recent(key, time.Now())) < b.limit
}
func (b *CircuitBreaker) Failure(key string) {
	b.mu.Lock()
	defer b.mu.Unlock()
	now := time.Now()
	b.failures[key] = append(b.recent(key, now), now)
}

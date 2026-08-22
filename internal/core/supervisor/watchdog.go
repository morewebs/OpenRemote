package supervisor

import (
	"context"
	"fmt"
	"log"
	"net/http"
	"time"
)

// Watchdog polls /health and restarts on crash-loop — spec 02 §6.
type Watchdog struct {
	HealthURL       string
	Interval        time.Duration
	FailureThreshold int
	CircuitBreakerWindow time.Duration
	MaxRestarts     int
	onRestart       func()
}

func NewWatchdog(addr string, onRestart func()) *Watchdog {
	return &Watchdog{
		HealthURL:       fmt.Sprintf("http://%s/health", addr),
		Interval:        10 * time.Second,
		FailureThreshold: 3,
		CircuitBreakerWindow: 15 * time.Minute,
		MaxRestarts:     3,
		onRestart:       onRestart,
	}
}

func (w *Watchdog) Run(ctx context.Context) {
	ticker := time.NewTicker(w.Interval)
	defer ticker.Stop()
	failures := 0
	var restartTimes []time.Time
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			if err := w.probe(); err != nil {
				failures++
				log.Printf("[watchdog] health probe failed (%d/%d): %v", failures, w.FailureThreshold, err)
				if failures >= w.FailureThreshold {
					now := time.Now()
					// prune outside window
					var recent []time.Time
					for _, t := range restartTimes {
						if now.Sub(t) < w.CircuitBreakerWindow {
							recent = append(recent, t)
						}
					}
					restartTimes = recent
					if len(restartTimes) >= w.MaxRestarts {
						log.Printf("[watchdog] CIRCUIT BREAKER: %d restarts in %v — halting", len(restartTimes), w.CircuitBreakerWindow)
						return
					}
					restartTimes = append(restartTimes, now)
					failures = 0
					if w.onRestart != nil {
						w.onRestart()
					}
				}
			} else {
				failures = 0
			}
		}
	}
}

func (w *Watchdog) probe() error {
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	req, _ := http.NewRequestWithContext(ctx, http.MethodGet, w.HealthURL, nil)
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		return fmt.Errorf("status %d", resp.StatusCode)
	}
	return nil
}

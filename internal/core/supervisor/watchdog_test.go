package supervisor

import (
	"context"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
	"time"
)

func TestWatchdog_ProbeSuccess(t *testing.T) {
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"status":"ok"}`))
	}))
	defer ts.Close()

	w := NewWatchdog(ts.Listener.Addr().String(), nil)
	if err := w.probe(); err != nil {
		t.Fatalf("expected probe to succeed, got error: %v", err)
	}
}

func TestWatchdog_ProbeFailure(t *testing.T) {
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
	}))
	defer ts.Close()

	w := NewWatchdog(ts.Listener.Addr().String(), nil)
	if err := w.probe(); err == nil {
		t.Fatalf("expected probe to fail on 500 status")
	}
}

func TestWatchdog_RunRestart(t *testing.T) {
	var healthy int32 = 1
	var restarts int32

	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if atomic.LoadInt32(&healthy) == 1 {
			w.WriteHeader(http.StatusOK)
		} else {
			w.WriteHeader(http.StatusInternalServerError)
		}
	}))
	defer ts.Close()

	w := NewWatchdog(ts.Listener.Addr().String(), func() {
		atomic.AddInt32(&restarts, 1)
		atomic.StoreInt32(&healthy, 1) // recover on restart
	})
	w.Interval = 20 * time.Millisecond
	w.FailureThreshold = 2

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	go w.Run(ctx)

	time.Sleep(50 * time.Millisecond)

	// Cause failure
	atomic.StoreInt32(&healthy, 0)

	// Wait for failure threshold + restart
	time.Sleep(150 * time.Millisecond)

	if atomic.LoadInt32(&restarts) == 0 {
		t.Fatalf("expected watchdog to trigger restart")
	}
}

package supervisor

import (
	"context"
	"fmt"
	"io"
	"log"
	"os/exec"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

// Supervise restarts the daemon after a crash or persistent failed health
// checks. PTY sessions are marked stopped on restart; tasks are never replayed.
func Supervise(ctx context.Context, binary string, args []string, addr string, stdout, stderr io.Writer) error {
	breaker := NewCircuitBreaker(3, 15*time.Minute)
	for ctx.Err() == nil {
		if !breaker.Allow(binary) {
			return fmt.Errorf("daemon circuit breaker opened after 3 failures in 15 minutes")
		}
		cmd := exec.Command(binary, args...)
		cmd.Stdout, cmd.Stderr = stdout, stderr
		input, err := cmd.StdinPipe()
		if err != nil {
			return err
		}
		if err := cmd.Start(); err != nil {
			_ = input.Close()
			return err
		}
		exited := make(chan struct{})
		var exitErr error
		go func() { exitErr = cmd.Wait(); close(exited) }()
		var stopOnce sync.Once
		stop := func() {
			stopOnce.Do(func() {
				_ = input.Close()
				select {
				case <-exited:
				case <-time.After(5 * time.Second):
					_ = cmd.Process.Kill()
					<-exited
				}
			})
		}
		watchCtx, cancelWatch := context.WithCancel(ctx)
		var unhealthy atomic.Bool
		if !strings.HasSuffix(addr, ":0") {
			watchdog := NewWatchdog(addr, func() { unhealthy.Store(true); stop() })
			go watchdog.Run(watchCtx)
		}
		select {
		case <-ctx.Done():
			stop()
		case <-exited:
		}
		cancelWatch()
		_ = input.Close()
		if ctx.Err() != nil {
			return nil
		}
		if exitErr == nil && !unhealthy.Load() {
			return nil
		}
		breaker.Failure(binary)
		log.Printf("[supervisor] daemon stopped (%v); restarting with stopped session records", exitErr)
		select {
		case <-ctx.Done():
			return nil
		case <-time.After(time.Second):
		}
	}
	return nil
}

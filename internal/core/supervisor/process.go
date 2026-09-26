package supervisor

import (
	"context"
	"errors"
	"fmt"
	"io"
	"log"
	"os"
	"os/exec"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

// RestartExitCode is the worker's deliberate-restart handshake: a self-update
// exits the worker with this code so the supervisor respawns the new binary
// without counting a failure against the circuit breaker.
const RestartExitCode = 75

// Supervise restarts the daemon after a crash or persistent failed health
// checks. PTY sessions are marked stopped on restart; tasks are never replayed.
// A worker exit of RestartExitCode (self-update) restarts immediately without a
// breaker failure; if the breaker does open, a leftover <binary>.old from a
// self-update is restored before giving up.
func Supervise(ctx context.Context, binary string, args []string, addr string, stdout, stderr io.Writer) error {
	breaker := NewCircuitBreaker(3, 15*time.Minute)
	for ctx.Err() == nil {
		if !breaker.Allow(binary) {
			// The release we swapped in may be broken; restore the backup
			// from the last self-update and keep supervising that instead.
			if _, err := os.Stat(binary + ".old"); err == nil {
				log.Printf("[supervisor] daemon keeps failing; rolling back to the previous binary")
				if err := os.Rename(binary+".old", binary); err == nil {
					breaker = NewCircuitBreaker(3, 15*time.Minute)
					continue
				}
				log.Printf("[supervisor] rollback failed: %v", err)
			}
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
		if exitCode(exitErr) == RestartExitCode {
			// A deliberate self-update restart: no failure recorded.
			log.Printf("[supervisor] daemon restarted for update (current %s); relaunching", exitErr)
			continue
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

// exitCode extracts the process exit status, returning 0 for clean exits and
// non-process errors (e.g. a binary that failed to spawn at all).
func exitCode(err error) int {
	var exitErr *exec.ExitError
	if errors.As(err, &exitErr) {
		return exitErr.ExitCode()
	}
	return 0
}

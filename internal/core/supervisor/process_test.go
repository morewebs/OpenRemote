package supervisor

import (
	"context"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"
)

// TestMain doubles as a supervised worker: Supervise re-spawns this test
// binary with the marker below, and the env-driven mode decides the exit.
// The counter file lets a "restart" worker decide when to settle.
func TestMain(m *testing.M) {
	if len(os.Args) > 1 && os.Args[1] == "supervisor-test-worker" {
		mode := os.Getenv("OPENREMOTE_SUPTEST_MODE")
		counterPath := os.Getenv("OPENREMOTE_SUPTEST_COUNTER")
		n := 0
		if data, err := os.ReadFile(counterPath); err == nil {
			n, _ = strconv.Atoi(strings.TrimSpace(string(data)))
		}
		n++
		_ = os.WriteFile(counterPath, []byte(strconv.Itoa(n)), 0o644)
		switch mode {
		case "restart":
			if n >= 5 {
				os.Exit(0)
			}
			os.Exit(RestartExitCode)
		case "fail":
			os.Exit(1)
		}
		os.Exit(0)
	}
	os.Exit(m.Run())
}

// supervisedCopy copies the test binary to the temp dir so rollback renames
// only ever touch copies, never the running test image.
func supervisedCopy(t *testing.T) string {
	t.Helper()
	src, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(src)
	if err != nil {
		t.Fatal(err)
	}
	dst := filepath.Join(t.TempDir(), "daemon.exe")
	if err := os.WriteFile(dst, data, 0o755); err != nil {
		t.Fatal(err)
	}
	return dst
}

func awaitSupervise(t *testing.T, binary string) error {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	done := make(chan error, 1)
	go func() {
		// A ":0" address keeps the watchdog out of the loop.
		done <- Supervise(ctx, binary, []string{"supervisor-test-worker"}, "127.0.0.1:0", io.Discard, io.Discard)
	}()
	select {
	case err := <-done:
		return err
	case <-time.After(55 * time.Second):
		t.Fatal("Supervise did not return in time")
		return nil
	}
}

// TestSuperviseRestartExitCodeDoesNotTripBreaker: a worker that keeps
// requesting deliberate restarts (exit 75) must be relaunched without the
// circuit breaker counting failures — more restarts than the breaker limit
// still settle once the worker exits cleanly.
func TestSuperviseRestartExitCodeDoesNotTripBreaker(t *testing.T) {
	binary := supervisedCopy(t)
	t.Setenv("OPENREMOTE_SUPTEST_MODE", "restart")
	t.Setenv("OPENREMOTE_SUPTEST_COUNTER", filepath.Join(t.TempDir(), "count"))

	if err := awaitSupervise(t, binary); err != nil {
		t.Fatalf("Supervise returned %v, want nil (restarts must not trip the breaker)", err)
	}
}

// TestSuperviseRollbackRestoresPreviousBinary: when the breaker opens with a
// self-update backup present, the backup is restored over the failing binary
// and supervision continues; a second breaker open (no backup left) surfaces
// the error.
func TestSuperviseRollbackRestoresPreviousBinary(t *testing.T) {
	binary := supervisedCopy(t)
	t.Setenv("OPENREMOTE_SUPTEST_MODE", "fail")

	// Simulate the leftover backup a self-update leaves behind: a second
	// copy of the same binary (so the post-rollback spawn still runs as a
	// failing worker, not a broken image).
	backup, err := os.ReadFile(binary)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(binary+".old", backup, 0o755); err != nil {
		t.Fatal(err)
	}

	supErr := awaitSupervise(t, binary)
	if supErr == nil {
		t.Fatal("expected circuit breaker error after failures with no backup")
	}
	if !strings.Contains(supErr.Error(), "circuit breaker") {
		t.Fatalf("unexpected error: %v", err)
	}
	if _, statErr := os.Stat(binary + ".old"); statErr == nil {
		t.Error("backup must be consumed by the rollback, not left behind")
	}
	if _, statErr := os.Stat(binary); statErr != nil {
		t.Error("daemon binary must still exist after rollback")
	}
}

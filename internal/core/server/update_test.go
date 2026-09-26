package server_test

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/morewebs/OpenRemote/internal/core/events"
	"github.com/morewebs/OpenRemote/internal/core/server"
	"github.com/morewebs/OpenRemote/internal/core/update"
)

// updateFeed serves a GitHub-style release whose binary asset is a marked
// payload with a matching SHA256SUMS.txt.
func updateFeed(t *testing.T, tag string) *httptest.Server {
	t.Helper()
	payload := []byte("daemon binary " + tag)
	sum := hex.EncodeToString(func() []byte { h := sha256.Sum256(payload); return h[:] }())
	mux := http.NewServeMux()
	mux.HandleFunc("/latest", func(w http.ResponseWriter, r *http.Request) {
		_, _ = fmt.Fprintf(w, `{"tag_name":"%s","assets":[
			{"name":"openremote-windows-amd64.exe","browser_download_url":"http://%s/binary"},
			{"name":"SHA256SUMS.txt","browser_download_url":"http://%s/sums"}]}`,
			tag, r.Host, r.Host)
	})
	mux.HandleFunc("/binary", func(w http.ResponseWriter, _ *http.Request) { _, _ = w.Write(payload) })
	mux.HandleFunc("/sums", func(w http.ResponseWriter, _ *http.Request) {
		_, _ = fmt.Fprintf(w, "%s  openremote-windows-amd64.exe\n", sum)
	})
	ts := httptest.NewServer(mux)
	t.Cleanup(ts.Close)
	return ts
}

// newUpdateServer builds a server whose updater targets the given feed and a
// temp fake binary, with a restart hook that records invocation.
func newUpdateServer(t *testing.T, feedURL, current string) (*server.Server, *httptest.Server, string, chan struct{}) {
	t.Helper()
	dir := t.TempDir()
	bus, err := events.Open(dir)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = bus.Close() })

	exe := filepath.Join(t.TempDir(), "daemon.exe")
	if err := os.WriteFile(exe, []byte("old binary"), 0o755); err != nil {
		t.Fatal(err)
	}
	restarted := make(chan struct{}, 1)
	srv := newTestServer(t, server.Config{
		Addr:         "127.0.0.1:0",
		DataDir:      dir,
		Token:        "test-token",
		AllowedRoots: []string{dir},
		RestartHook:  func() { restarted <- struct{}{} },
	}, bus)
	srv.TestSetUpdater(update.New(current, exe, feedURL+"/latest"))
	ts := httptest.NewServer(srv.Handler())
	t.Cleanup(ts.Close)
	return srv, ts, exe, restarted
}

func TestUpdateEndpointReportsStatus(t *testing.T) {
	feed := updateFeed(t, "v0.10.0")
	_, ts, _, _ := newUpdateServer(t, feed.URL, "0.9.0")

	res, err := authenticated(t, ts, http.MethodGet, "/api/v1/update")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = res.Body.Close() }()
	if res.StatusCode != http.StatusOK {
		t.Fatalf("GET /api/v1/update status = %d", res.StatusCode)
	}
	var status map[string]any
	if err := json.NewDecoder(res.Body).Decode(&status); err != nil {
		t.Fatal(err)
	}
	// No check has run yet: nothing is available but the endpoint answers.
	if status["current"] != "0.9.0" {
		t.Errorf("current = %v, want 0.9.0", status["current"])
	}
	if available, _ := status["available"].(bool); available {
		t.Error("available should be false before any check")
	}
}

func TestUpdateApplyViaEndpointSwapsAndRestarts(t *testing.T) {
	feed := updateFeed(t, "v0.10.0")
	_, ts, exe, restarted := newUpdateServer(t, feed.URL, "0.9.0")

	res, err := authenticated(t, ts, http.MethodPost, "/api/v1/update")
	if err != nil {
		t.Fatal(err)
	}
	if res.StatusCode != http.StatusAccepted {
		t.Fatalf("POST /api/v1/update status = %d", res.StatusCode)
	}
	_ = res.Body.Close()

	// The apply runs in the background: wait for the restart hook, which
	// fires after swap and shutdown.
	select {
	case <-restarted:
	case <-time.After(30 * time.Second):
		t.Fatal("restart hook never fired; apply did not complete")
	}
	got, err := os.ReadFile(exe)
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != "daemon binary v0.10.0" {
		t.Errorf("binary = %q, want updated payload", got)
	}

	// The apply status surfaces the result.
	res, err = authenticated(t, ts, http.MethodGet, "/api/v1/update")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = res.Body.Close() }()
	var status map[string]any
	if err := json.NewDecoder(res.Body).Decode(&status); err != nil {
		t.Fatal(err)
	}
	if v, _ := status["applying"].(bool); v {
		t.Error("applying should be false after completion")
	}
	if status["current"] != "0.10.0" {
		t.Errorf("current = %v, want 0.10.0 after apply", status["current"])
	}
}

func TestUpdateApplyMismatchSurfacesError(t *testing.T) {
	// A feed whose SHA256SUMS entry never matches the served binary.
	mux := http.NewServeMux()
	mux.HandleFunc("/latest", func(w http.ResponseWriter, r *http.Request) {
		_, _ = fmt.Fprintf(w, `{"tag_name":"v0.10.0","assets":[
			{"name":"openremote-windows-amd64.exe","browser_download_url":"http://%s/binary"},
			{"name":"SHA256SUMS.txt","browser_download_url":"http://%s/sums"}]}`, r.Host, r.Host)
	})
	mux.HandleFunc("/binary", func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte("daemon binary v0.10.0"))
	})
	mux.HandleFunc("/sums", func(w http.ResponseWriter, _ *http.Request) {
		_, _ = fmt.Fprint(w, "deadbeef  openremote-windows-amd64.exe\n")
	})
	mangling := httptest.NewServer(mux)
	t.Cleanup(mangling.Close)

	_, ts, exe, restarted := newUpdateServer(t, mangling.URL, "0.9.0")

	res, err := authenticated(t, ts, http.MethodPost, "/api/v1/update")
	if err != nil {
		t.Fatal(err)
	}
	if res.StatusCode != http.StatusAccepted {
		t.Fatalf("POST status = %d", res.StatusCode)
	}
	_ = res.Body.Close()

	select {
	case <-restarted:
		t.Fatal("restart hook fired despite checksum mismatch")
	case <-time.After(10 * time.Second):
	}

	got, err := os.ReadFile(exe)
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != "old binary" {
		t.Errorf("binary must be untouched on refusal, got %q", got)
	}

	res, err = authenticated(t, ts, http.MethodGet, "/api/v1/update")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = res.Body.Close() }()
	var status map[string]any
	if err := json.NewDecoder(res.Body).Decode(&status); err != nil {
		t.Fatal(err)
	}
	if applyErr, _ := status["applyError"].(string); applyErr == "" {
		t.Error("applyError should surface the refusal reason")
	}
}

func authenticated(t *testing.T, ts *httptest.Server, method, path string) (*http.Response, error) {
	t.Helper()
	req, err := http.NewRequest(method, ts.URL+path, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Authorization", "Bearer test-token")
	return http.DefaultClient.Do(req)
}

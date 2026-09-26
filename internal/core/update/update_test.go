package update

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
)

func TestNewer(t *testing.T) {
	cases := []struct {
		current, latest string
		want            bool
	}{
		{"0.1.0", "0.1.0", false},
		{"0.1.0", "0.1.1", true},
		{"0.1.9", "0.2.0", true},
		{"1.0.0", "0.9.9", false},
		{"v0.1.0", "v0.1.1", true},
		{"0.1.0", "v0.1.1", true},
		{"0.1", "0.1.1", true},
		{"0.2.0", "0.2.0-rc.1", false}, // prerelease is not newer than release
		{"0.2.0-rc.1", "0.2.0", true},
		{"0.2.0-rc.1", "0.2.0-rc.2", true},
		{"1.2.3", "1.2.4-rc.1", true},
	}
	for _, tc := range cases {
		if got := Newer(tc.current, tc.latest); got != tc.want {
			t.Errorf("Newer(%q, %q) = %v, want %v", tc.current, tc.latest, got, tc.want)
		}
	}
}

func TestMatchChecksum(t *testing.T) {
	sums := "abc123  other-file.zip\n" +
		"def456  openremote-windows-amd64.exe\n" +
		"789abc  openremote-linux-arm64\n"
	got, err := matchChecksum([]byte(sums), "openremote-windows-amd64.exe")
	if err != nil {
		t.Fatal(err)
	}
	if got != "def456" {
		t.Errorf("got %q, want def456", got)
	}
	if _, err := matchChecksum([]byte(sums), "missing"); err == nil {
		t.Error("expected error for missing file entry")
	}
}

// feedFixture serves a GitHub-style release with an asset payload and a
// matching (or mismatching) SHA256SUMS.txt.
type feedFixture struct {
	tag       string
	payload   []byte
	mangleSum bool
}

func (f feedFixture) start(t *testing.T) *httptest.Server {
	t.Helper()
	newBinary := []byte("new daemon binary for " + f.tag)
	if f.payload != nil {
		newBinary = f.payload
	}
	sum := sha256.Sum256(newBinary)
	sumHex := hex.EncodeToString(sum[:])
	if f.mangleSum {
		sumHex = "0000" + sumHex[4:]
	}
	mux := http.NewServeMux()
	mux.HandleFunc("/latest", func(w http.ResponseWriter, r *http.Request) {
		_, _ = fmt.Fprintf(w, `{"tag_name":"%s","assets":[
			{"name":"openremote-windows-amd64.exe","browser_download_url":"http://%s/binary"},
			{"name":"SHA256SUMS.txt","browser_download_url":"http://%s/sums"}]}`,
			f.tag, r.Host, r.Host)
	})
	mux.HandleFunc("/binary", func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write(newBinary)
	})
	mux.HandleFunc("/sums", func(w http.ResponseWriter, _ *http.Request) {
		_, _ = fmt.Fprintf(w, "%s  openremote-windows-amd64.exe\n", sumHex)
	})
	ts := httptest.NewServer(mux)
	t.Cleanup(ts.Close)
	return ts
}

func newTestManager(t *testing.T, feedURL string) (*Manager, string) {
	t.Helper()
	dir := t.TempDir()
	exe := filepath.Join(dir, "openremote.exe")
	if err := os.WriteFile(exe, []byte("old daemon binary"), 0o755); err != nil {
		t.Fatal(err)
	}
	return New("0.9.0", exe, feedURL+"/latest"), exe
}

func TestCheckResolvesAssetAndChecksum(t *testing.T) {
	ts := feedFixture{tag: "v0.10.0"}.start(t)
	m, _ := newTestManager(t, ts.URL)
	if err := m.Check(context.Background()); err != nil {
		t.Fatal(err)
	}
	st := m.Status()
	if !st.Available || st.Latest != "0.10.0" {
		t.Errorf("status = %+v, want available 0.10.0", st)
	}
}

func TestCheckSameVersionNotAvailable(t *testing.T) {
	ts := feedFixture{tag: "v0.9.0"}.start(t)
	m, _ := newTestManager(t, ts.URL)
	if err := m.Check(context.Background()); err != nil {
		t.Fatal(err)
	}
	if m.Status().Available {
		t.Error("same version must not be reported available")
	}
}

func TestApplySwapsBinary(t *testing.T) {
	ts := feedFixture{tag: "v0.10.0"}.start(t)
	m, exe := newTestManager(t, ts.URL)
	if err := m.Check(context.Background()); err != nil {
		t.Fatal(err)
	}
	if err := m.Apply(context.Background()); err != ErrRestartNeeded {
		t.Fatalf("Apply = %v, want ErrRestartNeeded", err)
	}
	got, err := os.ReadFile(exe)
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != "new daemon binary for v0.10.0" {
		t.Errorf("binary not swapped: %q", got)
	}
	if _, err := os.Stat(exe + ".old"); err != nil {
		t.Error("expected .old backup to exist after swap")
	}
	if m.Status().Current != "0.10.0" {
		t.Errorf("current = %q, want 0.10.0", m.Status().Current)
	}
}

func TestApplyRefusesChecksumMismatch(t *testing.T) {
	ts := feedFixture{tag: "v0.10.0", mangleSum: true}.start(t)
	m, exe := newTestManager(t, ts.URL)
	if err := m.Check(context.Background()); err != nil {
		t.Fatal(err)
	}
	err := m.Apply(context.Background())
	if err == nil || err == ErrRestartNeeded {
		t.Fatal("checksum mismatch must be refused")
	}
	got, readErr := os.ReadFile(exe)
	if readErr != nil {
		t.Fatal(readErr)
	}
	if string(got) != "old daemon binary" {
		t.Errorf("original binary must be untouched, got %q", got)
	}
	if _, statErr := os.Stat(exe + ".old"); statErr == nil {
		t.Error("no .old file may be created on refusal")
	}
}

func TestDevBuildRefusesSelfUpdate(t *testing.T) {
	dir := t.TempDir()
	exe := filepath.Join(dir, "openremote.exe")
	if err := os.WriteFile(exe, []byte("x"), 0o755); err != nil {
		t.Fatal(err)
	}
	m := New("dev", exe, DefaultFeedURL)
	if err := m.Check(context.Background()); err == nil {
		t.Error("dev builds must refuse self-update checks")
	}
}

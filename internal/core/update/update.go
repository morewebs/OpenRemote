// Package update implements daemon self-update from a GitHub-style release
// feed with SHA256-verified downloads and an atomic binary swap.
package update

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"runtime"
	"strconv"
	"strings"
	"time"
)

// DefaultFeedURL points at this project's latest GitHub release.
const DefaultFeedURL = "https://api.github.com/repos/morewebs/OpenRemote/releases/latest"

// ErrRestartNeeded reports that a new binary is in place and the process should
// exit so its supervisor restarts it.
var ErrRestartNeeded = errors.New("update applied; restart required")

// Status is the cached result of the last release check.
type Status struct {
	Current   string    `json:"current"`
	Latest    string    `json:"latest,omitempty"`
	Available bool      `json:"available"`
	CheckedAt time.Time `json:"checkedAt"`
	Note      string    `json:"note,omitempty"`
}

// Manager checks a release feed and swaps the running binary in place.
// The zero value is not usable; use New.
type Manager struct {
	feedURL   string
	client    *http.Client
	current   string
	exePath   string
	assetURL  string
	assetSHA  string
	available bool
	latest    string
	checkedAt time.Time
}

// New returns a Manager for the given current version stamp and executable
// path. An empty or "dev" version stamp disables updates (unstamped builds
// have no ordering to compare against a release feed).
func New(current, exePath, feedURL string) *Manager {
	if feedURL == "" {
		feedURL = DefaultFeedURL
	}
	return &Manager{feedURL: feedURL, client: &http.Client{Timeout: 60 * time.Second}, current: current, exePath: exePath}
}

// Status returns the cached check state. Available is false until a Check
// has found a strictly newer release.
func (m *Manager) Status() Status {
	return Status{Current: m.current, Latest: m.latest, Available: m.available, CheckedAt: m.checkedAt}
}

type releaseAsset struct {
	Name               string `json:"name"`
	BrowserDownloadURL string `json:"browser_download_url"`
}

type release struct {
	TagName string         `json:"tag_name"`
	Assets  []releaseAsset `json:"assets"`
}

// assetName is the raw-binary naming convention from the release workflow:
// openremote-<goos>-<goarch>[.exe].
func assetName() string {
	if runtime.GOOS == "windows" {
		return fmt.Sprintf("openremote-%s-%s.exe", runtime.GOOS, runtime.GOARCH)
	}
	return fmt.Sprintf("openremote-%s-%s", runtime.GOOS, runtime.GOARCH)
}

// Check fetches the release feed, resolves this platform's binary asset and
// its SHA256SUMS entry, and caches whether a newer release is available.
func (m *Manager) Check(ctx context.Context) error {
	if m.current == "" || m.current == "dev" {
		return errors.New("unstamped build (dev); self-update disabled")
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, m.feedURL, nil)
	if err != nil {
		return err
	}
	req.Header.Set("Accept", "application/vnd.github+json")
	var rel release
	if err := fetchJSON(m.client, req, &rel); err != nil {
		return fmt.Errorf("release feed: %w", err)
	}
	latest := strings.TrimPrefix(rel.TagName, "v")
	var binaryURL string
	for _, a := range rel.Assets {
		if a.Name == assetName() {
			binaryURL = a.BrowserDownloadURL
			break
		}
	}
	if binaryURL == "" {
		return fmt.Errorf("release %s has no %s asset", rel.TagName, assetName())
	}
	sumsURL := ""
	for _, a := range rel.Assets {
		if a.Name == "SHA256SUMS.txt" {
			sumsURL = a.BrowserDownloadURL
			break
		}
	}
	if sumsURL == "" {
		return fmt.Errorf("release %s has no SHA256SUMS.txt asset", rel.TagName)
	}
	req, err = http.NewRequestWithContext(ctx, http.MethodGet, sumsURL, nil)
	if err != nil {
		return err
	}
	body, err := fetchAll(m.client, req, 1<<20)
	if err != nil {
		return fmt.Errorf("SHA256SUMS.txt: %w", err)
	}
	sha, err := matchChecksum(body, assetName())
	if err != nil {
		return err
	}
	m.latest, m.assetURL, m.assetSHA = latest, binaryURL, sha
	m.available = Newer(m.current, latest)
	m.checkedAt = time.Now()
	return nil
}

// Apply downloads the cached release asset, verifies its SHA256 against the
// SHA256SUMS entry, and atomically swaps it in: the running binary is renamed
// to <exe>.old (the rollback backup) and the verified download takes its
// place. The old file is left for the supervisor's rollback path.
func (m *Manager) Apply(ctx context.Context) error {
	if !m.available || m.assetURL == "" {
		if err := m.Check(ctx); err != nil {
			return err
		}
		if !m.available {
			return fmt.Errorf("no update available (current %s, latest %s)", m.current, m.latest)
		}
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, m.assetURL, nil)
	if err != nil {
		return err
	}
	data, err := fetchAll(m.client, req, 512<<20)
	if err != nil {
		return fmt.Errorf("download update: %w", err)
	}
	sum := sha256.Sum256(data)
	if got := hex.EncodeToString(sum[:]); got != m.assetSHA {
		return fmt.Errorf("checksum mismatch: SHA256SUMS says %s, download hashes %s; refusing to apply", m.assetSHA, got)
	}
	if err := os.Rename(m.exePath, m.exePath+".old"); err != nil {
		return fmt.Errorf("preserve current binary: %w", err)
	}
	info, statErr := os.Stat(m.exePath + ".old")
	if err := os.WriteFile(m.exePath, data, 0o755); err != nil {
		// Put the original back; leaving the user without a binary is worse
		// than a failed update.
		if renameErr := os.Rename(m.exePath+".old", m.exePath); renameErr != nil {
			return fmt.Errorf("write new binary: %v (rollback also failed: %v)", err, renameErr)
		}
		return fmt.Errorf("write new binary: %w", err)
	}
	if statErr == nil {
		_ = os.Chmod(m.exePath, info.Mode())
	}
	m.current = m.latest
	m.available = false
	return ErrRestartNeeded
}

// fetchJSON fetches and decodes a JSON document, rejecting oversized bodies.
func fetchJSON(client *http.Client, req *http.Request, out any) error {
	body, err := fetchAll(client, req, 8<<20)
	if err != nil {
		return err
	}
	return json.Unmarshal(body, out)
}

func fetchAll(client *http.Client, req *http.Request, limit int64) ([]byte, error) {
	res, err := client.Do(req)
	if err != nil {
		return nil, err
	}
	defer func() { _ = res.Body.Close() }()
	if res.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("status %d", res.StatusCode)
	}
	return io.ReadAll(io.LimitReader(res.Body, limit))
}

// matchChecksum parses a sha256sum-format manifest and returns the hash for
// the named file.
func matchChecksum(sums []byte, name string) (string, error) {
	for _, line := range strings.Split(string(sums), "\n") {
		fields := strings.Fields(strings.TrimSpace(line))
		if len(fields) == 2 && strings.TrimPrefix(fields[1], "*") == name {
			return fields[0], nil
		}
	}
	return "", fmt.Errorf("SHA256SUMS.txt has no entry for %s", name)
}

// Newer reports whether latest is strictly greater than current using
// dotted-integer comparison; a prerelease suffix (anything after '-') sorts
// lower than the same version without one. Malformed components compare as 0.
func Newer(current, latest string) bool {
	cPre, cCore := splitPre(current)
	lPre, lCore := splitPre(latest)
	for i := 0; i < 3; i++ {
		c, l := nth(cCore, i), nth(lCore, i)
		if c != l {
			return l > c
		}
	}
	if cPre != lPre {
		// A release (no prerelease) sorts above any prerelease; otherwise
		// compare the suffixes lexicographically.
		if cPre == "" {
			return false
		}
		if lPre == "" {
			return true
		}
		return lPre > cPre
	}
	return false
}

func splitPre(v string) (pre, core string) {
	v = strings.TrimPrefix(strings.TrimSpace(v), "v")
	if i := strings.Index(v, "-"); i >= 0 {
		return v[i+1:], v[:i]
	}
	return "", v
}

func nth(core string, i int) int {
	parts := strings.Split(core, ".")
	if i >= len(parts) {
		return 0
	}
	n, _ := strconv.Atoi(parts[i])
	return n
}

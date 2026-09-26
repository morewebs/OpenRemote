package auth

import (
	"crypto/rand"
	"crypto/subtle"
	"encoding/hex"
	"fmt"
	"log"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync"
)

var noAuthWarnOnce sync.Once

const tokenBytes = 32 // 256-bit

func GenerateToken() (string, error) {
	b := make([]byte, tokenBytes)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return hex.EncodeToString(b), nil
}

func TokenPath(dataDir string) string {
	return filepath.Join(dataDir, "token")
}

func LoadOrCreateToken(dataDir string) (string, error) {
	if err := os.MkdirAll(dataDir, 0o700); err != nil {
		return "", err
	}
	path := TokenPath(dataDir)
	if data, err := os.ReadFile(path); err == nil {
		if loaded := strings.TrimSpace(string(data)); loaded != "" {
			return loaded, nil
		}
	}
	tok, err := GenerateToken()
	if err != nil {
		return "", err
	}
	if err := os.WriteFile(path, []byte(tok+"\n"), 0o600); err != nil {
		return "", err
	}
	return tok, nil
}

// CheckToken performs a constant-time comparison against expected token.
func CheckToken(expected, provided string) bool {
	if expected == "" {
		return true
	}
	if len(expected) != len(provided) {
		return false
	}
	return subtle.ConstantTimeCompare([]byte(expected), []byte(provided)) == 1
}

// Middleware enforces `Authorization: Bearer <token>` or `?token=<token>` unless token == "" (dev mode).
// Health endpoint and static assets (UI) are always allowed so the web companion
// can load in a browser without a pre-shared token; the SPA then supplies the
// token on its own API/WebSocket/SSE calls.
//
// Token sources (priority order):
//  1. Authorization: Bearer <token>  — preferred for all clients (constant-time compare via CheckToken).
//  2. ?token=<token> query parameter  — fallback ONLY for browser WebSocket and SSE connections,
//     which cannot set custom headers. Do not use for regular REST calls; prefer the header.
//     Query-param tokens may appear in access logs and browser history — treat them as
//     deprecated for non-WS/SSE usage.
func Middleware(token string, limiter *RateLimiter, next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/health" || isStaticAsset(r.URL.Path) {
			next.ServeHTTP(w, r)
			return
		}

		if limiter != nil {
			ip := ExtractIP(r)
			if !limiter.Allow(ip) {
				w.Header().Set("Content-Type", "application/json")
				w.WriteHeader(http.StatusTooManyRequests)
				_, _ = fmt.Fprint(w, `{"code":"ERR_RATE_LIMITED","message":"too many requests, please slow down"}`)
				return
			}
		}

		if token == "" {
			noAuthWarnOnce.Do(func() {
				log.Println("WARNING: OpenRemote authentication is DISABLED (token is empty). Anyone with network access can control sessions!")
			})
			next.ServeHTTP(w, r)
			return
		}

		// 1. Check Header (preferred)
		h := r.Header.Get("Authorization")
		if strings.HasPrefix(h, "Bearer ") {
			prov := strings.TrimPrefix(h, "Bearer ")
			if CheckToken(token, prov) {
				next.ServeHTTP(w, r)
				return
			}
		}

		// 2. Check Query param — fallback for browser WebSocket & SSE only
		// (see Middleware doc). Query tokens are logged, so treat as deprecated
		// for regular REST and migrate callers to the header.
		if q := r.URL.Query().Get("token"); q != "" {
			if CheckToken(token, q) {
				next.ServeHTTP(w, r)
				return
			}
		}

		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusUnauthorized)
		_, _ = fmt.Fprint(w, `{"code":"ERR_AUTH_REQUIRED","message":"missing or invalid Bearer token"}`)
	})
}

// isStaticAsset returns true for paths that belong to the web UI rather than
// the API surface, so a plain browser navigation does not require a bearer token.
// API/WS/SSE always require auth.
func isStaticAsset(path string) bool {
	// Anything under the API/WebSocket/SSE surface always requires auth.
	if strings.HasPrefix(path, "/api") ||
		strings.HasPrefix(path, "/ws") ||
		strings.HasPrefix(path, "/events") ||
		path == "/health" {
		return false
	}
	// Everything else is the SPA shell (/, /web, /web/*, index.html,
	// flutter .js, assets, favicons, SPA fallback, ...).
	return true
}

package auth

import (
	"crypto/rand"
	"crypto/subtle"
	"encoding/hex"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"strings"
)

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
		return strings.TrimSpace(string(data)), nil
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

// Middleware enforces `Authorization: Bearer <token>` or `?token=<token>` unless token == "" (dev mode)
// Health endpoint is always allowed. Also enforces rate limiting on unauthenticated/all endpoints.
func Middleware(token string, limiter *RateLimiter, next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/health" {
			next.ServeHTTP(w, r)
			return
		}

		if limiter != nil {
			ip := ExtractIP(r)
			if !limiter.Allow(ip) {
				w.Header().Set("Content-Type", "application/json")
				w.WriteHeader(http.StatusTooManyRequests)
				fmt.Fprint(w, `{"code":"ERR_RATE_LIMITED","message":"too many requests, please slow down"}`)
				return
			}
		}

		if token == "" {
			next.ServeHTTP(w, r)
			return
		}

		// 1. Check Header
		h := r.Header.Get("Authorization")
		if strings.HasPrefix(h, "Bearer ") {
			prov := strings.TrimPrefix(h, "Bearer ")
			if CheckToken(token, prov) {
				next.ServeHTTP(w, r)
				return
			}
		}

		// 2. Check Query param (for browser WebSockets & SSE)
		if q := r.URL.Query().Get("token"); q != "" {
			if CheckToken(token, q) {
				next.ServeHTTP(w, r)
				return
			}
		}

		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusUnauthorized)
		fmt.Fprint(w, `{"code":"ERR_AUTH_REQUIRED","message":"missing or invalid Bearer token"}`)
	})
}

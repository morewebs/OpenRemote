package auth

import (
	"crypto/rand"
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

// Middleware enforces `Authorization: Bearer <token>` or `?token=<token>` unless token == "" (dev mode)
// Health endpoint is always allowed.
func Middleware(token string, next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/health" {
			next.ServeHTTP(w, r)
			return
		}
		if token == "" {
			next.ServeHTTP(w, r)
			return
		}
		// 1. Check Header
		h := r.Header.Get("Authorization")
		if strings.HasPrefix(h, "Bearer ") && strings.TrimPrefix(h, "Bearer ") == token {
			next.ServeHTTP(w, r)
			return
		}
		// 2. Check Query param (for browser WebSockets & SSE)
		if q := r.URL.Query().Get("token"); q == token {
			next.ServeHTTP(w, r)
			return
		}

		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusUnauthorized)
		fmt.Fprint(w, `{"code":"ERR_AUTH_REQUIRED","message":"missing or invalid Bearer token"}`)
	})
}

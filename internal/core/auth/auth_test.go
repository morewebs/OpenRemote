package auth_test

import (
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"

	"github.com/morewebs/OpenRemote/internal/core/auth"
)

func TestTokenGenerationAndPersistence(t *testing.T) {
	tempDir := t.TempDir()

	tok1, err := auth.LoadOrCreateToken(tempDir)
	if err != nil {
		t.Fatalf("first LoadOrCreateToken failed: %v", err)
	}
	if len(tok1) != 64 { // 32 bytes hex encoded = 64 chars
		t.Fatalf("expected 64 char hex token, got %d", len(tok1))
	}

	tok2, err := auth.LoadOrCreateToken(tempDir)
	if err != nil {
		t.Fatalf("second LoadOrCreateToken failed: %v", err)
	}
	if tok1 != tok2 {
		t.Errorf("token was re-generated instead of loaded: %s != %s", tok1, tok2)
	}

	// Verify token file existence
	path := filepath.Join(tempDir, "token")
	if _, err := os.Stat(path); err != nil {
		t.Errorf("token file not found at %s", path)
	}
}

func TestAuthMiddleware(t *testing.T) {
	token := "secret-test-token-123"
	handler := auth.Middleware(token, http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("ok"))
	}))

	// 1. Health is unrestricted
	reqHealth := httptest.NewRequest(http.MethodGet, "/health", nil)
	recHealth := httptest.NewRecorder()
	handler.ServeHTTP(recHealth, reqHealth)
	if recHealth.Code != http.StatusOK {
		t.Errorf("health should bypass auth, got status %d", recHealth.Code)
	}

	// 2. Unauthenticated request to /api/v1/sessions should 401
	reqAPI := httptest.NewRequest(http.MethodGet, "/api/v1/sessions", nil)
	recAPI := httptest.NewRecorder()
	handler.ServeHTTP(recAPI, reqAPI)
	if recAPI.Code != http.StatusUnauthorized {
		t.Errorf("expected 401 Unauthorized, got %d", recAPI.Code)
	}

	// 3. Authenticated request
	reqAuth := httptest.NewRequest(http.MethodGet, "/api/v1/sessions", nil)
	reqAuth.Header.Set("Authorization", "Bearer "+token)
	recAuth := httptest.NewRecorder()
	handler.ServeHTTP(recAuth, reqAuth)
	if recAuth.Code != http.StatusOK {
		t.Errorf("expected 200 OK, got %d", recAuth.Code)
	}
}

package main

import (
	"os"
	"path/filepath"
	"testing"
)

func TestMask(t *testing.T) {
	if mask("short") != "***" {
		t.Fatalf("expected *** for short token, got %s", mask("short"))
	}
	long := "1234567890abcdef"
	m := mask(long)
	if m != "1234...cdef" {
		t.Fatalf("expected 1234...cdef, got %s", m)
	}
}

func TestStringSlice(t *testing.T) {
	var s stringSlice
	_ = s.Set("path1")
	_ = s.Set("path2")
	if len(s) != 2 || s[0] != "path1" || s[1] != "path2" {
		t.Fatalf("unexpected stringSlice content: %v", s)
	}
	if s.String() != "[path1 path2]" {
		t.Fatalf("unexpected String() output: %s", s.String())
	}
}

func TestRunToken(t *testing.T) {
	tempDir := filepath.Join(t.TempDir(), "openremote_test_data")
	_ = os.MkdirAll(tempDir, 0o755)

	// Run token creation
	runToken([]string{"-data", tempDir})

	tokenFile := filepath.Join(tempDir, "token")
	data, err := os.ReadFile(tokenFile)
	if err != nil || len(data) == 0 {
		t.Fatalf("expected token file to be created: %v", err)
	}

	// Run token rotation
	runToken([]string{"-data", tempDir, "-rotate"})
	data2, err := os.ReadFile(tokenFile)
	if err != nil || len(data2) == 0 {
		t.Fatalf("expected rotated token file: %v", err)
	}
	if string(data) == string(data2) {
		t.Fatalf("expected token to change after rotation")
	}
}

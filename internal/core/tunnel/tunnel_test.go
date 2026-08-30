package tunnel

import (
	"context"
	"testing"
)

type mockProvider struct {
	name      string
	installed bool
	running   bool
	url       string
}

func (m *mockProvider) Name() string { return m.name }
func (m *mockProvider) Detect() (string, error) {
	return "/bin/" + m.name, nil
}
func (m *mockProvider) Start(ctx context.Context, localAddr string) (string, error) {
	m.running = true
	m.url = "https://mock-" + m.name + ".example.com"
	return m.url, nil
}
func (m *mockProvider) Stop() error {
	m.running = false
	m.url = ""
	return nil
}
func (m *mockProvider) Info() ProviderInfo {
	return ProviderInfo{
		Name:      m.name,
		Installed: m.installed,
		Running:   m.running,
		URL:       m.url,
	}
}

func TestTunnelManager(t *testing.T) {
	mgr := NewManager()

	// Initial default provider is cloudflared
	list := mgr.List()
	if len(list) == 0 {
		t.Fatalf("expected at least 1 tunnel provider listed")
	}

	// Register a mock provider
	mock := &mockProvider{name: "mock-tunnel", installed: true}
	mgr.mu.Lock()
	mgr.providers[mock.Name()] = mock
	mgr.mu.Unlock()

	// Verify it shows in List
	found := false
	for _, info := range mgr.List() {
		if info.Name == "mock-tunnel" {
			found = true
			if !info.Installed {
				t.Fatalf("expected mock-tunnel to be installed")
			}
		}
	}
	if !found {
		t.Fatalf("expected mock-tunnel in list")
	}

	// Start tunnel
	u, err := mgr.Start(context.Background(), "mock-tunnel", "127.0.0.1:4097")
	if err != nil {
		t.Fatalf("failed to start tunnel: %v", err)
	}
	if u != "https://mock-mock-tunnel.example.com" {
		t.Fatalf("unexpected tunnel URL: %s", u)
	}

	// Stop tunnel
	if err := mgr.Stop(); err != nil {
		t.Fatalf("failed to stop tunnel: %v", err)
	}
	if mock.running {
		t.Fatalf("expected mock tunnel to be stopped")
	}

	// Starting invalid provider returns error
	_, err = mgr.Start(context.Background(), "nonexistent", "127.0.0.1:4097")
	if err == nil {
		t.Fatalf("expected error starting nonexistent provider")
	}
}

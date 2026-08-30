package tunnel

import (
	"bufio"
	"context"
	"fmt"
	"io"
	"os/exec"
	"regexp"
	"sync"
)

type ProviderInfo struct {
	Name        string `json:"name"`
	Installed   bool   `json:"installed"`
	Running     bool   `json:"running"`
	URL         string `json:"url,omitempty"`
	InstallHint string `json:"installHint,omitempty"`
}

type Provider interface {
	Name() string
	Detect() (string, error)
	Start(ctx context.Context, localAddr string) (string, error)
	Stop() error
	Info() ProviderInfo
}

// CloudflaredProvider exposes local daemon via Cloudflare Quick Tunnels.
type CloudflaredProvider struct {
	mu      sync.Mutex
	cmd     *exec.Cmd
	url     string
	running bool
}

func NewCloudflaredProvider() *CloudflaredProvider {
	return &CloudflaredProvider{}
}

func (p *CloudflaredProvider) Name() string { return "cloudflared" }

func (p *CloudflaredProvider) Detect() (string, error) {
	path, err := exec.LookPath("cloudflared")
	if err != nil {
		return "", fmt.Errorf("cloudflared not found in PATH")
	}
	return path, nil
}

func (p *CloudflaredProvider) Info() ProviderInfo {
	p.mu.Lock()
	defer p.mu.Unlock()
	_, err := p.Detect()
	return ProviderInfo{
		Name:        "cloudflared",
		Installed:   err == nil,
		Running:     p.running,
		URL:         p.url,
		InstallHint: "Install with 'brew install cloudflared' or 'winget install Cloudflare.cloudflared'",
	}
}

func (p *CloudflaredProvider) Start(ctx context.Context, localAddr string) (string, error) {
	p.mu.Lock()
	defer p.mu.Unlock()

	if p.running {
		return p.url, nil
	}

	bin, err := p.Detect()
	if err != nil {
		return "", err
	}

	targetURL := "http://" + localAddr
	cmd := exec.CommandContext(ctx, bin, "tunnel", "--url", targetURL, "--no-autoupdate")
	stderr, err := cmd.StderrPipe()
	if err != nil {
		return "", err
	}

	if err := cmd.Start(); err != nil {
		return "", err
	}

	p.cmd = cmd
	p.running = true

	urlCh := make(chan string, 1)
	reURL := regexp.MustCompile(`https:\/\/[a-zA-Z0-9\-]+\.trycloudflare\.com`)

	go func() {
		reader := bufio.NewReader(stderr)
		for {
			line, err := reader.ReadString('\n')
			if u := reURL.FindString(line); u != "" {
				select {
				case urlCh <- u:
				default:
				}
			}
			if err != nil {
				if err != io.EOF {
					// stream closed
				}
				break
			}
		}
	}()

	select {
	case foundURL := <-urlCh:
		p.url = foundURL
		return foundURL, nil
	case <-ctx.Done():
		_ = cmd.Process.Kill()
		p.running = false
		return "", ctx.Err()
	}
}

func (p *CloudflaredProvider) Stop() error {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.cmd != nil && p.cmd.Process != nil {
		_ = p.cmd.Process.Kill()
	}
	p.running = false
	p.url = ""
	return nil
}

// Manager aggregates all tunnel providers.
type Manager struct {
	mu        sync.RWMutex
	providers map[string]Provider
	active    Provider
}

func NewManager() *Manager {
	m := &Manager{
		providers: make(map[string]Provider),
	}
	cf := NewCloudflaredProvider()
	m.providers[cf.Name()] = cf
	return m
}

func (m *Manager) List() []ProviderInfo {
	m.mu.RLock()
	defer m.mu.RUnlock()
	var list []ProviderInfo
	for _, p := range m.providers {
		list = append(list, p.Info())
	}
	return list
}

func (m *Manager) Start(ctx context.Context, name string, localAddr string) (string, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	p, ok := m.providers[name]
	if !ok {
		return "", fmt.Errorf("tunnel provider %q not found", name)
	}
	url, err := p.Start(ctx, localAddr)
	if err != nil {
		return "", err
	}
	m.active = p
	return url, nil
}

func (m *Manager) Stop() error {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.active != nil {
		err := m.active.Stop()
		m.active = nil
		return err
	}
	return nil
}

package tunnel

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"net"
	"os/exec"
	"regexp"
	"sort"
	"sync"
	"time"
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
	Start(context.Context, string) (string, error)
	Stop() error
	Info() ProviderInfo
}

type processProvider struct {
	mu              sync.Mutex
	startMu         sync.Mutex
	name, hint, url string
	pattern         *regexp.Regexp
	args            func(string) []string
	preflight       func(context.Context, string) error
	command         func(context.Context, string, ...string) *exec.Cmd
	cancel          context.CancelFunc
	done            chan struct{}
	running         bool
	timeout         time.Duration
}

func (p *processProvider) Name() string            { return p.name }
func (p *processProvider) Detect() (string, error) { return exec.LookPath(p.name) }
func (p *processProvider) Info() ProviderInfo {
	_, err := p.Detect()
	p.mu.Lock()
	defer p.mu.Unlock()
	return ProviderInfo{Name: p.name, Installed: err == nil, Running: p.running, URL: p.url, InstallHint: p.hint}
}
func (p *processProvider) Start(ctx context.Context, addr string) (string, error) {
	p.startMu.Lock()
	defer p.startMu.Unlock()
	p.mu.Lock()
	if p.running {
		url := p.url
		p.mu.Unlock()
		return url, nil
	}
	runCtx, cancel := context.WithCancel(ctx)
	done := make(chan struct{})
	p.cancel, p.done = cancel, done
	p.mu.Unlock()
	started := false
	defer func() {
		if !started {
			cancel()
			close(done)
		}
	}()
	host, _, err := net.SplitHostPort(addr)
	if err != nil || (host != "localhost" && net.ParseIP(host) == nil) {
		return "", fmt.Errorf("invalid local tunnel address")
	}
	if host != "localhost" && !net.ParseIP(host).IsLoopback() {
		return "", fmt.Errorf("tunnels require a loopback daemon address")
	}
	bin, err := p.Detect()
	if err != nil {
		return "", err
	}
	if p.preflight != nil {
		check, stop := context.WithTimeout(runCtx, 10*time.Second)
		err = p.preflight(check, bin)
		stop()
		if err != nil {
			cancel()
			return "", err
		}
	}
	command := p.command
	if command == nil {
		command = exec.CommandContext
	}
	cmd := command(runCtx, bin, p.args("http://"+addr)...)
	output, err := cmd.StdoutPipe()
	if err != nil {
		cancel()
		return "", err
	}
	cmd.Stderr = cmd.Stdout
	if err := cmd.Start(); err != nil {
		_ = output.Close()
		cancel()
		return "", err
	}
	started = true
	p.mu.Lock()
	p.running = runCtx.Err() == nil
	p.mu.Unlock()
	urls := make(chan string, 1)
	go func() {
		scanner := bufio.NewScanner(output)
		scanner.Buffer(make([]byte, 4096), 256*1024)
		for scanner.Scan() {
			if url := p.pattern.FindString(scanner.Text()); url != "" {
				select {
				case urls <- url:
				default:
				}
			}
		}
		if scanner.Err() != nil {
			cancel()
		}
		_ = cmd.Wait()
		p.mu.Lock()
		if p.done == done {
			p.running = false
			p.url = ""
		}
		p.mu.Unlock()
		close(done)
	}()
	timeout := p.timeout
	if timeout == 0 {
		timeout = 30 * time.Second
	}
	timer := time.NewTimer(timeout)
	defer timer.Stop()
	select {
	case url := <-urls:
		p.mu.Lock()
		if runCtx.Err() != nil || !p.running {
			p.mu.Unlock()
			cancel()
			return "", fmt.Errorf("%s stopped during startup", p.name)
		}
		p.url = url
		p.mu.Unlock()
		return url, nil
	case <-done:
		cancel()
		return "", fmt.Errorf("%s exited before opening a tunnel; check its login and configuration", p.name)
	case <-runCtx.Done():
		cancel()
		<-done
		return "", runCtx.Err()
	case <-timer.C:
		_ = p.Stop()
		return "", fmt.Errorf("%s startup timed out", p.name)
	}
}
func (p *processProvider) Stop() error {
	p.mu.Lock()
	cancel, done := p.cancel, p.done
	p.cancel = nil
	p.running = false
	p.url = ""
	p.mu.Unlock()
	if cancel != nil {
		cancel()
	}
	if done != nil {
		select {
		case <-done:
		case <-time.After(5 * time.Second):
			return fmt.Errorf("%s did not stop in time", p.name)
		}
	}
	return nil
}

type CloudflaredProvider struct{ *processProvider }

func NewCloudflaredProvider() *CloudflaredProvider {
	return &CloudflaredProvider{&processProvider{name: "cloudflared", hint: "Install cloudflared from Cloudflare", pattern: regexp.MustCompile(`https://[a-zA-Z0-9-]+\.trycloudflare\.com`), args: func(target string) []string { return []string{"tunnel", "--url", target, "--no-autoupdate"} }}}
}

type TailscaleProvider struct{ *processProvider }

func NewTailscaleProvider() *TailscaleProvider {
	p := &processProvider{name: "tailscale", hint: "Install Tailscale and sign in to your tailnet", pattern: regexp.MustCompile(`https://[a-zA-Z0-9.-]+\.ts\.net(?::[0-9]+)?`), args: func(target string) []string { return []string{"serve", "--https=443", target} }}
	p.preflight = func(ctx context.Context, bin string) error {
		data, err := exec.CommandContext(ctx, bin, "serve", "status", "--json").Output()
		if err != nil {
			return fmt.Errorf("read Tailscale Serve configuration: %w", err)
		}
		var config struct {
			TCP map[string]any
			Web map[string]any
		}
		if err := json.Unmarshal(data, &config); err != nil {
			return err
		}
		if len(config.TCP) != 0 || len(config.Web) != 0 {
			return fmt.Errorf("existing Tailscale Serve configuration is in use; OpenRemote will not replace it")
		}
		return nil
	}
	return &TailscaleProvider{p}
}

type Manager struct {
	mu        sync.RWMutex
	providers map[string]Provider
	active    Provider
}

func NewManager() *Manager {
	cf, ts := NewCloudflaredProvider(), NewTailscaleProvider()
	return &Manager{providers: map[string]Provider{cf.Name(): cf, ts.Name(): ts}}
}
func (m *Manager) List() []ProviderInfo {
	m.mu.RLock()
	providers := make([]Provider, 0, len(m.providers))
	for _, p := range m.providers {
		providers = append(providers, p)
	}
	m.mu.RUnlock()
	list := make([]ProviderInfo, 0, len(providers))
	for _, p := range providers {
		list = append(list, p.Info())
	}
	sort.Slice(list, func(i, j int) bool { return list[i].Name < list[j].Name })
	return list
}
func (m *Manager) Start(ctx context.Context, name, addr string) (string, error) {
	m.mu.Lock()
	provider, ok := m.providers[name]
	if !ok {
		m.mu.Unlock()
		return "", fmt.Errorf("unknown tunnel provider %q", name)
	}
	if m.active != nil && m.active != provider {
		m.mu.Unlock()
		return "", fmt.Errorf("stop the active tunnel before starting another provider")
	}
	m.active = provider
	m.mu.Unlock()
	url, err := provider.Start(ctx, addr)
	if err != nil {
		m.mu.Lock()
		if m.active == provider {
			m.active = nil
		}
		m.mu.Unlock()
	}
	return url, err
}
func (m *Manager) Stop() error {
	m.mu.Lock()
	provider := m.active
	m.active = nil
	m.mu.Unlock()
	if provider != nil {
		return provider.Stop()
	}
	return nil
}

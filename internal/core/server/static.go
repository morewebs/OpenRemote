package server

import (
	"embed"
	"io/fs"
	"net/http"
	"strings"
)

// Embedded static assets (if built)
//
//go:embed all:dist/*
var embeddedAssets embed.FS

// StaticHandler serves the companion web application or fallback interface.
func StaticHandler() http.Handler {
	sub, err := fs.Sub(embeddedAssets, "dist")
	if err != nil {
		return http.HandlerFunc(serveFallback)
	}

	fileServer := http.FileServer(http.FS(sub))
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		// If requesting API or WS paths, let next handler deal with it
		if strings.HasPrefix(r.URL.Path, "/api/") ||
			strings.HasPrefix(r.URL.Path, "/ws") ||
			strings.HasPrefix(r.URL.Path, "/events") ||
			r.URL.Path == "/health" {
			http.NotFound(w, r)
			return
		}

		// Try opening the requested file
		f, err := sub.Open(strings.TrimPrefix(r.URL.Path, "/"))
		if err == nil {
			_ = f.Close()
			fileServer.ServeHTTP(w, r)
			return
		}

		// Check if index.html exists in embedded assets (SPA fallback)
		if idx, err := sub.Open("index.html"); err == nil {
			_ = idx.Close()
			r.URL.Path = "/"
			fileServer.ServeHTTP(w, r)
			return
		}

		serveFallback(w, r)
	})
}

func serveFallback(w http.ResponseWriter, _ *http.Request) {
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	w.WriteHeader(200)
	html := `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>OpenRemote</title>
    <style>
        :root {
            --bg: #09090b;
            --card: #18181b;
            --border: #27272a;
            --text: #f4f4f5;
            --muted: #a1a1aa;
            --purple: #7c3aed;
            --purple-hover: #6d28d9;
        }
        body {
            margin: 0;
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
            background: var(--bg);
            color: var(--text);
            display: flex;
            align-items: center;
            justify-content: center;
            min-height: 100vh;
        }
        .container {
            max-width: 520px;
            padding: 32px;
            background: var(--card);
            border: 1px solid var(--border);
            border-radius: 12px;
            box-shadow: 0 8px 30px rgba(0,0,0,0.5);
            text-align: center;
        }
        h1 {
            font-size: 24px;
            font-weight: 700;
            margin: 0 0 12px;
            display: flex;
            align-items: center;
            justify-content: center;
            gap: 8px;
        }
        .badge {
            font-size: 11px;
            background: var(--purple);
            color: white;
            padding: 2px 8px;
            border-radius: 9999px;
            font-weight: 600;
            text-transform: uppercase;
        }
        p {
            color: var(--muted);
            font-size: 14px;
            line-height: 1.5;
            margin: 0 0 24px;
        }
        .status-box {
            background: #121214;
            border: 1px solid var(--border);
            border-radius: 8px;
            padding: 16px;
            text-align: left;
            font-family: monospace;
            font-size: 13px;
            margin-bottom: 24px;
        }
        .status-row {
            display: flex;
            justify-content: space-between;
            margin-bottom: 8px;
        }
        .status-row:last-child {
            margin-bottom: 0;
        }
        .status-ok { color: #10b981; }
        .btn {
            display: inline-block;
            background: var(--purple);
            color: white;
            padding: 10px 20px;
            border-radius: 8px;
            text-decoration: none;
            font-weight: 600;
            font-size: 14px;
            transition: background 0.2s;
        }
        .btn:hover { background: var(--purple-hover); }
    </style>
</head>
<body>
    <div class="container">
        <h1>OpenRemote <span class="badge">Online</span></h1>
        <p>Daemon is running and ready. Connect your Flutter companion app or inspect API endpoints below.</p>
        <div class="status-box">
            <div class="status-row"><span>Status:</span><span class="status-ok">● Active</span></div>
            <div class="status-row"><span>Protocol:</span><span>Binary WS (0x01-0x06)</span></div>
            <div class="status-row"><span>Auth:</span><span>Bearer Token Active</span></div>
            <div class="status-row"><span>Drivers:</span><span>Claude, Antigravity, OpenCode, Codex, Pi</span></div>
        </div>
        <a href="/api/v1/agents" class="btn">View Agents API</a>
    </div>
</body>
</html>`
	_, _ = w.Write([]byte(html))
}

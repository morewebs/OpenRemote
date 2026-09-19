// OpenRemote Web Console — vanilla JS, no framework.
// Binary WS framing matches internal/protocol/frame.go (2-byte header: [opcode, slot]).

const OPCODES = {
  PTY_OUTPUT: 0x01,
  KEYSTROKE: 0x02,
  VIEWPORT_RESIZE: 0x03,
  CATCHUP: 0x04,
  JSONRPC: 0x05,
  PINGPONG: 0x06,
};

const TOKEN_KEY = "openremote.token";

// ---------- State ----------
const state = {
  token: "",
  sessions: [],
  activeSessionId: null,
  ws: null,
  term: null,
  fitAddon: null,
  approvals: new Map(), // approvalId -> event
  chatMessages: [],
  agents: [],
  cwdForActive: "",
};

// ---------- DOM refs ----------
const $ = (sel) => document.querySelector(sel);
const $$ = (sel) => Array.from(document.querySelectorAll(sel));

const els = {
  gate: $("#token-gate"),
  console: $("#console"),
  tokenInput: $("#token-input"),
  tokenRemember: $("#token-remember"),
  tokenSave: $("#token-save"),
  tokenError: $("#token-error"),
  healthBadge: $("#health-badge"),
  btnNewSession: $("#btn-new-session"),
  btnCopyToken: $("#btn-copy-token"),
  btnDisconnect: $("#btn-disconnect"),
  sessionList: $("#session-list"),
  sessionMeta: $("#session-meta"),
  tabs: $$(".tab"),
  panelTerminal: $("#panel-terminal"),
  panelChat: $("#panel-chat"),
  xtermHost: $("#xterm-host"),
  promptForm: $("#prompt-form"),
  promptInput: $("#prompt-input"),
  chatLog: $("#chat-log"),
  approvalList: $("#approval-list"),
  diffView: $("#diff-view"),
  btnRefreshDiff: $("#btn-refresh-diff"),
  fileTree: $("#file-tree"),
  dialog: $("#new-session-dialog"),
  nsForm: $("#new-session-form"),
  nsAgent: $("#ns-agent"),
  nsCwd: $("#ns-cwd"),
  nsWorktree: $("#ns-worktree"),
  nsTask: $("#ns-task"),
  nsSubmit: $("#ns-submit"),
  nsError: $("#ns-error"),
};

// ---------- Token handling ----------
function loadToken() {
  const fromQuery = new URLSearchParams(location.search).get("token");
  if (fromQuery) {
    state.token = fromQuery;
    try { localStorage.setItem(TOKEN_KEY, fromQuery); } catch {}
    return fromQuery;
  }
  try {
    const stored = localStorage.getItem(TOKEN_KEY);
    if (stored) { state.token = stored; return stored; }
  } catch {}
  return "";
}

function clearToken() {
  state.token = "";
  try { localStorage.removeItem(TOKEN_KEY); } catch {}
}

function showGate(errorMsg = "") {
  els.console.classList.add("hidden");
  els.gate.classList.remove("hidden");
  els.tokenInput.value = "";
  els.tokenError.textContent = errorMsg;
  els.tokenInput.focus();
}

function showConsole() {
  els.gate.classList.add("hidden");
  els.console.classList.remove("hidden");
}

// ---------- API client ----------
async function api(path, init = {}) {
  const headers = new Headers(init.headers || {});
  if (state.token) headers.set("Authorization", `Bearer ${state.token}`);
  if (init.body && typeof init.body === "object" && !(init.body instanceof FormData)) {
    headers.set("Content-Type", "application/json");
    init.body = JSON.stringify(init.body);
  }
  const res = await fetch(path, { ...init, headers });
  if (res.status === 401) {
    clearToken();
    showGate("Token rejected or expired.");
    throw new Error("unauthorized");
  }
  return res;
}

async function apiJson(path, init = {}) {
  const res = await api(path, init);
  if (!res.ok) {
    let msg = res.statusText;
    try { const j = await res.json(); msg = j.message || j.code || msg; } catch {}
    throw new Error(msg);
  }
  if (res.status === 204) return null;
  return res.json();
}

// ---------- Health ----------
let healthTimer = null;
async function refreshHealth() {
  try {
    const h = await apiJson("/health");
    els.healthBadge.textContent = h?.status === "ok" ? "Online" : "Degraded";
    els.healthBadge.className = `badge ${h?.status === "ok" ? "ok" : "off"}`;
  } catch {
    els.healthBadge.textContent = "Offline";
    els.healthBadge.className = "badge off";
  }
}

// ---------- Sessions ----------
async function refreshSessions() {
  try {
    const list = await apiJson("/api/v1/sessions");
    state.sessions = Array.isArray(list) ? list : [];
    renderSessions();
    // If active gone, deselect
    if (state.activeSessionId && !state.sessions.find((s) => s.sessionId === state.activeSessionId)) {
      setActiveSession(null);
    }
  } catch {}
}

function statusClass(s) {
  switch ((s.status || "").toLowerCase()) {
    case "running": return "status-running";
    case "idle": return "status-idle";
    case "waiting_approval": return "status-waiting";
    default: return "status-stopped";
  }
}

function renderSessions() {
  els.sessionList.innerHTML = "";
  for (const s of state.sessions) {
    const li = document.createElement("li");
    li.className = `session-item${s.sessionId === state.activeSessionId ? " active" : ""}`;
    li.dataset.id = s.sessionId;
    const title = s.worktreePath || s.cwd || s.sessionId.slice(0, 8);
    const branch = s.branchName ? ` · ${s.branchName}` : "";
    li.innerHTML = `
      <div class="session-title" title="${escapeAttr(s.cwd || "")}">${escapeHtml(title)}</div>
      <div class="session-meta">
        <span class="status-dot ${statusClass(s)}"></span>
        <span>${escapeHtml(s.agentId || "")}${escapeHtml(branch)}</span>
      </div>`;
    li.addEventListener("click", () => setActiveSession(s.sessionId));
    els.sessionList.appendChild(li);
  }
}

function setActiveSession(id) {
  state.activeSessionId = id;
  renderSessions();
  const s = state.sessions.find((x) => x.sessionId === id);
  state.cwdForActive = s?.cwd || "";
  els.sessionMeta.textContent = s ? `${s.agentId} · ${s.status}` : "";
  // Reset per-session UI
  state.chatMessages = [];
  state.approvals.clear();
  renderChat();
  renderApprovals();
  els.diffView.textContent = "";
  els.fileTree.innerHTML = "";
  // Reconnect WS to new session
  connectWS(id);
  // Fetch context in background
  if (id) {
    refreshDiff();
    refreshFiles();
    fetchCatchup(id);
  }
}

// ---------- WebSocket ----------
function wsUrl(sessionId) {
  const proto = location.protocol === "https:" ? "wss" : "ws";
  const u = new URL(`${proto}://${location.host}/ws`);
  if (sessionId) u.searchParams.set("sessionId", sessionId);
  if (state.token) u.searchParams.set("token", state.token);
  return u.toString();
}

function connectWS(sessionId) {
  if (state.ws) {
    try { state.ws.close(); } catch {}
    state.ws = null;
  }
  if (!sessionId) return;
  const ws = new WebSocket(wsUrl(sessionId));
  ws.binaryType = "arraybuffer";
  ws.onopen = () => {
    // Initial resize
    if (state.term && state.fitAddon) {
      try { state.fitAddon.fit(); } catch {}
      sendResize(state.term.cols, state.term.rows);
    }
  };
  ws.onmessage = (ev) => {
    if (ev.data instanceof ArrayBuffer) handleBinaryFrame(ev.data);
    else handleTextFrame(ev.data);
  };
  ws.onclose = () => {};
  ws.onerror = () => {};
  state.ws = ws;
}

function sendBinary(opcode, slot, payload) {
  if (!state.ws || state.ws.readyState !== WebSocket.OPEN) return;
  const hdr = new Uint8Array(2 + payload.length);
  hdr[0] = opcode;
  hdr[1] = slot;
  hdr.set(payload, 2);
  state.ws.send(hdr.buffer);
}

function sendKeystroke(data) {
  const enc = new TextEncoder();
  sendBinary(OPCODES.KEYSTROKE, 0, enc.encode(data));
}

function sendResize(cols, rows) {
  const buf = new Uint8Array(4);
  const dv = new DataView(buf.buffer);
  dv.setUint16(0, cols, false);
  dv.setUint16(2, rows, false);
  sendBinary(OPCODES.VIEWPORT_RESIZE, 0, buf);
}

function sendPing() {
  const buf = new Uint8Array(8);
  const dv = new DataView(buf.buffer);
  dv.setBigUint64(0, BigInt(Date.now()), false);
  sendBinary(OPCODES.PINGPONG, 0, buf);
}

function handleBinaryFrame(ab) {
  const u8 = new Uint8Array(ab);
  if (u8.length < 2) return;
  const opcode = u8[0];
  // const slot = u8[1];
  const payload = u8.slice(2);
  switch (opcode) {
    case OPCODES.PTY_OUTPUT:
      if (state.term) state.term.write(payload);
      break;
    case OPCODES.JSONRPC: {
      try {
        const text = new TextDecoder().decode(payload);
        const evt = JSON.parse(text);
        dispatchEvent(evt);
      } catch {}
      break;
    }
    case OPCODES.PINGPONG:
      // echo back
      sendBinary(OPCODES.PINGPONG, 0, payload);
      break;
  }
}

function handleTextFrame(text) {
  try {
    const obj = JSON.parse(text);
    dispatchEvent(obj);
  } catch {}
}

async function fetchCatchup(sessionId) {
  try {
    const evs = await apiJson(`/api/v1/sessions/${encodeURIComponent(sessionId)}?since=0`);
    if (Array.isArray(evs)) {
      for (const e of evs) dispatchEvent(e);
    }
  } catch {}
}

// ---------- Event dispatch ----------
function dispatchEvent(evt) {
  if (!evt || typeof evt !== "object") return;
  const type = evt.type;
  switch (type) {
    case "chat.message":
      pushChatMessage(evt);
      break;
    case "approval.requested":
      state.approvals.set(evt.approvalId, evt);
      renderApprovals();
      break;
    case "approval.resolved":
      state.approvals.delete(evt.approvalId);
      renderApprovals();
      break;
    case "diff.generated":
      els.diffView.textContent = evt.diffPatch || "";
      break;
    case "session.status":
      refreshSessions();
      break;
    case "question.asked":
      // Surface as a chat-style system message for now
      pushChatMessage({
        role: "system",
        kind: "text",
        text: `Question: ${evt.questionText}\nOptions: ${(evt.options || []).join(", ")}`,
        timestamp: evt.timestamp,
      });
      break;
    case "turn.completed":
      pushChatMessage({
        role: "system",
        kind: "text",
        text: `Turn completed${evt.summary ? ": " + evt.summary : ""}${typeof evt.costUsd === "number" ? ` · $${evt.costUsd.toFixed(4)}` : ""}`,
        timestamp: evt.timestamp,
      });
      break;
    case "auth.url":
      pushChatMessage({ role: "system", kind: "text", text: `Auth required: ${evt.url}`, timestamp: evt.timestamp });
      break;
  }
}

// ---------- Chat ----------
function pushChatMessage(m) {
  state.chatMessages.push(m);
  renderChat();
}

function renderChat() {
  els.chatLog.innerHTML = "";
  for (const m of state.chatMessages) {
    const div = document.createElement("div");
    const role = (m.role || "system").toLowerCase();
    div.className = `chat-msg ${role}`;
    const ts = m.timestamp ? new Date(m.timestamp).toLocaleTimeString() : "";
    const label = m.toolName ? `${role} · ${m.toolName}` : role;
    div.innerHTML = `<div class="chat-head"><span>${escapeHtml(label)}</span><span>${escapeHtml(ts)}</span></div><div class="chat-text">${escapeHtml(m.text || "")}</div>`;
    els.chatLog.appendChild(div);
  }
  els.chatLog.scrollTop = els.chatLog.scrollHeight;
}

// ---------- Approvals ----------
function renderApprovals() {
  els.approvalList.innerHTML = "";
  const items = Array.from(state.approvals.values());
  if (items.length === 0) {
    els.approvalList.className = "approval-list empty";
    els.approvalList.textContent = "No pending approvals";
    return;
  }
  els.approvalList.className = "approval-list";
  for (const a of items) {
    const card = document.createElement("div");
    card.className = "approval-card";
    card.innerHTML = `
      <div class="approval-tool">${escapeHtml(a.toolName || "tool")}</div>
      <div class="approval-cmd">${escapeHtml(a.command || "")}</div>
      <div class="approval-actions">
        <button class="btn small ok" data-id="${escapeAttr(a.approvalId)}" data-v="true">Approve</button>
        <button class="btn small danger" data-id="${escapeAttr(a.approvalId)}" data-v="false">Deny</button>
      </div>`;
    card.querySelectorAll("button[data-id]").forEach((b) => {
      b.addEventListener("click", async () => {
        const id = b.dataset.id;
        const approved = b.dataset.v === "true";
        try {
          await apiJson(`/api/v1/approval/${encodeURIComponent(id)}`, { method: "POST", body: { approved } });
          state.approvals.delete(id);
          renderApprovals();
        } catch (e) { alert("Failed: " + e.message); }
      });
    });
    els.approvalList.appendChild(card);
  }
}

// ---------- Diff & Files ----------
async function refreshDiff() {
  if (!state.activeSessionId) return;
  try {
    const res = await api(`/api/v1/diff/${encodeURIComponent(state.activeSessionId)}`);
    if (res.ok) els.diffView.textContent = await res.text();
  } catch {}
}

async function refreshFiles() {
  if (!state.cwdForActive) { els.fileTree.innerHTML = ""; return; }
  try {
    const entries = await apiJson(`/api/v1/files?dir=${encodeURIComponent(state.cwdForActive)}`);
    renderFileTree(Array.isArray(entries) ? entries : []);
  } catch {}
}

function renderFileTree(entries) {
  els.fileTree.innerHTML = "";
  const sorted = entries.slice().sort((a, b) => {
    if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
    return a.name.localeCompare(b.name);
  });
  for (const e of sorted) {
    const row = document.createElement("div");
    row.className = "file-row";
    const icon = e.isDir ? "📁" : "📄";
    const size = e.isDir ? "" : formatSize(e.size || 0);
    row.innerHTML = `<span class="file-icon">${icon}</span><span>${escapeHtml(e.name)}</span><span class="spacer"></span><span class="muted small">${size}</span>`;
    els.fileTree.appendChild(row);
  }
}

function formatSize(n) {
  if (n < 1024) return n + " B";
  if (n < 1024 * 1024) return (n / 1024).toFixed(1) + " KB";
  return (n / (1024 * 1024)).toFixed(2) + " MB";
}

// ---------- Terminal ----------
function initTerminal() {
  if (state.term) return;
  const t = new Terminal({
    cursorBlink: true,
    fontSize: 13,
    fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace',
    theme: { background: "#09090b", foreground: "#f4f4f5", cursor: "#7c3aed", selectionBackground: "#3f3f46" },
    allowProposedApi: true,
  });
  const fit = new FitAddon.FitAddon();
  t.loadAddon(fit);
  t.open(els.xtermHost);
  try { fit.fit(); } catch {}
  t.onData((data) => sendKeystroke(data));
  window.addEventListener("resize", () => {
    try { fit.fit(); sendResize(t.cols, t.rows); } catch {}
  });
  state.term = t;
  state.fitAddon = fit;
}

// ---------- Tabs ----------
function setupTabs() {
  els.tabs.forEach((tab) => {
    tab.addEventListener("click", () => {
      els.tabs.forEach((t) => t.classList.remove("active"));
      tab.classList.add("active");
      const which = tab.dataset.tab;
      els.panelTerminal.classList.toggle("active", which === "terminal");
      els.panelChat.classList.toggle("active", which === "chat");
      if (which === "terminal" && state.fitAddon) {
        setTimeout(() => { try { state.fitAddon.fit(); } catch {} }, 0);
      }
    });
  });
}

// ---------- Prompt ----------
function setupPrompt() {
  els.promptForm.addEventListener("submit", async (e) => {
    e.preventDefault();
    const txt = els.promptInput.value.trim();
    if (!txt || !state.activeSessionId) return;
    els.promptInput.value = "";
    pushChatMessage({ role: "user", kind: "text", text: txt, timestamp: Date.now() });
    try {
      await apiJson(`/api/v1/sessions/${encodeURIComponent(state.activeSessionId)}/prompt`, { method: "POST", body: { prompt: txt } });
    } catch (err) {
      pushChatMessage({ role: "system", kind: "text", text: "Failed to send prompt: " + err.message, timestamp: Date.now() });
    }
  });
}

// ---------- New Session Modal ----------
async function openNewSessionDialog() {
  els.nsError.textContent = "";
  // Populate agents if needed
  if (els.nsAgent.options.length === 0) {
    try {
      const list = await apiJson("/api/v1/agents");
      state.agents = Array.isArray(list) ? list : [];
      els.nsAgent.innerHTML = "";
      for (const a of state.agents) {
        const opt = document.createElement("option");
        opt.value = a.id;
        opt.textContent = `${a.displayName || a.id}${a.available ? "" : " (unavailable)"}`;
        if (!a.available) opt.disabled = true;
        els.nsAgent.appendChild(opt);
      }
    } catch (e) { els.nsError.textContent = "Could not load agents: " + e.message; }
  }
  // Default cwd to current active session's cwd or leave blank
  els.nsCwd.value = state.cwdForActive || "";
  els.dialog.showModal();
}

function setupNewSession() {
  els.btnNewSession.addEventListener("click", openNewSessionDialog);
  els.nsForm.addEventListener("submit", async (e) => {
    // method="dialog" prevents default submit; we intercept via click on submit button
  });
  els.nsSubmit.addEventListener("click", async (e) => {
    e.preventDefault();
    els.nsError.textContent = "";
    const agentId = els.nsAgent.value;
    const cwd = els.nsCwd.value.trim();
    if (!agentId || !cwd) { els.nsError.textContent = "Agent and working directory are required."; return; }
    const body = {
      agentId,
      cwd,
      useWorktree: els.nsWorktree.checked,
      cols: state.term ? state.term.cols : 120,
      rows: state.term ? state.term.rows : 30,
    };
    const task = els.nsTask.value.trim();
    if (task) body.taskName = task;
    try {
      const res = await apiJson("/api/v1/sessions", { method: "POST", body });
      els.dialog.close("created");
      await refreshSessions();
      setActiveSession(res.sessionId);
    } catch (err) { els.nsError.textContent = "Create failed: " + err.message; }
  });
}

// ---------- Topbar actions ----------
function setupTopbar() {
  els.btnCopyToken.addEventListener("click", async () => {
    try { await navigator.clipboard.writeText(state.token); els.btnCopyToken.textContent = "Copied"; setTimeout(() => els.btnCopyToken.textContent = "Copy Token", 1500); }
    catch { alert("Copy failed. Copy manually from `openremote token`."); }
  });
  els.btnDisconnect.addEventListener("click", () => {
    if (state.ws) { try { state.ws.close(); } catch {} }
    clearToken();
    showGate();
  });
  els.btnRefreshDiff.addEventListener("click", refreshDiff);
}

// ---------- Escape helpers ----------
function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}
function escapeAttr(s) { return escapeHtml(s); }

// ---------- Boot ----------
async function boot() {
  setupTabs();
  setupPrompt();
  setupNewSession();
  setupTopbar();
  initTerminal();

  const tok = loadToken();
  if (!tok) { showGate(); return; }

  showConsole();
  refreshHealth();
  healthTimer = setInterval(refreshHealth, 10000);
  await refreshSessions();
  // Auto-select first running session if any
  const running = state.sessions.find((s) => s.status === "running") || state.sessions[0];
  if (running) setActiveSession(running.sessionId);

  // Periodic session refresh
  setInterval(refreshSessions, 5000);
  // Ping loop to keep WS alive
  setInterval(sendPing, 25000);
}

boot();

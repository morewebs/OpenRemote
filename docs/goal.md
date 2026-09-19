# OpenRemote project vision

OpenRemote is an agent-agnostic remote companion for local coding agents. Its
implemented core combines a Go daemon, Flutter companion, Telegram adapter,
isolated terminal workers and durable event replay.

The objective is to launch, monitor and guide agents from another device while
keeping execution in an explicitly allowed local workspace. It provides
structured chat/approvals/questions, file and diff views, and terminal access
where the agent transport supports it.

See [implementation status](IMPLEMENTATION_STATUS.md) for supported transports,
security boundaries, verification and packaging. Original research/specification
files are design context; they do not establish production guarantees. No system
can promise zero ingress vulnerability or perfect fidelity across every CLI.

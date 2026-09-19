# Contributing to OpenRemote

Thank you for your interest in contributing to OpenRemote! This document covers everything you need to get started.

## Prerequisites

- **Go** ≥ 1.26 (see `go.mod` for the exact version)
- **Flutter** ≥ 3.44 stable (for companion client work)
- **Git** with LFS support (optional, for future binary assets)

## Repository Layout

```
OpenRemote/
├── cmd/openremote/       # Daemon CLI entry point
├── internal/             # Go packages (core, pty, drivers)
├── clients/companion/    # Flutter companion app
├── docs/                 # Architecture specs and guides
└── .github/workflows/    # CI pipelines
```

## Development Setup

### Go Daemon

```bash
git clone https://github.com/morewebs/OpenRemote.git
cd OpenRemote
go mod download
go run ./cmd/openremote          # starts daemon on 127.0.0.1:4097
```

### Flutter Companion

```bash
cd clients/companion
flutter pub get
flutter run -d chrome            # web
flutter run -d windows           # desktop
flutter run -d android           # mobile device/emulator
```

## Running Tests

### Go

```bash
go vet ./...                     # static analysis (required by CI)
go test -race -v -count=1 ./...  # full test suite with race detector
```

> **Note:** The `-race` flag requires CGO and a C toolchain. On Windows without gcc, run `go test -v -count=1 ./...` instead.

### Flutter

```bash
cd clients/companion
flutter analyze                  # static analysis (required by CI)
flutter test --reporter expanded # unit and widget tests
```

### Cross-Compile Check

Verify the daemon builds for all supported platforms:

```bash
CGO_ENABLED=0 GOOS=linux   GOARCH=amd64 go build -o /dev/null ./cmd/openremote
CGO_ENABLED=0 GOOS=linux   GOARCH=arm64 go build -o /dev/null ./cmd/openremote
CGO_ENABLED=0 GOOS=windows GOARCH=amd64 go build -o /dev/null ./cmd/openremote
CGO_ENABLED=0 GOOS=darwin  GOARCH=arm64 go build -o /dev/null ./cmd/openremote
```

## Code Style

### Go

- Run `go vet ./...` before committing — CI enforces this as a required check.
- Follow standard Go formatting (`gofmt`, `goimports`).
- Keep exported symbols documented with godoc comments.
- golangci-lint runs in CI as advisory; aim to keep it clean.

### Flutter / Dart

- Run `flutter analyze` before committing — CI enforces this as a required check.
- Follow the [Effective Dart](https://dart.dev/effective-dart) style guide.
- Use Riverpod for state management and GoRouter for navigation.
- Keep widgets small and composable; prefer feature-based folder structure.

## Pull Request Process

1. **Fork** the repository and create a branch from `main`.
2. **Make your changes** with clear, atomic commits.
3. **Run tests locally** (`go vet`, `go test`, `flutter analyze`, `flutter test`).
4. **Open a PR** against `main` with a descriptive title and summary.
5. **CI must pass** — lint, tests (Linux/macOS/Windows), and cross-compile checks are required.
6. A maintainer will review and merge. Squash-merge is preferred for clean history.

### What We Look For

- Tests covering new functionality or bug fixes.
- No unnecessary dependency additions.
- Changes that respect the existing architecture (see [`docs/spec/`](docs/spec/)).
- Clear commit messages explaining *why*, not just *what*.

## Reporting Issues

- Use [GitHub Issues](https://github.com/morewebs/OpenRemote/issues) for bugs and feature requests.
- For security vulnerabilities, see [SECURITY.md](SECURITY.md).

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).

## Embedded client build

Run `go run ./tools/build -output bin/openremote.exe` on Windows (omit `.exe` on Unix). The generated `webdist` directory is ignored; checked-in fallback assets remain unchanged. See [implementation status](docs/IMPLEMENTATION_STATUS.md) for signing and validation limitations.

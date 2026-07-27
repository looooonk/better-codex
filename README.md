# Better Codex

Better Codex is a full-screen terminal coding agent built on the Codex Rust
backend. It replaces the upstream chat-oriented terminal flow with a dense,
app-like workspace for navigating sessions, working with an agent, and
monitoring tools without leaving the terminal.

The project is under active development. It is a standalone fork, not a
drop-in replacement for the upstream Codex CLI.

## Overview

Running Better Codex opens a dashboard instead of immediately starting a chat.
From there you can create a session, search and resume previous work, or manage
the services available to the agent. The session view keeps the transcript,
composer, tool activity, approvals, and streaming output in one full-screen
interface.

### Supported

- Full-screen TUI on macOS and Linux.
- ChatGPT account and OpenAI API key authentication.
- Create, search, resume, fork, rename, archive, and delete sessions.
- Streaming agent responses and live tool output.
- Command approvals, sandbox controls, and file-change review.
- Model, reasoning effort, and service-tier selection.
- MCP servers, plugins, and optional connected app-server or exec-server
  deployments.
- Goal tracking and local shell commands entered as `!<command>`.

### Not supported

- Native Windows and WSL.
- A desktop, web, or IDE interface; Better Codex is terminal-only.
- Guaranteed compatibility with upstream CLI behavior, configuration changes,
  or release tooling.
- Fork-specific Homebrew or Apt packages.

## Installation

Make sure `$HOME/.local/bin` is on your `PATH` before starting the app.

Install the latest GitHub release on macOS or Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/looooonk/better-codex/main/scripts/install.sh | sh
```

The installer downloads the archive for the current CPU, verifies its SHA-256
checksum, and installs a `better-codex` launcher in `$HOME/.local/bin`. Releases
include binaries for Apple Silicon and Intel macOS, plus ARM64 and x86_64 Linux.

The macOS binaries are not code-signed yet. On first launch, macOS may require
you to allow the binary in System Settings under Privacy & Security.

To install a specific version:

```sh
curl -fsSL https://raw.githubusercontent.com/looooonk/better-codex/main/scripts/install.sh \
  | sh -s -- --version 0.1.0-alpha.3
```

### Build from source

The Rust workspace pins its toolchain, so `rustup` selects the required Rust
version automatically. Install Rust, Git, CMake, a C/C++ compiler, and
`pkg-config`. Linux builds also require `bubblewrap`.

```sh
git clone https://github.com/looooonk/better-codex.git
cd better-codex/codex-rs
cargo build --release -p codex-cli --bin codex
mkdir -p "$HOME/.local/bin"
install -m 755 target/release/codex "$HOME/.local/bin/better-codex"
```

## First run

Start the app and follow the sign-in prompt:

```sh
better-codex
```

Useful controls:

| Input | Action |
| --- | --- |
| `Enter` | Open the selected session or submit a prompt |
| `Ctrl+D` | Hide the dashboard or return to it |
| `!<command>` | Run a local shell command from the composer |
| `Esc` or `Ctrl+C` twice | Exit Better Codex |

## Updating

Managed installs prompt when a new release is available. You can also rerun the
installer at any time:

```sh
curl -fsSL https://raw.githubusercontent.com/looooonk/better-codex/main/scripts/install.sh | sh
```

## License

Better Codex retains the upstream project's [Apache-2.0 license](LICENSE).

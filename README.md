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
- Fork-specific Homebrew, Apt, or prebuilt binary releases. Install from source
  for now.

## Installation

The Rust workspace pins its toolchain, so `rustup` selects the required Rust
version automatically. The build output is named `codex`; the commands below
install it as `better-codex` to avoid replacing an upstream installation.

Make sure `$HOME/.local/bin` is on your `PATH` before starting the app.

### macOS

Install Apple's command-line build tools and Rust:

```sh
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Build and install Better Codex:

```sh
git clone https://github.com/looooonk/better-codex.git
cd better-codex/codex-rs
cargo build --release -p codex-cli --bin codex
mkdir -p "$HOME/.local/bin"
install -m 755 target/release/codex "$HOME/.local/bin/better-codex"
```

### Linux

Install Rust plus the native build and sandbox dependencies. On Ubuntu or
Debian:

```sh
sudo apt-get update
sudo apt-get install -y build-essential bubblewrap clang cmake curl git pkg-config
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Build and install Better Codex:

```sh
git clone https://github.com/looooonk/better-codex.git
cd better-codex/codex-rs
cargo build --release -p codex-cli --bin codex
mkdir -p "$HOME/.local/bin"
install -m 755 target/release/codex "$HOME/.local/bin/better-codex"
```

For other distributions, install the equivalent C/C++ build toolchain,
`bubblewrap`, CMake, Git, cURL, and `pkg-config` packages.

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

Pull the latest source, rebuild, and replace the installed binary:

```sh
cd better-codex
git pull --ff-only
cd codex-rs
cargo build --release -p codex-cli --bin codex
install -m 755 target/release/codex "$HOME/.local/bin/better-codex"
```

## License

Better Codex retains the upstream project's [Apache-2.0 license](LICENSE).

# Installing and building Better Codex

## System requirements

| Requirement                 | Details                               |
| --------------------------- | ------------------------------------- |
| Operating systems           | macOS 12+ or Ubuntu 20.04+/Debian 10+ |
| Git (optional, recommended) | 2.23+ for built-in PR helpers         |
| RAM                         | 4-GB minimum (8-GB recommended)       |

## Install a GitHub release

The installer downloads the archive for the current CPU from the Better Codex
GitHub releases page, verifies its SHA-256 checksum, and installs a
`better-codex` launcher:

```sh
curl -fsSL https://raw.githubusercontent.com/looooonk/better-codex/main/scripts/install.sh | sh
```

To install a specific version:

```sh
curl -fsSL https://raw.githubusercontent.com/looooonk/better-codex/main/scripts/install.sh \
  | sh -s -- --version 0.1.0-alpha.14
```

## Build from source

Install Git, a C/C++ compiler, CMake, and `pkg-config` in addition to Rust.
Linux development builds also require `bubblewrap`.

```bash
# Clone the repository and enter the Cargo workspace.
git clone https://github.com/looooonk/better-codex.git
cd better-codex/codex-rs

# Install the Rust toolchain, if necessary.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustup component add rustfmt
rustup component add clippy
# Install tools used by the workspace helpers.
cargo install --locked just
cargo install --locked dotslash
cargo install --locked cargo-nextest

# Build the internal Cargo binary used by Better Codex.
cargo build

# Launch the development build with a sample prompt. The Cargo target remains
# named codex internally.
cargo run --bin codex -- "explain this codebase to me"

# From the repository root, format and lint the crate you changed.
cd ..
just fmt
just fix -p <crate-you-touched>

# Run the relevant crate tests.
just test -p codex-tui
```

The root `justfile` runs Rust commands in `codex-rs` automatically. Use the
complete `just test` suite only when a shared-crate change requires it; routine
`--all-features` runs consume substantially more build time and disk space.

## Tracing / verbose logging

Better Codex is written in Rust, so it honors the `RUST_LOG` environment
variable to configure its logging behavior.

The TUI records diagnostics in bounded local stores by default. Set `log_dir`
explicitly to enable a plaintext TUI log for a run:

```bash
better-codex -c log_dir=./.codex-log
tail -F ./.codex-log/codex-tui.log
```

The non-interactive mode (`better-codex exec`) defaults to `RUST_LOG=error`,
but messages are printed inline, so there is no need to monitor a separate
file.

See the Rust documentation on [`RUST_LOG`](https://docs.rs/env_logger/latest/env_logger/#enabling-logging) for more information on the configuration options.

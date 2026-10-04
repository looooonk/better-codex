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
  | sh -s -- --version 0.1.0-alpha.16
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
CARGO_INCREMENTAL=0 just build --profile dev-small

# Launch the development build with a sample prompt. The Cargo target remains
# named codex internally.
./target/dev-small/codex --no-daemon "explain this codebase to me"

# From the repository root, format and lint the crate you changed.
cd ..
just fmt
just fix -p <crate-you-touched>

# Run the relevant crate tests.
just test -p codex-tui
```

The root `justfile` runs Rust commands in `codex-rs` automatically. Its Cargo
run recipes default to `dev-small`; `CODEX_CARGO_PROFILE=dev just codex --no-daemon`
selects the full development profile for the CLI and its helper together. Its Cargo
recipes verify and cache the pinned sandbox-enabled V8 archive and matching Rust
bindings under `codex-rs/target/rusty-v8`. The trusted release-manifest checksums
live under `third_party/v8`. For direct Cargo commands, use
`python3 ../scripts/rusty_v8.py -- cargo build ...` from `codex-rs`, or provide your
own `RUSTY_V8_ARCHIVE` and `RUSTY_V8_SRC_BINDING_PATH`. The bootstrap preserves
explicit overrides and source-build settings. Use the
complete `just test` suite only when a shared-crate change requires it; routine
`--all-features` runs consume substantially more build time and disk space.

Realtime voice is packaged separately from the main CLI. Release builds use
`bazel build -c opt //codex-rs/voice-host:codex-voice-host //third_party/voice:native_runtime`
from the repository root, then verify and assemble the helper and its pinned
GStreamer closure with the release packaging scripts. Voice requires macOS 14+
or Linux glibc 2.28+; the regular terminal app retains the requirements above.
Avoid building the entire Rust workspace merely to launch the app: the isolated
voice host requires additional native development libraries, and workspace builds
use more disk space.

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

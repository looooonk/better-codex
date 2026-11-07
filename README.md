<div align="center">

<img src=".github/assets/better-codex-logo.png" alt="Better Codex" width="400">

<br>

---

**A full-screen terminal workspace for Codex.**

Run agents, inspect tools, review edits, and manage every coding session without
leaving your terminal.

[![Release](https://img.shields.io/github/v/release/looooonk/better-codex?include_prereleases&sort=semver&style=flat-square&label=release)](https://github.com/looooonk/better-codex/releases)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-6f7dff?style=flat-square)](#system-requirements)
[![License](https://img.shields.io/github/license/looooonk/better-codex?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/built_with-Rust-b7410e?style=flat-square&logo=rust)](https://www.rust-lang.org/)

[Install](#install) · [Features](#everything-in-one-workspace) ·
[Documentation](#documentation) · [Contribute](CONTRIBUTING.md)

</div>

![Better Codex showing a live coding session, tool activity, settings, and workspace status](.github/assets/main-tui.png)

Better Codex reshapes the Codex Rust backend into a dense app-like TUI. Instead
of dropping directly into a single chat, it gives you a dashboard for starting,
finding, and resuming work, plus a session view where the conversation, active
tools, agent plan, file changes, and project status stay visible together.

## Install

Install the latest release on macOS or Linux, then launch it from any project:

```sh
curl -fsSL https://raw.githubusercontent.com/looooonk/better-codex/main/scripts/install.sh | sh
```

On first launch, choose ChatGPT sign-in, an OpenAI API key, or Amazon Bedrock
setup, then start a session from the dashboard.

### System requirements

| Platform | Supported releases                                      | Architectures           |
| -------- | ------------------------------------------------------- | ----------------------- |
| macOS    | macOS 12 and newer                                      | Apple Silicon and Intel |
| Linux    | Ubuntu 20.04+, Debian 10+, and compatible distributions | ARM64 and x86_64        |

Git is optional but recommended for the built-in repository and review
features. Realtime voice additionally requires macOS 14+ or a Linux distribution
with glibc 2.28+ and a working audio device. Native Windows and WSL are not
currently supported.

<details>
<summary><strong>Install a specific version</strong></summary>

```sh
curl -fsSL https://raw.githubusercontent.com/looooonk/better-codex/main/scripts/install.sh \
  | sh -s -- --version 0.1.0-alpha.20
```

Browse every published build on the
[Releases page](https://github.com/looooonk/better-codex/releases).

</details>

<details>
<summary><strong>Build from source</strong></summary>

Install Rust through [rustup](https://rustup.rs/), plus Git, CMake, a C/C++
compiler, and `pkg-config`. Linux builds also require `bubblewrap`.

```sh
git clone https://github.com/looooonk/better-codex.git
cd better-codex/codex-rs
cargo install --locked just
just build --release
mkdir -p "$HOME/.local/bin"
install -m 755 target/release/codex "$HOME/.local/bin/better-codex"
install -m 755 target/release/codex-code-mode-host "$HOME/.local/bin/codex-code-mode-host"
install -m 755 target/release/codex-exec-server "$HOME/.local/bin/codex-exec-server"
```

The workspace pins its Rust toolchain, so `rustup` selects the expected version
automatically. `just build`, `just test`, and the other Cargo recipes fetch
checksum-pinned sandboxed V8 libraries and matching bindings from upstream Codex
releases. Realtime voice also needs the separately packaged native helper
and GStreamer runtime; building the Rust binaries above does not bundle them.
Source run commands such as `just codex --no-daemon` use the small development
profile by default; set `CODEX_CARGO_PROFILE` to select another profile.
See the [complete build guide](docs/install.md) for development
tools, test commands, and logging.

</details>

## Everything in one workspace

|                            |                                                                                                                 |
| -------------------------- | --------------------------------------------------------------------------------------------------------------- |
| **Session-first workflow** | Create, search, resume, fork, rename, archive, and delete sessions from one dashboard.                          |
| **Live execution**         | Follow streaming responses and tool output while the agent works, with long results available in focused views. |
| **Built-in review**        | Inspect the complete edit set in a navigable, side-by-side diff before accepting the result.                    |
| **Visible agent state**    | Keep the plan, running tools, workspace changes, token usage, model, and reasoning effort in view.              |
| **Practical controls**     | Change models, permissions, appearance, authentication, and service tier without leaving the TUI.               |
| **Realtime voice**        | Talk with an agent through the isolated native voice helper; control microphone and playback from the workspace. |
| **Extensible backend**     | Use MCP servers, plugins, skills, goals, and connected app-server or exec-server deployments.                   |


<table>
  <tr>
    <td width="50%">
      <a href=".github/assets/tool-result.png"><img src=".github/assets/tool-result.png" alt="Expanded Better Codex tool output viewer"></a>
    </td>
    <td width="50%">
      <a href=".github/assets/file-diff.png"><img src=".github/assets/file-diff.png" alt="Better Codex side-by-side file diff viewer"></a>
    </td>
  </tr>
  <tr>
    <td><strong>Stay on top of tools.</strong> Open a focused result viewer for commands and other agent activity without losing the surrounding conversation.</td>
    <td><strong>Review the whole change.</strong> Move between changed files and inspect additions and removals in a full-screen diff.</td>
  </tr>
</table>


## Updates

Managed installs check for new releases on startup and offer to update in place.
You can also update directly from the command line:

```sh
better-codex update
```

Rerunning the installer works too. Either path activates the new verified
release while keeping the launcher path stable.

## Documentation

- [Documentation index](docs/README.md)
- [Getting started](docs/getting-started.md)
- [Installation and development setup](docs/install.md)
- [Configuration](docs/config.md)
- [Authentication](docs/authentication.md)
- [Sandboxing and approvals](docs/sandbox.md)
- [Skills](docs/skills.md)
- [Slash commands](docs/slash_commands.md)
- [Repository guide for contributors](docs/repository-guide.md)

## Community

Bug reports, feature ideas, documentation improvements, and focused pull
requests are welcome. If something is not working, check the
[existing issues](https://github.com/looooonk/better-codex/issues) and then
[open a report](https://github.com/looooonk/better-codex/issues/new/choose).
For larger changes, please start with an issue so the approach can be discussed
before implementation.

Read [CONTRIBUTING.md](CONTRIBUTING.md) for the development workflow and review
expectations.

## License

Licensed under [Apache-2.0](LICENSE). Better Codex retains the notices required
by the upstream project.

# Contributing to Better Codex

Thanks for taking the time to improve Better Codex. Bug reports, feature ideas,
documentation fixes, design feedback, and focused code contributions are all
welcome.

## Before you start

- Search the [issue tracker](https://github.com/looooonk/better-codex/issues)
  before opening a new report.
- Use the issue forms for bugs and feature proposals. Clear reproduction steps,
  terminal details, screenshots, and relevant logs make reports much easier to
  act on.
- Open an issue before starting a large change or a new feature. Early alignment
  helps avoid duplicated work and keeps the implementation consistent with the
  TUI architecture.
- Small documentation fixes and narrowly scoped corrections can go directly to
  a pull request.

## Development setup

Better Codex is a Rust workspace. Install Rust through
[rustup](https://rustup.rs/), clone the repository, and install the workspace
helpers:

```sh
git clone https://github.com/looooonk/better-codex.git
cd better-codex
cargo install --locked just
cargo install --locked dotslash
cargo install --locked cargo-nextest
```

See [docs/install.md](docs/install.md) for platform dependencies and a complete
source-build walkthrough.

## Making a change

1. Create a focused branch from the latest `main`.
2. Keep backend logic separate from TUI presentation concerns.
3. Add or update tests for behavior changes. User-visible TUI changes require
   matching `insta` snapshot coverage.
4. Update user-facing documentation when behavior changes.
5. Keep unrelated cleanups in separate pull requests.

Follow the repository conventions in [AGENTS.md](AGENTS.md), including the Rust
and TUI-specific guidance. The [repository guide](docs/repository-guide.md)
maps the main directories, ownership boundaries, generated files, and common
validation commands.

## Validate your work

Run formatting and the tests for the crate you changed. For example, a TUI
change should finish with:

```sh
just fmt
just test -p codex-tui
just fix -p codex-tui
```

If you intentionally changed TUI output, review every pending snapshot before
accepting it. Changes to shared crates may need the complete test suite; call
that out in the pull request if you could not run it locally.

## Open a pull request

Keep the title concise and complete the pull request template. A useful pull
request explains:

- what changed and why;
- the issue or user need it addresses;
- how the result was tested;
- any limitations, follow-up work, or platform-specific behavior;
- screenshots for visible TUI changes.

Reviews may ask for changes to scope, architecture, tests, or user experience.
That feedback is part of keeping a large, fast-moving Rust codebase maintainable.

## Community expectations

Be respectful, patient, and constructive. Assume good intent, criticize ideas
rather than people, and make room for contributors with different backgrounds
and levels of experience. Harassment, discrimination, and personal attacks are
not acceptable in project spaces.

Contributions submitted to this repository are licensed under the project's
[Apache-2.0 license](LICENSE).

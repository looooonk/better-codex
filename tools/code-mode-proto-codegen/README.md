# Code mode proto generators

This standalone Cargo package pins the generators used by the Bazel prost
toolchain. Its separate lockfile keeps those build executables out of the
production `codex-rs` dependency graph.

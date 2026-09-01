# Codex CLI Runtime for Python SDK

Platform-specific runtime package consumed by the published `openai-codex`.

This package is staged during release so the SDK can pin an exact Codex CLI
version without checking platform binaries into the repo.

`openai-codex-cli-bin` is intentionally wheel-only. Do not build or publish an
sdist for this package.

This runtime keeps its upstream package name. It is separate from the
`better-codex` launcher and is retained only for the Python SDK integration.

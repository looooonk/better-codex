# Configuration

Use Better Codex's settings views for options exposed by the TUI. Advanced and
backend options live in the retained Codex configuration model, normally in
`$CODEX_HOME/config.toml` or `~/.codex/config.toml` when `CODEX_HOME` is not
set.

The upstream references describe this shared model:

- [Basic configuration](https://developers.openai.com/codex/config-basic)
- [Advanced configuration](https://developers.openai.com/codex/config-advanced)
- [Configuration reference](https://developers.openai.com/codex/config-reference)
- [Sample configuration](https://developers.openai.com/codex/config-sample)

Better Codex may expose a different interface or different defaults from the
upstream CLI. Treat the settings shown by the installed Better Codex version
and its `--help` output as authoritative for fork-specific behavior.

## Lifecycle hooks

Admins can set top-level `allow_managed_hooks_only = true` in
`requirements.toml` to ignore user, project, and session hook configs while
still allowing managed hooks from requirements and managed config layers. This
setting is only supported in `requirements.toml`; putting it in `config.toml`
does not enable managed-hooks-only mode.

## Contributor note

Changes to `ConfigToml` or nested configuration types must update the checked-in
schema with `just write-config-schema`. Do not edit the generated schema by
hand.

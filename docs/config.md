# Configuration

Open the dashboard’s Settings tab with F4 or `/config` to change permissions,
appearance, and integrations. Use `/theme` to jump directly to the current app
theme. The message pane has controls for model, reasoning effort, service tier,
and permissions.

Better Codex reads the settings in `$CODEX_HOME/config.toml` (normally
`~/.codex/config.toml`). The first edit saves the current settings and your
change in a sibling file, `better-codex.toml`. Further Better Codex edits use
that file. The Codex App and CLI continue using `config.toml`. A selected
`--profile work` works the same way for `work.config.toml` and
`work.better-codex.toml`. Remove the private file to inherit the current
Codex settings again.

Better Codex also reads your existing command and network approval rules.
Approvals you save in Better Codex go into
`$CODEX_HOME/better-codex.exec-policy`. You can still manage the standard
clients' rules in `$CODEX_HOME/rules/`.

Plugin versions are shared. Local removal in Better Codex keeps downloads
and marketplace snapshots referenced by the standard user config or a named
profile. Updating an installed plugin updates the shared version. Removing a
cloud-managed plugin from your account applies to all clients signed into it.

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

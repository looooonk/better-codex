# Execution policy

Execution-policy files classify shell command prefixes as allowed, approval
required, or forbidden. Better Codex loads applicable user and project rules
before running commands.

See the local [`codex-execpolicy` reference](../codex-rs/execpolicy/README.md)
for the supported rule syntax, matching semantics, and the
`better-codex execpolicy check` command. The policy language is still in
preview; validate important rules with positive and negative examples in the
rule file.

The [Codex execution-policy guide](https://developers.openai.com/codex/exec-policy)
provides additional background for the retained policy model.

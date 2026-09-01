# Sandboxing and approvals

Sandboxing limits which files, processes, and network resources agent tools can
access. Approval policy determines when Better Codex pauses and asks before a
tool runs. Configure both deliberately: broader sandbox access and fewer
approval prompts are separate decisions.

The TUI shows the active permission state and presents approval requests in the
session workspace. Start with the narrowest profile that supports the task, and
only use unrestricted access in a workspace you trust.

See the [Codex security guide](https://developers.openai.com/codex/security) for
the sandbox and approval concepts retained by Better Codex. Execution-policy
rules provide an additional command-level layer; see
[Execution policy](execpolicy.md).

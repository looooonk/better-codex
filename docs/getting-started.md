# Getting started with Better Codex

Install the latest release by following the [installation guide](install.md),
then start Better Codex from the project you want it to work in:

```sh
cd path/to/your/project
better-codex
```

The first launch opens authentication. Sign in with ChatGPT or provide an
OpenAI API key, then create a session from the dashboard. The session workspace
keeps the conversation, active tools, plan, edits, and project status visible
together.

Start with a concrete request that includes the desired outcome and any
important constraints. Better Codex can inspect the current workspace before
making changes, so you do not need to paste files that are already present.

Useful inputs in the session composer:

| Input                   | Action                        |
| ----------------------- | ----------------------------- |
| `Enter`                 | Submit the message            |
| `Shift+Enter`           | Add a line without submitting |
| `!<command>`            | Run a local shell command     |
| `/`                     | Show available slash commands |
| `Ctrl+D`                | Hide or restore the dashboard |
| `Esc` or `Ctrl+C` twice | Exit Better Codex             |

Focused views show their own key bindings along the bottom edge. See the
[configuration](config.md), [sandbox](sandbox.md), and [skills](skills.md)
guides when you are ready to customize a session.

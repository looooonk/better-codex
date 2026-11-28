# Slash commands

Type `/` at the start of the composer to open the command picker. Type a command
name or an alias to filter the list. Use ↑ and ↓ to select, Tab to complete the
canonical name, Enter to run it, or Esc to dismiss the picker.

For example, `/sett` and `/opt` both suggest `/config`. Typing `/settings` or
`/options` and pressing Enter opens Settings directly.

| Commands | Action |
| --- | --- |
| `/config` (`/settings`, `/options`), `/theme`, `/keymap` | Open Settings, appearance, or keyboard shortcuts |
| `/new`, `/resume`, `/fork`, `/worktree` | Start, reopen, or branch a session |
| `/side` (`/btw`) | Open a side conversation. Use `/side return` to go back |
| `/rename NAME`, `/archive`, `/delete` | Manage the current session |
| `/model`, `/permissions` | Choose a model, reasoning effort, or permission mode |
| `/status`, `/usage` | Inspect session state or account usage |
| `/agent` (`/agents`, `/subagents`) | View agent communications. Select a white arrow for its message or an agent lane for its trajectory |
| `/plan [on\|off]`, `/compact` | Toggle planning mode or compact the context |
| `/review [instructions]`, `/diff` | Start a review or inspect session changes. Review also accepts `--base BRANCH` or `--commit SHA` |
| `/goal [objective]` | Inspect or set the goal. Use `edit`, `pause`, `resume`, or `clear` to manage it |
| `/recap`, `/find TEXT`, `/export [path]` | Read a recap, find conversation text, or export Markdown |
| `/copy [1–9]`, `/clear` | Copy a recent response by reverse index or clear the visible transcript |
| `/presets`, `/vim` | Manage saved prompts or compose in an external editor |
| `/attach PATH`, `/detach [number\|all]`, `/mention [query]`, `/ide` | Manage draft images, file mentions, or IDE context |
| `/pwd`, `/cd PATH`, `/init` | Inspect or change the working directory, or generate repository instructions |
| `/mcp`, `/plugins`, `/apps`, `/skills`, `/hooks` | Inspect or manage integrations |
| `/provider`, `/account`, `/login`, `/logout` | Manage model providers, accounts, or sign-in |
| `/memory`, `/experimental` | Inspect or change memory and feature settings |
| `/warnings`, `/approve [review-id]` | Inspect warnings and denied actions, or authorize a retry |
| `/ps`, `/clean` | Inspect or stop background terminals |
| `/statusline`, `/title`, `/pets` | Customize the status line, terminal title, or pet |
| `/voice`, `/daybreak` | Manage voice or Daybreak access |
| `/daemon`, `/exit` | Inspect the local background server or exit Better Codex |

Long command results open in a separate panel. Scroll with the arrow keys,
Page Up, Page Down, Home, or End. Press y to copy the result or Esc to return to the composer.
Short confirmations appear on the composer border.

To import a Claude Code setup, choose **Import Claude Code setup** in the
command palette (Ctrl+P) while running with `--no-daemon`.

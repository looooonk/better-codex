# Slash commands

Type `/` at the start of the session composer to see the commands supported by
Better Codex. The popup filters as you type and shows each command's current
description.

| Command             | Action                                                            |
| ------------------- | ----------------------------------------------------------------- |
| `/new`, `/resume`, `/fork` | Start, resume, or fork sessions |
| `/model`, `/permissions` | Select model, reasoning effort, or approval permissions |
| `/status`, `/agent` | Inspect session state or agents |
| `/review [instructions]` | Review changes; also accepts `--base BRANCH` or `--commit SHA` |
| `/plan [on\|off]` | Inspect or switch planning mode |
| `/mcp`, `/plugins`, `/skills`, `/hooks` | Manage integrations and inspect capabilities |
| `/memory [on\|off]` | Inspect or change this session's memory setting |
| `/experimental [name on\|off]` | Inspect features or change a feature setting |
| `/compact` | Compact the current context |
| `/ps`, `/clean` | List or stop background terminals |
| `/pwd`, `/cd PATH`, `/rename NAME` | Inspect or change session location and name |
| `/diff`, `/theme`, `/import` | Review changes, choose appearance, or import Claude Code setup |
| `/voice [on\|off\|mute\|unmute\|settings]` | Control realtime voice |
| `/ide [on\|off\|status]` | Include the connected IDE selection and open tabs in new messages |
| `/approve [review-id]` | Inspect denied actions or authorize one retry of a selected action |
| `/daemon [status]` | Inspect the local background server |
| `/daemon update latest` | Close the interface and update the daemon from Better Codex releases |
| `/daemon update from-cli` | Close the interface and pin the daemon to this CLI package |
| `/clear`            | Clear the visible transcript without deleting the saved session   |
| `/copy [1-9]`       | Copy the latest response, or an earlier response by reverse index |
| `/goal`             | Show the active goal                                              |
| `/goal <objective>` | Set or replace the active goal                                    |
| `/goal clear`       | Clear the active goal                                             |
| `/goal pause`       | Pause the active goal                                             |
| `/goal resume`      | Resume the active goal                                            |
| `/goal edit`        | Open the active objective in Vim or Neovim                        |
| `/login`            | Open account authentication                                       |
| `/logout`           | Sign out of the active account                                    |
| `/vim`              | Edit the prompt in Vim or Neovim                                  |
| `/exit`             | Exit Better Codex                                                 |

`/copy 1` selects the latest response, `/copy 2` the second latest, and so on
through `/copy 9`. Account changes and daemon updates wait until active work has
finished. Daemon updates are available only when connected to the local background
server; manage remote servers on their own host.

This list is specific to the full-screen Better Codex interface. Upstream Codex
CLI slash-command lists do not necessarily apply to this fork.

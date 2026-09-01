# Slash commands

Type `/` at the start of the session composer to see the commands supported by
Better Codex. The popup filters as you type and shows each command's current
description.

| Command             | Action                                                            |
| ------------------- | ----------------------------------------------------------------- |
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
through `/copy 9`. Account changes wait until active work has finished.

This list is specific to the full-screen Better Codex interface. Upstream Codex
CLI slash-command lists do not necessarily apply to this fork.

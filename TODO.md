# Better Codex To-Do List

The following is a list of changes that should be applied to better-codex soon.

- [ ] Remove "root" from the list of subagents, as it is just the main agent in the "CONVERSATION" pane.
- [ ] Display agents with their human-readable nickname, not the file-path--like name. However, maintain nesting behavior.
- [ ] Make subagent conversation panes more accesible by allowing the user to click subagent rows in the "Agents" tab to bring up their conversation logs.
- [ ] De-clutter subagent conversation panes to show less of raw tool and edit output, and make them more formatted to be digestible to the end user.
- [ ] Stop displaying the original prompt the user gave to the root Codex in the subagent panes.

The following is a list of items that better-codex should support down the road, but not immediately.

- [ ] Support a "true-yolo" mode, where every single possible disruption to a turn is automted. This includes security checks, model API limitations due to unavailability, etc.
- [ ] Support a variety of preset themes, such as tokyo night, catppuccin, dracula, gruvbox, etc, but also allow the user to port their own themes using a config file.
- [ ] Support better-codex unique features, such as customizable system prompts, agentic behavior, etc.
- [ ] Support TUI adjustments of Codex configurations, such as subagent count, access limitations, etc.
- [ ] Add support to run multiple Codex instances in the same better-codex TUI pane using tab switching and multiplexing.

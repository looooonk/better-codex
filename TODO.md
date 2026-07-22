# Better Codex To-Do List

The following is a list of changes that should be applied to better-codex soon.

- [x] When a model is steered, three sepearate copies of the steer message appear in the conversation log, from `YOU`, `AUDIT`, and `YOU` again. This should change, to only display one message from `YOU`.
- [x] CLI command tool calls should be truncated when they are too long.
- [x] Users should be able to use arrow keys, number keys, and the mouse when selecting actions on a permissions pop-up.
- [ ] Support for `/login` and `/logout` should be implemented, following the flow of the standard Codex CLI repository.
- [ ] There should be a timer measuring the current turn time (e.g. 1h 25m 47s) in the top bar. This should be different from the goal time, as the turn time resets every turn.

The following is a list of items that better-codex should support down the road, but not immediately.

- [ ] Support a "true-yolo" mode, where every single possible disruption to a turn is automted. This includes security checks, model API limitations due to unavailability, etc.
- [ ] Support a variety of preset themes, such as tokyo night, catppuccin, dracula, gruvbox, etc, but also allow the user to port their own themes using a config file.
- [ ] Support better-codex unique features, such as customizable system prompts, agentic behavior, etc.
- [ ] Support TUI adjustments of Codex configurations, such as subagent count, access limitations, etc.
- [ ] Add support to run multiple Codex instances in the same better-codex TUI pane using tab switching and multiplexing.

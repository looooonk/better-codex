# Non-interactive mode

Use `better-codex exec` when a script or CI job needs one agent turn without the
full-screen interface:

```sh
better-codex exec "summarize the current changes"
```

Pass `--json` for JSONL events or `--output-last-message <file>` to write the
final response to a file. A prompt can also come from standard input; run
`better-codex exec --help` for the options supported by the installed version.

Non-interactive mode uses the same authentication, configuration, sandbox, and
execution-policy backend as the TUI. See the
[Codex non-interactive reference](https://developers.openai.com/codex/noninteractive)
for the inherited execution model.

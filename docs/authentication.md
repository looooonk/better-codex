# Authentication

Better Codex supports ChatGPT sign-in and OpenAI API-key authentication. The
first launch presents the available methods before opening the dashboard.

Inside a session, use `/login` to open authentication again and `/logout` to
remove the active account. Finish or cancel active work before changing
accounts.

Authentication state is shared with retained Codex backend tools that use the
same Codex home directory. Never put API keys in repository configuration or
commit them to source control.

See the [Codex authentication reference](https://developers.openai.com/codex/auth)
for the underlying credential locations, environment variables, and managed
authentication options.

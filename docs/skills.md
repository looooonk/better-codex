# Skills

Skills package reusable instructions, scripts, and assets for coding agents.
Better Codex discovers skills through the retained Codex backend and exposes
available skills to sessions when their descriptions match the work.

Keep each skill narrowly scoped. Its `SKILL.md` should explain when to use it,
the required workflow, and which supporting files are relevant. Prefer scripts
and templates inside the skill over duplicating large procedures in prompts.

Repository-local skills belong under `.codex/skills`; user-level and system
skills can come from the configured Codex home. See the
[Codex skills guide](https://developers.openai.com/codex/skills) for the package
layout and discovery rules inherited by Better Codex.

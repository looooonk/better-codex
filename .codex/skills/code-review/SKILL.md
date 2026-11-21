---
name: code-review
description: Run a final code review on a pull request
---

Review the code yourself using all other code-review-* skills. Use subagents only when the user explicitly asks for multiple agents or delegation. If they do, follow their requested scope and agent count and give each reviewer the full path to the relevant skill.

Report every issue you find, including findings from any requested reviewers.
Use raw Markdown to report findings.
Number findings for ease of reference.
Each finding must include a specific file path and line number.

If the GitHub user running the review is the owner of the pull request add a `code-reviewed` label.
Do not leave GitHub comments unless explicitly asked.

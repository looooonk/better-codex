#!/usr/bin/env python3
"""Cross-platform shell launcher for `just` recipes.

This keeps recipe bodies as normal shell snippets while giving the justfile one
portable placeholder, `{args}`, for forwarding variadic recipe arguments.
"""

import os
import re
import shlex
from pathlib import Path
import shutil
import subprocess
import sys


ARGS_TOKEN = "{args}"
STDERR_NULL_TOKEN = "{stderr-null}"
POWERSHELL_ARGS = "@($args | Select-Object -Skip 1)"
POWERSHELL_STDERR_NULL = "2>$null; exit $LASTEXITCODE"
SH_ARGS = '"$@"'
SH_STDERR_NULL = "2>/dev/null"


def main() -> int:
    if len(sys.argv) < 2:
        print("just shell adapter expected a recipe command.", file=sys.stderr)
        return 1

    command = sys.argv[1]
    recipe_name = sys.argv[2] if len(sys.argv) > 2 else ""
    recipe_args = sys.argv[3:]

    if re.search(r"\bcargo\s+(?:build|run|clippy|nextest|bench)\b", command):
        sys.path.insert(0, str(Path(__file__).resolve().parent))
        from rusty_v8 import prepare

        os.environ.update(prepare([*shlex.split(command), *recipe_args]))

    if os.name == "nt":
        return run_powershell(command, recipe_name, recipe_args)
    else:
        return run_sh(command, recipe_name, recipe_args)


def run_sh(command: str, recipe_name: str, recipe_args: list[str]) -> int:
    command = command.replace(ARGS_TOKEN, SH_ARGS)
    command = command.replace(STDERR_NULL_TOKEN, SH_STDERR_NULL)
    os.execvp("sh", ["sh", "-cu", command, recipe_name, *recipe_args])


def run_powershell(command: str, recipe_name: str, recipe_args: list[str]) -> int:
    pwsh = shutil.which("pwsh.exe") or shutil.which("pwsh")
    if pwsh is None:
        print(
            "PowerShell ('pwsh') is required for Windows just recipes. "
            "Run 'just install' to install it.",
            file=sys.stderr,
        )
        return 1

    command = command.replace(ARGS_TOKEN, POWERSHELL_ARGS)
    command = command.replace(STDERR_NULL_TOKEN, POWERSHELL_STDERR_NULL)
    return subprocess.run(
        [
            pwsh,
            "-NoLogo",
            "-NoProfile",
            "-CommandWithArgs",
            command,
            recipe_name,
            *recipe_args,
        ],
        check=False,
    ).returncode


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Build the CLI and its helpers together, then launch the Cargo-reported executable."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

from rusty_v8 import prepare


BINARIES = ("codex", "codex-code-mode-host", "codex-exec-server")


def build(profile):
    command = [
        "cargo",
        "build",
        "--profile",
        profile,
        "--message-format=json-render-diagnostics",
    ]
    for package in ("codex-cli", "codex-code-mode-host", "codex-exec-server-cli"):
        command.extend(["-p", package])
    for binary in BINARIES:
        command.extend(["--bin", binary])
    executables = {}
    with subprocess.Popen(
        command,
        stdout=subprocess.PIPE,
        text=True,
        env={**os.environ, **prepare(command)},
    ) as cargo:
        for line in cargo.stdout:
            event = json.loads(line)
            if event.get("reason") == "compiler-artifact" and event.get("executable"):
                executables[event["target"]["name"]] = Path(event["executable"])
        status = cargo.wait()
    if status:
        raise subprocess.CalledProcessError(status, command)
    for binary in BINARIES:
        if binary not in executables or not executables[binary].is_file():
            raise ValueError(f"Cargo did not produce {binary}")
    return executables["codex"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", required=True)
    parser.add_argument("arguments", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    arguments = args.arguments[1:] if args.arguments[:1] == ["--"] else args.arguments
    try:
        binary = str(build(args.profile))
        os.execv(binary, [binary, *arguments])
    except subprocess.CalledProcessError as error:
        return error.returncode
    except (OSError, ValueError) as error:
        print(f"Could not launch Better Codex: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())

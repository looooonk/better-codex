#!/usr/bin/env python3

from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import unquote


archives, fallback = map(Path, sys.argv[1:3])
fallback.mkdir(parents=True, exist_ok=True)
retried = set()
for attempt in range(10):
    failed = set()
    with subprocess.Popen([
        "bazel", "build", f"--distdir={archives}", f"--distdir={fallback}",
        "-c", "opt", "//codex-rs/voice-host:codex-voice-host",
        "//third_party/voice:native_runtime",
    ], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True) as process:
        for line in process.stdout:
            print(line, end="", flush=True)
            if "Error downloading [" in line:
                failed.update(re.findall(
                    r"https://github.com/[^\s\],]+/releases/download/[^\s\],]+",
                    line,
                ))
        status = process.wait()
    if status == 0:
        sys.exit(0)
    pending = failed - retried
    if not pending or attempt == 9:
        sys.exit(status)
    for url in sorted(pending):
        match = re.fullmatch(
            r"https://github.com/([^/]+)/([^/]+)/releases/download/([^/]+)/([^/?#]+)",
            url,
        )
        if match is None:
            sys.exit(status)
        owner, repository, tag, name = map(unquote, match.groups())
        if Path(name).name != name or any(char in name for char in "*?[]"):
            raise ValueError(f"Unsupported archive name: {name}")
        print(f"Retrying {name} through the GitHub release API", flush=True)
        subprocess.run([
            "gh", "release", "download", tag, "--repo", f"{owner}/{repository}",
            "--pattern", name, "--dir", str(fallback), "--clobber",
        ], check=True)
        retried.add(url)
    # Bazel checks the pinned repository checksum before using a distdir file.

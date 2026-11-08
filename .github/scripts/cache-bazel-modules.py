#!/usr/bin/env python3

import base64
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
from urllib.parse import unquote


root = Path(__file__).resolve().parents[2]
output = Path(sys.argv[1])
output.mkdir(parents=True, exist_ok=True)
lock = json.loads((root / "MODULE.bazel.lock").read_text())["registryFileHashes"]


def resolve_module(item):
    url, checksum = item
    data = subprocess.check_output([
        "curl", "-fsSL", "--retry", "3", "--retry-all-errors",
        "--max-time", "30", "--retry-max-time", "60", url,
    ])
    if hashlib.sha256(data).hexdigest() != checksum:
        raise ValueError(f"Registry checksum mismatch: {url}")
    return resolve_release(json.loads(data))


def resolve_release(source):
    match = re.fullmatch(
        r"https://github.com/([^/]+)/([^/]+)/releases/download/([^/]+)/([^/?#]+)",
        source.get("url", ""),
    )
    if match is None:
        return None
    owner, repository, tag, name = map(unquote, match.groups())
    if Path(name).name != name or any(char in name for char in "*?[]"):
        raise ValueError(f"Unsupported archive name: {name}")
    if "sha256" in source:
        expected = bytes.fromhex(source["sha256"])
    else:
        algorithm, digest = source["integrity"].split("-", 1)
        if algorithm != "sha256":
            raise ValueError(f"Unsupported integrity algorithm: {algorithm}")
        expected = base64.b64decode(digest)
    return f"{owner}/{repository}", tag, name, expected


def download_module(module):
    repository, tag, name, expected = module
    archive = output / name
    subprocess.run([
        "gh", "release", "download", tag, "--repo", repository,
        "--pattern", name, "--dir", str(output), "--clobber",
    ], check=True)
    with archive.open("rb") as source:
        actual = hashlib.file_digest(source, "sha256").digest()
    if actual != expected:
        archive.unlink()
        raise ValueError(f"Archive checksum mismatch: {name}")


with ThreadPoolExecutor(max_workers=4) as pool:
    modules = [module for module in pool.map(
        resolve_module, [(url, checksum) for url, checksum in lock.items()
                         if url.endswith("/source.json")],
    ) if module is not None]
    voice = json.loads((root / "third_party/voice/sources.json").read_text())
    modules.extend(module for source in voice["sources"]
                   if (module := resolve_release(source)) is not None)
    if len({module[2] for module in modules}) != len(modules):
        raise ValueError("Module archive names must be unique")
    list(pool.map(download_module, modules))
print(f"Verified {len(modules)} Bazel module archives")

#!/usr/bin/env python3

import hashlib
import os
from pathlib import Path
import platform
import subprocess


version = "1.28.1"
checksums = {
    "bazelisk-darwin-amd64": "023225736cea5dc88f2b0807d5b1af4eb0f69a4ed45e3994b2c18c263bc80e48",
    "bazelisk-darwin-arm64": "dea3f3f5de2dbc5e269e0132cdd369d5efe738f7b973d5d4eb2b4f7055a97b39",
    "bazelisk-linux-amd64": "22e7d3a188699982f661cf4687137ee52d1f24fec1ec893d91a6c4d791a75de8",
    "bazelisk-linux-arm64": "8ded44b58a0d9425a4178af26cf17693feac3b87bdcfef0a2a0898fcd1afc9f2",
}
arch = {"x86_64": "amd64", "arm64": "arm64", "aarch64": "arm64"}[platform.machine()]
name = f"bazelisk-{platform.system().lower()}-{arch}"
expected = checksums[name]
directory = Path(os.environ["RUNNER_TEMP"]) / "release-bazelisk"
directory.mkdir(parents=True, exist_ok=True)
subprocess.run([
    "gh", "release", "download", f"v{version}", "--repo", "bazelbuild/bazelisk",
    "--pattern", name, "--dir", str(directory), "--clobber",
], check=True)
download = directory / name
if hashlib.sha256(download.read_bytes()).hexdigest() != expected:
    download.unlink()
    raise ValueError(f"Bazelisk checksum mismatch: {name}")
binary = download.replace(directory / "bazel")
binary.chmod(0o755)
with open(os.environ["GITHUB_PATH"], "a") as paths:
    print(directory, file=paths)

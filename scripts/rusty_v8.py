"""Fetch checksum-pinned sandboxed V8 artifacts for local Cargo commands."""

import hashlib
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
PROFILE = "ptrcomp_sandbox_release"


def checksums(data: bytes) -> dict[str, str]:
    entries = {}
    for line in data.decode().splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_.-]+)", line)
        if match is None or match[2] in entries:
            raise ValueError("invalid or duplicate V8 checksum entry")
        entries[match[2]] = match[1]
    return entries


def digest(path: Path) -> str:
    result = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            result.update(chunk)
    return result.hexdigest()


def fetch_verified(url: str, path: Path, expected: str) -> None:
    if path.is_file() and digest(path) == expected:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as temporary:
        staging = Path(temporary.name)
    try:
        for source in (
            url,
            url.replace("https://github.com/", "https://www.github.com/"),
        ):
            result = subprocess.run(
                [
                    "curl",
                    "-fLsS",
                    "--connect-timeout",
                    "15",
                    "--max-time",
                    "180",
                    "--output",
                    str(staging),
                    source,
                ],
                check=False,
            )
            if result.returncode == 0:
                break
        else:
            raise RuntimeError(
                f"could not download the pinned V8 artifact: {path.name}"
            )
        if digest(staging) != expected:
            raise ValueError(f"V8 checksum mismatch: {path.name}")
        staging.replace(path)
    finally:
        staging.unlink(missing_ok=True)


def target_for(arguments: list[str]) -> str:
    for index, argument in enumerate(arguments):
        if argument.startswith("--target="):
            return argument.removeprefix("--target=")
        if argument == "--target" and index + 1 < len(arguments):
            return arguments[index + 1]
    if target := os.environ.get("CARGO_BUILD_TARGET"):
        return target
    version = subprocess.check_output(["rustc", "-vV"], text=True)
    return next(
        line.removeprefix("host: ")
        for line in version.splitlines()
        if line.startswith("host: ")
    )


def prepare(arguments: list[str]) -> dict[str, str]:
    if os.environ.get("V8_FROM_SOURCE", "").lower() in {"1", "true"}:
        return {}
    archive_override = os.environ.get("RUSTY_V8_ARCHIVE")
    binding_override = os.environ.get("RUSTY_V8_SRC_BINDING_PATH")
    if archive_override or binding_override:
        if not archive_override or not binding_override:
            raise ValueError(
                "set RUSTY_V8_ARCHIVE and RUSTY_V8_SRC_BINDING_PATH together for a matching custom build"
            )
        return {}
    if os.environ.get("V8_FORCE_DEBUG", "").lower() in {"1", "true"}:
        raise ValueError(
            "audited V8 prebuilds use release mode; set V8_FROM_SOURCE=1 for a debug V8 build"
        )
    versions = re.findall(
        r'\[\[package\]\]\nname = "v8"\nversion = "([^"]+)"',
        (ROOT / "codex-rs/Cargo.lock").read_text(),
    )
    if len(versions) != 1:
        raise ValueError("expected exactly one pinned v8 crate in Cargo.lock")
    version = versions[0]
    target = target_for(arguments)
    archive = (
        f"rusty_v8_{PROFILE}_{target}.lib.gz"
        if target.endswith("-windows-msvc")
        else f"librusty_v8_{PROFILE}_{target}.a.gz"
    )
    binding = f"src_binding_{PROFILE}_{target}.rs"
    manifest = f"rusty_v8_{PROFILE}_{target}.sha256"
    trusted = checksums(
        (
            ROOT
            / f"third_party/v8/rusty_v8_{version.replace('.', '_')}_release_manifests.sha256"
        ).read_bytes()
    )
    if manifest not in trusted:
        raise ValueError(f"no audited sandboxed V8 prebuild for {target}")
    cache = (
        Path(os.environ.get("BETTER_CODEX_V8_CACHE", ROOT / "codex-rs/target/rusty-v8"))
        / version
        / target
    )
    base = f"https://github.com/openai/codex/releases/download/rusty-v8-v{version}"
    fetch_verified(f"{base}/{manifest}", cache / manifest, trusted[manifest])
    artifacts = checksums((cache / manifest).read_bytes())
    if set(artifacts) != {archive, binding}:
        raise ValueError(
            "V8 release manifest must contain the matching sandbox archive and bindings"
        )
    for name, expected in artifacts.items():
        fetch_verified(f"{base}/{name}", cache / name, expected)
    return {
        "RUSTY_V8_ARCHIVE": str((cache / archive).resolve()),
        "RUSTY_V8_SRC_BINDING_PATH": str((cache / binding).resolve()),
    }


if __name__ == "__main__":
    arguments = sys.argv[1:]
    if arguments[:1] == ["--"]:
        arguments = arguments[1:]
    if not arguments:
        raise SystemExit("usage: rusty_v8.py -- cargo COMMAND [ARGS]")
    os.environ.update(prepare(arguments))
    os.execvp(arguments[0], arguments)

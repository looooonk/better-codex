#!/usr/bin/env python3

import argparse
import gzip
import hashlib
import io
import json
import re
import subprocess
import tarfile
from dataclasses import dataclass
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
PACKAGE_ROOT = "better-codex-package"
RIPGREP_VERSION = "15.1.0"
VERSION_PATTERN = re.compile(
    r"^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$"
)


@dataclass(frozen=True)
class RipgrepRelease:
    archive: str
    sha256: str
    size: int


RIPGREP_RELEASES = {
    "aarch64-apple-darwin": RipgrepRelease(
        "ripgrep-15.1.0-aarch64-apple-darwin.tar.gz",
        "378e973289176ca0c6054054ee7f631a065874a352bf43f0fa60ef079b6ba715",
        1_777_930,
    ),
    "x86_64-apple-darwin": RipgrepRelease(
        "ripgrep-15.1.0-x86_64-apple-darwin.tar.gz",
        "64811cb24e77cac3057d6c40b63ac9becf9082eedd54ca411b475b755d334882",
        1_894_127,
    ),
    "aarch64-unknown-linux-musl": RipgrepRelease(
        "ripgrep-15.1.0-aarch64-unknown-linux-gnu.tar.gz",
        "2b661c6ef508e902f388e9098d9c4c5aca72c87b55922d94abdba830b4dc885e",
        1_869_959,
    ),
    "x86_64-unknown-linux-musl": RipgrepRelease(
        "ripgrep-15.1.0-x86_64-unknown-linux-musl.tar.gz",
        "1c9297be4a084eea7ecaedf93eb03d058d6faae29bbc57ecdaf5063921491599",
        2_263_077,
    ),
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build a Better Codex release package")
    parser.add_argument("--target", required=True, choices=sorted(RIPGREP_RELEASES))
    parser.add_argument("--version", required=True)
    parser.add_argument("--codex-bin", required=True, type=Path)
    parser.add_argument("--code-mode-host-bin", required=True, type=Path)
    parser.add_argument("--bwrap-bin", type=Path)
    parser.add_argument("--rg-bin", type=Path)
    parser.add_argument("--rg-license-dir", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    return parser.parse_args()


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def read_ripgrep_release(target: str) -> tuple[bytes, dict[str, bytes]]:
    release = RIPGREP_RELEASES[target]
    url = (
        f"https://github.com/BurntSushi/ripgrep/releases/download/"
        f"{RIPGREP_VERSION}/{release.archive}"
    )
    archive = subprocess.run(
        ["curl", "-fsSL", "--retry", "3", url],
        check=True,
        capture_output=True,
    ).stdout
    if len(archive) != release.size or sha256(archive) != release.sha256:
        raise ValueError(f"ripgrep archive verification failed for {target}")

    root = release.archive.removesuffix(".tar.gz")
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as tar:
        files = {}
        for name in ("rg", "COPYING", "LICENSE-MIT", "UNLICENSE"):
            member = tar.getmember(f"{root}/{name}")
            extracted = tar.extractfile(member)
            if extracted is None:
                raise ValueError(f"ripgrep archive entry is not a file: {member.name}")
            files[name] = extracted.read()
    return files.pop("rg"), files


def read_local_ripgrep(
    binary: Path, license_dir: Path
) -> tuple[Path, dict[str, bytes]]:
    licenses = {
        name: (license_dir / name).read_bytes()
        for name in ("COPYING", "LICENSE-MIT", "UNLICENSE")
    }
    return binary, licenses


def normalized_info(name: str, mode: int, size: int = 0) -> tarfile.TarInfo:
    info = tarfile.TarInfo(name)
    info.mode = mode
    info.size = size
    info.mtime = 0
    info.uid = 0
    info.gid = 0
    info.uname = ""
    info.gname = ""
    return info


def add_directory(tar: tarfile.TarFile, name: str) -> None:
    info = normalized_info(name.rstrip("/") + "/", 0o755)
    info.type = tarfile.DIRTYPE
    tar.addfile(info)


def add_bytes(tar: tarfile.TarFile, name: str, data: bytes, mode: int = 0o644) -> None:
    tar.addfile(normalized_info(name, mode, len(data)), io.BytesIO(data))


def add_path(tar: tarfile.TarFile, name: str, path: Path, mode: int) -> None:
    with path.open("rb") as source:
        tar.addfile(normalized_info(name, mode, path.stat().st_size), source)


def validate_inputs(args: argparse.Namespace) -> None:
    if not VERSION_PATTERN.fullmatch(args.version):
        raise ValueError(f"invalid release version: {args.version}")
    for path in (args.codex_bin, args.code_mode_host_bin):
        if not path.is_file():
            raise ValueError(f"required binary not found: {path}")
    is_linux = "linux" in args.target
    if is_linux != (args.bwrap_bin is not None):
        raise ValueError("--bwrap-bin is required only for Linux targets")
    if args.bwrap_bin is not None and not args.bwrap_bin.is_file():
        raise ValueError(f"bubblewrap binary not found: {args.bwrap_bin}")
    if (args.rg_bin is None) != (args.rg_license_dir is None):
        raise ValueError("--rg-bin and --rg-license-dir must be provided together")


def build_package(args: argparse.Namespace) -> None:
    validate_inputs(args)
    if args.rg_bin is None:
        rg, rg_licenses = read_ripgrep_release(args.target)
    else:
        rg, rg_licenses = read_local_ripgrep(args.rg_bin, args.rg_license_dir)

    metadata = {
        "layoutVersion": 1,
        "name": "better-codex",
        "version": args.version,
        "target": args.target,
        "entrypoint": "bin/codex",
        "path": "codex-path",
        "resources": "codex-resources",
    }
    directories = [
        PACKAGE_ROOT,
        f"{PACKAGE_ROOT}/bin",
        f"{PACKAGE_ROOT}/codex-path",
        f"{PACKAGE_ROOT}/codex-resources",
        f"{PACKAGE_ROOT}/licenses",
        f"{PACKAGE_ROOT}/licenses/ripgrep",
    ]
    if args.bwrap_bin is not None:
        directories.append(f"{PACKAGE_ROOT}/licenses/bubblewrap")

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("wb") as output:
        with gzip.GzipFile(
            filename="", mode="wb", fileobj=output, mtime=0
        ) as compressed:
            with tarfile.open(
                fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT
            ) as tar:
                for directory in directories:
                    add_directory(tar, directory)
                add_bytes(
                    tar,
                    f"{PACKAGE_ROOT}/codex-package.json",
                    json.dumps(metadata, indent=2, sort_keys=True).encode() + b"\n",
                )
                add_path(tar, f"{PACKAGE_ROOT}/bin/codex", args.codex_bin, 0o755)
                add_path(
                    tar,
                    f"{PACKAGE_ROOT}/bin/codex-code-mode-host",
                    args.code_mode_host_bin,
                    0o755,
                )
                if isinstance(rg, Path):
                    add_path(tar, f"{PACKAGE_ROOT}/codex-path/rg", rg, 0o755)
                else:
                    add_bytes(tar, f"{PACKAGE_ROOT}/codex-path/rg", rg, 0o755)
                if args.bwrap_bin is not None:
                    add_path(
                        tar,
                        f"{PACKAGE_ROOT}/codex-resources/bwrap",
                        args.bwrap_bin,
                        0o755,
                    )
                add_path(
                    tar, f"{PACKAGE_ROOT}/LICENSE", REPOSITORY_ROOT / "LICENSE", 0o644
                )
                add_path(
                    tar, f"{PACKAGE_ROOT}/NOTICE", REPOSITORY_ROOT / "NOTICE", 0o644
                )
                for name, contents in sorted(rg_licenses.items()):
                    add_bytes(tar, f"{PACKAGE_ROOT}/licenses/ripgrep/{name}", contents)
                if args.bwrap_bin is not None:
                    add_path(
                        tar,
                        f"{PACKAGE_ROOT}/licenses/bubblewrap/LICENSE",
                        REPOSITORY_ROOT / "codex-rs/vendor/bubblewrap/LICENSE",
                        0o644,
                    )


def main() -> None:
    args = parse_args()
    build_package(args)
    print(f"{file_sha256(args.output)}  {args.output.name}")


if __name__ == "__main__":
    main()

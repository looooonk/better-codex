#!/usr/bin/env python3

import hashlib
import json
import os
import platform
import stat
import subprocess
import tarfile
import tempfile
import unittest
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
BUILDER = REPOSITORY_ROOT / "scripts/build_release_package.py"
INSTALLER = REPOSITORY_ROOT / "scripts/install.sh"


def host_target() -> str:
    targets = {
        ("Darwin", "arm64"): "aarch64-apple-darwin",
        ("Darwin", "x86_64"): "x86_64-apple-darwin",
        ("Linux", "aarch64"): "aarch64-unknown-linux-musl",
        ("Linux", "x86_64"): "x86_64-unknown-linux-musl",
    }
    return targets[(platform.system(), platform.machine())]


class ReleasePackageTest(unittest.TestCase):
    def test_package_is_deterministic_and_installable(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            inputs = root / "inputs"
            inputs.mkdir()
            codex = self.write_executable(
                inputs / "codex",
                "#!/bin/sh\nprintf 'better-codex 0.1.0-alpha.1\\n'\n",
            )
            host = self.write_executable(
                inputs / "codex-code-mode-host",
                "#!/bin/sh\nexit 0\n",
            )
            rg = self.write_executable(inputs / "rg", "#!/bin/sh\nexit 0\n")
            target = host_target()
            for name in ("COPYING", "LICENSE-MIT", "UNLICENSE"):
                (inputs / name).write_text(f"{name}\n")

            def build_archive(output: Path, version: str) -> None:
                subprocess.run(
                    [
                        "python3",
                        str(BUILDER),
                        "--target",
                        target,
                        "--version",
                        version,
                        "--codex-bin",
                        str(codex),
                        "--code-mode-host-bin",
                        str(host),
                        "--rg-bin",
                        str(rg),
                        "--rg-license-dir",
                        str(inputs),
                        "--output",
                        str(output),
                        *(["--bwrap-bin", str(rg)] if "linux" in target else []),
                    ],
                    check=True,
                )

            first = root / "first.tar.gz"
            second = root / "second.tar.gz"
            for output in (first, second):
                build_archive(output, "0.1.0-alpha.1")

            self.assertEqual(self.digest(first), self.digest(second))
            with tarfile.open(first, "r:gz") as archive:
                metadata = json.load(
                    archive.extractfile("better-codex-package/codex-package.json")
                )
                self.assertEqual(
                    metadata,
                    {
                        "entrypoint": "bin/codex",
                        "layoutVersion": 1,
                        "name": "better-codex",
                        "path": "codex-path",
                        "resources": "codex-resources",
                        "target": target,
                        "version": "0.1.0-alpha.1",
                    },
                )
                executable_modes = {
                    member.name: stat.S_IMODE(member.mode)
                    for member in archive.getmembers()
                    if member.isfile() and stat.S_IMODE(member.mode) == 0o755
                }
                expected_modes = {
                    "better-codex-package/bin/codex": 0o755,
                    "better-codex-package/bin/codex-code-mode-host": 0o755,
                    "better-codex-package/codex-path/rg": 0o755,
                }
                if "linux" in target:
                    expected_modes["better-codex-package/codex-resources/bwrap"] = 0o755
                self.assertEqual(executable_modes, expected_modes)

            home = root / "home"
            install_root = root / "install"
            bin_dir = root / "bin"
            environment = os.environ | {
                "HOME": str(home),
                "BETTER_CODEX_ARCHIVE_PATH": str(first),
                "BETTER_CODEX_INSTALL_ROOT": str(install_root),
                "BETTER_CODEX_BIN_DIR": str(bin_dir),
            }
            subprocess.run(
                ["sh", str(INSTALLER), "--version", "0.1.0-alpha.1"],
                check=True,
                env=environment,
            )

            self.write_executable(
                codex,
                "#!/bin/sh\nprintf 'better-codex 0.1.0-alpha.2\\n'\n",
            )
            upgrade = root / "upgrade.tar.gz"
            build_archive(upgrade, "0.1.0-alpha.2")
            upgrade_environment = environment | {
                "BETTER_CODEX_ARCHIVE_PATH": str(upgrade)
            }
            subprocess.run(
                ["sh", str(INSTALLER), "--version", "0.1.0-alpha.2"],
                check=True,
                env=upgrade_environment,
            )

            output = subprocess.check_output(
                [bin_dir / "better-codex", "--version"],
                env=upgrade_environment,
                text=True,
            )
            self.assertEqual(output, "better-codex 0.1.0-alpha.2\n")
            self.assertEqual(
                (install_root / "current").resolve(),
                (install_root / "releases" / f"0.1.0-alpha.2-{target}").resolve(),
            )
            self.assertEqual(
                list(
                    (install_root / "releases" / f"0.1.0-alpha.1-{target}").glob(
                        ".current.*"
                    )
                ),
                [],
            )

    @staticmethod
    def write_executable(path: Path, contents: str) -> Path:
        path.write_text(contents)
        path.chmod(0o755)
        return path

    @staticmethod
    def digest(path: Path) -> str:
        return hashlib.sha256(path.read_bytes()).hexdigest()


if __name__ == "__main__":
    unittest.main()

#!/usr/bin/env python3

import hashlib
import os
import platform
import subprocess
import tarfile
import tempfile
import textwrap
import unittest
from pathlib import Path
from typing import Optional


REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
INSTALLER = REPOSITORY_ROOT / "scripts/install.sh"
VERSION = "0.1.0-alpha.7"


def host_target() -> str:
    targets = {
        ("Darwin", "arm64"): "aarch64-apple-darwin",
        ("Darwin", "x86_64"): "x86_64-apple-darwin",
        ("Linux", "aarch64"): "aarch64-unknown-linux-musl",
        ("Linux", "x86_64"): "x86_64-unknown-linux-musl",
    }
    return targets[(platform.system(), platform.machine())]


class InstallReleaseDiscoveryTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary_directory.name)
        self.target = host_target()
        self.archive = self.create_archive()
        self.checksum = self.root / f"{self.archive.name}.sha256"
        self.checksum.write_text(
            f"{hashlib.sha256(self.archive.read_bytes()).hexdigest()}  {self.archive.name}\n"
        )
        self.index = self.root / "latest-release"
        self.index.write_text(f"{VERSION}\n")
        self.metadata = self.root / "releases.json"
        self.metadata.write_text(f'{{\n  "tag_name": "v{VERSION}"\n}}\n')
        self.log = self.root / "curl.log"
        self.mock_bin = self.root / "bin"
        self.mock_bin.mkdir()
        self.write_mock_curl()

    def tearDown(self) -> None:
        self.temporary_directory.cleanup()

    def test_static_release_index_avoids_the_github_api(self) -> None:
        result = self.run_installer(index_mode="available", api_mode="unexpected")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("api.github.com", self.log.read_text())
        self.assertIn("raw.githubusercontent.com", self.log.read_text())

    def test_api_fallback_uses_an_available_github_token(self) -> None:
        result = self.run_installer(
            index_mode="missing",
            api_mode="available",
            github_token="test-token",
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        log = self.log.read_text()
        self.assertIn("api.github.com", log)
        self.assertIn("header=Authorization: Bearer test-token", log)

    def test_rate_limit_failure_is_actionable(self) -> None:
        result = self.run_installer(index_mode="missing", api_mode="rate-limited")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("GitHub API rate limit exhausted", result.stderr)
        self.assertIn("resets at Unix time 2000000000", result.stderr)
        self.assertIn("set GH_TOKEN or GITHUB_TOKEN", result.stderr)
        self.assertIn("pass --version VERSION", result.stderr)

    def run_installer(
        self,
        *,
        index_mode: str,
        api_mode: str,
        github_token: Optional[str] = None,
    ) -> subprocess.CompletedProcess[str]:
        home = self.root / "home"
        install_root = self.root / "install"
        launcher_bin = self.root / "launcher-bin"
        home.mkdir(exist_ok=True)
        environment = os.environ | {
            "HOME": str(home),
            "PATH": f"{self.mock_bin}{os.pathsep}{os.environ['PATH']}",
            "BETTER_CODEX_INSTALL_ROOT": str(install_root),
            "BETTER_CODEX_BIN_DIR": str(launcher_bin),
            "MOCK_API_MODE": api_mode,
            "MOCK_ARCHIVE": str(self.archive),
            "MOCK_CHECKSUM": str(self.checksum),
            "MOCK_CURL_LOG": str(self.log),
            "MOCK_INDEX": str(self.index),
            "MOCK_INDEX_MODE": index_mode,
            "MOCK_METADATA": str(self.metadata),
        }
        environment.pop("GH_TOKEN", None)
        environment.pop("GITHUB_TOKEN", None)
        if github_token is not None:
            environment["GH_TOKEN"] = github_token
        return subprocess.run(
            ["sh", str(INSTALLER)],
            capture_output=True,
            env=environment,
            text=True,
        )

    def create_archive(self) -> Path:
        package = self.root / "better-codex-package"
        binary_dir = package / "bin"
        binary_dir.mkdir(parents=True)
        self.write_executable(
            binary_dir / "codex",
            f"#!/bin/sh\nprintf 'better-codex {VERSION}\\n'\n",
        )
        self.write_executable(
            binary_dir / "codex-code-mode-host",
            "#!/bin/sh\nexit 0\n",
        )
        archive = self.root / f"better-codex-package-{self.target}.tar.gz"
        with tarfile.open(archive, "w:gz") as output:
            output.add(package, arcname=package.name)
        return archive

    def write_mock_curl(self) -> None:
        self.write_executable(
            self.mock_bin / "curl",
            textwrap.dedent(
                """\
                #!/bin/sh
                set -eu

                destination=
                headers=
                write_out=
                url=
                while [ "$#" -gt 0 ]; do
                    case "$1" in
                        -o|-D|-w)
                            option=$1
                            value=$2
                            shift 2
                            case "$option" in
                                -o) destination=$value ;;
                                -D) headers=$value ;;
                                -w) write_out=$value ;;
                            esac
                            ;;
                        -H)
                            printf 'header=%s\\n' "$2" >>"$MOCK_CURL_LOG"
                            shift 2
                            ;;
                        --connect-timeout|--retry)
                            shift 2
                            ;;
                        -*) shift ;;
                        *)
                            url=$1
                            shift
                            ;;
                    esac
                done
                printf 'url=%s\\n' "$url" >>"$MOCK_CURL_LOG"

                case "$url" in
                    *raw.githubusercontent.com*/scripts/latest-release)
                        [ "$MOCK_INDEX_MODE" = available ] || exit 22
                        cp "$MOCK_INDEX" "$destination"
                        ;;
                    *api.github.com*)
                        case "$MOCK_API_MODE" in
                            available)
                                cp "$MOCK_METADATA" "$destination"
                                printf 'HTTP/2 200\\r\\n\\r\\n' >"$headers"
                                status=200
                                ;;
                            rate-limited)
                                : >"$destination"
                                printf '%s\\r\\n' \\
                                    'HTTP/2 403' \\
                                    'x-ratelimit-remaining: 0' \\
                                    'x-ratelimit-reset: 2000000000' \\
                                    '' >"$headers"
                                status=403
                                ;;
                            *) exit 70 ;;
                        esac
                        [ -z "$write_out" ] || printf '%s' "$status"
                        ;;
                    *.tar.gz.sha256) cp "$MOCK_CHECKSUM" "$destination" ;;
                    *.tar.gz) cp "$MOCK_ARCHIVE" "$destination" ;;
                    *) exit 71 ;;
                esac
                """
            ),
        )

    @staticmethod
    def write_executable(path: Path, contents: str) -> None:
        path.write_text(contents)
        path.chmod(0o755)


if __name__ == "__main__":
    unittest.main()

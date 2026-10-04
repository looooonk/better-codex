import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import rusty_v8


class RustyV8Test(unittest.TestCase):
    def test_prepares_matching_sandbox_archive_and_bindings(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "codex-rs").mkdir()
            (root / "codex-rs/Cargo.lock").write_text(
                '[[package]]\nname = "v8"\nversion = "150.4.0"\n'
            )
            target = "aarch64-apple-darwin"
            archive = f"librusty_v8_ptrcomp_sandbox_release_{target}.a.gz"
            binding = f"src_binding_ptrcomp_sandbox_release_{target}.rs"
            manifest = f"rusty_v8_ptrcomp_sandbox_release_{target}.sha256"
            contents = {archive: b"archive", binding: b"bindings"}
            contents[manifest] = "".join(
                f"{hashlib.sha256(data).hexdigest()}  {name}\n"
                for name, data in contents.items()
            ).encode()
            trusted = root / "third_party/v8/rusty_v8_150_4_0_release_manifests.sha256"
            trusted.parent.mkdir(parents=True)
            trusted.write_text(
                f"{hashlib.sha256(contents[manifest]).hexdigest()}  {manifest}\n"
            )

            def download(url, path, expected):
                self.assertTrue(
                    url.startswith(
                        "https://github.com/openai/codex/releases/download/rusty-v8-v150.4.0/"
                    )
                )
                self.assertEqual(
                    hashlib.sha256(contents[path.name]).hexdigest(), expected
                )
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(contents[path.name])

            with (
                patch.object(rusty_v8, "ROOT", root),
                patch.dict(os.environ, {}, clear=True),
                patch.object(rusty_v8, "fetch_verified", side_effect=download),
            ):
                environment = rusty_v8.prepare(["--target", target])
            self.assertEqual(
                {key: Path(value).name for key, value in environment.items()},
                {"RUSTY_V8_ARCHIVE": archive, "RUSTY_V8_SRC_BINDING_PATH": binding},
            )

    def test_reuses_verified_cache_and_rejects_corrupt_download(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "archive.gz"
            path.write_bytes(b"verified")
            expected = hashlib.sha256(b"verified").hexdigest()
            with patch.object(rusty_v8.subprocess, "run") as process:
                rusty_v8.fetch_verified(
                    "https://github.com/example/archive.gz", path, expected
                )
                process.assert_not_called()
            path.write_bytes(b"old corrupt bytes")

            def corrupt(command, **kwargs):
                Path(command[command.index("--output") + 1]).write_bytes(
                    b"wrong artifact"
                )
                return subprocess.CompletedProcess(command, 0)

            with (
                patch.object(rusty_v8.subprocess, "run", side_effect=corrupt),
                self.assertRaisesRegex(ValueError, "checksum mismatch"),
            ):
                rusty_v8.fetch_verified(
                    "https://github.com/example/archive.gz", path, expected
                )
            self.assertEqual(path.read_bytes(), b"old corrupt bytes")
            self.assertEqual(list(path.parent.iterdir()), [path])

    def test_custom_build_requires_a_matching_pair_or_source_build(self):
        for environment, expected in [
            ({"V8_FROM_SOURCE": "1"}, {}),
            (
                {"RUSTY_V8_ARCHIVE": "custom", "RUSTY_V8_SRC_BINDING_PATH": "bindings"},
                {},
            ),
        ]:
            with patch.dict(os.environ, environment, clear=True):
                self.assertEqual(rusty_v8.prepare([]), expected)
        with (
            patch.dict(os.environ, {"RUSTY_V8_ARCHIVE": "custom"}, clear=True),
            self.assertRaisesRegex(ValueError, "together"),
        ):
            rusty_v8.prepare([])

    def test_manifest_rejects_duplicate_and_unsafe_paths(self):
        checksum = "a" * 64
        for content in (
            f"{checksum}  ../artifact\n",
            f"{checksum}  archive\n{checksum}  archive\n",
        ):
            with self.subTest(content=content), self.assertRaises(ValueError):
                rusty_v8.checksums(content.encode())


if __name__ == "__main__":
    unittest.main()

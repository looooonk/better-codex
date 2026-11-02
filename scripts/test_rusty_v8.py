import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import rusty_v8
from third_party.v8 import correct_release_binding


class RustyV8Test(unittest.TestCase):
    def test_prepares_matching_sandbox_archive_and_bindings(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "codex-rs").mkdir()
            (root / "codex-rs/Cargo.lock").write_text(
                '[[package]]\nname = "v8"\nversion = "152.2.0"\n'
            )
            target = "aarch64-apple-darwin"
            archive = f"librusty_v8_ptrcomp_sandbox_release_{target}.a.gz"
            binding = f"src_binding_ptrcomp_sandbox_release_{target}.rs"
            manifest = f"rusty_v8_ptrcomp_sandbox_release_{target}.sha256"
            contents = {
                archive: b"archive",
                binding: correct_release_binding.ORIGINAL + b"\n",
            }
            contents[manifest] = "".join(
                f"{hashlib.sha256(data).hexdigest()}  {name}\n"
                for name, data in contents.items()
            ).encode()
            trusted = root / "third_party/v8/rusty_v8_152_2_0_release_manifests.sha256"
            trusted.parent.mkdir(parents=True)
            trusted.write_text(
                f"{hashlib.sha256(contents[manifest]).hexdigest()}  {manifest}\n"
            )

            def download(url, path, expected):
                self.assertTrue(
                    url.startswith(
                        "https://github.com/openai/codex/releases/download/rusty-v8-v152.2.0/"
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
                patch.dict(
                    correct_release_binding.KNOWN_BINDINGS,
                    {
                        target: hashlib.sha256(contents[binding]).hexdigest(),
                    },
                ),
            ):
                environment = rusty_v8.prepare(["--target", target])
            self.assertEqual(
                {key: Path(value).name for key, value in environment.items()},
                {"RUSTY_V8_ARCHIVE": archive, "RUSTY_V8_SRC_BINDING_PATH": binding},
            )
            derived = Path(environment["RUSTY_V8_SRC_BINDING_PATH"])
            self.assertEqual(derived.parent.name, "corrected")
            self.assertEqual(
                derived.read_bytes(), correct_release_binding.CORRECTED + b"\n"
            )
            self.assertEqual(
                (derived.parent.parent / binding).read_bytes(), contents[binding]
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

    def test_binding_correction_preserves_original_and_repairs_derived_cache(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "original.rs"
            output = Path(directory) / "corrected/binding.rs"
            original = (
                b"pub const OTHER: usize = 0;\n"
                + correct_release_binding.ORIGINAL
                + b"\n"
            )
            source.write_bytes(original)
            for target in correct_release_binding.KNOWN_BINDINGS:
                with (
                    self.subTest(target=target),
                    patch.dict(
                        correct_release_binding.KNOWN_BINDINGS,
                        {target: hashlib.sha256(original).hexdigest()},
                    ),
                ):
                    actual = correct_release_binding.correct_binding(
                        "152.2.0", target, source, output
                    )
                    expected = original.replace(
                        correct_release_binding.ORIGINAL,
                        correct_release_binding.CORRECTED,
                    )
                    self.assertEqual(
                        (actual, actual.read_bytes(), source.read_bytes()),
                        (output, expected, original),
                    )
                    output.write_bytes(b"interrupted old cache")
                    correct_release_binding.correct_binding(
                        "152.2.0", target, source, output
                    )
                    self.assertEqual(output.read_bytes(), expected)

    def test_binding_correction_rejects_unknown_content_without_writing(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "original.rs"
            output = Path(directory) / "corrected.rs"
            source.write_bytes(correct_release_binding.ORIGINAL)
            output.write_bytes(b"existing output")
            for target in correct_release_binding.KNOWN_BINDINGS:
                with (
                    self.subTest(target=target),
                    self.assertRaisesRegex(ValueError, "unrecognized"),
                ):
                    correct_release_binding.correct_binding(
                        "152.2.0", target, source, output
                    )
            self.assertEqual(source.read_bytes(), correct_release_binding.ORIGINAL)
            self.assertEqual(output.read_bytes(), b"existing output")

    def test_binding_correction_requires_unique_capacity_and_separate_output(self):
        target = "aarch64-apple-darwin"
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "original.rs"
            output = Path(directory) / "corrected.rs"
            for contents in [b"missing capacity", correct_release_binding.ORIGINAL * 2]:
                source.write_bytes(contents)
                with patch.dict(
                    correct_release_binding.KNOWN_BINDINGS,
                    {target: hashlib.sha256(contents).hexdigest()},
                ):
                    with self.assertRaisesRegex(ValueError, "exactly one"):
                        correct_release_binding.correct_binding(
                            "152.2.0", target, source, output
                        )
                self.assertFalse(output.exists())
            source.write_bytes(correct_release_binding.ORIGINAL)
            output.hardlink_to(source)
            with patch.dict(
                correct_release_binding.KNOWN_BINDINGS,
                {target: hashlib.sha256(source.read_bytes()).hexdigest()},
            ):
                for alias in [source, output]:
                    with self.assertRaisesRegex(ValueError, "must not replace"):
                        correct_release_binding.correct_binding(
                            "152.2.0", target, source, alias
                        )
            self.assertEqual(source.read_bytes(), correct_release_binding.ORIGINAL)

    def test_binding_correction_leaves_other_versions_and_platforms_unchanged(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "original.rs"
            output = Path(directory) / "corrected.rs"
            source.write_bytes(correct_release_binding.ORIGINAL)
            for version, target in [
                ("152.2.1", "aarch64-apple-darwin"),
                ("152.2.0", "aarch64-pc-windows-msvc"),
                ("152.2.0", "x86_64-pc-windows-msvc"),
                ("152.2.0", "riscv64-unknown-linux-gnu"),
            ]:
                with self.subTest(version=version, target=target):
                    self.assertEqual(
                        correct_release_binding.correct_binding(
                            version, target, source, output
                        ),
                        source,
                    )
                    self.assertFalse(output.exists())
                    self.assertEqual(
                        source.read_bytes(), correct_release_binding.ORIGINAL
                    )


if __name__ == "__main__":
    unittest.main()

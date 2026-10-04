import argparse
import hashlib
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest

from build_release_package import build_package

VOICE_ROOT = Path(__file__).resolve().parents[1] / "third_party/voice"
sys.path.insert(0, str(VOICE_ROOT))
from runtime import PLUGINS, digest, required_library_paths


class VoicePackageTest(unittest.TestCase):
    def test_release_archive_contains_verified_voice_closure(self):
        for target in ("aarch64-apple-darwin", "x86_64-unknown-linux-musl"):
            with (
                self.subTest(target=target),
                tempfile.TemporaryDirectory() as directory,
            ):
                root = Path(directory)
                voice_target = target.replace("-linux-musl", "-linux-gnu")
                binary = root / "binary"
                binary.write_bytes(b"executable")
                binary.chmod(0o755)
                for name in ("COPYING", "LICENSE-MIT", "UNLICENSE"):
                    (root / name).write_text(name)
                runtime = root / "runtime"
                runtime.mkdir()
                plugin = (
                    "plugins/libgst{}.dylib"
                    if "darwin" in target
                    else "lib/gstreamer-1.0/libgst{}.so"
                )
                plugins = [plugin.format(name) for name in PLUGINS]
                libraries = []
                for name in (*plugins, *required_library_paths(voice_target)):
                    path = runtime / name
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(name.encode())
                    libraries.append({"path": name, "sha256": digest(path)})
                commit = "a" * 40
                (runtime / "runtime.json").write_text(
                    json.dumps(
                        {
                            "schemaVersion": 1,
                            "developmentOnly": False,
                            "distribution": "publicRelease",
                            "target": voice_target,
                            "sourceCommit": commit,
                            "sourceManifestSha256": digest(VOICE_ROOT / "sources.json"),
                            "plugins": plugins,
                            "libraries": libraries,
                        }
                    )
                )
                args = argparse.Namespace(
                    target=target,
                    version="0.1.0-alpha.15",
                    codex_bin=binary,
                    code_mode_host_bin=binary,
                    bwrap_bin=binary if "linux" in target else None,
                    rg_bin=binary,
                    rg_license_dir=root,
                    output=root / "package.tar.gz",
                    voice_host_bin=binary,
                    voice_runtime_dir=runtime,
                    build_commit=commit,
                )
                build_package(args)
                first = args.output.read_bytes()
                build_package(args)
                self.assertEqual(args.output.read_bytes(), first)
                with tarfile.open(args.output, "r:gz") as archive:
                    prefix = "better-codex-package/"
                    manifest = json.load(
                        archive.extractfile(
                            prefix + "codex-resources/voice/manifest.json"
                        )
                    )
                    self.assertEqual(manifest["buildCommit"], commit)
                    self.assertEqual(manifest["appTarget"], target)
                    self.assertEqual(manifest["voiceTarget"], voice_target)
                    for name, expected in manifest["sha256"].items():
                        self.assertEqual(
                            hashlib.sha256(
                                archive.extractfile(prefix + name).read()
                            ).hexdigest(),
                            expected,
                        )
                    self.assertTrue(
                        archive.getmember(
                            prefix + "codex-resources/voice/NOTICE.md"
                        ).isfile()
                    )
                (runtime / plugins[0]).write_bytes(b"changed")
                with self.assertRaisesRegex(ValueError, "digest mismatch"):
                    build_package(args)


if __name__ == "__main__":
    unittest.main()

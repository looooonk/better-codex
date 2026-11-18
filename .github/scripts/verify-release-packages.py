"""Check the complete release asset set against the tag before signing it."""

import hashlib
import json
import os
from pathlib import Path
import tarfile


def verify_packages(directory: Path, targets: list[str], version: str, commit: str) -> None:
    expected = set()
    for target in targets:
        archive = directory / f"better-codex-package-{target}.tar.gz"
        checksum = directory / f"{archive.name}.sha256"
        expected.update((archive, checksum))

        with archive.open("rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        if checksum.read_text().split() != [digest, archive.name]:
            raise ValueError(f"checksum mismatch for {archive.name}")

        with tarfile.open(archive, "r|gz") as package:
            for member in package:
                if member.name == "better-codex-package/codex-resources/voice/manifest.json":
                    if not member.isfile():
                        raise ValueError(f"invalid manifest in {archive.name}")
                    with package.extractfile(member) as source:
                        manifest = json.load(source)
                    break
            else:
                raise ValueError(f"missing manifest in {archive.name}")

        if (manifest["appTarget"], manifest["appVersion"], manifest["buildCommit"]) != (
            target, version, commit
        ):
            raise ValueError(f"{archive.name} does not match {target} at {version} ({commit})")

    unexpected = set(directory.iterdir()) - expected
    if unexpected:
        raise ValueError(f"unexpected release assets: {sorted(path.name for path in unexpected)}")


if __name__ == "__main__":
    matrix = json.loads(os.environ["MATRIX"])["include"]
    verify_packages(
        Path("dist"),
        [entry["target"] for entry in matrix],
        os.environ["VERSION"],
        os.environ["SOURCE_SHA"],
    )

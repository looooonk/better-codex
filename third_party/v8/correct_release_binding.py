"""Repair the verified 152.2.0 Unix binding's stale TypedArray capacity."""

import argparse
import hashlib
from pathlib import Path
import tempfile


KNOWN_BINDINGS = {
    f"{arch}-apple-darwin": "b9d5652ebca515eb2bf4c2ee4a6551b2567117b5aa720f430d2032616dd7e3b5"
    for arch in ("aarch64", "x86_64")
} | {
    f"{arch}-unknown-linux-{libc}": "3e4d5fecdbe0f98a6017e69d7ca82519a77fb032c5eb5e4cdbecdd3e2ec519d5"
    for arch in ("aarch64", "x86_64")
    for libc in ("gnu", "musl")
}
ORIGINAL = b"pub const v8__TYPED_ARRAY_MAX_SIZE_IN_HEAP: usize = 0;"
CORRECTED = b"pub const v8__TYPED_ARRAY_MAX_SIZE_IN_HEAP: usize = 64;"


def correct_binding(version: str, target: str, source: Path, output: Path) -> Path:
    if version != "152.2.0" or target not in KNOWN_BINDINGS:
        return source
    contents = source.read_bytes()
    if hashlib.sha256(contents).hexdigest() != KNOWN_BINDINGS[target]:
        raise ValueError("unrecognized V8 binding; refusing the 152.2.0 correction")
    if contents.count(ORIGINAL) != 1:
        raise ValueError("verified V8 binding must contain exactly one stale capacity")
    if source.resolve() == output.resolve() or (
        output.exists() and source.samefile(output)
    ):
        raise ValueError("corrected V8 binding must not replace its source")
    corrected = contents.replace(ORIGINAL, CORRECTED)
    if output.is_file() and output.read_bytes() == corrected:
        return output
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=output.parent, delete=False) as temporary:
        staging = Path(temporary.name)
        temporary.write(corrected)
    try:
        staging.replace(output)
    finally:
        staging.unlink(missing_ok=True)
    return output


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    print(correct_binding(args.version, args.target, args.input, args.output))

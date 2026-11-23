# Releasing Better Codex

GitHub releases are built for Apple Silicon and Intel macOS, plus ARM64 and
x86_64 Linux. The workflow publishes archives only for tags.

1. Update `workspace.package.version` in `codex-rs/Cargo.toml`.
2. Refresh `codex-rs/Cargo.lock` and run `just bazel-lock-update`.
3. Merge the version change into `main`.
4. Create and push a matching tag:

   ```sh
   git tag -a v0.1.0-alpha.1 -m "Better Codex 0.1.0-alpha.1"
   git push origin v0.1.0-alpha.1
   ```

The release workflow validates the tag against Cargo.toml, builds and smoke
tests all four packages, creates SHA-256 files and build attestations, then
publishes the GitHub release. The final job updates `scripts/latest-release` on
`main` so the installer and `better-codex update` pick up the new version.
Tags containing a prerelease suffix are marked as prereleases.

If all four packages passed their smoke tests and publishing failed, rerun the
current release workflow with the tag and the failed run's numeric ID:

```sh
gh workflow run release.yml --ref main -f release_tag="$tag" -f package_run_id="$run_id"
```

This reuses the retained packages, checks their targets, versions, source
commits, and checksums, and records the original run in the attestation.

Use the workflow's manual dispatch to test the build matrix without publishing.
The optional version input must match Cargo.toml.

macOS archives are currently unsigned. Code signing and notarization can be
added later without changing the archive layout or installer.

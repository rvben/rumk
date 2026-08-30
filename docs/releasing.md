# Releasing Rumk

Rumk releases are intentionally fail-safe: validation and every platform build finish before a
draft GitHub release is created, crates.io uses short-lived trusted-publishing credentials, and the
GitHub release remains a draft until crate publication succeeds.

## One-time repository setup

1. Make the GitHub repository public before enabling artifact attestations on a free plan.
2. Create a protected `release` environment and require reviewer approval.
3. Configure `rumk` on crates.io with this trusted publisher:
   - repository: `rvben/rumk`
   - workflow: `release.yml`
   - environment: `release`
4. Restrict GitHub Actions to approved actions and require full-length commit SHAs.
5. Protect `main`, require the CI workflow, and enable private vulnerability reporting.

No long-lived crates.io token is required.

## Preparing a release

1. Choose the next SemVer version. Continue the `0.0.x` sequence while the README says alpha.
2. Update `Cargo.toml`, `Cargo.lock`, and `CHANGELOG.md` to the identical version.
3. Run `make release-check`. It verifies formatting, Clippy, tests, the corpus, the package
   allowlist, and `cargo publish --dry-run`.
4. Commit the release preparation using Conventional Commits.
5. With explicit approval, push the preparation commit and run the Release workflow manually.
   Manual dispatch is always a non-publishing dry run that builds the complete platform matrix and
   produces an assembled checksum bundle.
6. Inspect every archive and its `SHA256SUMS` entry.
7. With explicit approval, create and push the signed tag `v<VERSION>`.

The tag-triggered workflow validates that the tag, Cargo package, and changelog versions match.
It then builds and smoke-tests native binaries for Linux x86-64/ARM64, macOS Intel/Apple Silicon,
and Windows x86-64. Only after all jobs pass does it create a draft release, publish the crate, and
publish the GitHub release.

## Failure handling

- If validation or a build fails, nothing public has been created. Fix the problem and retry.
- If a draft release exists but crates.io has not accepted the crate, delete the draft. If the tag
  existed only briefly and no release, artifact, package, checksum, or attestation was published,
  delete and recreate the tag, then retry the same version.
- Once crates.io or any release artifact, checksum, or attestation is public, preserve the tag and
  prepare a patch release. Never replace already published contents.
- Cargo can time out after an upload succeeds. Verify the crates.io version before deciding whether
  a failed publish step is safe to retry.

Publishing, pushing tags, and changing crates.io or GitHub settings are external actions and always
require explicit approval.

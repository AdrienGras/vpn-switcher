# Releasing

Releases are cut by pushing a `v*` tag. `release.yml` builds the archives for
four targets (`x86_64` and `aarch64` Linux musl, `x86_64` and `aarch64` macOS),
with completions, the man page, checksums and build provenance attestations,
publishes the GitHub release, and, once enabled, publishes to crates.io.

## Normal release

1. On a branch, bump `version` in `Cargo.toml`, run `cargo build` to update
   `Cargo.lock`, and add a `## [x.y.z] - <date>` section to `CHANGELOG.md`
   with its compare link. Use the real release date.
2. Check: `cargo fmt --all --check`, `cargo clippy --all-targets --locked -- -D warnings`,
   `cargo test`, `cargo +1.87 test --locked`, `cargo deny check`,
   `.github/scripts/check-version.sh vX.Y.Z`, `cargo publish --dry-run --locked`
   (the dry run does not upload anything).
3. Merge to `main` once CI is green.
4. Tag and push: `git tag vX.Y.Z && git push origin vX.Y.Z`.
5. Watch the `release` workflow. It fails early if the tag does not match
   `Cargo.toml` or if the changelog has no section for it.
6. Verify the result:
   `cargo binstall vpn-switcher --root "$(mktemp -d)" --no-confirm`, and
   `gh attestation verify vpns-<target>.tar.gz --repo AdrienGras/vpn-switcher`.

The `crates-io` job runs only when the repository variable
`CRATES_IO_TRUSTED_PUBLISHING` is `true` (see below).

## Repository settings

- **Actions > General > "Allow GitHub Actions to create and approve pull
  requests"** must be enabled: `demo.yml` opens a pull request with the
  regenerated GIF. (Already enabled.)
- **Settings > Code security > Private vulnerability reporting** must be
  enabled so that `SECURITY.md` is accurate.

## First release (0.3.0) runbook

Order matters. crates.io cannot declare a trusted publisher before the crate
exists, so the first publish is manual.

### Before tagging

1. Merge the release PR into `main`.
2. Run the `demo` workflow (Actions > demo > Run workflow). It opens a pull
   request with `demo/demo.gif`.
3. Merge the GIF pull request. The README published on crates.io is frozen at
   publish time, so its GIF must already exist.
4. Set the real date in the `## [0.3.0]` changelog heading if it changed.

### Tag

1. Push the tag: `git tag v0.3.0 && git push origin v0.3.0`. The GitHub release
   becomes public with its archives; the `crates-io` job is skipped because the
   variable is not set yet.
2. Publish by hand, from the tagged commit:
   ```sh
   git checkout v0.3.0
   cargo login            # a crates.io API token
   cargo publish --locked
   ```
   The archives are already public, so `cargo binstall` works as soon as the
   version appears.
3. On crates.io, declare the trusted publisher: repository
   `AdrienGras/vpn-switcher`, workflow `release.yml` (the GitHub environment is
   optional).
4. Create the repository variable `CRATES_IO_TRUSTED_PUBLISHING` with the value
   `true` (Settings > Secrets and variables > Actions > Variables). Later tags
   publish by themselves.
5. Revoke the API token you used in step 2.
6. Enable private vulnerability reporting (Settings > Code security).

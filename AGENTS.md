# lv Agent Notes

## Release New Version

- Update version in `Cargo.toml`.
- Run local checks:
  - `make check`
  - `make test`
  - `bash scripts/native-env.sh cargo clippy -- -D warnings`
  - `bash scripts/native-env.sh cargo fmt --check`
- Run local Flatpak release smoke for host arch:
  - `make flatpak-release`
- Do not use QEMU for release Flatpak builds. `x86_64` Flatpak builds require native x86_64 Linux. `aarch64` Flatpak builds require native ARM64 Linux or GitHub `ubuntu-24.04-arm`.
- Commit and push version/release changes.
- Create or publish GitHub release tag, for example:
  - `gh release create lv-vX.Y.Z --target main --title "lv: vX.Y.Z" --notes-file RELEASE_NOTES.md`
- GitHub release asset workflow runs on published releases.
- Release workflow must publish Linux Flatpak assets only:
  - `lv-X.Y.Z-x86_64.flatpak`
  - `lv-X.Y.Z-x86_64.flatpak.sha256`
  - `lv-X.Y.Z-aarch64.flatpak`
  - `lv-X.Y.Z-aarch64.flatpak.sha256`
- Release is valid only after both Flatpak smoke jobs pass. Smoke verifies install, scan, bundled libmpv linkage, and real video playback under Xvfb.
- If release workflow fails, inspect job logs with `gh run view <run-id> --log`, fix workflow/package issue, push fix, then recreate or retarget release tag to fixed commit.

# lv Agent Notes

## Release New Version

The release pipeline must run through GitHub Actions. Do not publish releases from a local machine: release assets are built and smoked on multiple GitHub-hosted targets.

### How it works

1. **Conventional commits**: All commits to `main` follow [Conventional Commits](https://www.conventionalcommits.org/):
   - `feat:` - new feature (minor bump)
   - `fix:` - bug fix (patch bump)
   - `feat!:` or `fix!:` - breaking change (major bump)
   - `docs:`, `chore:`, `refactor:`, `test:`, `style:` - no release

2. **release-please** (`.github/workflows/release.yml`): Runs on every push to `main`. Scans commits since last release, maintains a Release PR with the version bump + release notes. When the Release PR is merged, it creates the `vX.Y.Z` tag + GitHub Release.

3. **Flatpak build** (`.github/workflows/build.yml`): Triggered on release `published`. Builds Flatpak for x86_64 + aarch64, smokes them, attaches assets to the release.

### Release checklist

1. Ensure all changes on `main` use conventional commit messages.
2. Push to `main` - release-please creates/updates a Release PR.
3. Review and merge the Release PR - release-please tags and creates the GitHub Release.
4. `build.yml` runs automatically: builds Flatpaks, smokes tests, publishes assets.
5. Verify both smoke jobs pass for x86_64 + aarch64.
6. Verify release notes are specific to changes since the previous release. Keep docs/test/chore noise out unless it affects users or packaging.
7. If release workflow fails, inspect job logs with `gh run view <run-id> --log`, push fix to `main`, then rerun or recreate the GitHub Release so `build.yml` publishes assets from CI.

### Manual release (fallback)

Only if release-please is broken, use GitHub CI as the builder:
1. Commit version updates on `main` (`Cargo.toml`, `Cargo.lock`, `.release-please-manifest.json`).
2. Create a GitHub Release with tag `vX.Y.Z`, target `main`, and concise notes.
3. Let `build.yml` build, smoke, and attach Flatpak assets.
4. Verify release assets and checksums are attached by CI.

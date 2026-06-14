# lv Agent Notes

## Release New Version

The release pipeline is fully automated via GitHub Actions. Do not manually bump versions or create tags.

### How it works

1. **Conventional commits**: All commits to `main` follow [Conventional Commits](https://www.conventionalcommits.org/):
   - `feat:` — new feature (minor bump)
   - `fix:` — bug fix (patch bump)
   - `feat!:` or `fix!:` — breaking change (major bump)
   - `docs:`, `chore:`, `refactor:`, `test:`, `style:` — no release

2. **release-please** (`.github/workflows/release.yml`): Runs on every push to `main`. Scans commits since last release, maintains a Release PR with the version bump + changelog. When the Release PR is merged, it creates the tag + GitHub Release.

3. **Flatpak build** (`.github/workflows/build.yml`): Triggered on release `published`. Builds Flatpak for x86_64 + aarch64, smokes them, attaches assets to the release.

### Release checklist

1. Ensure all changes on `main` use conventional commit messages.
2. Push to `main` — release-please creates/updates a Release PR.
3. Review and merge the Release PR — release-please tags and creates the GitHub Release.
4. `build.yml` runs automatically: builds Flatpaks, smokes tests, publishes assets.
5. Verify both smoke jobs pass for x86_64 + aarch64.
6. If release workflow fails, inspect job logs with `gh run view <run-id> --log`, push fix to `main`, then retarget or recreate the release tag.

### Manual release (fallback)

Only if release-please is broken:
- `gh release create lv-vX.Y.Z --target main --title "lv: vX.Y.Z" --notes-file CHANGELOG.md`
- Verify Flatpak assets are attached.

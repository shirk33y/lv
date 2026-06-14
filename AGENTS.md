# lv Agent Notes

## Release New Version

The release pipeline must run through GitHub Actions. Do not publish releases from a local machine: release assets are built and smoked on multiple GitHub-hosted targets.

### How it works

1. **Conventional commits**: All commits to `main` must be release-please friendly.

2. **release-please** (`.github/workflows/release.yml`): Runs on every push to `main`. Scans commits since last release and maintains a Release PR with the version bump + release notes. It does not publish the release.

3. **Flatpak build** (`.github/workflows/build.yml`): Triggered on the merged release commit on `main`. Builds Flatpak for x86_64 + aarch64, smokes them, then creates the GitHub Release and attaches assets.

### Commit messages

Use Conventional Commits that describe user-visible impact:

- `feat: add searchable metadata filters` - release note + version bump
- `fix: keep cursor valid after watcher delete` - release note + version bump
- `perf: reuse ffprobe handle per worker` - release note + version bump
- `docs: explain Flatpak install modes` - no release note by default
- `test: cover daemon command queue` - no release note by default
- `chore: update release config` - no release note by default

Use scopes when they add clarity:

- `feat(cli): add duration filter`
- `fix(flatpak): bundle mpv runtime dependency`
- `docs(release): document CI-only release path`

Breaking changes must use `!` and a footer:

```text
feat(cli)!: rename sync background flag

BREAKING CHANGE: lv sync --background is now lv sync -b.
```

Keep squash commit titles specific. Avoid vague titles like `update`, `misc fixes`, `work`, or `release changes`; release-please turns commit titles into release notes.

Do not put unrelated docs, tests, refactors, and features in one commit. Split commits so release notes stay narrow.

For an intentional one-off version, prefer a dedicated commit footer:

```text
Release-As: 0.1.7
```

Do not hand-edit generated release notes unless correcting scope or wording before merging the release-please PR.

### Release checklist

1. Ensure all changes on `main` use release-please friendly conventional commit messages.
2. Push to `main` - release-please creates/updates a Release PR.
3. Review and merge the Release PR - release-please updates `main`, but no public release exists yet.
4. `build.yml` runs automatically: builds Flatpaks, smokes tests, then creates the GitHub Release and publishes assets.
5. Verify both smoke jobs pass for x86_64 + aarch64 before treating the release as published.
6. Verify release notes are specific to changes since the previous release. Keep docs/test/chore noise out unless it affects users or packaging.
7. If release workflow fails, inspect job logs with `gh run view <run-id> --log`, push fix to `main`, then rerun the build workflow so CI publishes the release after smoke passes.

### Manual release (fallback)

Only if release-please is broken, use GitHub CI as the builder:
1. Commit version updates on `main` (`Cargo.toml`, `Cargo.lock`, `.release-please-manifest.json`).
2. Let `build.yml` create the GitHub Release only after the Flatpak builds and smoke tests pass.
3. Verify release assets and checksums are attached by CI.

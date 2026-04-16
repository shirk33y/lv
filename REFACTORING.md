# Refactoring Plan: Consolidate Build Scripts & Distribution Config

**Goal**: Clean, intuitive directory structure following best practices from RustDesk, Lapce, and COSMIC.

## Current State
```
pkg/
├── appimage.sh          (build script)
├── deb.sh               (build script)
├── lv.desktop
├── lv-256.png
├── lv.svg
└── win64/
    └── README.md

scripts/
├── build-linux-intel.sh
├── build-linux-arm.sh
├── build-windows-intel.sh
├── build-flatpak.sh
├── ci.sh
├── clean.sh
├── dev-linux.sh
└── dev-windows.sh

root/
├── com.shirk33y.lv.json
└── docker/Dockerfile.flatpak
```

## Target State
```
scripts/
├── build-linux-intel.sh
├── build-linux-arm.sh
├── build-windows-intel.sh
├── build-appimage.sh         ← moved from pkg/appimage.sh
├── build-deb.sh              ← moved from pkg/deb.sh
├── build-flatpak.sh
├── ci.sh
├── clean.sh
├── dev-linux.sh
└── dev-windows.sh

dist/
├── flatpak/
│   ├── com.shirk33y.lv.json  ← moved from root/
│   ├── com.shirk33y.lv.metainfo.xml
│   └── Dockerfile.flatpak    ← moved from docker/
├── appimage/
│   ├── AppImageBuilder-x86_64.yml
│   └── AppImageBuilder-aarch64.yml
├── deb/
│   └── control.in
└── windows/
    └── (future: installer config)

res/
├── lv.desktop               ← moved from pkg/
├── icons/
│   ├── lv.svg              ← moved from pkg/
│   └── lv-256.png          ← moved from pkg/
└── screenshots/

pkg/                         ← DELETE (absorbed into scripts/ + dist/)
```

## Changes Required

### Phase 1: Move Build Scripts
1. `cp pkg/appimage.sh scripts/build-appimage.sh`
2. `cp pkg/deb.sh scripts/build-deb.sh`
3. Update shebangs/paths if needed
4. Delete `pkg/`

### Phase 2: Create dist/ Structure
1. `mkdir -p dist/{flatpak,appimage,deb,windows}`
2. `mv com.shirk33y.lv.json dist/flatpak/`
3. `mv docker/Dockerfile.flatpak dist/flatpak/`
4. Update `.gitignore` to exclude `/dist/` (keep only for source files)

### Phase 3: Create res/ Structure
1. `mkdir -p res/icons`
2. `mv pkg/lv.desktop res/`
3. `mv pkg/lv.svg res/icons/`
4. `mv pkg/lv-256.png res/icons/`

### Phase 4: Update Script References
Update paths in:
- `scripts/build-flatpak.sh`: `com.shirk33y.lv.json` → `dist/flatpak/com.shirk33y.lv.json`
- `scripts/build-appimage.sh`: reference AppImage config location (if any)
- `scripts/build-deb.sh`: reference deb config location (if any)

### Phase 5: Update Documentation
- `README.md`: Update build script examples
  - `pkg/appimage.sh` → `scripts/build-appimage.sh`
  - `pkg/deb.sh` → `scripts/build-deb.sh`
- `README.md`: Update script table

### Phase 6: Update .gitignore
Ensure proper patterns for new structure:
```
/build/              ← build artifacts (already done)
cargo-sources.json
/res/screenshots/    ← gitignored?
```

## Implementation Order
1. Move + rename build scripts (Phase 1)
2. Create dist/ and move files (Phase 2)
3. Create res/ and move files (Phase 3)
4. Update all script references (Phase 4)
5. Update documentation (Phase 5)
6. Verify: `git status` clean, test build scripts
7. Commit: "refactor: consolidate pkg/ into scripts/, dist/, res/"

## Benefits
- ✅ All build **scripts** in one place (`scripts/`)
- ✅ All distribution **configs** organized by format (`dist/`)
- ✅ All **assets** (icons, desktop files) in `res/`
- ✅ Follows industry best practices (RustDesk, Lapce, COSMIC)
- ✅ Clear, intuitive hierarchy
- ✅ Eliminates `pkg/` ambiguity (packaging? app resources?)

# Refactoring: Consolidate to `extra/` Directory Structure

**Goal**: Single `extra/` directory for all platform-specific packaging configs + static assets, following Lapce and Alacritty conventions.

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
├── io.github.shirk33y.lv.json
└── docker/Dockerfile.flatpak
```

## Final State
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

extra/                                   ← Single dir for all platform packaging
├── flatpak/
│   ├── io.github.shirk33y.lv.json            ← moved from root/
│   └── Dockerfile.flatpak              ← moved from docker/
├── linux/
│   └── lv.desktop                      ← moved from pkg/res/
├── windows/
│   ├── installer.nsi                   ← moved from pkg/dist/windows/
│   ├── lv.ico                          ← flattened from win64/
│   ├── lv.rc
│   ├── SDL2.dll
│   ├── SDL2.lib
│   ├── libmpv-2.dll
│   ├── mpv.lib
│   └── README.md
└── images/
    ├── lv.svg                          ← moved from pkg/res/icons/
    └── lv-256.png

pkg/  ← DELETED
dist/ ← DELETED
res/  ← DELETED
```

## Implementation (Completed)

### Step 1: Build Scripts
- ✅ `pkg/appimage.sh` → `scripts/build-appimage.sh`
- ✅ `pkg/deb.sh` → `scripts/build-deb.sh`
- ✅ Renamed `scripts/flatpak-build.sh` → `scripts/build-flatpak.sh` (verb prefix consistency)

### Step 2: Create `extra/` Structure
- ✅ `mkdir -p extra/{linux,flatpak,windows,images}`

### Step 3: Move Files to `extra/`
- ✅ `res/lv.desktop` → `extra/linux/lv.desktop`
- ✅ `res/icons/{lv.svg,lv-256.png}` → `extra/images/`
- ✅ `dist/flatpak/{io.github.shirk33y.lv.json,Dockerfile.flatpak}` → `extra/flatpak/`
- ✅ `dist/windows/installer.nsi` → `extra/windows/`
- ✅ `dist/windows/win64/{*.dll,*.lib,*.rc,README.md}` → `extra/windows/` (flattened)

### Step 4: Delete Old Dirs
- ✅ Deleted `pkg/`, `dist/`, `res/`

### Step 5: Update Script References
- ✅ `scripts/build-flatpak.sh`: `dist/flatpak/` → `extra/flatpak/` (2 locations)
- ✅ `README.md`: Updated manifest path reference

### Step 6: Update .gitignore
- ✅ Removed `/dist/` pattern (now source, not build output)

## Benefits
- ✅ Single `extra/` directory (industry standard: Lapce, Alacritty)
- ✅ Platform subdirs clearly separate concerns (flatpak vs linux vs windows vs images)
- ✅ Flattened `windows/` (no unnecessary `win64/` nesting)
- ✅ All build **scripts** in `scripts/` with verb prefix consistency
- ✅ All platform **configs** + **static assets** in `extra/{platform}/`
- ✅ Eliminates `dist/` vs `res/` ambiguity
- ✅ Clear, intuitive, follows best practices

# CLI Refactor

## Current State

Flat commands (clap):
```
lv track PATH      # add + scan, no watch
lv untrack PATH    # remove tracking + watch
lv watch PATH      # enable notify (only if tracked)
lv unwatch PATH    # disable notify
lv scan [PATH]     # rescan tracked dirs (or one-off)
lv status          # library stats
lv worker          # headless job processor
lv [paths...]      # GUI mode
```

Tags stored as JSON array in `meta.tags` column. Queried via `LIKE '%"c2"%'`.
Keys 2-8 toggle c2-c8. Key 9 toggles `like`. Ctrl+2-8 filter by collection.

## Plan — 4 phases

---

### Phase 1: meta_tags table + keybinding change

**Storage:**
```sql
CREATE TABLE meta_tags (
    meta_id INTEGER NOT NULL REFERENCES meta(id) ON DELETE CASCADE,
    tag     TEXT NOT NULL,
    PRIMARY KEY (meta_id, tag)
);
CREATE INDEX idx_meta_tags_tag ON meta_tags(tag);
```

**Migration:** batch INSERT from existing JSON. Requires deep web research for optimal batch approach. Must:
- Copy DB to `lv.db.backup.YYYYMMDD-HHMMSS` before migrating
- Read/parse in batches (1000 rows), INSERT OR IGNORE per batch
- Run in background thread, don't block GUI
- Show progress in status bar ("Migrating tags: 42%")

**Key mapping:**
| Key | No mod | Shift |
|-----|--------|-------|
| `y` | add `like` | remove `like` |
| `2`-`9` | add `c2`-`c9` | remove `c2`-`c9` |
| `r` | refresh/sync | — |

Ctrl+0 = toggle off collection filter. Ctrl+1 = temporary files filter.
Ctrl+2-9 = filter by collection c2-c9.

Old `files_by_collection(9)` (liked) becomes a standard tag query on `like`.
Old `toggle_like`/`toggle_collection` become separate add/remove functions.

**No UI changes** besides key handling. Content tags (`like`, `c2`-`c9`) unchanged.

---

### Phase 2: Property system + new CLI

**Commands:**
```
lv add PATH [-s KEY=VAL]...     # add dir to library
lv remove PATH                  # remove from library
lv sync [PATH]                  # one-shot: scan + process jobs
lv sync -b                      # daemon: continuous monitor
lv status                       # library stats
lv get PATH [prop]              # get property(ies)
lv set PATH KEY=VAL [KEY=VAL]   # set properties
```

**Hidden aliases** (deprecation warning, then remove later):
- `track PATH` → `add PATH`
- `untrack PATH` → `remove PATH`
- `watch PATH` → `set PATH watch_mode=notify`
- `unwatch PATH` → `set PATH watch_mode=disabled`
- `scan [PATH]` → `sync [PATH]`
- `worker` → `sync --wait`

**Properties (dir_properties table):**
| Key | Values | Default |
|-----|--------|---------|
| `watch_mode` | `auto`, `notify`, `poll`, `disabled` | `auto` |
| `recursive` | `true`, `false` | `true` |
| `poll_interval` | integer seconds | `60` |

`auto` = try notify on add, fall back to disabled.
Unknown properties stored but not validated (extensible).

**DB migration:**
```sql
CREATE TABLE dir_properties (
    dir_id INTEGER NOT NULL REFERENCES directories(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    value TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (dir_id, key)
);
```
- Migrate existing `watched=1` → `watch_mode=notify`, `watched=0` → `watch_mode=auto`
- `recursive` stays or moves to dir_properties

---

### Phase 3: lv find

```
lv find [OPTIONS] [pattern]

  -c, --collection <dir>...    Collection(s) to search (repeatable)
  -t, --type <type>            image | video | audio
  -e, --extension <ext>...     File extension (-e jpg -e png)
  -s, --size <spec>            +10M, -500K
  -d, --duration <spec>        +30s, -5m (video/audio)
  -b, --bitrate <spec>         +1000, -5000 (kbps)
  -r, --resolution <spec>      +2, -12, or preset: thumb|vga|sd|hd|4k|8k|photo
  -w, --width <spec>           +1920, -1024 (px)
  -H, --height <spec>          +1080, -720 (px)
  -S, --search <text>          AI prompt / metadata search
  -m, --modified <spec>        +1w, -2024-01-01
  -g, --glob                   Glob match (default)
  -r, --regex                  Regex match
  -i, --ignore-case
  -T, --tag <tag>              Filter by tag: like, c2-c9, or custom
  --sort <field>               name | size | duration | bitrate | resolution | random
  -0, --print0                 Null-separated output
  -C, --count                  Count only
  -l, --list                   Detailed listing
```

Spec syntax (fd-style):
```
+10M  → size >= 10 MB
-500K → size <= 500 KB
1G    → size ~= 1 GB
+30s  → duration >= 30s
-hd   → ≤ 2.1 MP
4k    → 8-9 MP range
+1w   → modified within last week
```

Resolution presets:
| Preset | Range (MP) | Matches |
|--------|-----------|---------|
| `thumb` | < 0.1 | icons, thumbnails |
| `vga` | ~0.3 (0.25-0.4) | 640×480 class |
| `sd` | 0.1-1 | standard def |
| `hd` | 0.9-2.1 | 720p/1080p |
| `4k` | 8-9 | UHD |
| `8k` | 33+ | 8K video |
| `photo` | 2+ | real photo threshold |

Same flags on `lv` pre-filter GUI.

Output: paths only by default. `-l` for detailed. `-C` for count. `-0` for pipe-safe.

---

### Phase 4: Daemon

**Architecture:**
- `lv sync -b` = headless daemon (systemd service)
- `lv` GUI = spawns internal daemon thread if none running, else connects via DB
- IPC via shared SQLite DB + `PRAGMA data_version` polling (3µs read)

**Daemon loop:**
1. Every 100ms: check `PRAGMA data_version` for changes
2. On change: re-read `directories` + `dir_properties`, compare with in-memory state
3. Adjust watchers: add/remove notify handles, start/stop poll timers
4. Check `commands` table for pending sync requests
5. Process jobs (hashing, EXIF) in background threads
6. Write progress/status back to DB

**`commands` table:**
```sql
CREATE TABLE commands (
    id        INTEGER PRIMARY KEY,
    action    TEXT NOT NULL,      -- sync, refresh, rescan
    path      TEXT,
    status    TEXT DEFAULT 'pending',  -- pending, running, done, failed
    requested_at TEXT DEFAULT (datetime('now')),
    completed_at TEXT
);
```

GUI writes sync request for current dir; daemon picks it up, processes, marks done.
`s`/`r` key triggers this (once daemon exists). Before daemon, `r` = inline DB refresh.

---

## Implementation Order

1. Phase 1 (tags + keybinds) — ship as v0.7
   - meta_tags table + migration
   - LIKE → JOIN rewrites
   - add/remove tag functions
   - y/shift+y, 2-9/shift+2-9 keybinds
2. Phase 2 (properties + CLI)
3. Phase 3 (find)
4. Phase 4 (daemon)

Each phase is independent and shippable.

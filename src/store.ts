import { signal, computed } from "@preact/signals";
import { invoke } from "@tauri-apps/api/core";

export interface FileEntry {
  id: number;
  path: string;
  dir: string;
  filename: string;
  meta_id: number | null;
  thumb_ready: boolean;
  shadow: string | null;
  liked: boolean;
}

export interface JobStatus {
  files: number;
  dirs: number;
  hashed: number;
  thumbs: number;
  watched: number;
  jobs_pending: number;
  jobs_running: number;
  jobs_done: number;
  jobs_failed: number;
  watched_paths: string[];
}

export const files = signal<FileEntry[]>([]);
export const cursorIndex = signal(0);
export const loading = signal(true);
export const showInfo = signal(false);
export const showLogs = signal(false);
export const showHelp = signal(false);
export const jobStatus = signal<JobStatus | null>(null);

export interface LogEntry {
  ts: number;
  level: "info" | "warn" | "error";
  msg: string;
}

export const logEntries = signal<LogEntry[]>([]);

export function addLog(level: LogEntry["level"], msg: string) {
  logEntries.value = [...logEntries.value, { ts: Date.now(), level, msg }];
}
export const cwd = signal("");
export const lastDir = signal("");

export const currentFile = computed(() => files.value[cursorIndex.value] ?? null);
export const totalFiles = computed(() => files.value.length);

/** Cursor position in sidebarItems[] — can point to folder or file */
export const sidebarCursor = signal(0);

/** Sync sidebarCursor from cursorIndex (call after direct cursorIndex writes) */
export function syncSidebarCursor() {
  const si = fileToSidebarIdx.value.get(cursorIndex.value);
  if (si != null) sidebarCursor.value = si;
}

// -- Sidebar items (folder headers interleaved with file tiles) ---------------

export type SidebarItem =
  | { type: "folder"; dir: string; dirFiles: FileEntry[] }
  | { type: "file"; file: FileEntry; fileIndex: number };

export const sidebarItems = computed((): SidebarItem[] => {
  const list = files.value;
  if (list.length === 0) return [];

  const items: SidebarItem[] = [];
  let curDir = "";
  let dirFiles: FileEntry[] = [];
  let dirStart = 0;

  function flush() {
    if (dirFiles.length === 0) return;
    items.push({ type: "folder", dir: curDir, dirFiles });
    for (let j = 0; j < dirFiles.length; j++) {
      items.push({ type: "file", file: dirFiles[j], fileIndex: dirStart + j });
    }
  }

  for (let i = 0; i < list.length; i++) {
    if (list[i].dir !== curDir) {
      flush();
      curDir = list[i].dir;
      dirFiles = [];
      dirStart = i;
    }
    dirFiles.push(list[i]);
  }
  flush();

  return items;
});

/** Map file index → sidebar item index (O(1) lookup) */
export const fileToSidebarIdx = computed(() => {
  const map = new Map<number, number>();
  const items = sidebarItems.value;
  for (let i = 0; i < items.length; i++) {
    const item = items[i];
    if (item.type === "file") map.set(item.fileIndex, i);
  }
  return map;
});

/** The folder item under the sidebar cursor, or null if cursor is on a file */
export const selectedFolder = computed((): { dir: string; dirFiles: FileEntry[] } | null => {
  const items = sidebarItems.value;
  const idx = sidebarCursor.value;
  if (idx < 0 || idx >= items.length) return null;
  const item = items[idx];
  return item.type === "folder" ? item : null;
});

// O(1) id → index lookup for 500k+ item lists
export const idIndex = computed(() => {
  const map = new Map<number, number>();
  const list = files.value;
  for (let i = 0; i < list.length; i++) map.set(list[i].id, i);
  return map;
});

export function indexOfId(id: number): number {
  return idIndex.value.get(id) ?? -1;
}

export function updateFileAt(idx: number, patch: Partial<FileEntry>) {
  const list = files.value;
  if (idx < 0 || idx >= list.length) return;
  const copy = list.slice();
  copy[idx] = { ...copy[idx], ...patch };
  files.value = copy;
}

export function moveCursor(delta: number): boolean {
  const items = sidebarItems.value;
  const next = sidebarCursor.value + delta;
  if (next < 0 || next >= items.length) return false;
  sidebarCursor.value = next;
  const item = items[next];
  if (item.type === "file") {
    cursorIndex.value = item.fileIndex;
  }
  return true;
}

export function setCursor(index: number) {
  if (index >= 0 && index < files.value.length) {
    cursorIndex.value = index;
    const si = fileToSidebarIdx.value.get(index);
    if (si != null) sidebarCursor.value = si;
  }
}

export function setCursorToFile(file: FileEntry) {
  const idx = indexOfId(file.id);
  if (idx >= 0) {
    cursorIndex.value = idx;
    const si = fileToSidebarIdx.value.get(idx);
    if (si != null) sidebarCursor.value = si;
  }
}

const BROWSER_SUPPORTED = new Set([
  "jpg", "jpeg", "png", "gif", "bmp", "webp", "svg", "ico",
  "mp4", "webm",
]);

export function extOf(filename: string): string {
  const i = filename.lastIndexOf(".");
  return i >= 0 ? filename.slice(i + 1).toLowerCase() : "";
}

export function filterSupported(raw: FileEntry[]): FileEntry[] {
  return raw.filter((f) => f && f.filename && BROWSER_SUPPORTED.has(extOf(f.filename)));
}

export async function loadFiles(dir?: string) {
  loading.value = true;
  try {
    const raw = await invoke<FileEntry[]>("get_files", { dir: dir ?? null });
    const result = filterSupported(raw);
    files.value = result;
    if (result.length > 0 && result[0].dir) {
      lastDir.value = result[0].dir;
    }
    if (cursorIndex.value >= result.length) {
      cursorIndex.value = Math.max(0, result.length - 1);
    }
    syncSidebarCursor();
    addLog("info", `loaded ${result.length} files${raw.length !== result.length ? ` (${raw.length - result.length} unsupported filtered)` : ""}`);
  } catch (e) {
    addLog("error", `loadFiles failed: ${e}`);
  } finally {
    loading.value = false;
  }
}

export async function jumpToFile(cmd: string, favMode = false) {
  const MAX_RETRIES = 5;
  for (let attempt = 0; attempt < MAX_RETRIES; attempt++) {
    try {
      const file = await invoke<FileEntry | null>(cmd);
      if (!file) return;

      if (favMode) {
        await loadFiles("♥");
        const newIdx = indexOfId(file.id);
        cursorIndex.value = newIdx >= 0 ? newIdx : 0;
        syncSidebarCursor();
        return;
      }

      const idx = indexOfId(file.id);
      if (idx >= 0) {
        cursorIndex.value = idx;
        syncSidebarCursor();
        return;
      }

      await loadFiles(file.dir);
      if (files.value.length > 0) {
        const newIdx = indexOfId(file.id);
        cursorIndex.value = newIdx >= 0 ? newIdx : 0;
        syncSidebarCursor();
        return;
      }
      // Dir had no supported files — retry with another random result
    } catch (e) {
      console.error(`${cmd} failed:`, e);
      return;
    }
  }
}

import { effect } from "@preact/signals";
import { invoke } from "@tauri-apps/api/core";
import { files, cursorIndex, addLog, type FileEntry } from "./store";

const MAX_PRELOADS = 20;
const NEXT_COUNT = 10;
const PREV_COUNT = 5;

const VIDEO_EXTS = new Set(["mp4", "webm"]);
const IS_WINDOWS = navigator.userAgent.includes("Windows");

function fileSrc(path: string): string {
  const encoded = encodeURIComponent(path);
  return IS_WINDOWS
    ? `http://lv-file.localhost/${encoded}`
    : `lv-file://localhost/${encoded}`;
}

function extOf(filename: string): string {
  const i = filename.lastIndexOf(".");
  return i >= 0 ? filename.slice(i + 1).toLowerCase() : "";
}

function isImage(f: FileEntry): boolean {
  return !VIDEO_EXTS.has(extOf(f.filename));
}

// Active preloads: url -> Image element
const active = new Map<string, HTMLImageElement>();
let generation = 0;
let lastLoggedCount = 0;

function computeDesired(): string[] {
  const list = files.value;
  const idx = cursorIndex.value;
  if (list.length === 0) return [];

  const urls: string[] = [];
  const seen = new Set<string>();

  function add(f: FileEntry) {
    if (!isImage(f)) return;
    const url = fileSrc(f.path);
    if (!seen.has(url)) {
      seen.add(url);
      urls.push(url);
    }
  }

  // P0: current file
  if (list[idx]) add(list[idx]);

  // P1: next X images
  for (let i = 1; i <= NEXT_COUNT && idx + i < list.length; i++) {
    add(list[idx + i]);
  }

  // P3: prev X images
  for (let i = 1; i <= PREV_COUNT && idx - i >= 0; i++) {
    add(list[idx - i]);
  }

  return urls.slice(0, MAX_PRELOADS);
}

function updatePreloads() {
  const gen = ++generation;
  const desired = new Set(computeDesired());

  // Cancel stale preloads not in the new desired set
  let cancelled = 0;
  for (const [url, img] of active) {
    if (!desired.has(url)) {
      img.src = "";
      active.delete(url);
      cancelled++;
    }
  }

  // Start new preloads
  let started = 0;
  for (const url of desired) {
    if (active.has(url)) continue;
    if (active.size >= MAX_PRELOADS) break;

    const img = new Image();
    active.set(url, img);
    started++;

    img.onload = () => {
      if (generation !== gen) return;
      active.delete(url);
    };
    img.onerror = () => {
      active.delete(url);
    };

    img.src = url;
  }

  const total = active.size;
  if ((started > 0 || cancelled > 0) && total !== lastLoggedCount) {
    lastLoggedCount = total;
    if (started > 0) {
      addLog("info", `preload: ${total} active (+${started}${cancelled > 0 ? `, -${cancelled} stale` : ""})`);
    }
  }
}

let boostTimer: ReturnType<typeof setTimeout> | null = null;
const BOOST_DEBOUNCE_MS = 150;

function boostViewContext() {
  if (boostTimer) clearTimeout(boostTimer);
  boostTimer = setTimeout(() => {
    const list = files.value;
    const idx = cursorIndex.value;
    if (list.length === 0) return;

    const fileIds: number[] = [];
    const metaIds: number[] = [];

    // Current file + nearby (same window as preloader)
    const start = Math.max(0, idx - PREV_COUNT);
    const end = Math.min(list.length - 1, idx + NEXT_COUNT);
    for (let i = start; i <= end; i++) {
      fileIds.push(list[i].id);
      if (list[i].meta_id != null) metaIds.push(list[i].meta_id!);
    }

    invoke("boost_jobs", { fileIds, metaIds }).catch(() => {});
  }, BOOST_DEBOUNCE_MS);
}

export function setupPreloader() {
  effect(() => {
    files.value;
    cursorIndex.value;
    updatePreloads();
    boostViewContext();
  });
}

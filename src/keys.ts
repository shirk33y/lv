import { invoke } from "@tauri-apps/api/core";
import {
  moveCursor,
  jumpToFile,
  loadFiles,
  currentFile,
  lastDir,
  files,
  cursorIndex,
  updateFileAt,
  filterSupported,
  showInfo,
  showLogs,
  showHelp,
  addLog,
  type FileEntry,
} from "./store";

let navigating = false;

async function navigateDir(delta: number) {
  if (navigating) return;
  const file = currentFile.value;
  let dir = file ? file.dir : lastDir.value;
  if (!dir) return;
  navigating = true;
  const MAX_SKIP = 50;
  for (let attempt = 0; attempt < MAX_SKIP; attempt++) {
    try {
      const raw = await invoke<FileEntry[]>("navigate_dir", {
        currentDir: dir,
        delta,
      });
      if (raw.length === 0) { navigating = false; return; }
      const filtered = filterSupported(raw);
      if (filtered.length > 0) {
        files.value = filtered;
        cursorIndex.value = 0;
        navigating = false;
        return;
      }
      // Dir was empty after filtering — skip to next in same direction
      dir = raw[0].dir;
      if (dir === (file ? file.dir : lastDir.value)) { navigating = false; return; }
    } catch (e) {
      console.error("navigate_dir failed:", e);
      navigating = false;
      return;
    }
  }
  navigating = false;
}

async function toggleLike() {
  const file = currentFile.value;
  if (!file) return;
  try {
    const liked = await invoke<boolean>("toggle_like", { fileId: file.id });
    updateFileAt(cursorIndex.value, { liked });
  } catch (e) {
    console.error("toggle_like failed:", e);
  }
}

export function setupKeyboard(): () => void {
  let heldKey: string | null = null;
  let rafId = 0;
  const INITIAL_DELAY = 300; // ms before repeat starts
  const INTERVAL = 50;       // ~20 fps once repeating
  let keyDownAt = 0;
  let lastMove = 0;

  function tick() {
    const now = performance.now();
    if (!heldKey) return;
    if (now - keyDownAt < INITIAL_DELAY) {
      rafId = requestAnimationFrame(tick);
      return;
    }
    if (now - lastMove >= INTERVAL) {
      moveCursor(heldKey === "j" ? 1 : -1);
      lastMove = now;
    }
    rafId = requestAnimationFrame(tick);
  }

  function onKeyUp(e: KeyboardEvent) {
    if (e.key === heldKey) {
      heldKey = null;
      cancelAnimationFrame(rafId);
    }
  }

  function onKeyDown(e: KeyboardEvent) {
    if ((e.target as HTMLElement)?.tagName === "INPUT") return;

    if (e.ctrlKey && e.key === "r") {
      e.preventDefault();
      window.location.reload();
      return;
    }

    switch (e.key) {
      case "j":
      case "k": {
        if (e.repeat || e.key === heldKey) return;
        moveCursor(e.key === "j" ? 1 : -1);
        heldKey = e.key;
        keyDownAt = performance.now();
        lastMove = keyDownAt;
        rafId = requestAnimationFrame(tick);
        break;
      }
      case "h":
        navigateDir(-1);
        break;
      case "l":
        navigateDir(1);
        break;
      case "u":
        jumpToFile("random_file");
        break;
      case "y":
        toggleLike();
        break;
      case "n":
        jumpToFile("newest_file");
        break;
      case "m":
        jumpToFile("random_fav", true);
        break;
      case "b":
        jumpToFile("latest_fav", true);
        break;
      case "c":
        if (currentFile.value) {
          navigator.clipboard.writeText(currentFile.value.path).catch(() => {});
        }
        break;
      case "f":
        invoke("toggle_fullscreen");
        break;
      case "i":
        if (showInfo.value) {
          showInfo.value = false;
        } else {
          showLogs.value = false;
          showInfo.value = true;
        }
        break;
      case "x":
        if (showLogs.value) {
          showLogs.value = false;
        } else {
          showInfo.value = false;
          showLogs.value = true;
        }
        break;
      case "r":
        addLog("info", "rescan started…");
        invoke<number>("rescan").then((count) => {
          addLog("info", `rescan done: ${count} new files discovered`);
          const file = currentFile.value;
          loadFiles(file?.dir);
        }).catch((e) => {
          addLog("error", `rescan failed: ${e}`);
        });
        break;
      case "?":
        showHelp.value = !showHelp.value;
        break;
      case "q":
        window.close();
        break;
    }
  }

  document.addEventListener("keyup", onKeyUp);
  document.addEventListener("keydown", onKeyDown);

  return () => {
    document.removeEventListener("keyup", onKeyUp);
    document.removeEventListener("keydown", onKeyDown);
    cancelAnimationFrame(rafId);
  };
}

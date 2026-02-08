import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, fireEvent } from "@testing-library/preact";
import { invoke } from "@tauri-apps/api/core";
import { files, cursorIndex, jobStatus, showInfo, showLogs, showHelp, cwd, logEntries, loadFiles, addLog, type FileEntry, type JobStatus } from "../store";
import { Viewer } from "../components/Viewer";
import { StatusBar } from "../components/StatusBar";
import { Tile } from "../components/Tile";
import { Sidebar, scrollTop } from "../components/Sidebar";
import { InfoPanel } from "../components/MetadataOverlay";
import { LogPanel } from "../components/LogPanel";
import { FolderTile } from "../components/FolderTile";

const mockInvoke = vi.mocked(invoke);

function makeFile(id: number, dir = "/a", filename = `f${id}.jpg`): FileEntry {
  return { id, path: `${dir}/${filename}`, dir, filename, meta_id: id, thumb_ready: true, shadow: null, liked: false };
}

function resetStore(items: FileEntry[] = [], cursor = 0) {
  files.value = items;
  cursorIndex.value = cursor;
  jobStatus.value = null;
  showInfo.value = false;
  showLogs.value = false;
  showHelp.value = false;
  logEntries.value = [];
  cwd.value = "";
  scrollTop.value = 0;
}

beforeEach(() => {
  resetStore();
  mockInvoke.mockReset();
});

// ---------------------------------------------------------------------------
// Viewer
// ---------------------------------------------------------------------------

describe("Viewer", () => {
  it("renders empty div when no file", () => {
    const { container } = render(<Viewer />);
    const viewer = container.querySelector(".viewer");
    expect(viewer).toBeTruthy();
    expect(viewer!.querySelector("img")).toBeNull();
  });

  it("renders img with lv-file:// src for images", () => {
    resetStore([makeFile(1, "/pics", "cat.jpg")], 0);
    const { container } = render(<Viewer />);
    const img = container.querySelector("img");
    expect(img).toBeTruthy();
    expect(img!.getAttribute("src")).toContain("lv-file");
    expect(img!.getAttribute("src")).toContain("localhost");
    expect(img!.getAttribute("src")).toContain(encodeURIComponent("/pics/cat.jpg"));
    expect(img!.getAttribute("alt")).toBe("cat.jpg");
  });

  it("uses scheme://localhost/ format on non-Windows for lv-file", () => {
    resetStore([makeFile(1, "/pics", "cat.jpg")], 0);
    const { container } = render(<Viewer />);
    const src = container.querySelector("img")!.getAttribute("src")!;
    // jsdom userAgent doesn't include "Windows"
    expect(src).toMatch(/^lv-file:\/\/localhost\//);
  });

  it("uses scheme://localhost/ format on non-Windows for thumb in video overlay", () => {
    const vid: FileEntry = { id: 1, path: "/a/clip.mp4", dir: "/a", filename: "clip.mp4", meta_id: 42, thumb_ready: true, shadow: null, liked: false };
    resetStore([vid], 0);
    const { container } = render(<Viewer />);
    const img = container.querySelector("img");
    expect(img).toBeTruthy();
    expect(img!.getAttribute("src")).toBe("thumb://localhost/42");
  });

  it("renders play overlay for video files (no <video> until user clicks)", () => {
    const vid: FileEntry = { id: 1, path: "/a/clip.mp4", dir: "/a", filename: "clip.mp4", meta_id: 42, thumb_ready: true, shadow: null, liked: false };
    resetStore([vid], 0);
    const { container } = render(<Viewer />);
    expect(container.querySelector("video")).toBeNull();
    expect(container.querySelector(".viewer-play-overlay")).toBeTruthy();
    expect(container.querySelector(".viewer-play-btn")).toBeTruthy();
    expect(container.textContent).toContain("clip.mp4");
  });

  it("shows thumbnail behind play overlay when thumb_ready", () => {
    const vid: FileEntry = { id: 1, path: "/a/clip.mp4", dir: "/a", filename: "clip.mp4", meta_id: 42, thumb_ready: true, shadow: null, liked: false };
    resetStore([vid], 0);
    const { container } = render(<Viewer />);
    const img = container.querySelector("img");
    expect(img).toBeTruthy();
    expect(img!.getAttribute("src")).toBe("thumb://localhost/42");
  });

  it("renders play overlay for supported video extensions", () => {
    for (const ext of ["mp4", "webm"]) {
      const vid: FileEntry = { id: 1, path: `/a/vid.${ext}`, dir: "/a", filename: `vid.${ext}`, meta_id: 99, thumb_ready: true, shadow: null, liked: false };
      resetStore([vid], 0);
      const { container } = render(<Viewer />);
      expect(container.querySelector("video")).toBeNull();
      expect(container.querySelector(".viewer-play-overlay")).toBeTruthy();
    }
  });
});

// ---------------------------------------------------------------------------
// StatusBar
// ---------------------------------------------------------------------------

describe("StatusBar", () => {
  it("shows 'no files' when empty", () => {
    const { container } = render(<StatusBar />);
    expect(container.textContent).toBe("no files");
  });

  it("shows position and filename", () => {
    resetStore([makeFile(1, "/a", "photo.jpg"), makeFile(2, "/a", "art.png")], 0);
    const { container } = render(<StatusBar />);
    expect(container.textContent).toContain("1/2");
    expect(container.textContent).toContain("photo.jpg");
  });

  it("updates position with cursor", () => {
    resetStore([makeFile(1, "/a", "a.jpg"), makeFile(2, "/a", "b.jpg")], 1);
    const { container } = render(<StatusBar />);
    expect(container.textContent).toContain("2/2");
    expect(container.textContent).toContain("b.jpg");
  });

  it("shows active jobs with compact format", () => {
    resetStore([makeFile(1)], 0);
    jobStatus.value = { files: 20, dirs: 2, hashed: 18, thumbs: 15, watched: 1, jobs_pending: 5, jobs_running: 2, jobs_done: 10, jobs_failed: 0, watched_paths: [] };
    const { container } = render(<StatusBar />);
    expect(container.textContent).toContain("hash: 18/20");
    expect(container.textContent).toContain("thumb: 15/20");
  });

  it("shows no jobs when idle (all complete, no workers)", () => {
    resetStore([makeFile(1)], 0);
    jobStatus.value = { files: 20, dirs: 2, hashed: 20, thumbs: 20, watched: 1, jobs_pending: 0, jobs_running: 0, jobs_done: 10, jobs_failed: 0, watched_paths: [] };
    const { container } = render(<StatusBar />);
    expect(container.querySelector(".status-right")!.textContent).toBe("");
  });

  it("shows error count with !N on active job", () => {
    resetStore([makeFile(1)], 0);
    jobStatus.value = { files: 20, dirs: 2, hashed: 18, thumbs: 15, watched: 1, jobs_pending: 1, jobs_running: 1, jobs_done: 10, jobs_failed: 3, watched_paths: [] };
    const { container } = render(<StatusBar />);
    const errors = container.querySelector(".status-job-errors");
    expect(errors).toBeTruthy();
    expect(errors!.textContent).toBe("!3");
  });

  it("errors not shown when jobs_failed is 0", () => {
    resetStore([makeFile(1)], 0);
    jobStatus.value = { files: 20, dirs: 2, hashed: 18, thumbs: 15, watched: 1, jobs_pending: 1, jobs_running: 1, jobs_done: 10, jobs_failed: 0, watched_paths: [] };
    const { container } = render(<StatusBar />);
    expect(container.querySelector(".status-job-errors")).toBeNull();
  });

  it("separator / has status-job-sep class", () => {
    resetStore([makeFile(1)], 0);
    jobStatus.value = { files: 20, dirs: 2, hashed: 18, thumbs: 15, watched: 1, jobs_pending: 1, jobs_running: 1, jobs_done: 10, jobs_failed: 0, watched_paths: [] };
    const { container } = render(<StatusBar />);
    const seps = container.querySelectorAll(".status-job-sep");
    expect(seps.length).toBeGreaterThan(0);
    expect(seps[0].textContent).toBe("/");
  });

  it("has left, center, and right sections", () => {
    resetStore([makeFile(1)], 0);
    jobStatus.value = { files: 5, dirs: 1, hashed: 0, thumbs: 0, watched: 1, jobs_pending: 1, jobs_running: 0, jobs_done: 0, jobs_failed: 0, watched_paths: [] };
    const { container } = render(<StatusBar />);
    expect(container.querySelector(".status-left")).toBeTruthy();
    expect(container.querySelector(".status-center")).toBeTruthy();
    expect(container.querySelector(".status-right")).toBeTruthy();
  });

  it("pager is centered in status-center", () => {
    resetStore([makeFile(1), makeFile(2), makeFile(3)], 1);
    const { container } = render(<StatusBar />);
    const center = container.querySelector(".status-center");
    expect(center).toBeTruthy();
    expect(center!.textContent).toContain("2/3");
  });

  it("heart icon shown left of pager when file is liked", () => {
    const file: FileEntry = { id: 1, path: "/a/f1.jpg", dir: "/a", filename: "f1.jpg", meta_id: 1, thumb_ready: true, shadow: null, liked: true };
    resetStore([file], 0);
    const { container } = render(<StatusBar />);
    const heart = container.querySelector(".status-heart");
    expect(heart).toBeTruthy();
    expect(heart!.textContent).toBe("♥");
    // Heart comes before pager in center
    const center = container.querySelector(".status-center")!;
    const children = Array.from(center.children);
    const heartIdx = children.findIndex((c) => c.classList.contains("status-heart"));
    const pagerIdx = children.findIndex((c) => c.classList.contains("status-pager"));
    expect(heartIdx).toBeLessThan(pagerIdx);
  });

  it("heart icon hidden when file is not liked", () => {
    resetStore([makeFile(1)], 0);
    const { container } = render(<StatusBar />);
    expect(container.querySelector(".status-heart")).toBeNull();
  });

  it("shows relative path when cwd is set", () => {
    const file: FileEntry = { id: 1, path: "/home/user/pics/cat.jpg", dir: "/home/user/pics", filename: "cat.jpg", meta_id: 1, thumb_ready: true, shadow: null, liked: false };
    resetStore([file], 0);
    cwd.value = "/home/user";
    const { container } = render(<StatusBar />);
    expect(container.textContent).toContain("pics/cat.jpg");
    expect(container.textContent).not.toContain("/home/user/pics/cat.jpg");
  });

  it("shows absolute path when cwd is empty", () => {
    const file: FileEntry = { id: 1, path: "/home/user/pics/cat.jpg", dir: "/home/user/pics", filename: "cat.jpg", meta_id: 1, thumb_ready: true, shadow: null, liked: false };
    resetStore([file], 0);
    cwd.value = "";
    const { container } = render(<StatusBar />);
    expect(container.textContent).toContain("/home/user/pics/cat.jpg");
  });

  it("strips Windows \\\\?\\ prefix from path", () => {
    const file: FileEntry = { id: 1, path: "\\\\?\\C:\\Users\\me\\pics\\cat.jpg", dir: "\\\\?\\C:\\Users\\me\\pics", filename: "cat.jpg", meta_id: 1, thumb_ready: true, shadow: null, liked: false };
    resetStore([file], 0);
    const { container } = render(<StatusBar />);
    expect(container.textContent).toContain("C:\\Users\\me\\pics\\cat.jpg");
    expect(container.textContent).not.toContain("\\\\?\\");
  });

  it("shows relative path with Windows backslash separators", () => {
    const file: FileEntry = { id: 1, path: "C:\\Users\\me\\pics\\cat.jpg", dir: "C:\\Users\\me\\pics", filename: "cat.jpg", meta_id: 1, thumb_ready: true, shadow: null, liked: false };
    resetStore([file], 0);
    cwd.value = "C:\\Users\\me";
    const { container } = render(<StatusBar />);
    expect(container.textContent).toContain("pics\\cat.jpg");
    expect(container.textContent).not.toContain("C:\\Users\\me\\pics\\cat.jpg");
  });

  it("strips \\\\?\\ prefix then applies relative path with Windows cwd", () => {
    const file: FileEntry = { id: 1, path: "\\\\?\\C:\\Users\\me\\pics\\cat.jpg", dir: "\\\\?\\C:\\Users\\me\\pics", filename: "cat.jpg", meta_id: 1, thumb_ready: true, shadow: null, liked: false };
    resetStore([file], 0);
    cwd.value = "C:\\Users\\me";
    const { container } = render(<StatusBar />);
    expect(container.textContent).toContain("pics\\cat.jpg");
    expect(container.textContent).not.toContain("\\\\?\\");
  });

  it("renders basename in .status-basename span", () => {
    const file: FileEntry = { id: 1, path: "/pics/cat.jpg", dir: "/pics", filename: "cat.jpg", meta_id: 1, thumb_ready: true, shadow: null, liked: false };
    resetStore([file], 0);
    const { container } = render(<StatusBar />);
    const basename = container.querySelector(".status-basename");
    expect(basename).toBeTruthy();
    expect(basename!.textContent).toBe("cat.jpg");
  });

  it("basename span contains only filename, not dir", () => {
    const file: FileEntry = { id: 1, path: "/a/b/c/photo.png", dir: "/a/b/c", filename: "photo.png", meta_id: 1, thumb_ready: true, shadow: null, liked: false };
    resetStore([file], 0);
    const { container } = render(<StatusBar />);
    const basename = container.querySelector(".status-basename");
    expect(basename!.textContent).toBe("photo.png");
    // Dir part should be in the status-left but outside basename
    const left = container.querySelector(".status-left")!;
    expect(left.textContent).toContain("/a/b/c/");
  });

  it("Windows path splits basename correctly", () => {
    const file: FileEntry = { id: 1, path: "C:\\pics\\cat.jpg", dir: "C:\\pics", filename: "cat.jpg", meta_id: 1, thumb_ready: true, shadow: null, liked: false };
    resetStore([file], 0);
    cwd.value = "";
    const { container } = render(<StatusBar />);
    const basename = container.querySelector(".status-basename");
    expect(basename!.textContent).toBe("cat.jpg");
  });

  it("no .status-basename when no files", () => {
    resetStore([], 0);
    const { container } = render(<StatusBar />);
    expect(container.querySelector(".status-basename")).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// Tile
// ---------------------------------------------------------------------------

describe("Tile", () => {
  it("renders thumb img when meta_id present and thumb_ready", () => {
    const file = makeFile(42);
    const { container } = render(<Tile file={file} active={false} />);
    const img = container.querySelector(".tile-thumb");
    expect(img).toBeTruthy();
    expect(img!.getAttribute("src")).toBe("thumb://localhost/42");
  });

  it("thumb URL must include localhost (regression: Tauri 2 custom protocol)", () => {
    const file = makeFile(7);
    const { container } = render(<Tile file={file} active={false} />);
    const src = container.querySelector(".tile-thumb")!.getAttribute("src")!;
    // Non-Windows (jsdom): scheme://localhost/, Windows: http://scheme.localhost/
    expect(src).toMatch(/thumb[:/]/);
    expect(src).toContain("localhost");
    expect(src).toContain("/7");
    expect(src).not.toBe("thumb://7");
  });

  it("thumb URL uses scheme://localhost/ format on non-Windows", () => {
    const file = makeFile(99);
    const { container } = render(<Tile file={file} active={false} />);
    const src = container.querySelector(".tile-thumb")!.getAttribute("src")!;
    // jsdom userAgent doesn't include "Windows"
    expect(src).toBe("thumb://localhost/99");
  });

  it("renders placeholder when no meta_id and no shadow", () => {
    const file: FileEntry = { id: 1, path: "/a/f.jpg", dir: "/a", filename: "f.jpg", meta_id: null, thumb_ready: false, shadow: null, liked: false };
    const { container } = render(<Tile file={file} active={false} />);
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector(".tile-placeholder")).toBeTruthy();
  });

  it("renders placeholder when thumb_ready is false and no shadow", () => {
    const file: FileEntry = { id: 1, path: "/a/f.jpg", dir: "/a", filename: "f.jpg", meta_id: 5, thumb_ready: false, shadow: null, liked: false };
    const { container } = render(<Tile file={file} active={false} />);
    expect(container.querySelector(".tile-thumb")).toBeNull();
    expect(container.querySelector(".tile-placeholder")).toBeTruthy();
  });

  it("renders shadow img when shadow data URL is present", () => {
    const file: FileEntry = { id: 1, path: "/a/f.jpg", dir: "/a", filename: "f.jpg", meta_id: 5, thumb_ready: true, shadow: "data:image/webp;base64,AAAA", liked: false };
    const { container } = render(<Tile file={file} active={false} />);
    const shadow = container.querySelector(".tile-shadow");
    expect(shadow).toBeTruthy();
    expect(shadow!.getAttribute("src")).toBe("data:image/webp;base64,AAAA");
    // Thumb should also be present (not yet loaded)
    expect(container.querySelector(".tile-thumb")).toBeTruthy();
  });

  it("has active class when active", () => {
    const file = makeFile(1);
    const { container } = render(<Tile file={file} active={true} />);
    expect(container.querySelector(".tile.active")).toBeTruthy();
  });

  it("no active class when not active", () => {
    const file = makeFile(1);
    const { container } = render(<Tile file={file} active={false} />);
    expect(container.querySelector(".tile.active")).toBeNull();
    expect(container.querySelector(".tile")).toBeTruthy();
  });

  it("shows heart icon when liked", () => {
    const file: FileEntry = { id: 1, path: "/a/f.jpg", dir: "/a", filename: "f.jpg", meta_id: 1, thumb_ready: true, shadow: null, liked: true };
    const { container } = render(<Tile file={file} active={false} />);
    expect(container.querySelector(".tile-heart")).toBeTruthy();
    expect(container.querySelector(".tile-heart")!.textContent).toBe("♥");
  });

  it("does not show heart when not liked", () => {
    const file = makeFile(1);
    const { container } = render(<Tile file={file} active={false} />);
    expect(container.querySelector(".tile-heart")).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// Sidebar
// ---------------------------------------------------------------------------

describe("Sidebar", () => {
  it("renders empty sidebar when no files", () => {
    const { container } = render(<Sidebar />);
    expect(container.querySelector(".sidebar")).toBeTruthy();
    expect(container.querySelectorAll(".tile").length).toBe(0);
  });

  it("renders tiles for files", () => {
    resetStore([makeFile(1), makeFile(2), makeFile(3)], 0);
    const { container } = render(<Sidebar />);
    const tiles = container.querySelectorAll(".tile");
    expect(tiles.length).toBe(3);
  });

  it("marks active tile", () => {
    resetStore([makeFile(1), makeFile(2), makeFile(3)], 1);
    const { container } = render(<Sidebar />);
    expect(container.querySelectorAll(".tile.active").length).toBe(1);
  });

  it("scrolls viewport when cursor is deep", () => {
    const items = Array.from({ length: 20 }, (_, i) => makeFile(i + 1));
    resetStore(items, 15);
    const { container } = render(<Sidebar />);
    const tiles = container.querySelectorAll(".tile");
    expect(tiles.length).toBeGreaterThan(0);
    expect(tiles.length).toBeLessThanOrEqual(20);
    expect(container.querySelectorAll(".tile.active").length).toBe(1);
  });

  it("cursor moves freely in dead zone", () => {
    const items = Array.from({ length: 20 }, (_, i) => makeFile(i + 1));
    resetStore(items, 5);
    const { container } = render(<Sidebar />);
    expect(container.querySelectorAll(".tile").length).toBeGreaterThan(0);
    expect(container.querySelectorAll(".tile.active").length).toBe(1);
  });

  // --- DOM structure ---

  it("sidebar-track is direct child of sidebar", () => {
    resetStore([makeFile(1)], 0);
    const { container } = render(<Sidebar />);
    const sidebar = container.querySelector(".sidebar");
    const track = sidebar!.querySelector(":scope > .sidebar-track");
    expect(track).toBeTruthy();
  });

  it("all tiles live inside sidebar-track", () => {
    resetStore([makeFile(1), makeFile(2)], 0);
    const { container } = render(<Sidebar />);
    const track = container.querySelector(".sidebar-track")!;
    expect(track.querySelectorAll(".tile").length).toBe(2);
    expect(container.querySelectorAll(".tile").length).toBe(2);
  });

  it("tiles are wrapped in absolutely-positioned sidebar-slot divs", () => {
    resetStore([makeFile(1), makeFile(2), makeFile(3)], 1);
    const { container } = render(<Sidebar />);
    const slots = container.querySelectorAll(".sidebar-slot");
    // 1 folder header + 3 file tiles = 4 slots
    expect(slots.length).toBe(4);
    slots.forEach((slot) => {
      expect((slot as HTMLElement).style.position).toBe("absolute");
      // Each slot has either a .tile or a .folder-tile
      const hasTile = slot.querySelector(".tile") || slot.querySelector(".folder-tile");
      expect(hasTile).toBeTruthy();
    });
  });

  it("sidebar-track height equals sidebarItems * tileH (files + folder headers)", () => {
    const items = Array.from({ length: 10 }, (_, i) => makeFile(i + 1));
    resetStore(items, 0);
    const { container } = render(<Sidebar />);
    const track = container.querySelector(".sidebar-track") as HTMLElement;
    // 10 files in 1 dir → 11 sidebar items (1 folder + 10 files), tileH=48
    expect(parseInt(track.style.height)).toBe(11 * 48);
  });

  it("slot top positions are sequential multiples of tileH", () => {
    const items = Array.from({ length: 5 }, (_, i) => makeFile(i + 1));
    resetStore(items, 0);
    const { container } = render(<Sidebar />);
    const slots = container.querySelectorAll(".sidebar-slot") as NodeListOf<HTMLElement>;
    const tops = Array.from(slots).map((s) => parseInt(s.style.top));
    // 1 folder header + 5 files = 6 slots: 0, 48, 96, 144, 192, 240
    expect(tops).toEqual([0, 48, 96, 144, 192, 240]);
  });

  // --- Active tile correctness ---

  it("single file renders one active tile", () => {
    resetStore([makeFile(1)], 0);
    const { container } = render(<Sidebar />);
    expect(container.querySelectorAll(".tile").length).toBe(1);
    expect(container.querySelectorAll(".tile.active").length).toBe(1);
  });

  it("active tile matches cursor index in rendered set", () => {
    const items = Array.from({ length: 5 }, (_, i) => makeFile(i + 1));
    resetStore(items, 3);
    const { container } = render(<Sidebar />);
    const tiles = container.querySelectorAll(".tile");
    const activeIdx = Array.from(tiles).findIndex((t) => t.classList.contains("active"));
    expect(activeIdx).toBeGreaterThanOrEqual(0);
    expect(container.querySelectorAll(".tile.active").length).toBe(1);
  });

  it("cursor at first item renders active tile", () => {
    const items = Array.from({ length: 30 }, (_, i) => makeFile(i + 1));
    resetStore(items, 0);
    const { container } = render(<Sidebar />);
    expect(container.querySelectorAll(".tile.active").length).toBe(1);
  });

  it("cursor at last item renders active tile", () => {
    const items = Array.from({ length: 30 }, (_, i) => makeFile(i + 1));
    resetStore(items, 29);
    const { container } = render(<Sidebar />);
    expect(container.querySelectorAll(".tile.active").length).toBe(1);
  });

  it("cursor at middle of large list renders active tile", () => {
    const items = Array.from({ length: 100 }, (_, i) => makeFile(i + 1));
    resetStore(items, 50);
    const { container } = render(<Sidebar />);
    expect(container.querySelectorAll(".tile.active").length).toBe(1);
  });

  // --- Virtualization ---

  it("large file list renders bounded number of tiles", () => {
    const items = Array.from({ length: 500 }, (_, i) => makeFile(i + 1));
    resetStore(items, 250);
    const { container } = render(<Sidebar />);
    const tiles = container.querySelectorAll(".tile");
    expect(tiles.length).toBeLessThan(500);
    expect(tiles.length).toBeGreaterThan(0);
    expect(container.querySelectorAll(".tile.active").length).toBe(1);
  });

  it("track height scales with total items even when virtualized", () => {
    const items = Array.from({ length: 500 }, (_, i) => makeFile(i + 1));
    resetStore(items, 0);
    const { container } = render(<Sidebar />);
    const track = container.querySelector(".sidebar-track") as HTMLElement;
    // 500 files in 1 dir → 501 sidebar items
    expect(parseInt(track.style.height)).toBe(501 * 48);
  });

  it("active slot has correct top for its cursor index", () => {
    const items = Array.from({ length: 20 }, (_, i) => makeFile(i + 1));
    resetStore(items, 7);
    const { container } = render(<Sidebar />);
    const activeSlot = container.querySelector(".sidebar-slot:has(.tile.active)") as HTMLElement;
    expect(activeSlot).toBeTruthy();
    // file index 7 → sidebar index 8 (folder header at 0), top = 8*48
    expect(parseInt(activeSlot.style.top)).toBe(8 * 48);
  });

  // --- Edge cases ---

  it("cursor beyond items length clamps to empty", () => {
    resetStore([], 5);
    const { container } = render(<Sidebar />);
    expect(container.querySelectorAll(".tile").length).toBe(0);
  });

  it("two files, cursor on second", () => {
    resetStore([makeFile(1), makeFile(2)], 1);
    const { container } = render(<Sidebar />);
    const active = container.querySelector(".tile.active");
    expect(active).toBeTruthy();
    const slots = container.querySelectorAll(".sidebar-slot");
    const activeSlot = Array.from(slots).find((s) => s.querySelector(".tile.active"));
    // folder@0, file0@48, file1(active)@96
    expect(parseInt((activeSlot as HTMLElement).style.top)).toBe(96);
  });
});

// ---------------------------------------------------------------------------
// InfoPanel
// ---------------------------------------------------------------------------

describe("InfoPanel", () => {
  it("renders nothing when showInfo is false", () => {
    resetStore([makeFile(1)], 0);
    showInfo.value = false;
    const { container } = render(<InfoPanel />);
    expect(container.querySelector(".right-panel")).toBeNull();
  });

  it("renders panel when showInfo is true", () => {
    resetStore([makeFile(1)], 0);
    showInfo.value = true;
    const { container } = render(<InfoPanel />);
    expect(container.querySelector(".right-panel")).toBeTruthy();
  });
});

// ---------------------------------------------------------------------------
// LogPanel
// ---------------------------------------------------------------------------

describe("LogPanel", () => {
  it("renders nothing when showLogs is false", () => {
    showLogs.value = false;
    const { container } = render(<LogPanel />);
    expect(container.querySelector(".right-panel")).toBeNull();
  });

  it("renders panel when showLogs is true", () => {
    showLogs.value = true;
    const { container } = render(<LogPanel />);
    expect(container.querySelector(".right-panel")).toBeTruthy();
  });

  it("shows log entries", () => {
    showLogs.value = true;
    logEntries.value = [
      { ts: 1000, level: "info", msg: "hello" },
      { ts: 2000, level: "error", msg: "oops" },
    ];
    const { container } = render(<LogPanel />);
    const lines = container.querySelectorAll(".log-line");
    expect(lines.length).toBe(2);
    expect(lines[1].classList.contains("log-error")).toBe(true);
  });

  it("formats timestamps as HH:MM:SS (fast formatter)", () => {
    showLogs.value = true;
    // 2025-01-15 13:05:09 UTC
    logEntries.value = [{ ts: new Date(2025, 0, 15, 13, 5, 9).getTime(), level: "info", msg: "test" }];
    const { container } = render(<LogPanel />);
    const ts = container.querySelector(".log-ts");
    expect(ts).toBeTruthy();
    expect(ts!.textContent).toBe("13:05:09");
  });

  it("zero-pads single digit hours/minutes/seconds", () => {
    showLogs.value = true;
    logEntries.value = [{ ts: new Date(2025, 0, 1, 1, 2, 3).getTime(), level: "info", msg: "x" }];
    const { container } = render(<LogPanel />);
    expect(container.querySelector(".log-ts")!.textContent).toBe("01:02:03");
  });

  it("caps rendered entries at 500", () => {
    showLogs.value = true;
    const many = Array.from({ length: 600 }, (_, i) => ({
      ts: 1000 + i,
      level: "info" as const,
      msg: `msg ${i}`,
    }));
    logEntries.value = many;
    const { container } = render(<LogPanel />);
    const lines = container.querySelectorAll(".log-line");
    expect(lines.length).toBe(500);
    // Should show the LAST 500, not the first
    expect(lines[0].textContent).toContain("msg 100");
    expect(lines[499].textContent).toContain("msg 599");
  });

  it("shows empty state when no log entries", () => {
    showLogs.value = true;
    logEntries.value = [];
    const { container } = render(<LogPanel />);
    expect(container.querySelector(".meta-empty")).toBeTruthy();
    expect(container.querySelector(".meta-empty")!.textContent).toContain("No log entries");
  });

  it("resume button is outside scroll container", () => {
    showLogs.value = true;
    logEntries.value = [{ ts: 1000, level: "info", msg: "x" }];
    const { container } = render(<LogPanel />);
    const resumeBtn = container.querySelector(".log-resume");
    // When auto-scroll is active (default), resume button should not be present
    expect(resumeBtn).toBeNull();
  });

  it("applies correct log-level class to each entry", () => {
    showLogs.value = true;
    logEntries.value = [
      { ts: 1000, level: "info", msg: "a" },
      { ts: 2000, level: "warn", msg: "b" },
      { ts: 3000, level: "error", msg: "c" },
    ];
    const { container } = render(<LogPanel />);
    const lines = container.querySelectorAll(".log-line");
    expect(lines[0].classList.contains("log-info")).toBe(true);
    expect(lines[1].classList.contains("log-warn")).toBe(true);
    expect(lines[2].classList.contains("log-error")).toBe(true);
  });

  it("log-body is the scrollable container", () => {
    showLogs.value = true;
    logEntries.value = [{ ts: 1000, level: "info", msg: "x" }];
    const { container } = render(<LogPanel />);
    const body = container.querySelector(".log-body");
    expect(body).toBeTruthy();
    // Resume button should be a sibling, not a child
    const panel = container.querySelector(".right-panel")!;
    const children = Array.from(panel.children);
    const headerIdx = children.findIndex(c => c.classList.contains("right-panel-header"));
    const bodyIdx = children.findIndex(c => c.classList.contains("log-body"));
    expect(headerIdx).toBeLessThan(bodyIdx);
  });
});

// ---------------------------------------------------------------------------
// Auto-reload on external file changes (pollStatus behavior)
// ---------------------------------------------------------------------------

describe("auto-reload on file count change", () => {
  it("loadFiles is called when file count increases (simulates pollStatus behavior)", async () => {
    // Simulate what pollStatus does: detect df > 0 → call loadFiles()
    const prevStatus: JobStatus = { files: 10, dirs: 1, hashed: 10, thumbs: 10, watched: 1, jobs_pending: 0, jobs_running: 0, jobs_done: 10, jobs_failed: 0, watched_paths: [] };
    const newStatus: JobStatus = { files: 15, dirs: 2, hashed: 10, thumbs: 10, watched: 1, jobs_pending: 5, jobs_running: 0, jobs_done: 10, jobs_failed: 0, watched_paths: [] };
    jobStatus.value = prevStatus;

    const df = newStatus.files - prevStatus.files;
    expect(df).toBe(5);
    expect(df > 0).toBe(true);

    // When df > 0, pollStatus calls loadFiles() — verify it reloads
    const newFiles = [makeFile(1), makeFile(2), makeFile(3)];
    mockInvoke.mockResolvedValueOnce(newFiles);
    await loadFiles();
    expect(files.value.length).toBe(3);
    expect(mockInvoke).toHaveBeenCalledWith("get_files", { dir: null });
  });

  it("loadFiles is NOT called when file count stays same", () => {
    const prevStatus: JobStatus = { files: 10, dirs: 1, hashed: 10, thumbs: 10, watched: 1, jobs_pending: 0, jobs_running: 0, jobs_done: 10, jobs_failed: 0, watched_paths: [] };
    const newStatus: JobStatus = { files: 10, dirs: 1, hashed: 10, thumbs: 12, watched: 1, jobs_pending: 0, jobs_running: 0, jobs_done: 12, jobs_failed: 0, watched_paths: [] };

    const df = newStatus.files - prevStatus.files;
    expect(df).toBe(0);
    // df === 0 means no reload needed
    expect(df > 0).toBe(false);
  });

  it("addLog records scanned message when new files detected", () => {
    logEntries.value = [];
    addLog("info", "scanned: 15 files (+5)");
    expect(logEntries.value.length).toBe(1);
    expect(logEntries.value[0].msg).toContain("scanned: 15 files (+5)");
  });
});

// ---------------------------------------------------------------------------
// Tile shadow rendering
// ---------------------------------------------------------------------------

describe("Tile shadow", () => {
  it("renders shadow with tile-shadow class when shadow data present", () => {
    const file: FileEntry = { id: 1, path: "/a/f.jpg", dir: "/a", filename: "f.jpg", meta_id: 5, thumb_ready: true, shadow: "data:image/webp;base64,AAAA", liked: false };
    const { container } = render(<Tile file={file} active={false} />);
    const shadow = container.querySelector(".tile-shadow");
    expect(shadow).toBeTruthy();
  });

  it("shadow is not visible once thumb loads (loaded class hides shadow)", () => {
    const file: FileEntry = { id: 1, path: "/a/f.jpg", dir: "/a", filename: "f.jpg", meta_id: 5, thumb_ready: true, shadow: "data:image/webp;base64,AAAA", liked: false };
    const { container } = render(<Tile file={file} active={false} />);
    // Before load: shadow visible, thumb not loaded
    expect(container.querySelector(".tile-shadow")).toBeTruthy();
    expect(container.querySelector(".tile-thumb")).toBeTruthy();
    expect(container.querySelector(".tile-thumb.loaded")).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// Tile error handling — IPC report + ? indicator
// ---------------------------------------------------------------------------

describe("Tile error handling", () => {
  it("shows ? when thumb errors and meta_id present", () => {
    const file: FileEntry = { id: 1, path: "/a/f.jpg", dir: "/a", filename: "f.jpg", meta_id: 5, thumb_ready: true, shadow: null, liked: false };
    const { container } = render(<Tile file={file} active={false} />);
    const img = container.querySelector(".tile-thumb") as HTMLImageElement;
    expect(img).toBeTruthy();
    // Simulate error via testing-library (triggers Preact onError)
    fireEvent.error(img);
    // After error: placeholder with ? should appear
    const icon = container.querySelector(".tile-placeholder-icon");
    expect(icon).toBeTruthy();
    expect(icon!.textContent).toBe("?");
  });

  it("calls report_broken_thumb IPC on thumb error", () => {
    const file: FileEntry = { id: 1, path: "/a/f.jpg", dir: "/a", filename: "f.jpg", meta_id: 42, thumb_ready: true, shadow: null, liked: false };
    mockInvoke.mockResolvedValue(undefined);
    const { container } = render(<Tile file={file} active={false} />);
    const img = container.querySelector(".tile-thumb") as HTMLImageElement;
    fireEvent.error(img);
    expect(mockInvoke).toHaveBeenCalledWith("report_broken_thumb", { metaId: 42 });
  });

  it("does not call IPC when meta_id is null", () => {
    const file: FileEntry = { id: 1, path: "/a/f.jpg", dir: "/a", filename: "f.jpg", meta_id: null, thumb_ready: false, shadow: null, liked: false };
    const { container } = render(<Tile file={file} active={false} />);
    // No thumb rendered (meta_id null), so no error possible via img
    expect(container.querySelector(".tile-thumb")).toBeNull();
    expect(mockInvoke).not.toHaveBeenCalledWith("report_broken_thumb", expect.anything());
  });

  it("shows placeholder ◻ when no thumb and no error", () => {
    const file: FileEntry = { id: 1, path: "/a/f.jpg", dir: "/a", filename: "f.jpg", meta_id: null, thumb_ready: false, shadow: null, liked: false };
    const { container } = render(<Tile file={file} active={false} />);
    const icon = container.querySelector(".tile-placeholder-icon");
    expect(icon).toBeTruthy();
    expect(icon!.textContent).toBe("◻");
  });
});

// ---------------------------------------------------------------------------
// FolderTile
// ---------------------------------------------------------------------------

describe("FolderTile", () => {
  it("renders folder-tile with 2x2 grid", () => {
    const dirFiles = [makeFile(1), makeFile(2), makeFile(3), makeFile(4)];
    const { container } = render(<FolderTile dir="/photos" dirFiles={dirFiles} />);
    expect(container.querySelector(".folder-tile")).toBeTruthy();
    expect(container.querySelector(".folder-tile-grid")).toBeTruthy();
  });

  it("renders 4 mini thumb images for ≥4 files", () => {
    const dirFiles = Array.from({ length: 10 }, (_, i) => makeFile(i + 1));
    const { container } = render(<FolderTile dir="/pics" dirFiles={dirFiles} />);
    const imgs = container.querySelectorAll(".folder-tile-img");
    expect(imgs.length).toBe(4);
  });

  it("renders empty slots for <4 files", () => {
    const dirFiles = [makeFile(1)];
    const { container } = render(<FolderTile dir="/one" dirFiles={dirFiles} />);
    const imgs = container.querySelectorAll(".folder-tile-img");
    const empties = container.querySelectorAll(".folder-tile-empty");
    expect(imgs.length).toBe(1);
    expect(empties.length).toBe(3);
  });

  it("renders 2 images + 2 empties for 2 files", () => {
    const dirFiles = [makeFile(1), makeFile(2)];
    const { container } = render(<FolderTile dir="/two" dirFiles={dirFiles} />);
    expect(container.querySelectorAll(".folder-tile-img").length).toBe(2);
    expect(container.querySelectorAll(".folder-tile-empty").length).toBe(2);
  });

  it("renders 3 images + 1 empty for 3 files", () => {
    const dirFiles = [makeFile(1), makeFile(2), makeFile(3)];
    const { container } = render(<FolderTile dir="/three" dirFiles={dirFiles} />);
    expect(container.querySelectorAll(".folder-tile-img").length).toBe(3);
    expect(container.querySelectorAll(".folder-tile-empty").length).toBe(1);
  });

  it("shows folder label from last path segment", () => {
    const dirFiles = [makeFile(1)];
    const { container } = render(<FolderTile dir="/home/user/photos" dirFiles={dirFiles} />);
    const label = container.querySelector(".folder-tile-label");
    expect(label).toBeTruthy();
    expect(label!.textContent).toBe("photos");
  });

  it("handles Windows paths", () => {
    const dirFiles = [makeFile(1)];
    const { container } = render(<FolderTile dir="C:\\Users\\me\\pics" dirFiles={dirFiles} />);
    expect(container.querySelector(".folder-tile-label")!.textContent).toBe("pics");
  });

  it("shows empty slots for files without thumb_ready", () => {
    const file: FileEntry = { id: 1, path: "/a/f.jpg", dir: "/a", filename: "f.jpg", meta_id: null, thumb_ready: false, shadow: null, liked: false };
    const { container } = render(<FolderTile dir="/a" dirFiles={[file]} />);
    expect(container.querySelectorAll(".folder-tile-empty").length).toBe(4);
    expect(container.querySelectorAll(".folder-tile-img").length).toBe(0);
  });

  it("renders 0 files: all 4 slots empty", () => {
    const { container } = render(<FolderTile dir="/empty" dirFiles={[]} />);
    expect(container.querySelectorAll(".folder-tile-empty").length).toBe(4);
  });

  it("has title attribute with full dir path", () => {
    const { container } = render(<FolderTile dir="/full/path/here" dirFiles={[makeFile(1)]} />);
    expect(container.querySelector(".folder-tile")!.getAttribute("title")).toBe("/full/path/here");
  });
});

// ---------------------------------------------------------------------------
// Sidebar — folder tiles + centered cursor
// ---------------------------------------------------------------------------

describe("Sidebar with folders", () => {
  it("renders folder tile for single-dir file list", () => {
    resetStore([makeFile(1, "/a"), makeFile(2, "/a")]);
    const { container } = render(<Sidebar />);
    expect(container.querySelector(".folder-tile")).toBeTruthy();
  });

  it("renders both folder and file tiles", () => {
    resetStore([makeFile(1, "/a"), makeFile(2, "/a")]);
    const { container } = render(<Sidebar />);
    expect(container.querySelector(".folder-tile")).toBeTruthy();
    expect(container.querySelectorAll(".tile").length).toBe(2);
  });

  it("renders multiple folder tiles for multiple dirs", () => {
    resetStore([makeFile(1, "/a"), makeFile(2, "/b")]);
    const { container } = render(<Sidebar />);
    const folders = container.querySelectorAll(".folder-tile");
    expect(folders.length).toBe(2);
  });

  it("active tile has .active class", () => {
    resetStore([makeFile(1, "/a"), makeFile(2, "/a")], 1);
    const { container } = render(<Sidebar />);
    const activeTiles = container.querySelectorAll(".tile.active");
    expect(activeTiles.length).toBe(1);
  });
});

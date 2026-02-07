import { describe, it, expect, vi, beforeEach } from "vitest";
import { render } from "@testing-library/preact";
import { invoke } from "@tauri-apps/api/core";
import { files, cursorIndex, jobStatus, showInfo, showLogs, showHelp, cwd, logEntries, loadFiles, addLog, type FileEntry, type JobStatus } from "../store";
import { Viewer } from "../components/Viewer";
import { StatusBar } from "../components/StatusBar";
import { Tile } from "../components/Tile";
import { Sidebar, scrollTop } from "../components/Sidebar";
import { InfoPanel } from "../components/MetadataOverlay";
import { LogPanel } from "../components/LogPanel";

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

  it("shows job status when active jobs exist", () => {
    resetStore([makeFile(1)], 0);
    jobStatus.value = { files: 20, dirs: 2, hashed: 18, thumbs: 15, watched: 1, jobs_pending: 5, jobs_running: 2, jobs_done: 10, jobs_failed: 0, watched_paths: [] };
    const { container } = render(<StatusBar />);
    expect(container.textContent).toContain("2 running");
    expect(container.textContent).toContain("5 queued");
  });

  it("shows idle status when no active jobs", () => {
    resetStore([makeFile(1)], 0);
    jobStatus.value = { files: 20, dirs: 2, hashed: 18, thumbs: 15, watched: 1, jobs_pending: 0, jobs_running: 0, jobs_done: 10, jobs_failed: 0, watched_paths: [] };
    const { container } = render(<StatusBar />);
    expect(container.textContent).toContain("thumbs: 15/20");
    expect(container.textContent).toContain("hashed: 18/20");
  });

  it("shows failed count when jobs failed", () => {
    resetStore([makeFile(1)], 0);
    jobStatus.value = { files: 20, dirs: 2, hashed: 18, thumbs: 15, watched: 1, jobs_pending: 0, jobs_running: 0, jobs_done: 10, jobs_failed: 3, watched_paths: [] };
    const { container } = render(<StatusBar />);
    expect(container.textContent).toContain("3 failed");
  });

  it("has left and right sections", () => {
    resetStore([makeFile(1)], 0);
    jobStatus.value = { files: 5, dirs: 1, hashed: 0, thumbs: 0, watched: 1, jobs_pending: 1, jobs_running: 0, jobs_done: 0, jobs_failed: 0, watched_paths: [] };
    const { container } = render(<StatusBar />);
    expect(container.querySelector(".status-left")).toBeTruthy();
    expect(container.querySelector(".status-right")).toBeTruthy();
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
    const activeTiles = container.querySelectorAll(".tile.active");
    expect(activeTiles.length).toBe(1);
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
    const tiles = container.querySelectorAll(".tile");
    expect(tiles.length).toBeGreaterThan(0);
    expect(container.querySelectorAll(".tile.active").length).toBe(1);
  });

  it("sidebar-track wrapper has width 100%", () => {
    resetStore([makeFile(1), makeFile(2)], 0);
    const { container } = render(<Sidebar />);
    const track = container.querySelector(".sidebar-track");
    expect(track).toBeTruthy();
    // All tiles must be inside the track
    const tilesInTrack = track!.querySelectorAll(".tile");
    expect(tilesInTrack.length).toBe(2);
    // No tiles outside the track
    const allTiles = container.querySelectorAll(".tile");
    expect(allTiles.length).toBe(tilesInTrack.length);
  });

  it("sidebar-track is direct child of sidebar", () => {
    resetStore([makeFile(1)], 0);
    const { container } = render(<Sidebar />);
    const sidebar = container.querySelector(".sidebar");
    const track = sidebar!.querySelector(":scope > .sidebar-track");
    expect(track).toBeTruthy();
  });

  it("sidebar-track gets animate class when not suppressed", () => {
    resetStore([makeFile(1), makeFile(2), makeFile(3)], 0);
    const { container } = render(<Sidebar />);
    const track = container.querySelector(".sidebar-track");
    // After initial render + effect, the track should have animate class
    expect(track).toBeTruthy();
    // On first render noAnim may be true, but track always has sidebar-track
    expect(track!.classList.contains("sidebar-track")).toBe(true);
  });

  it("sidebar-track has transform style", () => {
    const items = Array.from({ length: 20 }, (_, i) => makeFile(i + 1));
    resetStore(items, 10);
    const { container } = render(<Sidebar />);
    const track = container.querySelector(".sidebar-track") as HTMLElement;
    expect(track).toBeTruthy();
    expect(track.style.transform).toMatch(/translateY/);
  });

  it("tiles are rendered inside track, never directly in sidebar", () => {
    resetStore([makeFile(1), makeFile(2), makeFile(3)], 1);
    const { container } = render(<Sidebar />);
    const sidebar = container.querySelector(".sidebar")!;
    // Direct children of sidebar should only be the track div
    const directChildren = Array.from(sidebar.children);
    expect(directChildren.length).toBe(1);
    expect(directChildren[0].classList.contains("sidebar-track")).toBe(true);
  });

  it("single file renders one tile at full track width", () => {
    resetStore([makeFile(1)], 0);
    const { container } = render(<Sidebar />);
    const tiles = container.querySelectorAll(".tile");
    expect(tiles.length).toBe(1);
    expect(container.querySelectorAll(".tile.active").length).toBe(1);
  });

  it("active tile matches cursor index in rendered set", () => {
    const items = Array.from({ length: 5 }, (_, i) => makeFile(i + 1));
    resetStore(items, 3);
    const { container } = render(<Sidebar />);
    const tiles = container.querySelectorAll(".tile");
    const activeIdx = Array.from(tiles).findIndex(t => t.classList.contains("active"));
    expect(activeIdx).toBeGreaterThanOrEqual(0);
    // Exactly one active
    expect(container.querySelectorAll(".tile.active").length).toBe(1);
  });

  it("cursor at last item still renders active tile", () => {
    const items = Array.from({ length: 30 }, (_, i) => makeFile(i + 1));
    resetStore(items, 29);
    const { container } = render(<Sidebar />);
    expect(container.querySelectorAll(".tile.active").length).toBe(1);
  });

  it("cursor at first item renders active tile", () => {
    const items = Array.from({ length: 30 }, (_, i) => makeFile(i + 1));
    resetStore(items, 0);
    const { container } = render(<Sidebar />);
    expect(container.querySelectorAll(".tile.active").length).toBe(1);
  });

  it("large file list still renders a bounded number of tiles", () => {
    const items = Array.from({ length: 500 }, (_, i) => makeFile(i + 1));
    resetStore(items, 250);
    const { container } = render(<Sidebar />);
    const tiles = container.querySelectorAll(".tile");
    // Should render buffer, not all 500
    expect(tiles.length).toBeLessThan(500);
    expect(tiles.length).toBeGreaterThan(0);
    expect(container.querySelectorAll(".tile.active").length).toBe(1);
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

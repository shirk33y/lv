import { describe, it, expect, vi, beforeEach, beforeAll, afterAll } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  files,
  cursorIndex,
  loading,
  currentFile,
  totalFiles,
  moveCursor,
  setCursor,
  setCursorToFile,
  loadFiles,
  jumpToFile,
  indexOfId,
  idIndex,
  updateFileAt,
  filterSupported,
  sidebarItems,
  fileToSidebarIdx,
  type FileEntry,
  type SidebarItem,
} from "../store";

const mockInvoke = vi.mocked(invoke);

function makeFile(id: number, dir = "/a", filename = `f${id}.jpg`): FileEntry {
  return { id, path: `${dir}/${filename}`, dir, filename, meta_id: id, thumb_ready: true, shadow: null, liked: false };
}

function resetStore(items: FileEntry[] = [], cursor = 0) {
  files.value = items;
  cursorIndex.value = cursor;
  loading.value = false;
}

beforeEach(() => {
  resetStore();
  mockInvoke.mockReset();
});

// ---------------------------------------------------------------------------
// Signals / computed
// ---------------------------------------------------------------------------

describe("signals", () => {
  it("currentFile returns null when empty", () => {
    expect(currentFile.value).toBeNull();
  });

  it("currentFile tracks cursor", () => {
    const items = [makeFile(1), makeFile(2)];
    resetStore(items, 1);
    expect(currentFile.value?.id).toBe(2);
  });

  it("totalFiles reflects length", () => {
    resetStore([makeFile(1), makeFile(2), makeFile(3)]);
    expect(totalFiles.value).toBe(3);
  });
});

// ---------------------------------------------------------------------------
// moveCursor
// ---------------------------------------------------------------------------

describe("moveCursor", () => {
  it("moves forward", () => {
    resetStore([makeFile(1), makeFile(2), makeFile(3)], 0);
    moveCursor(1);
    expect(cursorIndex.value).toBe(1);
  });

  it("moves backward", () => {
    resetStore([makeFile(1), makeFile(2), makeFile(3)], 2);
    moveCursor(-1);
    expect(cursorIndex.value).toBe(1);
  });

  it("clamps at start", () => {
    resetStore([makeFile(1), makeFile(2)], 0);
    moveCursor(-1);
    expect(cursorIndex.value).toBe(0);
  });

  it("clamps at end", () => {
    resetStore([makeFile(1), makeFile(2)], 1);
    moveCursor(1);
    expect(cursorIndex.value).toBe(1);
  });

  it("noop on empty list", () => {
    resetStore([], 0);
    moveCursor(1);
    expect(cursorIndex.value).toBe(0);
  });

  it("handles large delta", () => {
    resetStore([makeFile(1), makeFile(2), makeFile(3)], 1);
    moveCursor(100);
    expect(cursorIndex.value).toBe(1); // doesn't jump past end
  });
});

// ---------------------------------------------------------------------------
// setCursor
// ---------------------------------------------------------------------------

describe("setCursor", () => {
  it("sets valid index", () => {
    resetStore([makeFile(1), makeFile(2), makeFile(3)], 0);
    setCursor(2);
    expect(cursorIndex.value).toBe(2);
  });

  it("ignores negative", () => {
    resetStore([makeFile(1)], 0);
    setCursor(-1);
    expect(cursorIndex.value).toBe(0);
  });

  it("ignores out of bounds", () => {
    resetStore([makeFile(1), makeFile(2)], 0);
    setCursor(5);
    expect(cursorIndex.value).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// setCursorToFile
// ---------------------------------------------------------------------------

describe("setCursorToFile", () => {
  it("finds file by id", () => {
    const items = [makeFile(10), makeFile(20), makeFile(30)];
    resetStore(items, 0);
    setCursorToFile(items[2]);
    expect(cursorIndex.value).toBe(2);
  });

  it("ignores file not in list", () => {
    resetStore([makeFile(1)], 0);
    setCursorToFile(makeFile(999));
    expect(cursorIndex.value).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// loadFiles
// ---------------------------------------------------------------------------

describe("loadFiles", () => {
  it("calls invoke and sets files", async () => {
    const items = [makeFile(1), makeFile(2)];
    mockInvoke.mockResolvedValueOnce(items);
    await loadFiles();
    expect(mockInvoke).toHaveBeenCalledWith("get_files", { dir: null });
    expect(files.value).toEqual(items);
    expect(loading.value).toBe(false);
  });

  it("passes dir when provided", async () => {
    mockInvoke.mockResolvedValueOnce([]);
    await loadFiles("/my/dir");
    expect(mockInvoke).toHaveBeenCalledWith("get_files", { dir: "/my/dir" });
  });

  it("clamps cursor if result is shorter", async () => {
    resetStore([makeFile(1), makeFile(2), makeFile(3)], 2);
    mockInvoke.mockResolvedValueOnce([makeFile(1)]);
    await loadFiles();
    expect(cursorIndex.value).toBe(0);
  });

  it("sets loading true during fetch", async () => {
    let resolve!: (v: FileEntry[]) => void;
    const p = new Promise<FileEntry[]>((r) => (resolve = r));
    mockInvoke.mockReturnValueOnce(p as any);
    const promise = loadFiles();
    expect(loading.value).toBe(true);
    resolve([]);
    await promise;
    expect(loading.value).toBe(false);
  });

  it("handles invoke error gracefully", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("db locked"));
    await loadFiles();
    expect(loading.value).toBe(false);
    // files unchanged
    expect(files.value).toEqual([]);
  });

  it("filters out unsupported file extensions", async () => {
    const items = [
      makeFile(1, "/a", "photo.jpg"),
      makeFile(2, "/a", "raw.cr2"),
      makeFile(3, "/a", "clip.mp4"),
      makeFile(4, "/a", "image.avif"),
      makeFile(5, "/a", "vid.mkv"),
      makeFile(6, "/a", "pic.png"),
    ];
    mockInvoke.mockResolvedValueOnce(items);
    await loadFiles();
    expect(files.value.map((f) => f.filename)).toEqual(["photo.jpg", "clip.mp4", "pic.png"]);
  });

  it("survives null/undefined entries in response", async () => {
    const items = [
      makeFile(1, "/a", "ok.jpg"),
      null as any,
      undefined as any,
      { id: 4, path: "/a/x", dir: "/a", filename: undefined, meta_id: null, thumb_ready: false, shadow: null, liked: false } as any,
      makeFile(5, "/a", "ok2.png"),
    ];
    mockInvoke.mockResolvedValueOnce(items);
    await loadFiles();
    expect(files.value.map((f) => f.filename)).toEqual(["ok.jpg", "ok2.png"]);
  });

  it("handles files with no extension", async () => {
    const items = [
      makeFile(1, "/a", "README"),
      makeFile(2, "/a", "photo.jpg"),
    ];
    mockInvoke.mockResolvedValueOnce(items);
    await loadFiles();
    expect(files.value.map((f) => f.filename)).toEqual(["photo.jpg"]);
  });
});

// ---------------------------------------------------------------------------
// jumpToFile
// ---------------------------------------------------------------------------

describe("jumpToFile", () => {
  it("jumps to file already in list", async () => {
    const items = [makeFile(1), makeFile(2), makeFile(3)];
    resetStore(items, 0);
    mockInvoke.mockResolvedValueOnce(items[2]); // random_file returns file 3
    await jumpToFile("random_file");
    expect(cursorIndex.value).toBe(2);
  });

  it("reloads dir if file not in current list", async () => {
    resetStore([makeFile(1, "/a")], 0);
    const newFile = makeFile(5, "/b", "new.jpg");
    mockInvoke
      .mockResolvedValueOnce(newFile) // random_file
      .mockResolvedValueOnce([makeFile(4, "/b"), newFile]); // get_files for /b
    await jumpToFile("random_file");
    expect(files.value.length).toBe(2);
    expect(cursorIndex.value).toBe(1);
  });

  it("handles null response (no file found)", async () => {
    resetStore([makeFile(1)], 0);
    mockInvoke.mockResolvedValueOnce(null);
    await jumpToFile("random_fav");
    expect(cursorIndex.value).toBe(0); // unchanged
  });

  it("handles error gracefully", async () => {
    resetStore([makeFile(1)], 0);
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    mockInvoke.mockRejectedValueOnce(new Error("fail"));
    await jumpToFile("newest_file");
    expect(cursorIndex.value).toBe(0); // unchanged
    spy.mockRestore();
  });

  it("falls back to cursor 0 when random file is unsupported", async () => {
    resetStore([makeFile(1, "/a")], 0);
    // Backend returns a .cr2 file — unsupported
    const unsupported = makeFile(99, "/b", "raw.cr2");
    mockInvoke
      .mockResolvedValueOnce(unsupported) // random_file returns unsupported file
      .mockResolvedValueOnce([makeFile(10, "/b", "pic.jpg"), unsupported]); // get_files for /b
    await jumpToFile("random_file");
    // .cr2 filtered out, cursor should fall back to 0 (pic.jpg)
    expect(files.value.length).toBe(1);
    expect(files.value[0].filename).toBe("pic.jpg");
    expect(cursorIndex.value).toBe(0);
  });

  it("falls back to cursor 0 when only unsupported files in dir", async () => {
    resetStore([makeFile(1, "/a")], 0);
    const unsupported = makeFile(99, "/b", "raw.cr2");
    mockInvoke
      .mockResolvedValueOnce(unsupported) // random_file
      .mockResolvedValueOnce([unsupported, makeFile(100, "/b", "other.dng")]); // get_files
    await jumpToFile("random_file");
    // All files filtered out — empty list, cursor at 0
    expect(files.value.length).toBe(0);
    expect(cursorIndex.value).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// idIndex / indexOfId
// ---------------------------------------------------------------------------

describe("idIndex", () => {
  it("builds id-to-index map", () => {
    resetStore([makeFile(10), makeFile(20), makeFile(30)]);
    expect(indexOfId(10)).toBe(0);
    expect(indexOfId(20)).toBe(1);
    expect(indexOfId(30)).toBe(2);
  });

  it("returns -1 for unknown id", () => {
    resetStore([makeFile(1)]);
    expect(indexOfId(999)).toBe(-1);
  });

  it("updates when files change", () => {
    resetStore([makeFile(1), makeFile(2)]);
    expect(indexOfId(2)).toBe(1);
    files.value = [makeFile(2), makeFile(1)];
    expect(indexOfId(2)).toBe(0);
    expect(indexOfId(1)).toBe(1);
  });

  it("handles empty files list", () => {
    resetStore([]);
    expect(indexOfId(1)).toBe(-1);
    expect(idIndex.value.size).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// updateFileAt
// ---------------------------------------------------------------------------

describe("updateFileAt", () => {
  it("patches a single file", () => {
    resetStore([makeFile(1), makeFile(2)]);
    updateFileAt(0, { liked: true });
    expect(files.value[0].liked).toBe(true);
    expect(files.value[1].liked).toBe(false);
  });

  it("does nothing for out of bounds index", () => {
    resetStore([makeFile(1)]);
    updateFileAt(5, { liked: true });
    expect(files.value[0].liked).toBe(false);
  });

  it("does nothing for negative index", () => {
    resetStore([makeFile(1)]);
    updateFileAt(-1, { liked: true });
    expect(files.value[0].liked).toBe(false);
  });

  it("creates new array reference (triggers signal)", () => {
    const items = [makeFile(1), makeFile(2)];
    resetStore(items);
    const before = files.value;
    updateFileAt(0, { liked: true });
    expect(files.value).not.toBe(before);
  });
});

// ---------------------------------------------------------------------------
// filterSupported
// ---------------------------------------------------------------------------

describe("filterSupported", () => {
  it("keeps supported image formats", () => {
    const result = filterSupported([
      makeFile(1, "/a", "a.jpg"),
      makeFile(2, "/a", "b.jpeg"),
      makeFile(3, "/a", "c.png"),
      makeFile(4, "/a", "d.gif"),
      makeFile(5, "/a", "e.bmp"),
      makeFile(6, "/a", "f.webp"),
      makeFile(7, "/a", "g.svg"),
      makeFile(8, "/a", "h.ico"),
    ]);
    expect(result.length).toBe(8);
  });

  it("keeps supported video formats", () => {
    const result = filterSupported([
      makeFile(1, "/a", "a.mp4"),
      makeFile(2, "/a", "b.webm"),
    ]);
    expect(result.length).toBe(2);
  });

  it("removes unsupported formats", () => {
    const result = filterSupported([
      makeFile(1, "/a", "a.cr2"),
      makeFile(2, "/a", "b.nef"),
      makeFile(3, "/a", "c.heic"),
      makeFile(4, "/a", "d.avif"),
      makeFile(5, "/a", "e.psd"),
      makeFile(6, "/a", "f.mkv"),
      makeFile(7, "/a", "g.avi"),
      makeFile(8, "/a", "h.flv"),
      makeFile(9, "/a", "i.wmv"),
      makeFile(10, "/a", "j.tiff"),
    ]);
    expect(result.length).toBe(0);
  });

  it("is case-insensitive", () => {
    const result = filterSupported([
      makeFile(1, "/a", "a.JPG"),
      makeFile(2, "/a", "b.Png"),
      makeFile(3, "/a", "c.MP4"),
    ]);
    expect(result.length).toBe(3);
  });

  it("handles mixed null/undefined entries", () => {
    const result = filterSupported([
      null as any,
      undefined as any,
      makeFile(1, "/a", "ok.jpg"),
      { id: 2, path: "/a/x", dir: "/a", filename: "", meta_id: null, thumb_ready: false, shadow: null, liked: false } as any,
    ]);
    expect(result.length).toBe(1);
  });

  it("handles empty filenames and no extension", () => {
    const result = filterSupported([
      makeFile(1, "/a", "README"),
      makeFile(2, "/a", ".hidden"),
      makeFile(3, "/a", "photo.jpg"),
    ]);
    expect(result.length).toBe(1);
    expect(result[0].filename).toBe("photo.jpg");
  });
});

// ---------------------------------------------------------------------------
// Performance — 500k files
// ---------------------------------------------------------------------------

describe("performance at scale", () => {
  const N = 500_000;
  let bigList: FileEntry[];

  // Build once, reuse across tests
  beforeAll(() => {
    bigList = Array.from({ length: N }, (_, i) => ({
      id: i + 1,
      path: `/d${Math.floor(i / 1000)}/f${i}.jpg`,
      dir: `/d${Math.floor(i / 1000)}`,
      filename: `f${i}.jpg`,
      meta_id: i + 1,
      thumb_ready: true,
      shadow: null,
      liked: false,
    }));
  });

  it("idIndex builds in < 500ms for 500k files", () => {
    files.value = bigList;
    const t0 = performance.now();
    const map = idIndex.value;
    const elapsed = performance.now() - t0;
    expect(map.size).toBe(N);
    expect(elapsed).toBeLessThan(500);
  });

  it("indexOfId O(1) lookup after idIndex build", () => {
    files.value = bigList;
    idIndex.value; // ensure built
    const t0 = performance.now();
    for (let i = 0; i < 10_000; i++) {
      indexOfId(Math.floor(Math.random() * N) + 1);
    }
    const elapsed = performance.now() - t0;
    // 10k random lookups in < 50ms
    expect(elapsed).toBeLessThan(50);
  });

  it("filterSupported processes 500k files in < 500ms", () => {
    const t0 = performance.now();
    const result = filterSupported(bigList);
    const elapsed = performance.now() - t0;
    expect(result.length).toBe(N); // all .jpg
    expect(elapsed).toBeLessThan(500);
  });

  it("moveCursor is O(1) with 500k files", () => {
    files.value = bigList;
    cursorIndex.value = 0;
    const t0 = performance.now();
    for (let i = 0; i < 10_000; i++) {
      moveCursor(1);
    }
    const elapsed = performance.now() - t0;
    expect(cursorIndex.value).toBe(10_000);
    expect(elapsed).toBeLessThan(50);
  });

  it("setCursorToFile is O(1) via idIndex", () => {
    files.value = bigList;
    idIndex.value; // ensure built
    const target = bigList[N - 1]; // last file
    const t0 = performance.now();
    setCursorToFile(target);
    const elapsed = performance.now() - t0;
    expect(cursorIndex.value).toBe(N - 1);
    expect(elapsed).toBeLessThan(5);
  });

  it("updateFileAt is O(n) but completes in < 500ms for 500k", () => {
    files.value = bigList;
    const t0 = performance.now();
    updateFileAt(250_000, { liked: true });
    const elapsed = performance.now() - t0;
    expect(files.value[250_000].liked).toBe(true);
    expect(elapsed).toBeLessThan(500);
  });

  afterAll(() => {
    resetStore(); // cleanup
  });
});

// ---------------------------------------------------------------------------
// sidebarItems — folder headers interleaved with file tiles
// ---------------------------------------------------------------------------

describe("sidebarItems", () => {
  it("empty when no files", () => {
    resetStore([]);
    expect(sidebarItems.value).toEqual([]);
  });

  it("single dir: 1 folder header + N file items", () => {
    const items = [makeFile(1, "/a"), makeFile(2, "/a"), makeFile(3, "/a")];
    resetStore(items);
    const si = sidebarItems.value;
    expect(si.length).toBe(4); // 1 folder + 3 files
    expect(si[0].type).toBe("folder");
    expect((si[0] as any).dir).toBe("/a");
    expect((si[0] as any).dirFiles.length).toBe(3);
    expect(si[1].type).toBe("file");
    expect((si[1] as any).fileIndex).toBe(0);
    expect(si[3].type).toBe("file");
    expect((si[3] as any).fileIndex).toBe(2);
  });

  it("multiple dirs: folder headers partition files", () => {
    const items = [
      makeFile(1, "/a"), makeFile(2, "/a"),
      makeFile(3, "/b"), makeFile(4, "/b"), makeFile(5, "/b"),
    ];
    resetStore(items);
    const si = sidebarItems.value;
    // /a: 1 folder + 2 files = 3, /b: 1 folder + 3 files = 4 → total 7
    expect(si.length).toBe(7);
    expect(si[0].type).toBe("folder");
    expect((si[0] as any).dir).toBe("/a");
    expect(si[1].type).toBe("file");
    expect(si[2].type).toBe("file");
    expect(si[3].type).toBe("folder");
    expect((si[3] as any).dir).toBe("/b");
    expect((si[3] as any).dirFiles.length).toBe(3);
    expect(si[4].type).toBe("file");
    expect((si[4] as any).fileIndex).toBe(2); // index in files array
    expect(si[6].type).toBe("file");
    expect((si[6] as any).fileIndex).toBe(4);
  });

  it("folder dirFiles contains all files for that dir", () => {
    const items = [makeFile(1, "/x"), makeFile(2, "/x"), makeFile(3, "/y")];
    resetStore(items);
    const si = sidebarItems.value;
    const folderX = si[0] as Extract<SidebarItem, { type: "folder" }>;
    expect(folderX.dirFiles.map(f => f.id)).toEqual([1, 2]);
    const folderY = si[3] as Extract<SidebarItem, { type: "folder" }>;
    expect(folderY.dirFiles.map(f => f.id)).toEqual([3]);
  });

  it("single file: 1 folder + 1 file", () => {
    resetStore([makeFile(1, "/z")]);
    const si = sidebarItems.value;
    expect(si.length).toBe(2);
    expect(si[0].type).toBe("folder");
    expect(si[1].type).toBe("file");
  });
});

// ---------------------------------------------------------------------------
// fileToSidebarIdx — maps file index to sidebar item index
// ---------------------------------------------------------------------------

describe("fileToSidebarIdx", () => {
  it("empty when no files", () => {
    resetStore([]);
    expect(fileToSidebarIdx.value.size).toBe(0);
  });

  it("maps file indices correctly with single dir", () => {
    resetStore([makeFile(1, "/a"), makeFile(2, "/a"), makeFile(3, "/a")]);
    const map = fileToSidebarIdx.value;
    // sidebar: [folder@0, file0@1, file1@2, file2@3]
    expect(map.get(0)).toBe(1);
    expect(map.get(1)).toBe(2);
    expect(map.get(2)).toBe(3);
  });

  it("maps file indices correctly with multiple dirs", () => {
    resetStore([
      makeFile(1, "/a"), makeFile(2, "/a"),
      makeFile(3, "/b"),
    ]);
    const map = fileToSidebarIdx.value;
    // sidebar: [folder-a@0, file0@1, file1@2, folder-b@3, file2@4]
    expect(map.get(0)).toBe(1);
    expect(map.get(1)).toBe(2);
    expect(map.get(2)).toBe(4);
  });

  it("every file index is mapped", () => {
    const items = [
      makeFile(1, "/a"), makeFile(2, "/a"),
      makeFile(3, "/b"), makeFile(4, "/b"), makeFile(5, "/b"),
    ];
    resetStore(items);
    const map = fileToSidebarIdx.value;
    expect(map.size).toBe(5);
    for (let i = 0; i < 5; i++) {
      expect(map.has(i)).toBe(true);
    }
  });
});

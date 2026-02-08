import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { files, cursorIndex, showInfo, showLogs, showHelp, type FileEntry } from "../store";

const mockInvoke = vi.mocked(invoke);

function makeFile(id: number, dir = "/a", filename = `f${id}.jpg`): FileEntry {
  return { id, path: `${dir}/${filename}`, dir, filename, meta_id: id, thumb_ready: true, shadow: null, liked: false };
}

function resetStore(items: FileEntry[] = [], cursor = 0) {
  files.value = items;
  cursorIndex.value = cursor;
  showInfo.value = false;
  showLogs.value = false;
  showHelp.value = false;
}

function pressKey(key: string, opts: KeyboardEventInit = {}) {
  document.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, ...opts }));
}

function releaseKey(key: string) {
  document.dispatchEvent(new KeyboardEvent("keyup", { key, bubbles: true }));
}

let cleanup: (() => void) | null = null;

beforeEach(async () => {
  resetStore([makeFile(1), makeFile(2), makeFile(3)], 0);
  mockInvoke.mockReset();
  if (!cleanup) {
    const { setupKeyboard } = await import("../keys");
    cleanup = setupKeyboard();
  }
});

afterEach(() => {
  // Release any held key between tests
  releaseKey("j");
  releaseKey("k");
});

describe("keyboard navigation", () => {
  it("j moves cursor forward", () => {
    pressKey("j");
    expect(cursorIndex.value).toBe(1);
  });

  it("k moves cursor backward", () => {
    resetStore([makeFile(1), makeFile(2), makeFile(3)], 2);
    pressKey("k");
    expect(cursorIndex.value).toBe(1);
  });

  it("j at end navigates to next dir", () => {
    resetStore([makeFile(1, "/a")], 0);
    mockInvoke.mockResolvedValueOnce([makeFile(10, "/b")]);
    pressKey("j");
    expect(cursorIndex.value).toBe(0); // stays until async resolves
    expect(mockInvoke).toHaveBeenCalledWith("navigate_dir", {
      currentDir: "/a",
      delta: 1,
    });
  });

  it("j at end loads next dir files", async () => {
    resetStore([makeFile(1, "/a")], 0);
    mockInvoke.mockResolvedValueOnce([makeFile(10, "/b"), makeFile(11, "/b")]);
    pressKey("j");
    await vi.waitFor(() => {
      expect(files.value[0]?.dir).toBe("/b");
    });
    expect(cursorIndex.value).toBe(0); // first file
    expect(files.value.length).toBe(2);
  });

  it("k at start navigates to prev dir (cursor at last file)", async () => {
    resetStore([makeFile(1, "/b")], 0);
    mockInvoke.mockResolvedValueOnce([makeFile(5, "/a"), makeFile(6, "/a"), makeFile(7, "/a")]);
    pressKey("k");
    await vi.waitFor(() => {
      expect(files.value[0]?.dir).toBe("/a");
    });
    expect(cursorIndex.value).toBe(2); // last file of prev dir
    expect(files.value.length).toBe(3);
  });

  it("j at end with single-file dir crosses to next dir", async () => {
    resetStore([makeFile(1, "/a")], 0);
    mockInvoke.mockResolvedValueOnce([makeFile(20, "/c")]);
    pressKey("j");
    await vi.waitFor(() => {
      expect(files.value[0]?.dir).toBe("/c");
    });
    expect(files.value.length).toBe(1);
  });

  it("h navigates dir backward", () => {
    mockInvoke.mockResolvedValueOnce([makeFile(10, "/b")]);
    pressKey("h");
    expect(mockInvoke).toHaveBeenCalledWith("navigate_dir", {
      currentDir: "/a",
      delta: -1,
    });
  });

  it("l navigates dir forward", () => {
    mockInvoke.mockResolvedValueOnce([makeFile(10, "/b")]);
    pressKey("l");
    expect(mockInvoke).toHaveBeenCalledWith("navigate_dir", {
      currentDir: "/a",
      delta: 1,
    });
  });

  it("u invokes random_file", () => {
    mockInvoke.mockResolvedValueOnce(null);
    pressKey("u");
    expect(mockInvoke).toHaveBeenCalledWith("random_file");
  });

  it("n invokes newest_file", () => {
    mockInvoke.mockResolvedValueOnce(null);
    pressKey("n");
    expect(mockInvoke).toHaveBeenCalledWith("newest_file");
  });

  it("m invokes random_fav", () => {
    mockInvoke.mockResolvedValueOnce(null);
    pressKey("m");
    expect(mockInvoke).toHaveBeenCalledWith("random_fav");
  });

  it("b invokes latest_fav", () => {
    mockInvoke.mockResolvedValueOnce(null);
    pressKey("b");
    expect(mockInvoke).toHaveBeenCalledWith("latest_fav");
  });

  it("y invokes toggle_like", () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    pressKey("y");
    expect(mockInvoke).toHaveBeenCalledWith("toggle_like", { fileId: 1 });
  });

  it("f invokes toggle_fullscreen", () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    pressKey("f");
    expect(mockInvoke).toHaveBeenCalledWith("toggle_fullscreen");
  });

  it("i toggles showInfo", () => {
    expect(showInfo.value).toBe(false);
    pressKey("i");
    expect(showInfo.value).toBe(true);
    pressKey("i");
    expect(showInfo.value).toBe(false);
  });

  it("r invokes rescan", () => {
    pressKey("r");
    expect(mockInvoke).toHaveBeenCalledWith("rescan");
  });

  it("q calls window.close", () => {
    const closeSpy = vi.spyOn(window, "close").mockImplementation(() => {});
    pressKey("q");
    expect(closeSpy).toHaveBeenCalled();
    closeSpy.mockRestore();
  });

  it("ignores unmapped keys", () => {
    pressKey("z");
    pressKey("1");
    expect(mockInvoke).not.toHaveBeenCalled();
    expect(cursorIndex.value).toBe(0);
  });

  it("l filters unsupported files from navigate_dir result", async () => {
    mockInvoke.mockResolvedValueOnce([
      makeFile(10, "/b", "pic.jpg"),
      makeFile(11, "/b", "raw.cr2"),
      makeFile(12, "/b", "vid.mkv"),
    ]);
    pressKey("l");
    await vi.waitFor(() => {
      expect(files.value.length).toBe(1);
    });
    expect(files.value[0].filename).toBe("pic.jpg");
    expect(cursorIndex.value).toBe(0);
  });

  it("l skips dirs that are empty after filtering", async () => {
    // First dir: only unsupported files
    mockInvoke
      .mockResolvedValueOnce([makeFile(10, "/b", "raw.cr2")]) // /b all unsupported
      .mockResolvedValueOnce([makeFile(20, "/c", "ok.jpg")]); // /c has supported
    pressKey("l");
    await vi.waitFor(() => {
      expect(files.value[0]?.dir).toBe("/c");
    });
    expect(files.value.length).toBe(1);
    expect(files.value[0].filename).toBe("ok.jpg");
  });

  it("l stops when navigate_dir returns empty (no more dirs)", async () => {
    const original = [makeFile(1), makeFile(2), makeFile(3)];
    resetStore(original, 0);
    mockInvoke.mockResolvedValueOnce([]); // no more dirs
    pressKey("l");
    await vi.waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledTimes(1);
    });
    // files unchanged
    expect(files.value).toEqual(original);
  });

  it("c copies current file path to clipboard", () => {
    const writeTextSpy = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText: writeTextSpy } });
    pressKey("c");
    expect(writeTextSpy).toHaveBeenCalledWith("/a/f1.jpg");
  });

  it("c does nothing when no file is loaded", () => {
    resetStore([], 0);
    const writeTextSpy = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText: writeTextSpy } });
    pressKey("c");
    expect(writeTextSpy).not.toHaveBeenCalled();
  });

  it("Ctrl+R calls window.location.reload", () => {
    const reloadSpy = vi.fn();
    Object.defineProperty(window, "location", {
      value: { ...window.location, reload: reloadSpy },
      writable: true,
      configurable: true,
    });
    pressKey("r", { ctrlKey: true });
    expect(reloadSpy).toHaveBeenCalled();
  });

  it("Ctrl+R does not trigger rescan", () => {
    const reloadSpy = vi.fn();
    Object.defineProperty(window, "location", {
      value: { ...window.location, reload: reloadSpy },
      writable: true,
      configurable: true,
    });
    pressKey("r", { ctrlKey: true });
    expect(mockInvoke).not.toHaveBeenCalledWith("rescan");
  });
});

describe("keyboard cleanup (HMR regression)", () => {
  it("cleanup removes handlers — no double-fire", async () => {
    // Tear down current keyboard
    cleanup!();
    cleanup = null;

    const { setupKeyboard } = await import("../keys");

    // Simulate HMR: setup, cleanup, setup again
    const cleanup1 = setupKeyboard();
    cleanup1();
    cleanup = setupKeyboard();

    resetStore([makeFile(1), makeFile(2), makeFile(3)], 0);
    pressKey("j");
    // Should move exactly 1, not 2
    expect(cursorIndex.value).toBe(1);
  });

  it("double setupKeyboard without cleanup causes double-fire", async () => {
    // Tear down current
    cleanup!();
    cleanup = null;

    const { setupKeyboard } = await import("../keys");

    // Two setups without cleanup in between — simulates the old HMR bug
    const cleanup1 = setupKeyboard();
    const cleanup2 = setupKeyboard();

    resetStore([makeFile(1), makeFile(2), makeFile(3)], 0);
    pressKey("j");
    // Two handlers → cursor moves twice
    expect(cursorIndex.value).toBe(2);

    // Clean up both to restore state
    cleanup1();
    cleanup2();

    // Re-setup cleanly for remaining tests
    cleanup = setupKeyboard();
  });

  it("m with favMode does not produce duplicate files", async () => {
    const favFile = makeFile(99, "/favs", "best.jpg");
    // random_fav returns a file, then get_files returns the fav list
    mockInvoke
      .mockResolvedValueOnce(favFile)             // random_fav
      .mockResolvedValueOnce([favFile]);           // get_files for ♥
    pressKey("m");
    await vi.waitFor(() => {
      expect(files.value.length).toBe(1);
    });
    // Exactly 1 file, not duplicated
    expect(files.value[0].id).toBe(99);
  });

  it("after cleanup, keys no longer fire", async () => {
    cleanup!();
    cleanup = null;

    resetStore([makeFile(1), makeFile(2), makeFile(3)], 0);
    pressKey("j");
    // No handler active — cursor stays at 0
    expect(cursorIndex.value).toBe(0);

    // Re-setup for other tests
    const { setupKeyboard } = await import("../keys");
    cleanup = setupKeyboard();
  });
});

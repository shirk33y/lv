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

function pressKey(key: string) {
  document.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
}

// setupKeyboard adds addEventListener — call only once to avoid stacking
let keyboardReady = false;
beforeEach(async () => {
  resetStore([makeFile(1), makeFile(2), makeFile(3)], 0);
  mockInvoke.mockReset();
  if (!keyboardReady) {
    const { setupKeyboard } = await import("../keys");
    setupKeyboard();
    keyboardReady = true;
  }
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

  it("j at end stays", () => {
    resetStore([makeFile(1), makeFile(2)], 1);
    pressKey("j");
    expect(cursorIndex.value).toBe(1);
  });

  it("k at start stays", () => {
    pressKey("k");
    expect(cursorIndex.value).toBe(0);
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
});

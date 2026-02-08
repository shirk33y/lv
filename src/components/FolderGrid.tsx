import { useRef, useEffect } from "preact/hooks";
import { useSignal } from "@preact/signals";
import type { FileEntry } from "../store";
import { cursorIndex, indexOfId, sidebarCursor, fileToSidebarIdx } from "../store";

const IS_WINDOWS = navigator.userAgent.includes("Windows");

function thumbUrl(metaId: number): string {
  return IS_WINDOWS
    ? `http://thumb.localhost/${metaId}`
    : `thumb://localhost/${metaId}`;
}

interface Props {
  dir: string;
  dirFiles: FileEntry[];
}

export function FolderGrid({ dir, dirFiles }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const visibleRange = useSignal<[number, number]>([0, 30]);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    function onScroll() {
      if (!el) return;
      const scrollY = el.scrollTop;
      const viewH = el.clientHeight;
      const thumbSize = 220;
      const cols = Math.max(1, Math.floor(el.clientWidth / thumbSize));
      const startRow = Math.floor(scrollY / thumbSize);
      const endRow = Math.ceil((scrollY + viewH) / thumbSize) + 1;
      const start = Math.max(0, startRow * cols);
      const end = Math.min(dirFiles.length, endRow * cols);
      visibleRange.value = [start, end];
    }

    onScroll();
    el.addEventListener("scroll", onScroll, { passive: true });
    const ro = new ResizeObserver(onScroll);
    ro.observe(el);
    return () => {
      el.removeEventListener("scroll", onScroll);
      ro.disconnect();
    };
  }, [dir, dirFiles.length]);

  const thumbSize = 220;
  const containerWidth = containerRef.current?.clientWidth || 800;
  const cols = Math.max(1, Math.floor(containerWidth / thumbSize));
  const rows = Math.ceil(dirFiles.length / cols);
  const totalH = rows * thumbSize;

  const [vStart, vEnd] = visibleRange.value;

  function onClickThumb(file: FileEntry) {
    const idx = indexOfId(file.id);
    if (idx >= 0) {
      cursorIndex.value = idx;
      const si = fileToSidebarIdx.value.get(idx);
      if (si != null) sidebarCursor.value = si;
    }
  }

  const label = dir.split(/[/\\]/).filter(Boolean).pop() || dir;

  return (
    <div class="folder-grid" ref={containerRef}>
      <div class="folder-grid-header">{label} ({dirFiles.length})</div>
      <div class="folder-grid-body" style={{ height: totalH, position: "relative" }}>
        {dirFiles.map((file, i) => {
          if (i < vStart || i >= vEnd) return null;
          const row = Math.floor(i / cols);
          const col = i % cols;
          const hasSrc = file.meta_id != null && file.thumb_ready;
          return (
            <div
              key={file.id}
              class="folder-grid-cell"
              style={{
                position: "absolute",
                top: row * thumbSize,
                left: col * thumbSize,
                width: thumbSize,
                height: thumbSize,
              }}
              onClick={() => onClickThumb(file)}
            >
              {hasSrc ? (
                <img
                  class="folder-grid-img"
                  src={thumbUrl(file.meta_id!)}
                  alt={file.filename}
                  loading="lazy"
                />
              ) : (
                <div class="folder-grid-placeholder">◻</div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

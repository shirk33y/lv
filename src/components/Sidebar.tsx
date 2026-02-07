import { signal } from "@preact/signals";
import { useRef, useEffect } from "preact/hooks";
import { files, cursorIndex, moveCursor } from "../store";
import { Tile } from "./Tile";

/** Exposed for tests */
export const scrollTop = signal(0);

const BUFFER = 5;

export function Sidebar() {
  const items = files.value;
  const cursor = cursorIndex.value;
  const containerRef = useRef<HTMLDivElement>(null);

  const tileH = containerRef.current?.clientWidth || 48;
  const viewH = containerRef.current?.clientHeight || 600;
  const viewCount = Math.max(1, Math.floor(viewH / tileH));

  // Render window centered on cursor
  const half = Math.floor(viewCount / 2) + BUFFER;
  const start = Math.max(0, cursor - half);
  const end = Math.min(items.length, cursor + half + 1);

  // Keep cursor visible: set scrollTop so cursor is roughly centered
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const h = el.clientWidth || 48;
    const view = Math.max(1, Math.floor(el.clientHeight / h));
    const margin = Math.min(3, Math.floor(view / 4));
    const minTop = (cursor - view + margin + 1) * h;
    const maxTop = (cursor - margin) * h;
    if (el.scrollTop < minTop) el.scrollTop = minTop;
    if (el.scrollTop > maxTop) el.scrollTop = maxTop;
    scrollTop.value = el.scrollTop;
  }, [cursor]);

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const delta = e.deltaY > 0 ? 1 : e.deltaY < 0 ? -1 : 0;
    if (delta) moveCursor(delta);
  }

  const totalH = items.length * tileH;

  return (
    <div class="sidebar" ref={containerRef} onWheel={onWheel}>
      <div class="sidebar-track" style={{ height: totalH, position: "relative" }}>
        {items.slice(start, end).map((file, i) => {
          const idx = start + i;
          return (
            <div
              key={file.id}
              class="sidebar-slot"
              style={{ position: "absolute", top: idx * tileH, width: "100%", height: tileH }}
            >
              <Tile file={file} active={idx === cursor} />
            </div>
          );
        })}
      </div>
    </div>
  );
}

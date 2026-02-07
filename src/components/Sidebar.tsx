import { signal } from "@preact/signals";
import { useRef, useEffect, useCallback } from "preact/hooks";
import { files, cursorIndex, moveCursor } from "../store";
import { Tile } from "./Tile";

/** Exposed for tests — current native scrollTop */
export const scrollTop = signal(0);

const BUFFER = 5;

export function Sidebar() {
  const items = files.value;
  const cursor = cursorIndex.value;
  const containerRef = useRef<HTMLDivElement>(null);
  const activeRef = useRef<HTMLDivElement>(null);
  const scrollY = useRef(0);

  // Tile height = sidebar width (square tiles via aspect-ratio)
  const tileH = containerRef.current?.clientWidth || 48;
  const viewH = containerRef.current?.clientHeight || 600;
  const visibleCount = Math.ceil(viewH / tileH) + BUFFER * 2;

  // Render window: always includes cursor
  const scrollIdx = Math.floor(scrollY.current / tileH);
  let start = Math.max(0, scrollIdx - BUFFER);
  let end = Math.min(items.length, scrollIdx + visibleCount);
  // Ensure cursor tile is rendered
  if (cursor < start) start = cursor;
  if (cursor >= end) end = cursor + 1;

  // Scroll active tile into view on cursor change
  useEffect(() => {
    activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [cursor]);

  const onScroll = useCallback((e: Event) => {
    const el = e.target as HTMLElement;
    scrollY.current = el.scrollTop;
    scrollTop.value = el.scrollTop;
  }, []);

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const delta = e.deltaY > 0 ? 1 : e.deltaY < 0 ? -1 : 0;
    if (delta) moveCursor(delta);
  }

  const totalH = items.length * tileH;

  return (
    <div class="sidebar" ref={containerRef} onScroll={onScroll} onWheel={onWheel}>
      <div class="sidebar-track" style={{ height: totalH, position: "relative" }}>
        {items.slice(start, end).map((file, i) => {
          const idx = start + i;
          return (
            <div
              key={file.id}
              ref={idx === cursor ? activeRef : undefined}
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

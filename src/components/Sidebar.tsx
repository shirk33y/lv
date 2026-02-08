import { signal } from "@preact/signals";
import { useRef, useEffect } from "preact/hooks";
import { cursorIndex, moveCursor, sidebarItems, fileToSidebarIdx } from "../store";
import { Tile } from "./Tile";
import { FolderTile } from "./FolderTile";

/** Exposed for tests */
export const scrollTop = signal(0);

const BUFFER = 5;

export function Sidebar() {
  const items = sidebarItems.value;
  const cursor = cursorIndex.value;
  const containerRef = useRef<HTMLDivElement>(null);

  const tileH = containerRef.current?.clientWidth || 48;
  const viewH = containerRef.current?.clientHeight || 600;
  const viewCount = Math.max(1, Math.floor(viewH / tileH));

  // Find the sidebar index of the current cursor file
  const cursorSIdx = fileToSidebarIdx.value.get(cursor) ?? 0;

  // Render window centered on cursor's sidebar position
  const half = Math.floor(viewCount / 2) + BUFFER;
  const start = Math.max(0, cursorSIdx - half);
  const end = Math.min(items.length, cursorSIdx + half + 1);

  // Always center cursor tile in viewport
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const h = el.clientWidth || 48;
    const target = cursorSIdx * h - el.clientHeight / 2 + h / 2;
    el.scrollTop = Math.max(0, target);
    scrollTop.value = el.scrollTop;
  }, [cursorSIdx]);

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const delta = e.deltaY > 0 ? 1 : e.deltaY < 0 ? -1 : 0;
    if (delta) moveCursor(delta);
  }

  const totalH = items.length * tileH;

  return (
    <div class="sidebar" ref={containerRef} onWheel={onWheel}>
      <div class="sidebar-track" style={{ height: totalH, position: "relative" }}>
        {items.slice(start, end).map((item, i) => {
          const idx = start + i;
          if (item.type === "folder") {
            return (
              <div
                key={`dir-${item.dir}`}
                class="sidebar-slot"
                style={{ position: "absolute", top: idx * tileH, width: "100%", height: tileH }}
              >
                <FolderTile dir={item.dir} dirFiles={item.dirFiles} />
              </div>
            );
          }
          return (
            <div
              key={item.file.id}
              class="sidebar-slot"
              style={{ position: "absolute", top: idx * tileH, width: "100%", height: tileH }}
            >
              <Tile file={item.file} active={item.fileIndex === cursor} />
            </div>
          );
        })}
      </div>
    </div>
  );
}

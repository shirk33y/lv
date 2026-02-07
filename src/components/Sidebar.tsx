import { signal, useSignal } from "@preact/signals";
import { useRef, useEffect } from "preact/hooks";
import { files, cursorIndex, moveCursor } from "../store";
import { Tile } from "./Tile";

export const scrollTop = signal(0);
const anchor = signal(0);

export function Sidebar() {
  const items = files.value;
  const cursor = cursorIndex.value;
  const sidebarRef = useRef<HTMLDivElement>(null);
  const noAnim = useSignal(false);

  const tileH = sidebarRef.current?.clientWidth ?? 48;
  const availH = sidebarRef.current ? sidebarRef.current.clientHeight - 28 : 600;
  const viewportSize = Math.max(1, Math.floor(availH / tileH));
  const margin = Math.min(3, Math.floor(viewportSize / 4));

  const lo = Math.max(0, cursor - (viewportSize - margin - 1));
  const hi = Math.max(0, cursor - margin);
  let s = scrollTop.value;
  if (s < lo) s = lo;
  if (s > hi) s = hi;
  scrollTop.value = s;

  // Anchor-based virtual scroll for smooth animation
  const BUFFER = 8;
  let a = anchor.value;
  if (s < a + 1 || s + viewportSize > a + viewportSize + BUFFER * 2 - 1 || a >= items.length) {
    a = Math.max(0, s - BUFFER);
    anchor.value = a;
    noAnim.value = true;
  }

  const renderStart = a;
  const renderEnd = Math.min(items.length, a + viewportSize + BUFFER * 2);
  const visible = items.slice(renderStart, renderEnd);
  const offsetPx = (s - renderStart) * tileH;

  useEffect(() => {
    if (noAnim.value) {
      requestAnimationFrame(() => { noAnim.value = false; });
    }
  });

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const delta = e.deltaY > 0 ? 1 : e.deltaY < 0 ? -1 : 0;
    if (delta) moveCursor(delta);
  }

  return (
    <div class="sidebar" ref={sidebarRef} onWheel={onWheel}>
      <div
        class={`sidebar-track${noAnim.value ? "" : " sidebar-animate"}`}
        style={{ transform: `translateY(-${offsetPx}px)` }}
      >
        {visible.map((file, i) => (
          <Tile
            key={file.id}
            file={file}
            active={renderStart + i === cursor}
          />
        ))}
      </div>
    </div>
  );
}

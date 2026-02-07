import { signal } from "@preact/signals";
import { files, cursorIndex, moveCursor } from "../store";
import { Tile } from "./Tile";

export const scrollTop = signal(0);

export function Sidebar() {
  const items = files.value;
  const cursor = cursorIndex.value;

  const viewportSize = 12;
  const margin = 3;
  // Clamp scroll so cursor stays within [margin .. viewportSize-margin-1]
  const lo = Math.max(0, cursor - (viewportSize - margin - 1));
  const hi = Math.max(0, cursor - margin);
  // Keep previous scroll if it's within valid range, otherwise clamp
  let s = scrollTop.value;
  if (s < lo) s = lo;
  if (s > hi) s = hi;
  scrollTop.value = s;
  const visible = items.slice(s, s + viewportSize);

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const delta = e.deltaY > 0 ? 1 : e.deltaY < 0 ? -1 : 0;
    if (delta) moveCursor(delta);
  }

  return (
    <div class="sidebar" onWheel={onWheel}>
      {visible.map((file, i) => (
        <Tile
          key={file.id}
          file={file}
          active={s + i === cursor}
        />
      ))}
    </div>
  );
}

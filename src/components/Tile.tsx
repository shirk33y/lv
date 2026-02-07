import { useSignal } from "@preact/signals";
import type { FileEntry } from "../store";
import { cursorIndex, indexOfId } from "../store";

const IS_WINDOWS = navigator.userAgent.includes("Windows");

function thumbUrl(metaId: number): string {
  return IS_WINDOWS
    ? `http://thumb.localhost/${metaId}`
    : `thumb://localhost/${metaId}`;
}

interface Props {
  file: FileEntry;
  active: boolean;
}

export function Tile({ file, active }: Props) {
  const thumbSrc = file.meta_id && file.thumb_ready ? thumbUrl(file.meta_id) : "";
  const loaded = useSignal(false);
  const errored = useSignal(false);

  const showThumb = thumbSrc && !errored.value;
  const showShadow = file.shadow && !loaded.value;
  const showPlaceholder = !showThumb && !showShadow;

  function onClick() {
    const idx = indexOfId(file.id);
    if (idx >= 0) cursorIndex.value = idx;
  }

  return (
    <div class={`tile${active ? " active" : ""}`} onClick={onClick}>
      {showShadow ? (
        <img class="tile-shadow" src={file.shadow!} alt="" />
      ) : null}
      {showThumb ? (
        <img
          class={`tile-thumb${loaded.value ? " loaded" : ""}`}
          src={thumbSrc}
          alt={file.filename}
          loading="lazy"
          onLoad={() => { loaded.value = true; }}
          onError={() => { errored.value = true; }}
        />
      ) : null}
      {showPlaceholder ? (
        <div class="tile-placeholder">
          <span class="tile-placeholder-icon">{errored.value ? "⚠" : "◻"}</span>
        </div>
      ) : null}
      {file.liked ? <span class="tile-heart">♥</span> : null}
    </div>
  );
}

import type { FileEntry } from "../store";

const IS_WINDOWS = navigator.userAgent.includes("Windows");

function thumbUrl(metaId: number): string {
  return IS_WINDOWS
    ? `http://thumb.localhost/${metaId}`
    : `thumb://localhost/${metaId}`;
}

/** Pick 4 deterministic preview files: first, ~1/3, ~2/3, last */
function pickFour(dirFiles: FileEntry[]): (FileEntry | null)[] {
  const n = dirFiles.length;
  if (n === 0) return [null, null, null, null];
  if (n === 1) return [dirFiles[0], null, null, null];
  if (n === 2) return [dirFiles[0], null, null, dirFiles[1]];
  if (n === 3) return [dirFiles[0], dirFiles[1], null, dirFiles[2]];
  return [
    dirFiles[0],
    dirFiles[Math.floor(n / 3)],
    dirFiles[Math.floor((2 * n) / 3)],
    dirFiles[n - 1],
  ];
}

function MiniThumb({ file }: { file: FileEntry | null }) {
  if (!file || !file.meta_id || !file.thumb_ready) {
    return <div class="folder-tile-empty" />;
  }
  return (
    <img
      class="folder-tile-img"
      src={thumbUrl(file.meta_id)}
      alt=""
      loading="lazy"
      onError={(e) => {
        (e.currentTarget as HTMLImageElement).style.display = "none";
      }}
    />
  );
}

interface Props {
  dir: string;
  dirFiles: FileEntry[];
}

export function FolderTile({ dir, dirFiles }: Props) {
  const [first, rand1, rand2, last] = pickFour(dirFiles);
  const label = dir.split(/[/\\]/).filter(Boolean).pop() || dir;

  return (
    <div class="folder-tile" title={dir}>
      <div class="folder-tile-grid">
        <MiniThumb file={first} />
        <MiniThumb file={rand1} />
        <MiniThumb file={rand2} />
        <MiniThumb file={last} />
      </div>
      <div class="folder-tile-label">{label}</div>
    </div>
  );
}

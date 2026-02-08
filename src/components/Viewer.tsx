import { useSignal } from "@preact/signals";
import { useEffect, useRef } from "preact/hooks";
import { convertFileSrc } from "@tauri-apps/api/core";
import { currentFile, selectedFolder } from "../store";
import { FolderGrid } from "./FolderGrid";

const VIDEO_EXTS = new Set(["mp4", "webm"]);

const IS_WINDOWS = navigator.userAgent.includes("Windows");

function protoUrl(scheme: string, path: string): string {
  return IS_WINDOWS
    ? `http://${scheme}.localhost/${path}`
    : `${scheme}://localhost/${path}`;
}

function fileSrc(path: string): string {
  return protoUrl("lv-file", encodeURIComponent(path));
}

function thumbSrc(metaId: number): string {
  return protoUrl("thumb", String(metaId));
}

function extOf(filename: string): string {
  const i = filename.lastIndexOf(".");
  return i >= 0 ? filename.slice(i + 1).toLowerCase() : "";
}

function VideoViewer({ file }: { file: { path: string; filename: string; meta_id: number | null; thumb_ready: boolean } }) {
  const playing = useSignal(false);
  const error = useSignal<string | null>(null);
  const videoRef = useRef<HTMLVideoElement>(null);

  useEffect(() => {
    playing.value = false;
    error.value = null;
  }, [file.path]);

  if (!playing.value) {
    return (
      <>
        {file.meta_id && file.thumb_ready ? (
          <img src={thumbSrc(file.meta_id)} alt={file.filename} />
        ) : null}
        <div class="viewer-play-overlay" onClick={() => { playing.value = true; }}>
          <span class="viewer-play-btn">▶</span>
          <span class="viewer-play-file">{file.filename}</span>
        </div>
      </>
    );
  }

  return (
    <>
      <video
        ref={videoRef}
        src={convertFileSrc(file.path)}
        class="viewer-video"
        controls
        autoPlay
        onError={(e) => {
          const el = e.currentTarget as HTMLVideoElement;
          const code = el?.error?.code;
          const msg = el?.error?.message || "Unknown error";
          const codeStr = code != null
            ? { 1: "ABORTED", 2: "NETWORK", 3: "DECODE", 4: "SRC_NOT_SUPPORTED" }[code] ?? `CODE_${code}`
            : "";
          error.value = `${codeStr}: ${msg}`;
        }}
      />
      {error.value && (
        <div class="viewer-error">
          <span>⚠ {error.value}</span>
          <span class="viewer-error-file">{file.filename}</span>
        </div>
      )}
    </>
  );
}

export function Viewer() {
  const folder = selectedFolder.value;
  if (folder) {
    return (
      <div class="viewer">
        <FolderGrid dir={folder.dir} dirFiles={folder.dirFiles} />
      </div>
    );
  }

  const file = currentFile.value;
  if (!file) {
    return <div class="viewer" />;
  }

  const ext = extOf(file.filename);
  const isVideo = VIDEO_EXTS.has(ext);

  if (isVideo) {
    return (
      <div class="viewer">
        <VideoViewer file={file} />
      </div>
    );
  }

  return (
    <div class="viewer">
      <img src={fileSrc(file.path)} alt={file.filename} />
    </div>
  );
}

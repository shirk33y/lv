import { useSignal, useSignalEffect } from "@preact/signals";
import { invoke } from "@tauri-apps/api/core";
import { currentFile, showInfo } from "../store";

interface FileMeta {
  file_id: number;
  path: string;
  dir: string;
  filename: string;
  size: number | null;
  modified_at: string | null;
  hash_sha512: string | null;
  meta_id: number | null;
  width: number | null;
  height: number | null;
  format: string | null;
  duration_ms: number | null;
  bitrate: number | null;
  codecs: string | null;
  tags: string[];
  thumb_ready: boolean;
}

function formatSize(bytes: number | null): string {
  if (bytes == null) return "—";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function formatDuration(ms: number | null): string {
  if (ms == null) return "—";
  const s = Math.floor(ms / 1000);
  const m = Math.floor(s / 60);
  const h = Math.floor(m / 60);
  if (h > 0) return `${h}:${String(m % 60).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
  return `${m}:${String(s % 60).padStart(2, "0")}`;
}

export function InfoPanel() {
  const meta = useSignal<FileMeta | null>(null);

  useSignalEffect(() => {
    const file = currentFile.value;
    const visible = showInfo.value;
    if (!visible || !file) {
      meta.value = null;
      return;
    }
    invoke<FileMeta | null>("get_file_metadata", { fileId: file.id })
      .then((m) => { meta.value = m; })
      .catch(() => { meta.value = null; });
  });

  if (!showInfo.value) return null;

  const m = meta.value;

  const rows: [string, string][] = [];
  if (m) {
    rows.push(["Filename", m.filename]);
    rows.push(["Path", m.path]);
    rows.push(["Directory", m.dir]);
    rows.push(["Size", formatSize(m.size)]);
    rows.push(["Modified", m.modified_at ?? "—"]);
    if (m.format) rows.push(["Format", m.format]);
    if (m.width && m.height) rows.push(["Dimensions", `${m.width} × ${m.height}`]);
    if (m.duration_ms) rows.push(["Duration", formatDuration(m.duration_ms)]);
    if (m.bitrate) rows.push(["Bitrate", `${Math.round(m.bitrate / 1000)} kbps`]);
    if (m.codecs) rows.push(["Codecs", m.codecs]);
    rows.push(["Tags", m.tags.length > 0 ? m.tags.join(", ") : "—"]);
    rows.push(["Thumb ready", m.thumb_ready ? "yes" : "no"]);
    if (m.meta_id) rows.push(["Meta ID", String(m.meta_id)]);
    if (m.hash_sha512) rows.push(["SHA-512", m.hash_sha512]);
  }

  return (
    <div class="right-panel">
      <div class="right-panel-header">Info</div>
      <div class="right-panel-body">
        {m ? (
          <table class="info-table">
            <tbody>
              {rows.map(([label, value]) => (
                <tr key={label}>
                  <td class="meta-label">{label}</td>
                  <td class="meta-value">{value}</td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : (
          <div class="meta-empty">No metadata</div>
        )}
      </div>
    </div>
  );
}

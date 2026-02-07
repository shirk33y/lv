import { currentFile, cursorIndex, totalFiles, jobStatus, cwd } from "../store";

function relativePath(absPath: string, cwdStr: string): string {
  if (!cwdStr) return absPath;
  const sep = absPath.includes("\\") ? "\\" : "/";
  if (absPath.startsWith(cwdStr + sep)) {
    return absPath.slice(cwdStr.length + 1);
  }
  return absPath;
}

export function StatusBar() {
  const file = currentFile.value;
  const pos = cursorIndex.value + 1;
  const total = totalFiles.value;
  const status = jobStatus.value;
  const cwdStr = cwd.value;

  let statusText = "";
  if (status) {
    const parts: string[] = [];
    parts.push(`hashed: ${status.hashed}/${status.files}`);
    parts.push(`thumbs: ${status.thumbs}/${status.files}`);
    const active = status.jobs_pending + status.jobs_running;
    if (active > 0) {
      parts.push(`⏳ ${status.jobs_running} running, ${status.jobs_pending} queued`);
    }
    if (status.jobs_failed > 0) {
      parts.push(`⚠ ${status.jobs_failed} failed`);
    }
    statusText = parts.join("  ");
  }

  const rawPath = file ? file.path.replace(/^\\\\\?\\/, "") : "";
  const displayPath = rawPath ? relativePath(rawPath, cwdStr) : "";
  const heart = file?.liked ? "♥ " : "";

  return (
    <div class="status-bar">
      <span class="status-left">
        {file ? `${heart}${pos}/${total}  ${displayPath}` : "no files"}
      </span>
      {statusText ? <span class="status-right">{statusText}</span> : null}
    </div>
  );
}

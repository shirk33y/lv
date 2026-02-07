import { useRef, useEffect, useMemo } from "preact/hooks";
import { useSignal } from "@preact/signals";
import { showLogs, logEntries, type LogEntry } from "../store";

/** Fast timestamp formatter — avoids toLocaleTimeString() which is ~50× slower */
function fmtTime(ts: number): string {
  const d = new Date(ts);
  const h = d.getHours();
  const m = d.getMinutes();
  const s = d.getSeconds();
  return `${h < 10 ? "0" : ""}${h}:${m < 10 ? "0" : ""}${m}:${s < 10 ? "0" : ""}${s}`;
}

/** Max entries to keep rendered — older ones are discarded for perf */
const MAX_LOG_ENTRIES = 500;

function LogLine({ entry }: { entry: LogEntry }) {
  return (
    <div class={`log-line log-${entry.level}`}>
      <span class="log-ts">{fmtTime(entry.ts)}</span>
      {" "}
      {entry.msg}
    </div>
  );
}

export function LogPanel() {
  const scrollRef = useRef<HTMLDivElement>(null);
  const autoScroll = useSignal(true);

  if (!showLogs.value) return null;

  const entries = logEntries.value;
  const visible = useMemo(
    () => entries.length > MAX_LOG_ENTRIES ? entries.slice(-MAX_LOG_ENTRIES) : entries,
    [entries],
  );

  // Auto-scroll after DOM commit (useEffect fires after paint)
  useEffect(() => {
    if (!autoScroll.value) return;
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [visible.length]);

  function onScroll() {
    const el = scrollRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 2;
    autoScroll.value = atBottom;
  }

  return (
    <div class="right-panel">
      <div class="right-panel-header">Logs</div>
      <div class="right-panel-body log-body" ref={scrollRef} onScroll={onScroll}>
        {visible.length === 0 ? (
          <div class="meta-empty">No log entries</div>
        ) : (
          visible.map((e, i) => <LogLine key={i} entry={e} />)
        )}
      </div>
      {!autoScroll.value && entries.length > 0 ? (
        <button
          class="log-resume"
          onClick={() => {
            autoScroll.value = true;
            const el = scrollRef.current;
            if (el) el.scrollTop = el.scrollHeight;
          }}
        >
          ▼ Resume
        </button>
      ) : null}
    </div>
  );
}

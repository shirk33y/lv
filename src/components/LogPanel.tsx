import { useRef } from "preact/hooks";
import { useSignal, useSignalEffect } from "@preact/signals";
import { showLogs, logEntries } from "../store";

export function LogPanel() {
  const scrollRef = useRef<HTMLDivElement>(null);
  const autoScroll = useSignal(true);

  useSignalEffect(() => {
    const _len = logEntries.value.length; // track signal
    if (autoScroll.value && scrollRef.current) {
      // defer to after DOM paint
      requestAnimationFrame(() => {
        if (scrollRef.current) {
          scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
        }
      });
    }
  });

  if (!showLogs.value) return null;

  function onScroll() {
    const el = scrollRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 30;
    autoScroll.value = atBottom;
  }

  const entries = logEntries.value;

  return (
    <div class="right-panel">
      <div class="right-panel-header">Logs</div>
      <div class="right-panel-body log-body" ref={scrollRef} onScroll={onScroll}>
        {entries.length === 0 ? (
          <div class="meta-empty">No log entries</div>
        ) : (
          entries.map((e, i) => (
            <div key={i} class={`log-line log-${e.level}`}>
              <span class="log-ts">{new Date(e.ts).toLocaleTimeString()}</span>
              {" "}
              {e.msg}
            </div>
          ))
        )}
        {!autoScroll.value && entries.length > 0 ? (
          <button
            class="log-resume"
            onClick={() => {
              autoScroll.value = true;
              if (scrollRef.current) {
                scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
              }
            }}
          >
            ▼ Resume
          </button>
        ) : null}
      </div>
    </div>
  );
}

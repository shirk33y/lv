import { useEffect } from "preact/hooks";
import { invoke } from "@tauri-apps/api/core";
import { Sidebar } from "./components/Sidebar";
import { Viewer } from "./components/Viewer";
import { StatusBar } from "./components/StatusBar";
import { InfoPanel } from "./components/MetadataOverlay";
import { LogPanel } from "./components/LogPanel";
import { HelpOverlay } from "./components/HelpOverlay";
import { loadFiles, jobStatus, addLog, cwd, type JobStatus } from "./store";
import { setupKeyboard } from "./keys";
import { setupPreloader } from "./preloader";

async function pollStatus() {
  try {
    const prev = jobStatus.value;
    const s = await invoke<JobStatus>("get_status");
    jobStatus.value = s;
    if (prev) {
      const dt = s.thumbs - prev.thumbs;
      const df = s.files - prev.files;
      const dh = s.hashed - prev.hashed;
      if (df > 0) {
        addLog("info", `scanned: ${s.files} files (+${df})`);
        loadFiles();
      }
      if (dh > 0) addLog("info", `hashed: ${s.hashed}/${s.files} (+${dh})`);
      if (dt > 0) addLog("info", `thumbs: ${s.thumbs}/${s.files} (+${dt})`);
      if (s.jobs_failed > prev.jobs_failed) addLog("error", `jobs failed: ${s.jobs_failed} (+${s.jobs_failed - prev.jobs_failed})`);
      if (s.jobs_running > 0 && prev.jobs_running === 0) addLog("info", `jobs started: ${s.jobs_running} running, ${s.jobs_pending} queued`);
      if (prev.jobs_running > 0 && s.jobs_running === 0 && s.jobs_pending === 0) addLog("info", "✓ all jobs complete");
    } else {
      addLog("info", `status: ${s.files} files, ${s.thumbs} thumbs, ${s.hashed} hashed`);
    }
  } catch { /* ignore */ }
}

export function App() {
  useEffect(() => {
    invoke<string>("get_cwd").then((c) => { cwd.value = c; }).catch(() => {});
    addLog("info", "starting…");
    invoke<string | null>("get_first_dir").then((dir) => {
      loadFiles(dir ?? undefined);
    }).catch(() => loadFiles());
    const cleanupKeys = setupKeyboard();
    setupPreloader();
    pollStatus();
    const id = setInterval(pollStatus, 2000);
    return () => { cleanupKeys(); clearInterval(id); };
  }, []);

  return (
    <div class="layout">
      <Sidebar />
      <Viewer />
      <InfoPanel />
      <LogPanel />
      <HelpOverlay />
      <StatusBar />
    </div>
  );
}

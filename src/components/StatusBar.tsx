import { useRef, useEffect } from "preact/hooks";
import { useSignal } from "@preact/signals";
import { currentFile, cursorIndex, totalFiles, jobStatus, cwd, type JobStatus } from "../store";

function relativePath(absPath: string, cwdStr: string): string {
  if (!cwdStr) return absPath;
  const sep = absPath.includes("\\") ? "\\" : "/";
  if (absPath.startsWith(cwdStr + sep)) {
    return absPath.slice(cwdStr.length + 1);
  }
  return absPath;
}

interface JobDisplay {
  label: string;
  current: number;
  total: number;
  errors: number;
}

function activeJobs(s: JobStatus): JobDisplay[] {
  const jobs: JobDisplay[] = [];
  const working = s.jobs_running > 0 || s.jobs_pending > 0;
  if (working && s.hashed < s.files) {
    jobs.push({ label: "hash", current: s.hashed, total: s.files, errors: 0 });
  }
  if (working && s.thumbs < s.files) {
    jobs.push({ label: "thumb", current: s.thumbs, total: s.files, errors: 0 });
  }
  // Attach global error count to first active job
  if (jobs.length > 0 && s.jobs_failed > 0) {
    jobs[0].errors = s.jobs_failed;
  }
  return jobs;
}

function JobChip({ job }: { job: JobDisplay }) {
  return (
    <span class="status-job">
      {job.label}: {job.current}
      {job.errors > 0 && <span class="status-job-errors">!{job.errors}</span>}
      <span class="status-job-sep">/</span>
      {job.total}
    </span>
  );
}

function CompletedChip({ label }: { label: string }) {
  return (
    <span class="status-job">
      {label}: <span class="status-job-complete">complete</span>
    </span>
  );
}

export function StatusBar() {
  const file = currentFile.value;
  const pos = cursorIndex.value + 1;
  const total = totalFiles.value;
  const status = jobStatus.value;
  const cwdStr = cwd.value;

  // Track completed jobs (show "complete" for 2s then remove)
  const prevRef = useRef<JobStatus | null>(null);
  const doneJobs = useSignal<Set<string>>(new Set());

  useEffect(() => {
    const prev = prevRef.current;
    prevRef.current = status;
    if (!prev || !status) return;
    const newDone: string[] = [];
    if (prev.hashed < prev.files && status.hashed >= status.files && status.files > 0) newDone.push("hash");
    if (prev.thumbs < prev.files && status.thumbs >= status.files && status.files > 0) newDone.push("thumb");
    if (newDone.length === 0) return;
    doneJobs.value = new Set([...doneJobs.value, ...newDone]);
    const timer = setTimeout(() => {
      const next = new Set(doneJobs.value);
      newDone.forEach((j) => next.delete(j));
      doneJobs.value = next;
    }, 2000);
    return () => clearTimeout(timer);
  }, [status]);

  const rawPath = file ? file.path.replace(/^\\\\\?\\/, "") : "";
  const displayPath = rawPath ? relativePath(rawPath, cwdStr) : "";

  // Split into dir + basename
  const sep = displayPath.includes("\\") ? "\\" : "/";
  const lastSep = displayPath.lastIndexOf(sep);
  const dirPart = lastSep >= 0 ? displayPath.slice(0, lastSep + 1) : "";
  const basePart = lastSep >= 0 ? displayPath.slice(lastSep + 1) : displayPath;

  const jobs = status ? activeJobs(status) : [];
  const done = doneJobs.value;

  return (
    <div class="status-bar">
      <span class="status-left">
        {file ? (
          <>
            {dirPart}<span class="status-basename">{basePart}</span>
          </>
        ) : "no files"}
      </span>
      <span class="status-center">
        {file?.liked && <span class="status-heart">♥</span>}
        <span class="status-pager">{file ? `${pos}/${total}` : ""}</span>
      </span>
      <span class="status-right">
        {jobs.map((j) => <JobChip key={j.label} job={j} />)}
        {[...done].filter((d) => !jobs.find((j) => j.label === d)).map((d) => (
          <CompletedChip key={d} label={d} />
        ))}
      </span>
    </div>
  );
}

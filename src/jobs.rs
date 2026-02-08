//! Background job engine with independent metadata layers.
//!
//! Layers: Hash, Exif (more to come: xattr, tiny_thumb).
//! Workers process missing layers lazily, with resource throttling
//! and permanent-failure debounce.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::db::Db;

// ── Layer enum ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Layer {
    Hash,
    Exif,
    AiBasic,
}

impl Layer {
    pub fn name(&self) -> &'static str {
        match self {
            Layer::Hash => "hash",
            Layer::Exif => "exif",
            Layer::AiBasic => "ai_basic",
        }
    }
}

const LAYERS: &[Layer] = &[Layer::Hash, Layer::Exif, Layer::AiBasic];

// ── Stats (shared with UI via Arc) ──────────────────────────────────────

pub struct JobStats {
    pub done: AtomicU64,
    pub failed: AtomicU64,
    pub active: AtomicU32,
    pub turbo: AtomicBool,
    last_error: Mutex<String>,
    // Rate tracking
    rate_snapshot: AtomicU64,
    rate_time: Mutex<Instant>,
    pub jobs_per_min: AtomicU32, // scaled x10 for one decimal
}

impl JobStats {
    fn new() -> Self {
        Self {
            done: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            active: AtomicU32::new(0),
            turbo: AtomicBool::new(false),
            last_error: Mutex::new(String::new()),
            rate_snapshot: AtomicU64::new(0),
            rate_time: Mutex::new(Instant::now()),
            jobs_per_min: AtomicU32::new(0),
        }
    }

    fn record_done(&self) {
        self.done.fetch_add(1, Ordering::Relaxed);
    }

    fn record_fail(&self, err: &str) {
        self.failed.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut le) = self.last_error.lock() {
            le.clear();
            le.push_str(&err[..err.len().min(120)]);
        }
    }

    pub fn last_error(&self) -> String {
        self.last_error
            .lock()
            .map(|e| e.clone())
            .unwrap_or_default()
    }

    /// Call periodically (~every 5s) to update jobs_per_min.
    pub fn update_rate(&self) {
        let done_now = self.done.load(Ordering::Relaxed);
        let prev = self.rate_snapshot.swap(done_now, Ordering::Relaxed);
        if let Ok(mut t) = self.rate_time.lock() {
            let elapsed = t.elapsed().as_secs_f64();
            if elapsed > 0.5 {
                let delta = done_now.saturating_sub(prev) as f64;
                let per_min = (delta / elapsed * 60.0 * 10.0) as u32; // x10
                self.jobs_per_min.store(per_min, Ordering::Relaxed);
                *t = Instant::now();
            }
        }
    }
}

// ── Engine ──────────────────────────────────────────────────────────────

pub struct JobEngine {
    pub stats: Arc<JobStats>,
    quit: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
}

impl JobEngine {
    pub fn start(db: Db) -> Self {
        db.ensure_jobs_schema();

        let stats = Arc::new(JobStats::new());
        let quit = Arc::new(AtomicBool::new(false));

        let ncpus = thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);

        // Spawn worker threads (1 base + extras that activate in turbo)
        let num_workers = (ncpus / 2).clamp(1, 4);
        let mut handles = Vec::new();

        for worker_id in 0..num_workers {
            let db = db.clone();
            let stats = stats.clone();
            let quit = quit.clone();
            let h = thread::Builder::new()
                .name(format!("job-worker-{}", worker_id))
                .spawn(move || worker_loop(db, stats, quit, worker_id))
                .expect("spawn worker");
            handles.push(h);
        }

        // Rate updater thread
        {
            let stats = stats.clone();
            let quit = quit.clone();
            let h = thread::Builder::new()
                .name("job-rate".into())
                .spawn(move || {
                    while !quit.load(Ordering::Relaxed) {
                        thread::sleep(Duration::from_secs(5));
                        stats.update_rate();
                    }
                })
                .expect("spawn rate updater");
            handles.push(h);
        }

        eprintln!("jobs: {} workers, lazy mode", num_workers);

        JobEngine {
            stats,
            quit,
            handles,
        }
    }

    pub fn stop(&mut self) {
        self.quit.store(true, Ordering::Release);
        for h in self.handles.drain(..) {
            h.join().ok();
        }
    }
}

impl Drop for JobEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

// ── Worker loop ─────────────────────────────────────────────────────────

fn worker_loop(db: Db, stats: Arc<JobStats>, quit: Arc<AtomicBool>, worker_id: usize) {
    // Worker 0 always runs. Workers 1+ only run in turbo mode.
    loop {
        if quit.load(Ordering::Relaxed) {
            break;
        }

        let turbo = stats.turbo.load(Ordering::Relaxed);

        // Non-primary workers sleep in lazy mode
        if worker_id > 0 && !turbo {
            thread::sleep(Duration::from_secs(2));
            continue;
        }

        // Find next work item
        let work = find_work(&db);

        if let Some((file_id, layer, path)) = work {
            stats.active.fetch_add(1, Ordering::Relaxed);
            let t0 = Instant::now();

            let result = process_layer(&db, file_id, layer, &path);

            let elapsed = t0.elapsed();
            stats.active.fetch_sub(1, Ordering::Relaxed);

            match result {
                Ok(()) => stats.record_done(),
                Err(e) => {
                    db.record_job_fail(file_id, layer.name(), &e);
                    stats.record_fail(&e);
                }
            }

            // Throttle: sleep proportional to job duration
            // Lazy: ~30% CPU → sleep ~2.3x job time
            // Turbo: ~80% CPU → sleep ~0.25x job time
            let factor = if turbo { 0.25 } else { 2.3 };
            let sleep = Duration::from_secs_f64(elapsed.as_secs_f64() * factor);
            thread::sleep(sleep.min(Duration::from_secs(5)));
        } else {
            // No work available, idle
            let idle = if turbo {
                Duration::from_secs(3)
            } else {
                Duration::from_secs(10)
            };
            thread::sleep(idle);
        }
    }
}

fn find_work(db: &Db) -> Option<(i64, Layer, String)> {
    for layer in LAYERS {
        let result = match layer {
            Layer::Hash => db.next_missing_hash(),
            Layer::Exif => db.next_missing_exif(),
            Layer::AiBasic => db.next_missing_pnginfo(),
        };
        if let Some((file_id, path)) = result {
            return Some((file_id, *layer, path));
        }
    }
    None
}

// ── Layer processors ────────────────────────────────────────────────────

fn process_layer(db: &Db, file_id: i64, layer: Layer, path: &str) -> Result<(), String> {
    match layer {
        Layer::Hash => process_hash(db, file_id, path),
        Layer::Exif => process_exif(db, file_id, path),
        Layer::AiBasic => process_ai_basic(db, file_id, path),
    }
}

// ── Hash layer ──────────────────────────────────────────────────────────

const FAST_HASH_THRESHOLD: u64 = 2 * 1024 * 1024;
const FINGERPRINT_CHUNK: usize = 64 * 1024;

fn process_hash(db: &Db, file_id: i64, path: &str) -> Result<(), String> {
    use sha2::{Digest, Sha512};
    use std::io::{Read, Seek, SeekFrom};

    // Try xattr cache first (instant on Linux)
    #[cfg(unix)]
    {
        if let Ok(Some(v)) = xattr_get(path, "user.lv.sha512") {
            if let Ok(h) = String::from_utf8(v) {
                db.file_set_hash_meta(file_id, &h);
                return Ok(());
            }
        }
    }

    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let file_size = file.metadata().map_err(|e| e.to_string())?.len();

    let hash = if file_size > FAST_HASH_THRESHOLD {
        // Fingerprint: head + tail + size
        let mut hasher = Sha512::new();
        let mut head = vec![0u8; FINGERPRINT_CHUNK.min(file_size as usize)];
        file.read_exact(&mut head).map_err(|e| e.to_string())?;
        hasher.update(&head);

        if file_size > FINGERPRINT_CHUNK as u64 * 2 {
            file.seek(SeekFrom::End(-(FINGERPRINT_CHUNK as i64)))
                .map_err(|e| e.to_string())?;
            let mut tail = vec![0u8; FINGERPRINT_CHUNK];
            file.read_exact(&mut tail).map_err(|e| e.to_string())?;
            hasher.update(&tail);
        }
        hasher.update(file_size.to_le_bytes());
        format!("fp:{:x}", hasher.finalize())
    } else {
        // Full SHA-512
        let mut hasher = Sha512::new();
        let mut buf = [0u8; 65536];
        loop {
            let n = file.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        format!("{:x}", hasher.finalize())
    };

    // Cache in xattr (ignore errors)
    #[cfg(unix)]
    xattr_set(path, "user.lv.sha512", hash.as_bytes());

    db.file_set_hash_meta(file_id, &hash);
    Ok(())
}

#[cfg(unix)]
fn xattr_get(path: &str, name: &str) -> Result<Option<Vec<u8>>, ()> {
    use std::ffi::CString;
    let c_path = CString::new(path).map_err(|_| ())?;
    let c_name = CString::new(name).map_err(|_| ())?;

    // First call to get size
    let size = unsafe { libc::getxattr(c_path.as_ptr(), c_name.as_ptr(), std::ptr::null_mut(), 0) };
    if size < 0 {
        return Ok(None);
    }
    let mut buf = vec![0u8; size as usize];
    let ret = unsafe {
        libc::getxattr(
            c_path.as_ptr(),
            c_name.as_ptr(),
            buf.as_mut_ptr() as *mut _,
            buf.len(),
        )
    };
    if ret < 0 {
        return Ok(None);
    }
    buf.truncate(ret as usize);
    Ok(Some(buf))
}

#[cfg(unix)]
fn xattr_set(path: &str, name: &str, value: &[u8]) {
    use std::ffi::CString;
    if let (Ok(c_path), Ok(c_name)) = (CString::new(path), CString::new(name)) {
        unsafe {
            libc::setxattr(
                c_path.as_ptr(),
                c_name.as_ptr(),
                value.as_ptr() as *const _,
                value.len(),
                0,
            );
        }
    }
}

// ── Exif layer ──────────────────────────────────────────────────────────

fn process_exif(db: &Db, file_id: i64, path: &str) -> Result<(), String> {
    let dims = image::image_dimensions(path).map_err(|e| e.to_string())?;
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    let format = match ext.as_str() {
        "jpg" | "jpeg" => "JPEG",
        "png" => "PNG",
        "webp" => "WebP",
        "gif" => "GIF",
        "bmp" => "BMP",
        "tiff" | "tif" => "TIFF",
        _ => "Unknown",
    };
    db.meta_set_dimensions(file_id, dims.0, dims.1, format);
    Ok(())
}

// ── AI Basic layer ──────────────────────────────────────────────────────

fn process_ai_basic(db: &Db, file_id: i64, path: &str) -> Result<(), String> {
    let ai = crate::aimeta::extract_png(path)?;
    let info = if ai.model.is_empty() {
        ai.prompt.clone()
    } else if ai.prompt.is_empty() {
        format!("model: {}", ai.model)
    } else {
        format!("{}\n\nmodel: {}", ai.prompt, ai.model)
    };
    if info.is_empty() {
        return Err("no AI metadata".into());
    }
    db.meta_set_pnginfo(file_id, &info);
    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_names() {
        assert_eq!(Layer::Hash.name(), "hash");
        assert_eq!(Layer::Exif.name(), "exif");
    }

    #[test]
    fn stats_rate_calculation() {
        let stats = JobStats::new();
        stats.done.store(100, Ordering::Relaxed);
        // Force rate snapshot to 0 so delta = 100
        stats.rate_snapshot.store(0, Ordering::Relaxed);
        // Set rate_time to 1 second ago
        *stats.rate_time.lock().unwrap() = Instant::now() - Duration::from_secs(1);
        stats.update_rate();
        // 100 jobs in 1 second = 6000/min, x10 = 60000
        let rpm = stats.jobs_per_min.load(Ordering::Relaxed);
        assert!(rpm > 0, "should compute positive rate");
    }

    #[test]
    fn stats_last_error() {
        let stats = JobStats::new();
        assert!(stats.last_error().is_empty());
        stats.record_fail("test error");
        assert_eq!(stats.last_error(), "test error");
        stats.record_fail("newer error");
        assert_eq!(stats.last_error(), "newer error");
    }
}

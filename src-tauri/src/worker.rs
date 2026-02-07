use std::thread;
use std::time::Duration;

use crate::data::Db;
use crate::thumbs;

/// Run headless worker.
/// `once` = true: drain all pending jobs then return.
/// `once` = false: loop forever, polling every 2s.
pub fn run_headless(db: &Db, once: bool) {
    use crate::debug::dbg_log;
    let num_cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let worker_threads = (num_cpus / 4).max(1);

    if once {
        println!("lv worker: draining jobs ({} CPUs available)", num_cpus);
    } else {
        println!(
            "lv worker: {} threads (of {} CPUs), looping",
            worker_threads, num_cpus
        );
    }

    let mut total_done = 0usize;
    let mut total_failed = 0usize;

    loop {
        let mut did_work = false;

        // Hash jobs
        while let Some(job) = db.jobs_claim_next("hash") {
            did_work = true;
            if let Some(file_id) = job.file_id {
                let path = db.file_path(file_id).unwrap_or_else(|| "?".into());
                dbg_log!("hash job #{} {} file_id={}", job.id, path, file_id);
                match process_hash_job(db, file_id) {
                    Ok(_) => {
                        dbg_log!("hash job #{} done", job.id);
                        db.jobs_mark_done(job.id);
                        total_done += 1;
                    }
                    Err(e) => {
                        dbg_log!("hash job #{} failed: {}", job.id, e);
                        db.jobs_mark_failed(job.id, &e.to_string());
                        total_failed += 1;
                    }
                }
            } else {
                db.jobs_mark_failed(job.id, "missing file_id");
                total_failed += 1;
            }
        }

        // Thumbnail jobs
        while let Some(job) = db.jobs_claim_next("thumbnail") {
            did_work = true;
            if let Some(meta_id) = job.meta_id {
                let path = db.file_path_for_meta(meta_id).unwrap_or_else(|| "?".into());
                dbg_log!("thumb job #{} {} meta_id={}", job.id, path, meta_id);
                match thumbs::generate_for_meta(db, meta_id) {
                    Ok(_) => {
                        dbg_log!("thumb job #{} done", job.id);
                        db.jobs_mark_done(job.id);
                        total_done += 1;
                    }
                    Err(e) => {
                        dbg_log!("thumb job #{} failed: {}", job.id, e);
                        db.jobs_mark_failed(job.id, &e.to_string());
                        total_failed += 1;
                    }
                }
            } else {
                db.jobs_mark_failed(job.id, "missing meta_id");
                total_failed += 1;
            }
        }

        if once && !did_work {
            println!("done: {} ok, {} failed", total_done, total_failed);
            return;
        }

        if !did_work {
            dbg_log!("idle, sleeping 2s");
            thread::sleep(Duration::from_secs(2));
        }
    }
}

/// Threshold above which we use fast fingerprint hash instead of full SHA-512.
/// 2 MB — most images are below this; videos are above.
const FAST_HASH_THRESHOLD: u64 = 2 * 1024 * 1024;
/// How many bytes to read from head and tail for fingerprint hash.
const FINGERPRINT_CHUNK: usize = 64 * 1024;

fn process_hash_job(db: &Db, file_id: i64) -> anyhow::Result<()> {
    use crate::debug::dbg_log;
    use sha2::{Digest, Sha512};
    use std::io::{Read, Seek, SeekFrom};

    let path = db
        .file_path(file_id)
        .ok_or_else(|| anyhow::anyhow!("file not found"))?;

    // 1. Try xattr cache first — instant (unix only)
    #[cfg(unix)]
    let cached = xattr::get(&path, "user.lv.sha512")
        .ok()
        .flatten()
        .and_then(|v| String::from_utf8(v).ok());
    #[cfg(not(unix))]
    let cached: Option<String> = None;

    let hash = if let Some(h) = cached {
        dbg_log!("hash from xattr: {}", &path);
        h
    } else {
        let mut file = std::fs::File::open(&path)?;
        let file_size = file.metadata()?.len();

        let hash = if file_size > FAST_HASH_THRESHOLD {
            // 2. Large file → fingerprint: first 64KB + last 64KB + size
            dbg_log!(
                "fingerprint hash ({}MB): {}",
                file_size / (1024 * 1024),
                &path
            );
            let mut hasher = Sha512::new();

            // Read first chunk
            let mut head = vec![0u8; FINGERPRINT_CHUNK.min(file_size as usize)];
            file.read_exact(&mut head)?;
            hasher.update(&head);

            // Read last chunk (if file is big enough for a distinct tail)
            if file_size > FINGERPRINT_CHUNK as u64 * 2 {
                file.seek(SeekFrom::End(-(FINGERPRINT_CHUNK as i64)))?;
                let mut tail = vec![0u8; FINGERPRINT_CHUNK];
                file.read_exact(&mut tail)?;
                hasher.update(&tail);
            }

            // Mix in file size for uniqueness
            hasher.update(file_size.to_le_bytes());

            format!("fp:{:x}", hasher.finalize())
        } else {
            // 3. Small file → full SHA-512
            dbg_log!("full hash ({}KB): {}", file_size / 1024, &path);
            let mut hasher = Sha512::new();
            let mut buf = [0u8; 65536];
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            format!("{:x}", hasher.finalize())
        };

        // Cache in xattr (ignore errors on network/WSL FS)
        #[cfg(unix)]
        let _ = xattr::set(&path, "user.lv.sha512", hash.as_bytes());
        hash
    };

    // Upsert meta row and link file
    let meta_id = db
        .meta_upsert(&hash)
        .ok_or_else(|| anyhow::anyhow!("meta upsert failed"))?;
    db.file_set_hash(file_id, &hash, meta_id);

    // Enqueue thumbnail job if not ready
    if !db.meta_thumb_ready(meta_id) {
        db.jobs_enqueue_thumb(meta_id, 0);
    }

    Ok(())
}

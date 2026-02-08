//! Texture cache (LRU, GPU-resident) + background image preloader.
//!
//! Flow:
//!   1. Preloader::schedule(path) → spawns thread → decodes to RGBA → stores in ready map
//!   2. TextureCache::pump_uploads() → takes ready decoded images → uploads to GL textures
//!   3. TextureCache::get(path) → returns GL texture id if cached
//!
//! Background threads only do CPU work (image decode). GL uploads happen on the main thread.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::thread;

use image::GenericImageView;

/// Decoded image: raw RGBA pixels ready for GL upload.
pub struct DecodedImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl DecodedImage {
    /// Decode an image file to RGBA. Returns None on failure.
    pub fn from_file(path: &str) -> Option<Self> {
        let img = image::open(path).ok()?;
        let (w, h) = img.dimensions();
        let rgba = img.into_rgba8().into_raw();
        Some(DecodedImage {
            rgba,
            width: w,
            height: h,
        })
    }
}

/// Info about a cached GL texture.
#[derive(Clone, Copy)]
pub struct TexInfo {
    pub gl_id: u32,
    pub width: u32,
    pub height: u32,
}

/// LRU texture cache — keeps up to `capacity` GL textures on the GPU.
pub struct TextureCache {
    capacity: usize,
    /// path → TexInfo
    map: HashMap<String, TexInfo>,
    /// LRU order: front = oldest, back = newest
    order: VecDeque<String>,
}

impl TextureCache {
    pub fn new(capacity: usize) -> Self {
        TextureCache {
            capacity,
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    /// Check if a path is already cached as a GL texture.
    pub fn has(&self, path: &str) -> bool {
        self.map.contains_key(path)
    }

    /// Get texture info for a cached path.
    pub fn get(&self, path: &str) -> Option<TexInfo> {
        self.map.get(path).copied()
    }

    /// Upload a decoded image to a GL texture and cache it.
    pub fn upload(&mut self, path: &str, img: DecodedImage) {
        if self.map.contains_key(path) {
            // Already cached — just touch LRU
            self.touch(path);
            return;
        }

        // Evict if at capacity
        while self.map.len() >= self.capacity {
            if let Some(old_path) = self.order.pop_front() {
                if let Some(info) = self.map.remove(&old_path) {
                    unsafe {
                        gl::DeleteTextures(1, &info.gl_id);
                    }
                }
            }
        }

        // Create GL texture
        let gl_id = unsafe {
            let mut tex = 0u32;
            gl::GenTextures(1, &mut tex);
            gl::BindTexture(gl::TEXTURE_2D, tex);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA8 as i32,
                img.width as i32,
                img.height as i32,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                img.rgba.as_ptr() as *const _,
            );
            gl::BindTexture(gl::TEXTURE_2D, 0);
            tex
        };

        self.map.insert(
            path.to_string(),
            TexInfo {
                gl_id,
                width: img.width,
                height: img.height,
            },
        );
        self.order.push_back(path.to_string());
    }

    /// Move a path to the back of the LRU (most recently used).
    fn touch(&mut self, path: &str) {
        if let Some(pos) = self.order.iter().position(|p| p == path) {
            self.order.remove(pos);
        }
        self.order.push_back(path.to_string());
    }

    /// Upload any images that background threads have finished decoding.
    pub fn pump_uploads(&mut self) {
        // Currently a no-op — preloaded images are taken via Preloader::try_take.
        // This hook exists for future use (e.g. auto-uploading from a channel).
    }
}

impl Drop for TextureCache {
    fn drop(&mut self) {
        for info in self.map.values() {
            unsafe {
                gl::DeleteTextures(1, &info.gl_id);
            }
        }
    }
}

/// Background preloader — decodes images on worker threads.
pub struct Preloader {
    /// Paths currently being decoded or already decoded (not yet taken).
    pending: Arc<Mutex<HashSet<String>>>,
    /// Decoded images waiting to be taken or uploaded.
    ready: Arc<Mutex<HashMap<String, DecodedImage>>>,
}

impl Preloader {
    pub fn new() -> Self {
        Preloader {
            pending: Arc::new(Mutex::new(HashSet::new())),
            ready: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Check if a path is being decoded or is ready.
    pub fn is_pending(&self, path: &str) -> bool {
        self.pending.lock().unwrap().contains(path)
    }

    /// Try to take a decoded image (removes it from ready map).
    pub fn try_take(&self, path: &str) -> Option<DecodedImage> {
        let mut ready = self.ready.lock().unwrap();
        let img = ready.remove(path);
        if img.is_some() {
            self.pending.lock().unwrap().remove(path);
        }
        img
    }

    /// Schedule background decode of an image file.
    pub fn schedule(&self, path: String) {
        {
            let mut pending = self.pending.lock().unwrap();
            if pending.contains(&path) {
                return;
            }
            pending.insert(path.clone());
        }

        let pending = self.pending.clone();
        let ready = self.ready.clone();

        thread::spawn(move || {
            if let Some(img) = DecodedImage::from_file(&path) {
                // Store in ready map
                ready.lock().unwrap().insert(
                    path.clone(),
                    DecodedImage {
                        rgba: img.rgba,
                        width: img.width,
                        height: img.height,
                    },
                );
            } else {
                // Failed — remove from pending
                pending.lock().unwrap().remove(&path);
            }
        });
    }
}

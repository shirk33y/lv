use crate::data::Db;
use tauri::http::{Request, Response};

/// Max file size we'll serve through this protocol (100 MB).
/// Videos should use the Viewer's <video> tag with convertFileSrc instead.
const MAX_SERVE_SIZE: u64 = 100 * 1024 * 1024;

/// Handle `lv-file://localhost/<encoded_path>` — serve original files from disk.
pub fn handle_file_request(request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let uri = request.uri();
    let raw_path = uri.path().trim_start_matches('/');
    let path = urlencoding::decode(raw_path).unwrap_or_default();

    let meta = match std::fs::metadata(path.as_ref()) {
        Ok(m) => m,
        Err(_) => {
            return Response::builder()
                .status(404)
                .body(b"file not found".to_vec())
                .unwrap();
        }
    };

    if meta.len() > MAX_SERVE_SIZE {
        return Response::builder()
            .status(413)
            .header("Content-Type", "text/plain")
            .body(b"file too large for this protocol".to_vec())
            .unwrap();
    }

    match std::fs::read(path.as_ref()) {
        Ok(data) => {
            let mime = guess_mime(&path);
            Response::builder()
                .status(200)
                .header("Content-Type", mime)
                .header("Cache-Control", "public, max-age=3600")
                .body(data)
                .unwrap()
        }
        Err(_) => Response::builder()
            .status(404)
            .body(b"file not found".to_vec())
            .unwrap(),
    }
}

fn guess_mime(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        "tiff" | "tif" => "image/tiff",
        "ico" => "image/x-icon",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        _ => "application/octet-stream",
    }
}

/// Handle `thumb://localhost/<meta_id>` or `thumb://localhost/<meta_id>/<size_tag>`
/// Serve WebP thumbnails from SQLite. Default size_tag is "default".
pub fn handle_thumb_request(db: &Db, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let uri = request.uri();
    let path = uri.path().trim_start_matches('/');
    let parts: Vec<&str> = path.splitn(2, '/').collect();
    let meta_id: i64 = match parts.first().and_then(|s| s.parse().ok()) {
        Some(id) => id,
        None => {
            return Response::builder()
                .status(400)
                .header("Content-Type", "text/plain")
                .body(b"invalid meta_id".to_vec())
                .unwrap();
        }
    };
    let size_tag = parts.get(1).unwrap_or(&"default");

    match db.thumb_get(meta_id, size_tag) {
        Some(data) => Response::builder()
            .status(200)
            .header("Content-Type", "image/webp")
            .header("Cache-Control", "public, max-age=31536000, immutable")
            .body(data)
            .unwrap(),
        None => Response::builder()
            .status(404)
            .header("Content-Type", "text/plain")
            .body(b"not found".to_vec())
            .unwrap(),
    }
}

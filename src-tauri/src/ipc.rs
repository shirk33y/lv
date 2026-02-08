use std::path::Path;

use crate::data::{Db, FileDto, FileMetaDto, StatusInfo};
use crate::scanner;

pub struct AppState {
    pub db: Db,
}

type CmdResult<T = ()> = Result<T, String>;

#[tauri::command]
pub fn get_files(
    state: tauri::State<'_, AppState>,
    dir: Option<String>,
) -> CmdResult<Vec<FileDto>> {
    Ok(if let Some(d) = dir {
        if d == "♥" {
            state.db.files_all_fav()
        } else {
            state.db.files_by_dir(&d)
        }
    } else {
        state.db.files_all()
    })
}

#[tauri::command]
pub fn navigate_dir(
    state: tauri::State<'_, AppState>,
    current_dir: String,
    delta: i32,
) -> CmdResult<Vec<FileDto>> {
    let dirs = state.db.files_dirs();
    if dirs.is_empty() {
        return Ok(vec![]);
    }
    let cur_idx = dirs.iter().position(|d| d == &current_dir).unwrap_or(0);
    let new_idx = (cur_idx as i64 + delta as i64).clamp(0, dirs.len() as i64 - 1) as usize;
    Ok(state.db.files_by_dir(&dirs[new_idx]))
}

#[tauri::command]
pub fn toggle_like(state: tauri::State<'_, AppState>, file_id: i64) -> CmdResult<bool> {
    let meta_id = state
        .db
        .meta_id_for_file(file_id)
        .ok_or("file has no metadata yet")?;
    let mut tags = state.db.meta_get_tags(meta_id);

    let liked = if tags.contains(&"like".to_string()) {
        tags.retain(|t| t != "like");
        state.db.history_record(file_id, "unlike");
        false
    } else {
        tags.push("like".to_string());
        state.db.history_record(file_id, "like");
        true
    };

    state.db.meta_set_tags(meta_id, &tags);
    Ok(liked)
}

#[tauri::command]
pub fn record_view(state: tauri::State<'_, AppState>, file_id: i64) -> CmdResult {
    state.db.history_record(file_id, "view");
    Ok(())
}

#[tauri::command]
pub fn random_file(state: tauri::State<'_, AppState>) -> CmdResult<Option<FileDto>> {
    Ok(state.db.file_random())
}

#[tauri::command]
pub fn newest_file(state: tauri::State<'_, AppState>) -> CmdResult<Option<FileDto>> {
    Ok(state.db.file_newest())
}

#[tauri::command]
pub fn random_fav(state: tauri::State<'_, AppState>) -> CmdResult<Option<FileDto>> {
    Ok(state.db.file_random_fav())
}

#[tauri::command]
pub fn latest_fav(state: tauri::State<'_, AppState>) -> CmdResult<Option<FileDto>> {
    Ok(state.db.file_latest_fav())
}

#[tauri::command]
pub fn get_file_metadata(
    state: tauri::State<'_, AppState>,
    file_id: i64,
) -> CmdResult<Option<FileMetaDto>> {
    Ok(state.db.file_metadata(file_id))
}

#[tauri::command]
pub fn get_status(state: tauri::State<'_, AppState>) -> CmdResult<StatusInfo> {
    Ok(state.db.status())
}

#[tauri::command]
pub fn rescan(state: tauri::State<'_, AppState>) -> CmdResult<usize> {
    let dirs = state.db.watched_list_active();
    let mut total = 0;
    for dir in &dirs {
        total += scanner::discover(&state.db, Path::new(dir));
    }
    Ok(total)
}

#[tauri::command]
pub fn boost_jobs(
    state: tauri::State<'_, AppState>,
    file_ids: Vec<i64>,
    meta_ids: Vec<i64>,
) -> CmdResult {
    state.db.jobs_boost(&file_ids, &meta_ids);
    Ok(())
}

#[tauri::command]
pub fn get_first_dir(state: tauri::State<'_, AppState>) -> CmdResult<Option<String>> {
    Ok(state.db.files_first_dir())
}

#[tauri::command]
pub fn get_cwd() -> CmdResult<String> {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn report_broken_thumb(state: tauri::State<'_, AppState>, meta_id: i64) -> CmdResult {
    state.db.report_broken_thumb(meta_id);
    Ok(())
}

#[tauri::command]
pub fn toggle_fullscreen(window: tauri::WebviewWindow) -> CmdResult {
    let cur = window.is_fullscreen().unwrap_or(false);
    window.set_fullscreen(!cur).map_err(|e| e.to_string())?;
    Ok(())
}

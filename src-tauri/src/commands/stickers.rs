//! Thin glue between the Decorations → Browse page and
//! `services::sticker_catalog` / `services::sticker_library`.

use crate::services::sticker_catalog::{self, Catalog};
use crate::services::sticker_library::{self, SavedStickerView};
use crate::utils::error::AppError;

/// The catalog Browse shows. Never an error: an unreachable server comes back
/// as the cached list, flagged `offline`.
#[tauri::command]
pub async fn fetch_sticker_catalog() -> Catalog {
    sticker_catalog::fetch().await
}

/// Every downloaded sticker, image included — what fills the palette and what
/// the overlay draws. Reads each image from disk, so it runs off the async
/// runtime.
#[tauri::command]
pub async fn list_saved_stickers() -> Result<Vec<SavedStickerView>, AppError> {
    tokio::task::spawn_blocking(sticker_library::list)
        .await
        .map_err(|e| AppError::StickerFailed(format!("background task panicked: {e}")))?
}

/// Downloads a sticker from the catalog by id, verifies it and saves it.
/// Saving over one already there is how an updated image replaces the old.
#[tauri::command]
pub async fn download_sticker(id: String) -> Result<SavedStickerView, AppError> {
    sticker_library::download(&id).await
}

/// Deletes a downloaded sticker. The frontend removes any copies placed on
/// the window, which are placement and not this module's to know about.
#[tauri::command]
pub fn remove_saved_sticker(id: String) -> Result<(), AppError> {
    sticker_library::remove(&id)
}

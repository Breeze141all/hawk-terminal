use std::path::{Path, PathBuf};

/// Ensures the journal images storage directory exists.
pub fn ensure_journal_images_dir() -> Result<PathBuf, String> {
    let dir = data::journal_images_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create journal images dir: {e}"))?;
    }
    Ok(dir)
}

/// Attempts to read an image from the system clipboard (either raw image bytes or file path text)
/// and saves both the full-size PNG and a downscaled thumbnail (max 320x180).
/// Returns the saved relative file name (e.g. `<uuid>.png`).
pub fn save_clipboard_image() -> Result<String, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("Clipboard error: {e}"))?;

    // 1. Try reading raw image data from clipboard (e.g. Snipping Tool, PrtScn, browser copy)
    if let Ok(img_data) = clipboard.get_image() {
        let width = img_data.width as u32;
        let height = img_data.height as u32;
        let bytes = img_data.bytes.into_owned();

        let rgba_img = image::RgbaImage::from_raw(width, height, bytes)
            .ok_or_else(|| "Failed to construct RGBA image from clipboard data".to_string())?;

        return save_rgba_image_and_thumb(&rgba_img);
    }

    // 2. Fallback: check if clipboard text is a valid path to an image file
    if let Ok(text) = clipboard.get_text() {
        let trimmed = text.trim().trim_matches('"');
        let path = Path::new(trimmed);
        if path.exists()
            && path.is_file()
            && let Some(ext) = path.extension().and_then(|s| s.to_str())
            && matches!(
                ext.to_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "bmp"
            )
        {
            return import_local_image_file(path);
        }
    }

    Err(
        "No image found in clipboard. Take a screenshot (Win+Shift+S or PrtScn) and try again."
            .to_string(),
    )
}

/// Imports an image from an external local file path, converts/saves it as a PNG in
/// the journal images directory, and generates a corresponding thumbnail.
pub fn import_local_image_file(source: &Path) -> Result<String, String> {
    let img = image::open(source).map_err(|e| format!("Failed to open image file: {e}"))?;
    let rgba = img.to_rgba8();
    save_rgba_image_and_thumb(&rgba)
}

/// Helper to save full-size PNG and thumbnail given an RgbaImage.
fn save_rgba_image_and_thumb(rgba_img: &image::RgbaImage) -> Result<String, String> {
    ensure_journal_images_dir()?;

    let file_id = uuid::Uuid::new_v4();
    let file_name = format!("{file_id}.png");
    let full_path = data::journal_image_path(&file_name);
    let thumb_path = data::journal_thumb_path(&file_name);

    // Save full-size image
    rgba_img
        .save(&full_path)
        .map_err(|e| format!("Failed to save full image: {e}"))?;

    // Generate and save downscaled thumbnail (max 320x180 preserving aspect ratio)
    let thumb = image::imageops::thumbnail(rgba_img, 320, 180);
    let _ = thumb.save(&thumb_path);

    Ok(file_name)
}

/// Opens native OS file picker to select an image file.
pub fn pick_image_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Select Chart Image or Screenshot")
        .add_filter(
            "Images (*.png, *.jpg, *.jpeg, *.webp, *.bmp)",
            &["png", "jpg", "jpeg", "webp", "bmp"],
        )
        .pick_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_save_rgba_image_and_thumb() {
        // Create a 10x10 dummy image
        let img = image::RgbaImage::new(10, 10);
        let result = save_rgba_image_and_thumb(&img);
        assert!(result.is_ok());

        let file_name = result.unwrap();
        let full_path = data::journal_image_path(&file_name);
        let thumb_path = data::journal_thumb_path(&file_name);

        assert!(full_path.exists());
        assert!(thumb_path.exists());

        // Cleanup
        data::delete_journal_image_files(&file_name);
        assert!(!full_path.exists());
        assert!(!thumb_path.exists());
    }
}

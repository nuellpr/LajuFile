//! Embedded file-type and category icons rendered as egui textures.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use egui::{
    Color32, ColorImage, Context, Id, Pos2, Rect, Sense, TextureHandle, TextureOptions, Ui, Vec2,
};
use lajufile_core::store::Category;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconKey {
    CatCompressed,
    CatDocuments,
    CatMusic,
    CatOther,
    CatPrograms,
    CatVideo,
    CatWebsite,
    Ext7z,
    ExtApk,
    ExtAvi,
    ExtDoc,
    ExtExe,
    ExtGif,
    ExtGz,
    ExtIso,
    ExtJpg,
    ExtMkv,
    ExtMp3,
    ExtMp4,
    ExtMsi,
    ExtPdf,
    ExtPng,
    ExtPpt,
    ExtRar,
    ExtWav,
    ExtXls,
    ExtZip,
}

fn png_bytes(key: IconKey) -> &'static [u8] {
    match key {
        IconKey::CatCompressed => {
            include_bytes!("../../../../assets/file-icons/kategori/compressed.png")
        }
        IconKey::CatDocuments => {
            include_bytes!("../../../../assets/file-icons/kategori/documents.png")
        }
        IconKey::CatMusic => include_bytes!("../../../../assets/file-icons/kategori/music.png"),
        IconKey::CatOther => include_bytes!("../../../../assets/file-icons/kategori/other.png"),
        IconKey::CatPrograms => {
            include_bytes!("../../../../assets/file-icons/kategori/programs.png")
        }
        IconKey::CatVideo => include_bytes!("../../../../assets/file-icons/kategori/video.png"),
        IconKey::CatWebsite => include_bytes!("../../../../assets/file-icons/kategori/website.png"),
        IconKey::Ext7z => include_bytes!("../../../../assets/file-icons/jenis-file/7z.png"),
        IconKey::ExtApk => include_bytes!("../../../../assets/file-icons/jenis-file/apk.png"),
        IconKey::ExtAvi => include_bytes!("../../../../assets/file-icons/jenis-file/avi.png"),
        IconKey::ExtDoc => include_bytes!("../../../../assets/file-icons/jenis-file/doc.png"),
        IconKey::ExtExe => include_bytes!("../../../../assets/file-icons/jenis-file/exe.png"),
        IconKey::ExtGif => include_bytes!("../../../../assets/file-icons/jenis-file/gif.png"),
        IconKey::ExtGz => include_bytes!("../../../../assets/file-icons/jenis-file/gz.png"),
        IconKey::ExtIso => include_bytes!("../../../../assets/file-icons/jenis-file/iso.png"),
        IconKey::ExtJpg => include_bytes!("../../../../assets/file-icons/jenis-file/jpg.png"),
        IconKey::ExtMkv => include_bytes!("../../../../assets/file-icons/jenis-file/mkv.png"),
        IconKey::ExtMp3 => include_bytes!("../../../../assets/file-icons/jenis-file/mp3.png"),
        IconKey::ExtMp4 => include_bytes!("../../../../assets/file-icons/jenis-file/mp4.png"),
        IconKey::ExtMsi => include_bytes!("../../../../assets/file-icons/jenis-file/msi.png"),
        IconKey::ExtPdf => include_bytes!("../../../../assets/file-icons/jenis-file/pdf.png"),
        IconKey::ExtPng => include_bytes!("../../../../assets/file-icons/jenis-file/png.png"),
        IconKey::ExtPpt => include_bytes!("../../../../assets/file-icons/jenis-file/ppt.png"),
        IconKey::ExtRar => include_bytes!("../../../../assets/file-icons/jenis-file/rar.png"),
        IconKey::ExtWav => include_bytes!("../../../../assets/file-icons/jenis-file/wav.png"),
        IconKey::ExtXls => include_bytes!("../../../../assets/file-icons/jenis-file/xls.png"),
        IconKey::ExtZip => include_bytes!("../../../../assets/file-icons/jenis-file/zip.png"),
    }
}

pub fn category_key(category: Category) -> IconKey {
    match category {
        Category::Video => IconKey::CatVideo,
        Category::Music => IconKey::CatMusic,
        Category::Images => IconKey::ExtPng,
        Category::Documents => IconKey::CatDocuments,
        Category::Programs => IconKey::CatPrograms,
        Category::Compressed => IconKey::CatCompressed,
        Category::Other => IconKey::CatOther,
    }
}

pub fn file_key(file_name: &str, category: Category, is_video_kind: bool) -> IconKey {
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "7z" => IconKey::Ext7z,
        "apk" => IconKey::ExtApk,
        "avi" => IconKey::ExtAvi,
        "doc" | "docx" | "txt" | "rtf" | "odt" | "md" => IconKey::ExtDoc,
        "exe" => IconKey::ExtExe,
        "gif" => IconKey::ExtGif,
        "gz" | "tgz" | "xz" | "bz2" | "zst" | "tar" => IconKey::ExtGz,
        "iso" | "img" => IconKey::ExtIso,
        "jpg" | "jpeg" | "webp" | "avif" | "heic" | "heif" | "tif" | "tiff" => IconKey::ExtJpg,
        "mkv" | "webm" | "flv" => IconKey::ExtMkv,
        "mp3" | "m4a" | "aac" | "flac" | "ogg" | "opus" => IconKey::ExtMp3,
        "mp4" | "mov" | "m4v" | "wmv" => IconKey::ExtMp4,
        "msi" | "msix" => IconKey::ExtMsi,
        "pdf" | "epub" => IconKey::ExtPdf,
        "png" | "bmp" | "ico" | "svg" | "psd" => IconKey::ExtPng,
        "ppt" | "pptx" => IconKey::ExtPpt,
        "rar" => IconKey::ExtRar,
        "wav" => IconKey::ExtWav,
        "xls" | "xlsx" | "csv" => IconKey::ExtXls,
        "zip" => IconKey::ExtZip,
        "html" | "htm" => IconKey::CatWebsite,
        _ if is_video_kind => IconKey::ExtMp4,
        _ => category_key(category),
    }
}

fn texture_for(ctx: &Context, key: IconKey) -> Option<TextureHandle> {
    let cache_id = Id::unique("lajufile_icon_textures");
    if let Some(handle) = ctx.data_mut(|data| {
        data.get_temp::<Arc<HashMap<IconKey, TextureHandle>>>(cache_id)
            .and_then(|map| map.get(&key).cloned())
    }) {
        return Some(handle);
    }

    let icon = eframe::icon_data::from_png_bytes(png_bytes(key)).ok()?;
    let image =
        ColorImage::from_rgba_unmultiplied([icon.width as usize, icon.height as usize], &icon.rgba);
    let handle = ctx.load_texture(
        format!("lajufile_icon_{key:?}"),
        image,
        TextureOptions::LINEAR,
    );
    ctx.data_mut(|data| {
        let mut map = data
            .get_temp::<Arc<HashMap<IconKey, TextureHandle>>>(cache_id)
            .map(|arc| (*arc).clone())
            .unwrap_or_default();
        map.insert(key, handle.clone());
        data.insert_temp(cache_id, Arc::new(map));
    });
    Some(handle)
}

pub fn paint_icon_at(ui: &Ui, rect: Rect, key: IconKey) {
    if let Some(tex) = texture_for(ui.ctx(), key) {
        let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
        ui.painter().image(tex.id(), rect, uv, Color32::WHITE);
    }
}

pub fn draw_file_icon(
    ui: &mut Ui,
    file_name: &str,
    category: Category,
    is_video_kind: bool,
    size: f32,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let key = file_key(file_name, category, is_video_kind);
    paint_icon_at(ui, rect, key);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_extensions_and_categories_to_icons() {
        assert_eq!(
            file_key("clip.mp4", Category::Video, false),
            IconKey::ExtMp4
        );
        assert_eq!(
            file_key("setup.exe", Category::Programs, false),
            IconKey::ExtExe
        );
        assert_eq!(
            file_key("book.pdf", Category::Documents, false),
            IconKey::ExtPdf
        );
        assert_eq!(
            file_key("watch (1)", Category::Video, true),
            IconKey::ExtMp4
        );
        assert_eq!(
            file_key("test.bin", Category::Other, false),
            IconKey::CatOther
        );
    }
}

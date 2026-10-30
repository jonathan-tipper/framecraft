//! One pixel snapshot feeds clipboard, file drag and portable evidence packs.
//! Existing flattened files are decoded unchanged; only live documents render.
use crate::editor::{self, Document};
use image::RgbaImage;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub const MAX_PACK_FRAMES: usize = 100;
pub const MAX_PACK_PIXELS: u64 = 64_000_000;

#[derive(Clone)]
pub struct Snapshot {
    pub pixels: RgbaImage,
}
impl Snapshot {
    pub fn current(doc: &Document) -> Result<Self, String> {
        Self::pixels(editor::render(doc)?)
    }
    pub fn pixels(pixels: RgbaImage) -> Result<Self, String> {
        let (w, h) = pixels.dimensions();
        if w == 0 || h == 0 || w > 16384 || h > 16384 || u64::from(w) * u64::from(h) > 64_000_000 {
            return Err("Image exceeds the supported pixel bounds.".into());
        }
        Ok(Self { pixels })
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let parent = path.parent().ok_or("Choose a destination folder.")?;
        let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        self.pixels
            .write_to(temp.as_file_mut(), image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        temp.as_file().sync_all().map_err(|e| e.to_string())?;
        temp.persist(path).map_err(|e| e.to_string())?;
        fs::File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| {
                format!(
                    "PNG written to {} but directory durability is not confirmed: {e}",
                    path.display()
                )
            })?;
        Ok(())
    }
    pub fn drag_file(&self) -> Result<DragFile, String> {
        let directory = tempfile::Builder::new()
            .prefix("framecraft-drag-")
            .tempdir()
            .map_err(|e| e.to_string())?;
        let path = directory.path().join("frame-0001.png");
        self.save(&path)?;
        Ok(DragFile { directory, path })
    }
}
pub struct DragFile {
    directory: tempfile::TempDir,
    pub path: PathBuf,
}
impl DragFile {
    pub fn directory(&self) -> &Path {
        self.directory.path()
    }
}

pub struct PackFrame {
    pub snapshot: Snapshot,
    pub caption: String,
    pub variant_label: String,
}
pub fn validate_pack(frames: &[PackFrame]) -> Result<(), String> {
    if frames.is_empty() || frames.len() > MAX_PACK_FRAMES {
        return Err("Select between 1 and 100 frames.".into());
    }
    let mut pixels = 0u64;
    for (i, frame) in frames.iter().enumerate() {
        pixels +=
            u64::from(frame.snapshot.pixels.width()) * u64::from(frame.snapshot.pixels.height());
        if frame.caption.chars().count() > 2000 {
            return Err(format!("Frame {} caption exceeds 2,000 characters.", i + 1));
        }
    }
    if pixels > MAX_PACK_PIXELS {
        return Err("This pack exceeds 64 million pixels. Export a smaller selection.".into());
    }
    Ok(())
}

/// Returns the final new folder only after every file has been staged and synced.
pub fn export_pack(
    parent: &Path,
    name: &str,
    title: &str,
    frames: &[PackFrame],
    source_roots: &[PathBuf],
    cancel: &AtomicBool,
    progress: impl Fn(usize),
) -> Result<PathBuf, String> {
    validate_pack(frames)?;
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err("Use a simple folder name.".into());
    }
    if title.chars().count() > 200 {
        return Err("Pack title exceeds 200 characters.".into());
    }
    let parent = parent
        .canonicalize()
        .map_err(|e| format!("Cannot open destination: {e}"))?;
    for root in source_roots {
        let root = root
            .canonicalize()
            .map_err(|e| format!("Cannot verify source folder: {e}"))?;
        if parent.starts_with(root) {
            return Err("Choose a destination outside the source session folder.".into());
        }
    }
    let destination = parent.join(name);
    if destination.symlink_metadata().is_ok() {
        return Err("That folder already exists. Choose a new name.".into());
    }
    let stage = tempfile::Builder::new()
        .prefix(".framecraft-pack-")
        .tempdir_in(&parent)
        .map_err(|e| e.to_string())?;
    let mut html=format!("<!doctype html>\n<html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src 'self'; style-src 'unsafe-inline'\"><title>{}</title><style>body{{font:17px system-ui,sans-serif;max-width:1100px;margin:3rem auto;padding:0 1.5rem;background:#fafafa;color:#20242a}}figure{{margin:3rem 0}}img{{max-width:100%;height:auto;border:1px solid #ccc}}figcaption{{white-space:pre-wrap;margin:.8rem 0}}h1{{font-size:2rem}}</style><h1>{}</h1>\n", escape(title),escape(title));
    for (i, frame) in frames.iter().enumerate() {
        if cancel.load(Ordering::Acquire) {
            return Err("Pack export cancelled. No completed pack was published.".into());
        }
        let file = format!("frame-{:04}.png", i + 1);
        frame
            .snapshot
            .save(&stage.path().join(&file))
            .map_err(|e| format!("Frame {}: {e}", i + 1))?;
        html.push_str(&format!(
            "<figure><img src=\"{file}\" alt=\"Frame {}\"><figcaption>{}</figcaption></figure>\n",
            i + 1,
            escape(&frame.caption)
        ));
        progress(i + 1);
    }
    html.push_str("</html>\n");
    let mut index = fs::File::create(stage.path().join("index.html")).map_err(|e| e.to_string())?;
    index
        .write_all(html.as_bytes())
        .and_then(|_| index.sync_all())
        .map_err(|e| e.to_string())?;
    fs::File::open(stage.path())
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    if cancel.load(Ordering::Acquire) {
        return Err("Pack export cancelled. No completed pack was published.".into());
    }
    publish_directory(stage.path(), &destination)?;
    // The staging path is now absent; TempDir cleanup cannot touch the published folder.
    fs::File::open(&parent).and_then(|f| f.sync_all()).map_err(|e|format!("Pack written to {} but directory durability is not confirmed: {e}. Keep the existing pack and inspect it before retrying.",destination.display()))?;
    Ok(destination)
}
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
#[cfg(target_os = "macos")]
fn publish_directory(from: &Path, to: &Path) -> Result<(), String> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    unsafe extern "C" {
        fn renamex_np(
            from: *const std::ffi::c_char,
            to: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
    let a = CString::new(from.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let b = CString::new(to.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    // RENAME_EXCL prevents replacing any existing destination, including an empty folder.
    if unsafe { renamex_np(a.as_ptr(), b.as_ptr(), 4) } == 0 {
        Ok(())
    } else {
        Err(format!(
            "Cannot publish pack: {}",
            std::io::Error::last_os_error()
        ))
    }
}
#[cfg(not(target_os = "macos"))]
fn publish_directory(_from: &Path, _to: &Path) -> Result<(), String> {
    Err("Atomic evidence-pack publication currently requires macOS.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{Annotation, Rect, Style, Theme};
    fn frame() -> PackFrame {
        PackFrame {
            snapshot: Snapshot::pixels(RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255])))
                .unwrap(),
            caption: "<script>secret & \"quoted\"</script>".into(),
            variant_label: "Original".into(),
        }
    }
    #[test]
    fn current_redactions_match_drag_and_flattened_variants() {
        let doc = Document {
            source: RgbaImage::from_pixel(16, 16, image::Rgba([255, 0, 0, 255])),
            crop: None,
            annotations: vec![Annotation::Redact {
                rect: Rect {
                    x: 0,
                    y: 0,
                    width: 8,
                    height: 8,
                },
            }],
            style: Style {
                padding: 0,
                radius: 0,
                shadow: false,
                theme: Theme::Paper,
            },
        };
        let snapshot = Snapshot::current(&doc).unwrap();
        let drag = snapshot.drag_file().unwrap();
        let decoded = editor::load_image(&drag.path).unwrap();
        assert_eq!(snapshot.pixels, decoded);
        assert_eq!(drag.path.file_name().unwrap(), "frame-0001.png");
        assert_eq!(Snapshot::pixels(decoded).unwrap().pixels, snapshot.pixels);
        assert_ne!(snapshot.pixels.get_pixel(0, 0).0, [255, 0, 0, 255]);
    }
    #[test]
    fn escape_all_markup() {
        assert_eq!(escape("<&>\"'"), "&lt;&amp;&gt;&quot;&#39;");
    }
    #[test]
    fn cancellation_and_source_alias_publish_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        fs::create_dir(&source).unwrap();
        let frames = vec![frame()];
        let cancelled = AtomicBool::new(true);
        assert!(export_pack(
            dir.path(),
            "pack",
            "Evidence",
            &frames,
            &[],
            &cancelled,
            |_| {}
        )
        .is_err());
        assert!(!dir.path().join("pack").exists());
        assert!(export_pack(
            &source.join("."),
            "pack",
            "Evidence",
            &frames,
            std::slice::from_ref(&source),
            &AtomicBool::new(false),
            |_| {}
        )
        .is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn pack_is_exact_neutral_and_does_not_clobber() {
        let dir = tempfile::tempdir().unwrap();
        let frames = vec![frame(), frame()];
        let output = export_pack(
            dir.path(),
            "pack",
            "<Title>",
            &frames,
            &[],
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap();
        assert_eq!(fs::read_dir(&output).unwrap().count(), 3);
        let html = fs::read_to_string(output.join("index.html")).unwrap();
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<script>"));
        assert!(html.contains("frame-0001.png"));
        assert!(html.contains("frame-0002.png"));
        assert_eq!(
            editor::load_image(&output.join("frame-0001.png")).unwrap(),
            frames[0].snapshot.pixels
        );
        assert!(export_pack(
            dir.path(),
            "pack",
            "Other",
            &frames,
            &[],
            &AtomicBool::new(false),
            |_| {}
        )
        .is_err());
        let stage = dir.path().join("stage");
        fs::create_dir(&stage).unwrap();
        let existing = dir.path().join("empty");
        fs::create_dir(&existing).unwrap();
        assert!(publish_directory(&stage, &existing).is_err());
        assert!(stage.exists());
    }
    #[cfg(unix)]
    #[test]
    fn symbolic_link_into_source_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        fs::create_dir(&source).unwrap();
        let alias = dir.path().join("alias");
        std::os::unix::fs::symlink(&source, &alias).unwrap();
        assert!(export_pack(
            &alias,
            "pack",
            "Evidence",
            &[frame()],
            &[source],
            &AtomicBool::new(false),
            |_| {}
        )
        .is_err());
    }
}

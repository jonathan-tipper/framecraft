//! Versioned local session metadata. PNGs remain the source of image content.
//! Only explicit confirmation imports files; no background folder indexing occurs.
use crate::session::{CaptureHandle, CaptureState, SavedCapture};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, Weak,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tempfile::NamedTempFile;

pub const MANIFEST_NAME: &str = "framecraft-session.json";
const LOCK_NAME: &str = ".framecraft-session.lock";
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;
const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_RECENTS: usize = 12;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageRecord {
    pub file: String,
    pub bytes: u64,
    pub fingerprint: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureRecord {
    pub id: String,
    pub sequence: u64,
    pub captured_at: String,
    pub title: String,
    pub original: ImageRecord,
    pub edited: Vec<ImageRecord>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    pub session_id: String,
    pub title: String,
    pub captures: Vec<CaptureRecord>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageState {
    Available,
    Missing,
    ExternallyRenamed(PathBuf),
    Modified,
    Unreadable(String),
}
impl ImageState {
    pub fn label(&self) -> String {
        match self {
            Self::Available => "Available".into(),
            Self::Missing => "Missing file. Locate and import a replacement.".into(),
            Self::ExternallyRenamed(path) => format!(
                "Externally renamed: {}. Confirm recovery to relink.",
                path.display()
            ),
            Self::Modified => {
                "Externally modified. Review and explicitly import before using it.".into()
            }
            Self::Unreadable(error) => format!("Unreadable: {error}"),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureVariant {
    Original,
    Edited(usize),
}
#[derive(Clone, Debug)]
pub struct VariantInfo {
    pub variant: CaptureVariant,
    pub path: PathBuf,
    pub state: ImageState,
}
#[derive(Debug)]
pub struct OpenPreview {
    pub folder: PathBuf,
    pub manifest: Manifest,
    pub messages: Vec<String>,
    pub import_count: usize,
    pub needs_confirmation: bool,
    expected_manifest: Option<Vec<u8>>,
    replace_invalid: bool,
}
impl OpenPreview {
    pub fn variants(&self, capture: &CaptureRecord) -> Vec<VariantInfo> {
        variant_infos(&self.folder, capture)
    }
}
fn variant_infos(folder: &Path, record: &CaptureRecord) -> Vec<VariantInfo> {
    std::iter::once((CaptureVariant::Original, &record.original))
        .chain(
            record
                .edited
                .iter()
                .enumerate()
                .map(|(i, image)| (CaptureVariant::Edited(i), image)),
        )
        .map(|(variant, image)| VariantInfo {
            variant,
            path: folder.join(&image.file),
            state: inspect_image(folder, image),
        })
        .collect()
}
#[derive(Debug)]
struct LibraryState {
    manifest: Manifest,
    warning: Option<String>,
    expected_manifest: Option<Vec<u8>>,
}
#[derive(Debug)]
pub struct SessionLibrary {
    folder: PathBuf,
    state: Mutex<LibraryState>,
    // The OS releases the advisory lock even after process termination.
    _lock: File,
    pub(crate) handles: Mutex<HashMap<String, Weak<Mutex<CaptureState>>>>,
}

fn cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("Opening cancelled. The previous session is unchanged.".into())
    } else {
        Ok(())
    }
}
fn id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "{now:032x}-{:08x}-{:016x}",
        std::process::id(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    )
}
fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-')
}
fn valid_capture_time(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 15
        || bytes[8] != b'-'
        || !bytes[..8]
            .iter()
            .chain(bytes[9..].iter())
            .all(u8::is_ascii_digit)
    {
        return false;
    }
    let parse = |range: std::ops::Range<usize>| value[range].parse::<u32>().unwrap_or(0);
    let (year, month, day, hour, minute, second) = (
        parse(0..4),
        parse(4..6),
        parse(6..8),
        parse(9..11),
        parse(11..13),
        parse(13..15),
    );
    let max_day = match month {
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                29
            } else {
                28
            }
        }
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => 0,
    };
    year >= 1970 && day > 0 && day <= max_day && hour < 24 && minute < 60 && second < 60
}
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.contains(['/', '\\'])
        && matches!(
            Path::new(name).components().next(),
            Some(Component::Normal(_))
        )
        && Path::new(name).components().count() == 1
        && name.ends_with(".png")
}
fn regular_path(folder: &Path, name: &str) -> Result<PathBuf, String> {
    if !safe_name(name) {
        return Err("Unsafe image path in session metadata.".into());
    }
    let path = folder.join(name);
    let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if !metadata.file_type().is_file() {
        return Err("Only regular image files inside the session are supported; symbolic links are rejected.".into());
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err("Image exceeds the 512 MiB file limit.".into());
    }
    Ok(path)
}
fn fingerprint(path: &Path) -> Result<(u64, String), String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    if length > MAX_FILE_BYTES {
        return Err("Image exceeds the 512 MiB file limit.".into());
    }
    let mut hash = 0xcbf29ce484222325u64;
    let mut buffer = [0u8; 64 * 1024];
    let mut count = 0u64;
    loop {
        let size = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if size == 0 {
            break;
        }
        count += size as u64;
        if count > MAX_FILE_BYTES {
            return Err("Image exceeds the 512 MiB file limit.".into());
        }
        for byte in &buffer[..size] {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
    Ok((count, format!("{hash:016x}")))
}
fn record_image(folder: &Path, path: &Path) -> Result<ImageRecord, String> {
    if path.parent() != Some(folder) {
        return Err("An external image must be explicitly copied into the session first.".into());
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Image filename is not valid UTF-8.")?;
    let path = regular_path(folder, name)?;
    crate::editor::load_image(&path)?;
    let (bytes, fingerprint) = fingerprint(&path)?;
    Ok(ImageRecord {
        file: name.into(),
        bytes,
        fingerprint,
    })
}
fn validate(manifest: &Manifest) -> Result<(), String> {
    if manifest.version != 1 {
        return Err(format!("Session schema {} is not supported. Open it with a compatible Framecraft version; no files were changed.", manifest.version));
    }
    if !valid_id(&manifest.session_id)
        || manifest.title.chars().count() > 120
        || manifest.captures.len() > MAX_ENTRIES
    {
        return Err("Session metadata has invalid identity, title or capture limits.".into());
    }
    let mut ids = HashSet::new();
    let mut files = HashSet::new();
    let mut sequences = HashSet::new();
    for capture in &manifest.captures {
        if !valid_id(&capture.id)
            || !ids.insert(&capture.id)
            || capture.sequence == 0
            || !sequences.insert(capture.sequence)
            || capture.title.chars().count() > 120
            || !valid_capture_time(&capture.captured_at)
            || capture.edited.len() > 100
        {
            return Err("Session metadata has duplicate or invalid capture identities, sequences or text limits.".into());
        }
        for image in std::iter::once(&capture.original).chain(capture.edited.iter()) {
            if !safe_name(&image.file)
                || !files.insert(&image.file)
                || image.bytes > MAX_FILE_BYTES
                || image.fingerprint.len() != 16
                || !image.fingerprint.bytes().all(|c| c.is_ascii_hexdigit())
            {
                return Err("Session metadata contains an unsafe or duplicate image path or invalid fingerprint.".into());
            }
        }
    }
    Ok(())
}
fn read_manifest(folder: &Path) -> Result<Option<Vec<u8>>, String> {
    let path = folder.join(MANIFEST_NAME);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Could not inspect session metadata: {e}")),
        Ok(meta) if !meta.file_type().is_file() || meta.len() > MAX_MANIFEST_BYTES => {
            Err("Session metadata must be a regular file smaller than 4 MiB.".into())
        }
        Ok(_) => fs::read(path)
            .map(Some)
            .map_err(|e| format!("Could not read session metadata: {e}")),
    }
}
fn lock_folder(folder: &Path) -> Result<File, String> {
    let path = folder.join(LOCK_NAME);
    if fs::symlink_metadata(&path).is_ok_and(|m| !m.file_type().is_file()) {
        return Err("The session lock must be a regular file, not a symbolic link.".into());
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| format!("Could not open the session lock: {e}"))?;
    file.try_lock().map_err(|e| format!("This session is open in another process or its filesystem cannot safely lock it. Close the other session or copy the folder to a local Mac disk: {e}"))?;
    Ok(file)
}
fn atomic_json(folder: &Path, name: &str, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("Metadata exceeds the 4 MiB limit.".into());
    }
    let mut file = NamedTempFile::new_in(folder).map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|e| format!("Could not sync session metadata: {e}"))?;
    file.persist(folder.join(name))
        .map_err(|e| format!("Could not replace session metadata: {}", e.error))?;
    sync_dir(folder).map_err(|e| {
        format!("Metadata was written but directory durability could not be confirmed: {e}")
    })
}
fn sync_dir(folder: &Path) -> std::io::Result<()> {
    File::open(folder)?.sync_all()
}
fn edited_original_name(name: &str) -> Option<String> {
    let (stem, number) = name.strip_suffix(".png")?.rsplit_once("-edited-")?;
    if number.is_empty() || !number.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!("{stem}.png"))
}
fn parse_capture(name: &str) -> Option<(u64, String, String)> {
    let stem = name.strip_suffix(".png")?;
    // Edited copies must be explicitly attached to their original, never imported as originals.
    if stem
        .rsplit_once("-edited-")
        .is_some_and(|(_, n)| n.bytes().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    let mut parts = stem.splitn(4, '-');
    let date = parts.next()?;
    let time = parts.next()?;
    let sequence = parts.next()?.parse().ok()?;
    let title = parts.next()?;
    if date.len() != 8
        || time.len() != 6
        || !date.bytes().chain(time.bytes()).all(|c| c.is_ascii_digit())
        || title.is_empty()
        || sequence == 0
    {
        return None;
    }
    let captured_at = format!("{date}-{time}");
    if !valid_capture_time(&captured_at) {
        return None;
    }
    Some((
        sequence,
        captured_at,
        title.replace('-', " ").chars().take(120).collect(),
    ))
}

impl SessionLibrary {
    pub fn create(folder: &Path, title: &str) -> Result<Arc<Self>, String> {
        let lock = lock_folder(folder)?;
        if read_manifest(folder)?.is_some() {
            return Err("A session manifest already exists; open it instead.".into());
        }
        let manifest = Manifest {
            version: 1,
            session_id: id(),
            title: title.chars().take(120).collect(),
            captures: vec![],
        };
        atomic_json(folder, MANIFEST_NAME, &manifest)?;
        Ok(Arc::new(Self {
            folder: folder.to_path_buf(),
            state: Mutex::new(LibraryState {
                manifest,
                warning: None,
                expected_manifest: read_manifest(folder)?,
            }),
            _lock: lock,
            handles: Mutex::new(HashMap::new()),
        }))
    }
    /// Read-only preview. Dropping it or cancelling does not write to the chosen folder.
    pub fn preview(folder: &Path, cancel: &AtomicBool) -> Result<OpenPreview, String> {
        cancelled(cancel)?;
        let folder = folder
            .canonicalize()
            .map_err(|e| format!("Session folder is unavailable: {e}"))?;
        if !folder.is_dir() {
            return Err("Choose a session folder.".into());
        }
        let expected_manifest = read_manifest(&folder)?;
        let mut messages = vec![];
        let mut replace_invalid = false;
        let mut manifest = if let Some(bytes) = &expected_manifest {
            // Version rejection is deliberately separate from malformed-manifest recovery.
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) {
                if let Some(version) = value.get("version").and_then(|v| v.as_u64()) {
                    if version != 1 {
                        return Err(format!("Session schema {version} is unsupported. Use a compatible Framecraft version; the folder is unchanged."));
                    }
                }
            }
            match serde_json::from_slice::<Manifest>(bytes)
                .map_err(|e| e.to_string())
                .and_then(|m| {
                    validate(&m)?;
                    Ok(m)
                }) {
                Ok(manifest) => manifest,
                Err(error) => {
                    messages.push(format!("Invalid metadata: {error}. Explicit recovery will preserve a backup and import only recognised PNGs."));
                    replace_invalid = true;
                    Manifest {
                        version: 1,
                        session_id: id(),
                        title: "Recovered session".into(),
                        captures: vec![],
                    }
                }
            }
        } else {
            messages.push("Legacy folder. Confirm import to create metadata; existing PNGs retain their bytes and names.".into());
            Manifest {
                version: 1,
                session_id: id(),
                title: "Imported session".into(),
                captures: vec![],
            }
        };
        let mut names = vec![];
        for entry in fs::read_dir(&folder).map_err(|e| e.to_string())? {
            cancelled(cancel)?;
            if names.len() >= MAX_ENTRIES * 2 {
                return Err("Folder exceeds the 20,000-entry inspection limit. Split it into smaller sessions.".into());
            }
            let entry = entry.map_err(|e| e.to_string())?;
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
        names.sort();
        let mut import_count = 0;
        let mut changed = false;
        let known: HashSet<String> = manifest
            .captures
            .iter()
            .flat_map(|c| {
                std::iter::once(&c.original)
                    .chain(c.edited.iter())
                    .map(|i| i.file.clone())
            })
            .collect();
        let mut sequences: HashSet<u64> = manifest.captures.iter().map(|c| c.sequence).collect();
        let mut edited_candidates = Vec::new();
        for name in names {
            cancelled(cancel)?;
            if known.contains(&name) || !name.ends_with(".png") {
                continue;
            }
            if let Some(original_name) = edited_original_name(&name) {
                edited_candidates.push((name, original_name));
                continue;
            }
            let Some((sequence, captured_at, title)) = parse_capture(&name) else {
                messages.push(format!("Not imported automatically: {name}"));
                continue;
            };
            let image = match record_image(&folder, &folder.join(&name)) {
                Ok(image) => image,
                Err(e) => {
                    messages.push(format!("Not imported: {name}: {e}"));
                    continue;
                }
            };
            // A byte-identical renamed file can retain the old capture identity.
            if let Some(capture) = manifest.captures.iter_mut().find(|c| {
                !folder.join(&c.original.file).exists()
                    && c.original.bytes == image.bytes
                    && c.original.fingerprint == image.fingerprint
                    && c.sequence == sequence
            }) {
                messages.push(format!(
                    "Relink externally renamed frame {sequence}: {} → {name}",
                    capture.original.file
                ));
                capture.original = image;
                changed = true;
                continue;
            }
            if !sequences.insert(sequence) {
                messages.push(format!(
                    "Ambiguous sequence {sequence}, not imported: {name}"
                ));
                continue;
            }
            if manifest.captures.len() >= MAX_ENTRIES {
                return Err("Session exceeds the 10,000-capture limit.".into());
            }
            manifest.captures.push(CaptureRecord {
                id: id(),
                sequence,
                captured_at,
                title,
                original: image,
                edited: vec![],
            });
            import_count += 1;
        }
        for (name, original_name) in edited_candidates {
            cancelled(cancel)?;
            let Some((sequence, time, _)) = parse_capture(&original_name) else {
                messages.push(format!("Not imported automatically: {name}"));
                continue;
            };
            let Some(capture) = manifest
                .captures
                .iter_mut()
                .find(|c| c.sequence == sequence && c.captured_at == time)
            else {
                messages.push(format!("Edited image has no recognised original: {name}"));
                continue;
            };
            if capture.edited.len() >= 100 {
                messages.push(format!("Edited-copy limit reached; not imported: {name}"));
                continue;
            }
            match record_image(&folder, &folder.join(&name)) {
                Ok(image) => {
                    capture.edited.push(image);
                    messages.push(format!(
                        "Recover saved edited image for frame {sequence}: {name}"
                    ));
                    changed = true;
                }
                Err(error) => messages.push(format!("Not imported: {name}: {error}")),
            }
        }
        manifest.captures.sort_by_key(|c| c.sequence);
        for capture in &manifest.captures {
            cancelled(cancel)?;
            for image in std::iter::once(&capture.original).chain(capture.edited.iter()) {
                let state = inspect_image(&folder, image);
                if state != ImageState::Available {
                    messages.push(format!(
                        "Frame {} ({}): {}",
                        capture.sequence,
                        image.file,
                        state.label()
                    ));
                }
            }
        }
        let needs_confirmation =
            expected_manifest.is_none() || replace_invalid || import_count > 0 || changed;
        Ok(OpenPreview {
            folder,
            manifest,
            messages,
            import_count,
            needs_confirmation,
            expected_manifest,
            replace_invalid,
        })
    }
    pub fn confirm(preview: OpenPreview, cancel: &AtomicBool) -> Result<Arc<Self>, String> {
        cancelled(cancel)?;
        let lock = lock_folder(&preview.folder)?;
        if read_manifest(&preview.folder)? != preview.expected_manifest {
            return Err("Session metadata changed while the preview was open. Open the folder again to review its current state.".into());
        }
        validate(&preview.manifest)?;
        cancelled(cancel)?;
        if preview.needs_confirmation {
            if preview.replace_invalid {
                let backup = preview
                    .folder
                    .join(format!("framecraft-session-recovery-{}.json", id()));
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(backup)
                    .map_err(|e| e.to_string())?;
                file.write_all(preview.expected_manifest.as_deref().unwrap_or_default())
                    .and_then(|_| file.sync_all())
                    .map_err(|e| {
                        format!("Could not preserve the invalid manifest; recovery cancelled: {e}")
                    })?;
                sync_dir(&preview.folder).map_err(|e| e.to_string())?;
            }
            atomic_json(&preview.folder, MANIFEST_NAME, &preview.manifest)?;
        }
        let expected_manifest = read_manifest(&preview.folder)?;
        Ok(Arc::new(Self {
            folder: preview.folder,
            state: Mutex::new(LibraryState {
                manifest: preview.manifest,
                warning: None,
                expected_manifest,
            }),
            _lock: lock,
            handles: Mutex::new(HashMap::new()),
        }))
    }
    pub fn folder(&self) -> &Path {
        &self.folder
    }
    pub fn manifest(&self) -> Result<Manifest, String> {
        Ok(self
            .state
            .lock()
            .map_err(|_| "Session metadata lock was interrupted.")?
            .manifest
            .clone())
    }
    pub fn warning(&self) -> Option<String> {
        self.state.lock().ok().and_then(|s| s.warning.clone())
    }
    pub fn capture_handles(self: &Arc<Self>) -> Result<Vec<CaptureHandle>, String> {
        Ok(self
            .manifest()?
            .captures
            .iter()
            .map(|c| {
                CaptureHandle::tracked(
                    SavedCapture {
                        path: self.folder.join(&c.original.file),
                        title: c.title.clone(),
                        sequence: c.sequence,
                    },
                    self.clone(),
                    c.id.clone(),
                )
            })
            .collect())
    }
    fn update(
        &self,
        change: impl FnOnce(&mut Manifest) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Session metadata lock was interrupted.")?;
        let mut candidate = state.manifest.clone();
        change(&mut candidate)?;
        validate(&candidate)?;
        // Keep the in-memory identity current even when the durable PNG has outlived
        // a metadata failure. Never overwrite an externally changed manifest.
        state.manifest = candidate;
        let result: Result<(), String> = (|| {
            if read_manifest(&self.folder)? != state.expected_manifest {
                return Err("The manifest changed outside this session. Close and reopen the folder to review recovery.".into());
            }
            atomic_json(&self.folder, MANIFEST_NAME, &state.manifest)?;
            state.expected_manifest = read_manifest(&self.folder)?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                state.warning = None;
                Ok(())
            }
            Err(error) => {
                let warning = format!("The PNG is saved, but session metadata is out of date: {error}. Keep this folder and reopen it to recover.");
                state.warning = Some(warning.clone());
                Err(warning)
            }
        }
    }
    pub(crate) fn add_capture(&self, capture: &SavedCapture) -> Result<String, String> {
        let image = record_image(&self.folder, &capture.path)?;
        let capture_id = id();
        let captured_at = capture
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(parse_capture)
            .map(|(_, t, _)| t)
            .unwrap_or_default();
        let record = CaptureRecord {
            id: capture_id.clone(),
            sequence: capture.sequence,
            captured_at,
            title: capture.title.clone(),
            original: image,
            edited: vec![],
        };
        self.update(|manifest| {
            manifest.captures.push(record);
            Ok(())
        })?;
        Ok(capture_id)
    }
    pub(crate) fn identity_for(&self, capture: &SavedCapture) -> Option<String> {
        self.manifest()
            .ok()?
            .captures
            .iter()
            .find(|c| c.sequence == capture.sequence)
            .map(|c| c.id.clone())
    }
    pub(crate) fn renamed(&self, capture_id: &str, capture: &SavedCapture) -> Result<(), String> {
        let file = capture
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("Invalid capture filename.")?
            .to_string();
        self.update(|m| {
            let record = m
                .captures
                .iter_mut()
                .find(|c| c.id == capture_id)
                .ok_or("Capture metadata is unavailable.")?;
            record.original.file = file;
            record.title = capture.title.clone();
            Ok(())
        })
    }
    pub fn register_edit(&self, capture_id: &str, path: &Path) -> Result<(), String> {
        let image = record_image(&self.folder, path)?;
        self.update(|m| {
            let record = m
                .captures
                .iter_mut()
                .find(|c| c.id == capture_id)
                .ok_or("Capture metadata is unavailable.")?;
            if !record.edited.iter().any(|i| i.file == image.file) {
                record.edited.push(image);
            }
            Ok(())
        })
    }
    fn capture_record(&self, capture_id: &str) -> Result<CaptureRecord, String> {
        self.state
            .lock()
            .map_err(|_| "Session metadata lock was interrupted.")?
            .manifest
            .captures
            .iter()
            .find(|c| c.id == capture_id)
            .cloned()
            .ok_or_else(|| "Capture metadata is unavailable.".into())
    }
    pub fn variants(&self, capture_id: &str) -> Result<Vec<VariantInfo>, String> {
        Ok(variant_infos(
            &self.folder,
            &self.capture_record(capture_id)?,
        ))
    }
    pub fn variant(
        &self,
        capture_id: &str,
        variant: &CaptureVariant,
    ) -> Result<VariantInfo, String> {
        let record = self.capture_record(capture_id)?;
        let image = match variant {
            CaptureVariant::Original => Some(&record.original),
            CaptureVariant::Edited(index) => record.edited.get(*index),
        }
        .ok_or("The selected saved edit is unavailable; choose Original explicitly to use it.")?;
        Ok(VariantInfo {
            variant: variant.clone(),
            path: self.folder.join(&image.file),
            state: inspect_image(&self.folder, image),
        })
    }
    /// Fast menu descriptors only. Loading/output always performs full validation.
    pub fn variant_choices(&self, capture_id: &str) -> Result<Vec<VariantInfo>, String> {
        let record = self.capture_record(capture_id)?;
        Ok(
            std::iter::once((CaptureVariant::Original, &record.original))
                .chain(
                    record
                        .edited
                        .iter()
                        .enumerate()
                        .map(|(i, image)| (CaptureVariant::Edited(i), image)),
                )
                .map(|(variant, image)| VariantInfo {
                    variant,
                    path: self.folder.join(&image.file),
                    state: match regular_path(&self.folder, &image.file) {
                        Ok(_) => ImageState::Available,
                        Err(e) => {
                            if self.folder.join(&image.file).exists() {
                                ImageState::Unreadable(e)
                            } else {
                                ImageState::Missing
                            }
                        }
                    },
                })
                .collect(),
        )
    }
    pub(crate) fn note_metadata_failure(&self, error: String) {
        if let Ok(mut state) = self.state.lock() {
            state.warning=Some(format!("The PNG is saved, but session metadata could not be updated: {error}. Keep the folder and reopen it to recover."));
        }
    }
    /// Explicit replacement re-encodes validated pixels inside the session without modifying the external source.
    pub fn import_replacement(&self, capture_id: &str, source: &Path) -> Result<PathBuf, String> {
        if !fs::symlink_metadata(source)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_file()
        {
            return Err("Choose a regular image file, not a symbolic link.".into());
        }
        if fs::metadata(source).map_err(|e| e.to_string())?.len() > MAX_FILE_BYTES {
            return Err("Replacement exceeds the 512 MiB file limit.".into());
        }
        let pixels = crate::editor::load_image(source)?;
        let record = self.capture_record(capture_id)?;
        let mut temp = NamedTempFile::new_in(&self.folder).map_err(|e| e.to_string())?;
        use image::ImageEncoder;
        image::codecs::png::PngEncoder::new(temp.as_file_mut())
            .write_image(
                pixels.as_raw(),
                pixels.width(),
                pixels.height(),
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| e.to_string())?;
        temp.as_file().sync_all().map_err(|e| e.to_string())?;
        let path = self.folder.join(format!(
            "{}-{:04}-relinked-{}.png",
            record.captured_at,
            record.sequence,
            id()
        ));
        temp.persist_noclobber(&path)
            .map_err(|e| e.error.to_string())?;
        sync_dir(&self.folder).map_err(|e| e.to_string())?;
        let image = record_image(&self.folder, &path)?;
        let metadata_result = self.update(|m| {
            m.captures
                .iter_mut()
                .find(|c| c.id == capture_id)
                .ok_or("Unknown capture identity.")?
                .original = image;
            Ok(())
        });
        if let Err(error) = metadata_result {
            self.note_metadata_failure(error);
        }
        Ok(path)
    }
}
fn inspect_image(folder: &Path, image: &ImageRecord) -> ImageState {
    if !folder.join(&image.file).exists() {
        return ImageState::Missing;
    }
    let path = match regular_path(folder, &image.file) {
        Ok(path) => path,
        Err(e) => return ImageState::Unreadable(e),
    };
    if let Err(e) = crate::editor::load_image(&path) {
        return ImageState::Unreadable(e);
    }
    match fingerprint(&path) {
        Ok((bytes, hash)) if bytes == image.bytes && hash == image.fingerprint => {
            ImageState::Available
        }
        Ok(_) => ImageState::Modified,
        Err(e) => ImageState::Unreadable(e),
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecentSessions {
    pub folders: Vec<PathBuf>,
}
impl RecentSessions {
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        if fs::metadata(path).map_err(|e| e.to_string())?.len() > 64 * 1024 {
            return Err("Recent-session index exceeds its limit.".into());
        }
        let mut recent: Self = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("Could not read recent sessions: {e}"))?;
        recent.folders.truncate(MAX_RECENTS);
        Ok(recent)
    }
    pub fn remember(&mut self, folder: PathBuf) {
        self.folders.retain(|p| p != &folder);
        self.folders.insert(0, folder);
        self.folders.truncate(MAX_RECENTS);
    }
    pub fn forget(&mut self, folder: &Path) {
        self.folders.retain(|p| p != folder);
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let parent = path
            .parent()
            .ok_or("Recent-session index has no parent folder.")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        atomic_json(
            parent,
            path.file_name()
                .and_then(|n| n.to_str())
                .ok_or("Invalid recent index path.")?,
            self,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{SessionStore, Trigger};
    fn cancel() -> AtomicBool {
        AtomicBool::new(false)
    }
    fn png(folder: &Path, name: &str, pixel: u8) -> PathBuf {
        let path = folder.join(name);
        image::RgbaImage::from_pixel(4, 4, image::Rgba([pixel, 2, 3, 255]))
            .save(&path)
            .unwrap();
        path
    }
    #[test]
    fn legacy_preview_cancel_writes_nothing_and_confirm_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let capture = png(dir.path(), "20261028-100000-0001-first.png", 1);
        png(dir.path(), "unrelated.png", 2);
        let bytes = fs::read(capture).unwrap();
        let preview = SessionLibrary::preview(dir.path(), &cancel()).unwrap();
        assert_eq!(preview.import_count, 1);
        assert!(preview.needs_confirmation);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        drop(preview);
        let library = SessionLibrary::confirm(
            SessionLibrary::preview(dir.path(), &cancel()).unwrap(),
            &cancel(),
        )
        .unwrap();
        let first = library.manifest().unwrap();
        drop(library);
        let reopened = SessionLibrary::confirm(
            SessionLibrary::preview(dir.path(), &cancel()).unwrap(),
            &cancel(),
        )
        .unwrap();
        assert_eq!(reopened.manifest().unwrap().session_id, first.session_id);
        assert_eq!(
            reopened.manifest().unwrap().captures[0].id,
            first.captures[0].id
        );
        assert_eq!(
            fs::read(dir.path().join("20261028-100000-0001-first.png")).unwrap(),
            bytes
        );
    }
    #[test]
    fn stable_identity_survives_rename_edit_and_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SessionStore::create(dir.path(), "Test").unwrap();
        let capture = store
            .save(
                &image::RgbaImage::from_pixel(3, 3, image::Rgba([1, 2, 3, 255])),
                None,
                Trigger::Manual,
            )
            .unwrap();
        let handle = store.handle(capture);
        let id = handle.id().unwrap();
        let renamed = handle.rename("Chosen title").unwrap();
        let edited = png(store.folder(), "flattened-edited.png", 9);
        handle.register_edit(&edited).unwrap();
        let folder = store.folder().to_path_buf();
        drop(handle);
        drop(store);
        let library = SessionLibrary::confirm(
            SessionLibrary::preview(&folder, &cancel()).unwrap(),
            &cancel(),
        )
        .unwrap();
        let restored = library.capture_handles().unwrap().remove(0);
        assert_eq!(restored.id().unwrap(), id);
        assert_eq!(restored.snapshot().unwrap().title, "Chosen title");
        assert_eq!(
            restored.snapshot().unwrap().path.canonicalize().unwrap(),
            renamed.path.canonicalize().unwrap()
        );
        assert_eq!(restored.variants().unwrap().len(), 2);
    }
    #[test]
    fn concurrent_open_is_rejected_until_lock_is_released() {
        let dir = tempfile::tempdir().unwrap();
        let library = SessionLibrary::create(dir.path(), "Session").unwrap();
        assert!(SessionLibrary::confirm(
            SessionLibrary::preview(dir.path(), &cancel()).unwrap(),
            &cancel()
        )
        .unwrap_err()
        .contains("another process"));
        drop(library);
        assert!(SessionLibrary::confirm(
            SessionLibrary::preview(dir.path(), &cancel()).unwrap(),
            &cancel()
        )
        .is_ok());
    }
    #[test]
    fn unknown_schema_is_rejected_without_changes() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = br#"{"version":99}"#;
        fs::write(dir.path().join(MANIFEST_NAME), bytes).unwrap();
        assert!(SessionLibrary::preview(dir.path(), &cancel())
            .unwrap_err()
            .contains("unsupported"));
        assert_eq!(fs::read(dir.path().join(MANIFEST_NAME)).unwrap(), bytes);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn invalid_metadata_requires_explicit_recovery_and_keeps_backup() {
        let dir = tempfile::tempdir().unwrap();
        let original = b"{truncated";
        fs::write(dir.path().join(MANIFEST_NAME), original).unwrap();
        png(dir.path(), "20261028-100000-0001-first.png", 1);
        let preview = SessionLibrary::preview(dir.path(), &cancel()).unwrap();
        assert!(preview.needs_confirmation);
        assert_eq!(fs::read(dir.path().join(MANIFEST_NAME)).unwrap(), original);
        let library = SessionLibrary::confirm(preview, &cancel()).unwrap();
        assert_eq!(library.manifest().unwrap().captures.len(), 1);
        assert!(fs::read_dir(dir.path()).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("framecraft-session-recovery-")));
    }
    #[test]
    fn validation_rejects_unsafe_paths_duplicate_ids_and_sequences() {
        let dir = tempfile::tempdir().unwrap();
        png(dir.path(), "20261028-100000-0001-first.png", 1);
        let mut manifest = SessionLibrary::preview(dir.path(), &cancel())
            .unwrap()
            .manifest;
        manifest.captures[0].original.file = "../outside.png".into();
        assert!(validate(&manifest).is_err());
        manifest.captures[0].original.file = "good.png".into();
        let mut duplicate = manifest.captures[0].clone();
        duplicate.original.file = "other.png".into();
        manifest.captures.push(duplicate);
        assert!(validate(&manifest).is_err());
    }
    #[test]
    fn interruption_orphans_and_external_changes_are_explicit() {
        let dir = tempfile::tempdir().unwrap();
        let library = SessionLibrary::create(dir.path(), "Test").unwrap();
        drop(library);
        let path = png(dir.path(), "20261028-100000-0001-first.png", 1);
        let preview = SessionLibrary::preview(dir.path(), &cancel()).unwrap();
        assert_eq!(preview.import_count, 1);
        assert!(preview.needs_confirmation);
        let library = SessionLibrary::confirm(preview, &cancel()).unwrap();
        let capture_id = library.manifest().unwrap().captures[0].id.clone();
        png(dir.path(), "20261028-100000-0001-first.png", 7);
        assert_eq!(
            library.variants(&capture_id).unwrap()[0].state,
            ImageState::Modified
        );
        fs::remove_file(path).unwrap();
        assert_eq!(
            library.variants(&capture_id).unwrap()[0].state,
            ImageState::Missing
        );
    }
    #[test]
    fn metadata_failure_keeps_saved_png_and_reports_warning() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SessionStore::create(dir.path(), "Test").unwrap();
        fs::remove_file(store.folder().join(MANIFEST_NAME)).unwrap();
        fs::create_dir(store.folder().join(MANIFEST_NAME)).unwrap();
        let saved = store
            .save(&image::RgbaImage::new(4, 4), None, Trigger::Manual)
            .unwrap();
        assert!(saved.path.exists());
        assert!(store.handle(saved).metadata_warning().is_some());
    }
    #[test]
    fn cancelled_confirmation_preserves_folder() {
        let dir = tempfile::tempdir().unwrap();
        png(dir.path(), "20261028-100000-0001-first.png", 1);
        let preview = SessionLibrary::preview(dir.path(), &cancel()).unwrap();
        assert!(SessionLibrary::confirm(preview, &AtomicBool::new(true)).is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn recents_are_bounded_and_forgetting_does_not_delete_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let mut recents = RecentSessions::default();
        for n in 0..20 {
            recents.remember(dir.path().join(n.to_string()));
        }
        assert_eq!(recents.folders.len(), 12);
        recents.remember(dir.path().to_path_buf());
        recents.forget(dir.path());
        assert!(dir.path().is_dir());
        let path = dir.path().join("recents.json");
        recents.save(&path).unwrap();
        assert_eq!(
            RecentSessions::load(&path).unwrap().folders,
            recents.folders
        );
    }
    #[test]
    fn orphan_edited_images_are_previewed_and_recovered_without_duplicate_originals() {
        let dir = tempfile::tempdir().unwrap();
        png(dir.path(), "20261028-100000-0001-source.png", 1);
        let edited = png(dir.path(), "20261028-100000-0001-source-edited-01.png", 99);
        let bytes = fs::read(&edited).unwrap();
        let preview = SessionLibrary::preview(dir.path(), &cancel()).unwrap();
        assert!(preview.needs_confirmation);
        assert_eq!(preview.manifest.captures.len(), 1);
        assert_eq!(preview.manifest.captures[0].edited.len(), 1);
        assert!(preview.messages.iter().any(|m| m.contains("saved edited")));
        let library = SessionLibrary::confirm(preview, &cancel()).unwrap();
        assert_eq!(
            library.capture_handles().unwrap()[0]
                .load_variant(&CaptureVariant::Edited(0))
                .unwrap()
                .0
                .get_pixel(0, 0)
                .0[0],
            99
        );
        drop(library);
        let again = SessionLibrary::preview(dir.path(), &cancel()).unwrap();
        assert!(!again.needs_confirmation);
        assert_eq!(again.manifest.captures[0].edited.len(), 1);
        assert_eq!(fs::read(edited).unwrap(), bytes);
    }
    #[test]
    fn external_manifest_changes_are_not_overwritten_by_a_rename() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SessionStore::create(dir.path(), "Work").unwrap();
        let capture = store
            .save(&image::RgbaImage::new(4, 4), None, Trigger::Manual)
            .unwrap();
        let handle = store.handle(capture);
        let path = store.folder().join(MANIFEST_NAME);
        let mut external: Manifest = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        external.title = "External change".into();
        let bytes = serde_json::to_vec(&external).unwrap();
        fs::write(&path, &bytes).unwrap();
        let renamed = handle.rename("Manual title").unwrap();
        assert!(renamed.path.exists());
        assert_eq!(fs::read(path).unwrap(), bytes);
        assert!(handle
            .metadata_warning()
            .unwrap()
            .contains("changed outside"));
    }
    #[test]
    fn repeated_handle_lookup_shares_the_manual_name_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SessionStore::create(dir.path(), "Work").unwrap();
        let capture = store
            .save(&image::RgbaImage::new(4, 4), None, Trigger::Manual)
            .unwrap();
        let first = store.handle(capture.clone());
        let second = store.handle(capture.clone());
        first.rename("Manual title").unwrap();
        assert_eq!(second.snapshot().unwrap().title, "Manual title");
        assert!(second
            .rename_if_unchanged(&capture, "Stale OCR title")
            .unwrap()
            .is_none());
    }
    #[test]
    fn missing_edited_variant_never_falls_back_to_original() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SessionStore::create(dir.path(), "Work").unwrap();
        let saved = store
            .save(&image::RgbaImage::new(4, 4), None, Trigger::Manual)
            .unwrap();
        let handle = store.handle(saved);
        let edited = png(store.folder(), "saved-edited.png", 9);
        handle.register_edit(&edited).unwrap();
        fs::remove_file(edited).unwrap();
        assert!(handle.load_variant(&CaptureVariant::Edited(0)).is_err());
        assert!(handle.load_variant(&CaptureVariant::Original).is_ok());
    }
    #[test]
    fn external_rename_recovery_retains_identity_and_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SessionStore::create(dir.path(), "Work").unwrap();
        let saved = store
            .save(&image::RgbaImage::new(4, 4), None, Trigger::Manual)
            .unwrap();
        let id = store.handle(saved.clone()).id().unwrap();
        let original = fs::read(&saved.path).unwrap();
        let name = saved
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .replace("manual-capture", "moved");
        let destination = store.folder().join(name);
        fs::rename(&saved.path, &destination).unwrap();
        let folder = store.folder().to_path_buf();
        drop(store);
        let preview = SessionLibrary::preview(&folder, &cancel()).unwrap();
        assert!(preview.needs_confirmation);
        assert!(preview
            .messages
            .iter()
            .any(|m| m.contains("externally renamed")));
        let library = SessionLibrary::confirm(preview, &cancel()).unwrap();
        assert_eq!(library.manifest().unwrap().captures[0].id, id);
        assert_eq!(fs::read(destination).unwrap(), original);
    }
    #[test]
    fn explicit_relink_copies_without_changing_external_source() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SessionStore::create(dir.path(), "Work").unwrap();
        let saved = store
            .save(&image::RgbaImage::new(4, 4), None, Trigger::Manual)
            .unwrap();
        let handle = store.handle(saved.clone());
        fs::remove_file(&saved.path).unwrap();
        let external = tempfile::tempdir().unwrap();
        let source = png(external.path(), "external.png", 77);
        let bytes = fs::read(&source).unwrap();
        handle.import_replacement(&source).unwrap();
        assert_eq!(fs::read(source).unwrap(), bytes);
        assert_eq!(handle.load().unwrap().0.get_pixel(0, 0).0[0], 77);
        assert_eq!(
            handle.snapshot().unwrap().path.parent(),
            Some(store.folder())
        );
    }
    #[test]
    fn replacement_metadata_failure_keeps_the_new_handle_path_and_visible_warning() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SessionStore::create(dir.path(), "Work").unwrap();
        let saved = store
            .save(&image::RgbaImage::new(4, 4), None, Trigger::Manual)
            .unwrap();
        let handle = store.handle(saved.clone());
        let external = tempfile::tempdir().unwrap();
        let source = png(external.path(), "external.png", 77);
        fs::remove_file(store.folder().join(MANIFEST_NAME)).unwrap();
        fs::create_dir(store.folder().join(MANIFEST_NAME)).unwrap();
        handle.import_replacement(&source).unwrap();
        assert!(handle.snapshot().unwrap().path.exists());
        assert_ne!(handle.snapshot().unwrap().path, saved.path);
        assert!(saved.path.exists());
        assert!(handle.metadata_warning().is_some());
    }
    #[test]
    fn capture_preferences_round_trip_validate_and_keep_missing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        let mut preferences = SessionPreferences {
            destination: Some(dir.path().join("unavailable-volume")),
            source_hint: Some(SourceHint {
                display: 2,
                x: 100,
                y: 200,
                width: 1200,
                height: 700,
            }),
            ..SessionPreferences::default()
        };
        preferences.save(&path).unwrap();
        assert_eq!(SessionPreferences::load(&path).unwrap(), preferences);
        assert!(!preferences.destination.as_ref().unwrap().exists());
        preferences.interval = 0;
        assert!(preferences.save(&path).is_err());
        fs::write(&path, b"invalid").unwrap();
        assert!(SessionPreferences::load(&path)
            .unwrap_err()
            .contains("Reset"));
    }
    #[cfg(unix)]
    #[test]
    fn symlink_images_are_not_imported() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let image = png(other.path(), "outside.png", 1);
        std::os::unix::fs::symlink(image, dir.path().join("20261028-100000-0001-first.png"))
            .unwrap();
        let preview = SessionLibrary::preview(dir.path(), &cancel()).unwrap();
        assert_eq!(preview.import_count, 0);
        assert!(preview.messages.iter().any(|m| m.contains("symbolic")));
    }
}

/// A remembered source is a description only. It is never converted back into
/// a capture target without the user making a fresh selection.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceHint {
    pub display: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SessionPreferences {
    pub version: u32,
    pub title: String,
    pub destination: Option<PathBuf>,
    pub display: u32,
    pub schedule: String,
    pub interval: u64,
    pub threshold: f32,
    pub settle: f32,
    pub auto_name: bool,
    pub max_minutes: u64,
    pub max_frames: u64,
    pub source_hint: Option<SourceHint>,
}
impl Default for SessionPreferences {
    fn default() -> Self {
        Self {
            version: 1,
            title: "Meeting notes".into(),
            destination: None,
            display: 1,
            schedule: "manual".into(),
            interval: 15,
            threshold: 0.04,
            settle: 1.0,
            auto_name: true,
            max_minutes: 120,
            max_frames: 500,
            source_hint: None,
        }
    }
}
impl SessionPreferences {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.title.chars().count() > 120
            || !(1..=16).contains(&self.display)
            || !["manual", "interval", "change"].contains(&self.schedule.as_str())
            || !(5..=120).contains(&self.interval)
            || !self.threshold.is_finite()
            || !(0.01..=0.25).contains(&self.threshold)
            || !self.settle.is_finite()
            || !(0.5..=5.0).contains(&self.settle)
            || !(1..=480).contains(&self.max_minutes)
            || !(1..=2000).contains(&self.max_frames)
            || self
                .destination
                .as_ref()
                .is_some_and(|p| p.as_os_str().len() > 4096)
            || self.source_hint.as_ref().is_some_and(|h| {
                h.display == 0
                    || h.display > 16
                    || h.width == 0
                    || h.height == 0
                    || h.width > 16384
                    || h.height > 16384
                    || h.x.checked_add(h.width).is_none()
                    || h.y.checked_add(h.height).is_none()
            })
        {
            return Err("Capture preferences are invalid or use an unsupported version. Reset capture defaults before saving.".into());
        }
        Ok(())
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        if fs::metadata(path).map_err(|e| e.to_string())?.len() > 16 * 1024 {
            return Err(
                "Capture preferences exceed the 16 KiB limit. Reset capture defaults.".into(),
            );
        }
        let value: Self = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| {
                format!("Could not read capture preferences: {e}. Reset capture defaults.")
            })?;
        value.validate()?;
        Ok(value)
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let parent = path
            .parent()
            .ok_or("Capture preferences have no parent folder.")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        atomic_json(
            parent,
            path.file_name()
                .and_then(|n| n.to_str())
                .ok_or("Invalid capture preferences path.")?,
            self,
        )
    }
}

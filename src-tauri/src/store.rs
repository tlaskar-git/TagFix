// Sweep store: JSON files on disk, no database.
//
// Layout: sweeps/<yyyy-mm-dd>-<slug>/ containing sweep.json and tag-NN.png.
// Writes are atomic (temp file plus rename) so killing the process mid-sweep
// loses at most the tag currently being typed.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub number: u32,
    pub image: String,
    pub captured_utc: String,
    pub monitor_index: u32,
    pub dpi_scale: f64,
    pub region: Rect,
    pub window_title: String,
    pub process_name: String,
    pub screen_resolution: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub area: String,
    #[serde(default)]
    pub dropped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Sweep {
    pub schema_version: u32,
    pub slug: String,
    pub created_utc: String,
    pub tags: Vec<Tag>,
}

impl Sweep {
    pub fn new(slug: &str, created_utc: &str) -> Self {
        Sweep {
            schema_version: SCHEMA_VERSION,
            slug: slug.to_string(),
            created_utc: created_utc.to_string(),
            tags: Vec::new(),
        }
    }

    pub fn next_tag_number(&self) -> u32 {
        self.tags.iter().map(|t| t.number).max().unwrap_or(0) + 1
    }
}

/// Sanitize a slug: lowercase, ascii alphanumerics and hyphens only.
pub fn sanitize_slug(input: &str) -> String {
    let mut out = String::new();
    let mut last_hyphen = true;
    for c in input.trim().chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            last_hyphen = false;
        } else if !last_hyphen {
            out.push('-');
            last_hyphen = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("sweep");
    }
    out
}

pub fn sweep_dir_name(date_utc: &str, slug: &str) -> String {
    format!("{}-{}", date_utc, slug)
}

pub fn tag_image_name(number: u32) -> String {
    format!("tag-{:02}.png", number)
}

/// Atomic JSON write: write to a temp file in the same directory, then rename.
pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json.as_bytes())?;
    // On Windows, rename over an existing file fails; remove first.
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

pub struct SweepStore {
    root: PathBuf,
}

impl SweepStore {
    pub fn new(root: PathBuf) -> Self {
        SweepStore { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn sweep_json_path(&self, dir_name: &str) -> PathBuf {
        self.root.join(dir_name).join("sweep.json")
    }

    /// Create a new sweep folder for today. Errors if it already exists.
    pub fn create_sweep(&self, slug: &str, now_utc: &str) -> io::Result<(String, Sweep)> {
        let slug = sanitize_slug(slug);
        let date = &now_utc[..10];
        let dir_name = sweep_dir_name(date, &slug);
        let dir = self.root.join(&dir_name);
        if dir.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("sweep folder already exists: {}", dir_name),
            ));
        }
        fs::create_dir_all(&dir)?;
        let sweep = Sweep::new(&slug, now_utc);
        write_json_atomic(&self.sweep_json_path(&dir_name), &sweep)?;
        Ok((dir_name, sweep))
    }

    /// List sweep folder names, newest first, with tag counts.
    pub fn list_sweeps(&self) -> io::Result<Vec<(String, usize)>> {
        let mut out = Vec::new();
        if !self.root.exists() {
            return Ok(out);
        }
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let json = self.sweep_json_path(&name);
            if json.exists() {
                let sweep = self.load_sweep(&name)?;
                out.push((name, sweep.tags.len()));
            }
        }
        out.sort_by(|a, b| b.0.cmp(&a.0));
        Ok(out)
    }

    pub fn load_sweep(&self, dir_name: &str) -> io::Result<Sweep> {
        let raw = fs::read_to_string(self.sweep_json_path(dir_name))?;
        // Tolerate a UTF-8 BOM: hand-edited files often carry one.
        let raw = raw.trim_start_matches('\u{feff}');
        serde_json::from_str(raw).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    pub fn save_sweep(&self, dir_name: &str, sweep: &Sweep) -> io::Result<()> {
        write_json_atomic(&self.sweep_json_path(dir_name), sweep)
    }

    /// The active sweep: the newest existing sweep, or a fresh default one.
    pub fn active_sweep(&self, now_utc: &str) -> io::Result<(String, Sweep)> {
        if let Some((name, _)) = self.list_sweeps()?.into_iter().next() {
            let sweep = self.load_sweep(&name)?;
            return Ok((name, sweep));
        }
        fs::create_dir_all(&self.root)?;
        self.create_sweep("default", now_utc)
    }

    /// Append a tag to a sweep and persist immediately.
    pub fn append_tag(&self, dir_name: &str, tag: Tag) -> io::Result<Sweep> {
        let mut sweep = self.load_sweep(dir_name)?;
        sweep.tags.push(tag);
        self.save_sweep(dir_name, &sweep)?;
        Ok(sweep)
    }

    /// Edit the operator-facing fields of one tag.
    pub fn update_tag(
        &self,
        dir_name: &str,
        number: u32,
        text: &str,
        severity: &str,
        area: &str,
    ) -> io::Result<Sweep> {
        let mut sweep = self.load_sweep(dir_name)?;
        let tag = sweep
            .tags
            .iter_mut()
            .find(|t| t.number == number)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such tag"))?;
        tag.text = text.to_string();
        tag.severity = severity.to_string();
        tag.area = area.to_string();
        self.save_sweep(dir_name, &sweep)?;
        Ok(sweep)
    }

    /// Soft delete: dropped tags stay in sweep.json so a later sweep can
    /// pick them back up.
    pub fn set_dropped(&self, dir_name: &str, number: u32, dropped: bool) -> io::Result<Sweep> {
        let mut sweep = self.load_sweep(dir_name)?;
        let tag = sweep
            .tags
            .iter_mut()
            .find(|t| t.number == number)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such tag"))?;
        tag.dropped = dropped;
        self.save_sweep(dir_name, &sweep)?;
        Ok(sweep)
    }

    /// Reorder tags to match `order` (a list of tag numbers). Tags not named
    /// in the list keep their relative order after the named ones.
    pub fn reorder_tags(&self, dir_name: &str, order: &[u32]) -> io::Result<Sweep> {
        let mut sweep = self.load_sweep(dir_name)?;
        let mut remaining = std::mem::take(&mut sweep.tags);
        let mut reordered = Vec::with_capacity(remaining.len());
        for number in order {
            if let Some(pos) = remaining.iter().position(|t| t.number == *number) {
                reordered.push(remaining.remove(pos));
            }
        }
        reordered.extend(remaining);
        sweep.tags = reordered;
        self.save_sweep(dir_name, &sweep)?;
        Ok(sweep)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tagfix-test-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn sample_tag(number: u32) -> Tag {
        Tag {
            number,
            image: tag_image_name(number),
            captured_utc: "2026-08-13T10:00:00Z".into(),
            monitor_index: 0,
            dpi_scale: 1.5,
            region: Rect { x: 10, y: 20, width: 300, height: 200 },
            window_title: "Some App".into(),
            process_name: "someapp.exe".into(),
            screen_resolution: "2496x1664".into(),
            text: "button misaligned".into(),
            severity: "high".into(),
            area: "layout".into(),
            dropped: false,
        }
    }

    #[test]
    fn slug_sanitizes_spaces_and_case() {
        assert_eq!(sanitize_slug("My Cool Sweep"), "my-cool-sweep");
    }

    #[test]
    fn slug_collapses_symbol_runs() {
        assert_eq!(sanitize_slug("a__b!!c"), "a-b-c");
    }

    #[test]
    fn slug_trims_leading_and_trailing_junk() {
        assert_eq!(sanitize_slug("  --hello-- "), "hello");
    }

    #[test]
    fn slug_empty_falls_back() {
        assert_eq!(sanitize_slug("!!!"), "sweep");
    }

    #[test]
    fn sweep_dir_name_format() {
        assert_eq!(sweep_dir_name("2026-08-13", "login-page"), "2026-08-13-login-page");
    }

    #[test]
    fn tag_image_name_zero_pads() {
        assert_eq!(tag_image_name(3), "tag-03.png");
        assert_eq!(tag_image_name(42), "tag-42.png");
    }

    #[test]
    fn schema_version_is_written() {
        let sweep = Sweep::new("s", "2026-08-13T10:00:00Z");
        let json = serde_json::to_string(&sweep).unwrap();
        assert!(json.contains("\"schemaVersion\":1"));
    }

    #[test]
    fn sweep_round_trips_through_json() {
        let mut sweep = Sweep::new("round", "2026-08-13T10:00:00Z");
        sweep.tags.push(sample_tag(1));
        let json = serde_json::to_string_pretty(&sweep).unwrap();
        let back: Sweep = serde_json::from_str(&json).unwrap();
        assert_eq!(sweep, back);
    }

    #[test]
    fn tag_defaults_apply_for_missing_fields() {
        // A tag written before Phase 3 fields existed must still load.
        let json = r#"{
            "number": 1, "image": "tag-01.png",
            "capturedUtc": "2026-08-13T10:00:00Z", "monitorIndex": 0,
            "dpiScale": 1.0,
            "region": {"x":0,"y":0,"width":10,"height":10},
            "windowTitle": "t", "processName": "p.exe",
            "screenResolution": "800x600"
        }"#;
        let tag: Tag = serde_json::from_str(json).unwrap();
        assert_eq!(tag.text, "");
        assert!(!tag.dropped);
    }

    #[test]
    fn create_sweep_writes_folder_and_json() {
        let store = SweepStore::new(tmp_root("create"));
        let (name, sweep) = store.create_sweep("My Sweep", "2026-08-13T10:00:00Z").unwrap();
        assert_eq!(name, "2026-08-13-my-sweep");
        assert_eq!(sweep.tags.len(), 0);
        assert!(store.sweep_json_path(&name).exists());
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn create_sweep_rejects_duplicates() {
        let store = SweepStore::new(tmp_root("dup"));
        store.create_sweep("x", "2026-08-13T10:00:00Z").unwrap();
        let err = store.create_sweep("x", "2026-08-13T11:00:00Z").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn list_sweeps_newest_first_with_counts() {
        let store = SweepStore::new(tmp_root("list"));
        store.create_sweep("alpha", "2026-08-12T10:00:00Z").unwrap();
        let (b_name, _) = store.create_sweep("beta", "2026-08-13T10:00:00Z").unwrap();
        store.append_tag(&b_name, sample_tag(1)).unwrap();
        let listed = store.list_sweeps().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].0, "2026-08-13-beta");
        assert_eq!(listed[0].1, 1);
        assert_eq!(listed[1].1, 0);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn append_tag_persists_immediately() {
        let store = SweepStore::new(tmp_root("append"));
        let (name, _) = store.create_sweep("s", "2026-08-13T10:00:00Z").unwrap();
        store.append_tag(&name, sample_tag(1)).unwrap();
        store.append_tag(&name, sample_tag(2)).unwrap();
        let loaded = store.load_sweep(&name).unwrap();
        assert_eq!(loaded.tags.len(), 2);
        assert_eq!(loaded.next_tag_number(), 3);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn active_sweep_creates_default_when_empty() {
        let store = SweepStore::new(tmp_root("active"));
        let (name, sweep) = store.active_sweep("2026-08-13T10:00:00Z").unwrap();
        assert_eq!(name, "2026-08-13-default");
        assert_eq!(sweep.slug, "default");
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn active_sweep_prefers_newest_existing() {
        let store = SweepStore::new(tmp_root("active2"));
        store.create_sweep("old", "2026-08-12T10:00:00Z").unwrap();
        store.create_sweep("new", "2026-08-13T09:00:00Z").unwrap();
        let (name, _) = store.active_sweep("2026-08-13T10:00:00Z").unwrap();
        assert_eq!(name, "2026-08-13-new");
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn atomic_write_leaves_no_tmp_file() {
        let store = SweepStore::new(tmp_root("atomic"));
        let (name, _) = store.create_sweep("s", "2026-08-13T10:00:00Z").unwrap();
        store.append_tag(&name, sample_tag(1)).unwrap();
        let dir = store.root().join(&name);
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn update_tag_edits_fields() {
        let store = SweepStore::new(tmp_root("update"));
        let (name, _) = store.create_sweep("s", "2026-08-13T10:00:00Z").unwrap();
        store.append_tag(&name, sample_tag(1)).unwrap();
        store.update_tag(&name, 1, "new text", "low", "copy").unwrap();
        let sweep = store.load_sweep(&name).unwrap();
        assert_eq!(sweep.tags[0].text, "new text");
        assert_eq!(sweep.tags[0].severity, "low");
        assert_eq!(sweep.tags[0].area, "copy");
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn update_tag_missing_number_errors() {
        let store = SweepStore::new(tmp_root("update-miss"));
        let (name, _) = store.create_sweep("s", "2026-08-13T10:00:00Z").unwrap();
        let err = store.update_tag(&name, 9, "t", "high", "layout").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn dropped_tags_stay_in_json() {
        let store = SweepStore::new(tmp_root("drop"));
        let (name, _) = store.create_sweep("s", "2026-08-13T10:00:00Z").unwrap();
        store.append_tag(&name, sample_tag(1)).unwrap();
        store.append_tag(&name, sample_tag(2)).unwrap();
        store.set_dropped(&name, 1, true).unwrap();
        let sweep = store.load_sweep(&name).unwrap();
        assert_eq!(sweep.tags.len(), 2);
        assert!(sweep.tags[0].dropped);
        assert!(!sweep.tags[1].dropped);
        // And it can be picked back up.
        store.set_dropped(&name, 1, false).unwrap();
        assert!(!store.load_sweep(&name).unwrap().tags[0].dropped);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn reorder_tags_applies_given_order() {
        let store = SweepStore::new(tmp_root("reorder"));
        let (name, _) = store.create_sweep("s", "2026-08-13T10:00:00Z").unwrap();
        for n in 1..=3 {
            store.append_tag(&name, sample_tag(n)).unwrap();
        }
        store.reorder_tags(&name, &[3, 1, 2]).unwrap();
        let sweep = store.load_sweep(&name).unwrap();
        let numbers: Vec<u32> = sweep.tags.iter().map(|t| t.number).collect();
        assert_eq!(numbers, vec![3, 1, 2]);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn reorder_keeps_unnamed_tags_at_end() {
        let store = SweepStore::new(tmp_root("reorder2"));
        let (name, _) = store.create_sweep("s", "2026-08-13T10:00:00Z").unwrap();
        for n in 1..=3 {
            store.append_tag(&name, sample_tag(n)).unwrap();
        }
        store.reorder_tags(&name, &[2]).unwrap();
        let sweep = store.load_sweep(&name).unwrap();
        let numbers: Vec<u32> = sweep.tags.iter().map(|t| t.number).collect();
        assert_eq!(numbers, vec![2, 1, 3]);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn load_sweep_tolerates_utf8_bom() {
        let store = SweepStore::new(tmp_root("bom"));
        let (name, sweep) = store.create_sweep("s", "2026-08-13T10:00:00Z").unwrap();
        let path = store.sweep_json_path(&name);
        let json = serde_json::to_string(&sweep).unwrap();
        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice(json.as_bytes());
        fs::write(&path, with_bom).unwrap();
        let loaded = store.load_sweep(&name).unwrap();
        assert_eq!(loaded, sweep);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn next_tag_number_survives_gaps() {
        let mut sweep = Sweep::new("g", "2026-08-13T10:00:00Z");
        sweep.tags.push(sample_tag(1));
        sweep.tags.push(sample_tag(5));
        assert_eq!(sweep.next_tag_number(), 6);
    }
}

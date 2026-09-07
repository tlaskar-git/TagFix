// Sweep store: JSON files on disk, no database.
//
// Layout: sweeps/<yyyy-mm-dd>-<slug>/ containing sweep.json and tag-NN.png.
// Writes are atomic (temp file plus rename) so killing the process mid-sweep
// loses at most the tag currently being typed.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 2;

/// Tag kinds. A quote tag carries text instead of pixels; both share one
/// sweep and one number sequence.
pub const KIND_REGION: &str = "region";
pub const KIND_QUOTE: &str = "quote";

/// Attachment labels.
pub const LABEL_COMPARE: &str = "compare";
pub const LABEL_AFTER: &str = "after";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// An extra crop added to an existing tag: a comparison taken right after
/// the tag, or an "after" shot on a carried tag.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub image: String,
    pub region: Rect,
    pub captured_utc: String,
    /// LABEL_COMPARE or LABEL_AFTER.
    pub label: String,
}

/// Where a carried tag came from. `image` is the copy of the original crop
/// taken into the new sweep, so the source sweep can be deleted without
/// losing the evidence.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CarriedFrom {
    pub sweep: String,
    pub number: u32,
    pub image: Option<String>,
    pub text: String,
}

fn default_kind() -> String {
    KIND_REGION.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub number: u32,
    /// The crop file. None on a quote tag unless quoteScreenshot is on, and
    /// None on a carried tag until an "after" is captured.
    #[serde(default)]
    pub image: Option<String>,
    pub captured_utc: String,
    pub monitor_index: u32,
    pub dpi_scale: f64,
    /// None for quote tags: there is no rectangle to point at.
    #[serde(default)]
    pub region: Option<Rect>,
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
    // Schema version 2 below. Every one of these has a default so a v1
    // sweep.json loads without a migration step.
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub context_image: Option<String>,
    #[serde(default)]
    pub quote: String,
    /// The raw CF_HTML fragment. Stored, not rendered this round.
    #[serde(default)]
    pub quote_html: Option<String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub element: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub carried_from: Option<CarriedFrom>,
}

impl Default for Tag {
    fn default() -> Self {
        Tag {
            number: 0,
            image: None,
            captured_utc: String::new(),
            monitor_index: 0,
            dpi_scale: 1.0,
            region: None,
            window_title: String::new(),
            process_name: String::new(),
            screen_resolution: String::new(),
            text: String::new(),
            severity: String::new(),
            area: String::new(),
            dropped: false,
            kind: default_kind(),
            context_image: None,
            quote: String::new(),
            quote_html: None,
            url: String::new(),
            element: String::new(),
            target: String::new(),
            attachments: Vec::new(),
            carried_from: None,
        }
    }
}

impl Tag {
    /// The crop file name, or an empty string when there is no crop.
    /// Renderers want a &str, not an Option, on every path.
    pub fn image_name(&self) -> &str {
        self.image.as_deref().unwrap_or("")
    }

    /// The crop rectangle, or a zero rectangle when there is no crop.
    pub fn region_or_zero(&self) -> Rect {
        self.region.clone().unwrap_or_default()
    }

    pub fn is_quote(&self) -> bool {
        self.kind == KIND_QUOTE
    }

    /// The file name the next attachment on this tag should use.
    pub fn next_attachment_name(&self) -> String {
        tag_attachment_name(self.number, self.attachments.len() as u32 + 1)
    }
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

/// The reduced size shot of the whole window with the crop outlined.
pub fn tag_context_image_name(number: u32) -> String {
    format!("tag-{:02}-context.png", number)
}

/// Attachments are numbered from one within their tag: tag-03-a1.png.
pub fn tag_attachment_name(number: u32, index: u32) -> String {
    format!("tag-{:02}-a{}.png", number, index)
}

/// The copy of the original crop kept beside a carried tag.
pub fn tag_before_image_name(number: u32) -> String {
    format!("tag-{:02}-before.png", number)
}

/// The optional foreground window shot of a quote tag.
pub fn tag_quote_image_name(number: u32) -> String {
    format!("tag-{:02}-quote.png", number)
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

    /// The file naming the sweep new tags go into.
    ///
    /// Without it "newest" would mean "the name that sorts last", which is
    /// wrong the moment two sweeps are created on one day and the newer one
    /// sorts earlier: 2026-09-07-alpha created after 2026-09-07-zulu would
    /// never receive a tag.
    pub fn active_marker_path(&self) -> PathBuf {
        self.root.join("active.txt")
    }

    pub fn set_active_sweep(&self, dir_name: &str) -> io::Result<()> {
        fs::create_dir_all(&self.root)?;
        fs::write(self.active_marker_path(), dir_name.trim().as_bytes())
    }

    /// The marked sweep, when the marker names one that still exists. A
    /// deleted or hand-mangled name is ignored rather than fatal, and a
    /// name with a path separator in it is never followed.
    pub fn marked_active_sweep(&self) -> Option<String> {
        let raw = fs::read_to_string(self.active_marker_path()).ok()?;
        let name = raw.trim();
        if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
            return None;
        }
        if self.sweep_json_path(name).exists() {
            Some(name.to_string())
        } else {
            None
        }
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
        // A sweep is created in order to be used, so it becomes active.
        let _ = self.set_active_sweep(&dir_name);
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

    /// The active sweep: whatever the marker names, else the newest
    /// existing sweep, else a fresh default one.
    pub fn active_sweep(&self, now_utc: &str) -> io::Result<(String, Sweep)> {
        if let Some(name) = self.marked_active_sweep() {
            let sweep = self.load_sweep(&name)?;
            return Ok((name, sweep));
        }
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

    /// Is there already a sweep folder for this date? The day rollover
    /// setting starts a fresh sweep on the first capture of a new day
    /// rather than extending yesterday's.
    pub fn sweep_exists_for_date(&self, date_utc: &str) -> io::Result<bool> {
        let prefix = format!("{}-", date_utc);
        Ok(self
            .list_sweeps()?
            .into_iter()
            .any(|(name, _)| name.starts_with(&prefix)))
    }

    /// Edit the operator-facing fields of one tag.
    pub fn update_tag(
        &self,
        dir_name: &str,
        number: u32,
        text: &str,
        severity: &str,
        area: &str,
        target: &str,
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
        tag.target = target.to_string();
        self.save_sweep(dir_name, &sweep)?;
        Ok(sweep)
    }

    /// Add an extra crop to a tag that is already saved.
    pub fn append_attachment(
        &self,
        dir_name: &str,
        number: u32,
        attachment: Attachment,
    ) -> io::Result<Sweep> {
        let mut sweep = self.load_sweep(dir_name)?;
        let tag = sweep
            .tags
            .iter_mut()
            .find(|t| t.number == number)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such tag"))?;
        tag.attachments.push(attachment);
        self.save_sweep(dir_name, &sweep)?;
        Ok(sweep)
    }

    /// Record where a tag was carried from. Separate from carry_forward so
    /// a repair or an import can set it without copying files.
    pub fn set_carried_from(
        &self,
        dir_name: &str,
        number: u32,
        carried: Option<CarriedFrom>,
    ) -> io::Result<Sweep> {
        let mut sweep = self.load_sweep(dir_name)?;
        let tag = sweep
            .tags
            .iter_mut()
            .find(|t| t.number == number)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such tag"))?;
        tag.carried_from = carried;
        self.save_sweep(dir_name, &sweep)?;
        Ok(sweep)
    }

    /// Copy a tag from an earlier sweep into another one as a re-report.
    ///
    /// The new tag takes the next number in the destination sweep, keeps the
    /// text and the chips, and gets its own copy of the original crop as
    /// tag-NN-before.png so the source sweep can later be deleted. It has no
    /// `image` of its own until someone captures an "after". The source
    /// sweep is never written to.
    pub fn carry_forward(
        &self,
        from_dir: &str,
        from_number: u32,
        to_dir: &str,
        now_utc: &str,
    ) -> io::Result<Tag> {
        let source_sweep = self.load_sweep(from_dir)?;
        let source = source_sweep
            .tags
            .iter()
            .find(|t| t.number == from_number)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such tag"))?
            .clone();

        let mut target_sweep = self.load_sweep(to_dir)?;
        let number = target_sweep.next_tag_number();

        // Copy the original crop next to the new tag. A missing source file
        // is not fatal: the text of a re-report is worth keeping on its own.
        let mut before: Option<String> = None;
        if let Some(original) = source.image.as_ref() {
            let src_path = self.root.join(from_dir).join(original);
            if src_path.exists() {
                let name = tag_before_image_name(number);
                let dst_path = self.root.join(to_dir).join(&name);
                fs::copy(&src_path, &dst_path)?;
                before = Some(name);
            }
        }

        let tag = Tag {
            number,
            image: None,
            captured_utc: now_utc.to_string(),
            monitor_index: source.monitor_index,
            dpi_scale: source.dpi_scale,
            region: source.region.clone(),
            window_title: source.window_title.clone(),
            process_name: source.process_name.clone(),
            screen_resolution: source.screen_resolution.clone(),
            text: source.text.clone(),
            severity: source.severity.clone(),
            area: source.area.clone(),
            dropped: false,
            kind: source.kind.clone(),
            context_image: None,
            quote: source.quote.clone(),
            quote_html: source.quote_html.clone(),
            url: source.url.clone(),
            element: source.element.clone(),
            target: source.target.clone(),
            attachments: Vec::new(),
            carried_from: Some(CarriedFrom {
                sweep: from_dir.to_string(),
                number: from_number,
                image: before,
                text: source.text.clone(),
            }),
        };
        target_sweep.tags.push(tag.clone());
        self.save_sweep(to_dir, &target_sweep)?;
        Ok(tag)
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
            image: Some(tag_image_name(number)),
            captured_utc: "2026-08-13T10:00:00Z".into(),
            monitor_index: 0,
            dpi_scale: 1.5,
            region: Some(Rect { x: 10, y: 20, width: 300, height: 200 }),
            window_title: "Some App".into(),
            process_name: "someapp.exe".into(),
            screen_resolution: "2496x1664".into(),
            text: "button misaligned".into(),
            severity: "high".into(),
            area: "layout".into(),
            dropped: false,
            ..Tag::default()
        }
    }

    fn quote_tag(number: u32, quote: &str) -> Tag {
        Tag {
            number,
            captured_utc: "2026-08-13T10:05:00Z".into(),
            window_title: "Claude".into(),
            process_name: "claude.exe".into(),
            screen_resolution: "2496x1664".into(),
            text: "wrong, on.slobal.com stays DNS only".into(),
            severity: "high".into(),
            area: "copy".into(),
            kind: KIND_QUOTE.into(),
            quote: quote.into(),
            quote_html: Some("<b>the relay</b>".into()),
            url: "https://slobal.com/portal".into(),
            target: "slobal.com".into(),
            ..Tag::default()
        }
    }

    /// Write a real v1 sweep.json by hand: no schema v2 field anywhere and
    /// image and region as plain values rather than nullable ones.
    fn write_v1_sweep(store: &SweepStore, dir_name: &str) {
        let dir = store.root().join(dir_name);
        fs::create_dir_all(&dir).unwrap();
        let json = r#"{
            "schemaVersion": 1,
            "slug": "legacy",
            "createdUtc": "2026-08-01T09:00:00Z",
            "tags": [
                {
                    "number": 1,
                    "image": "tag-01.png",
                    "capturedUtc": "2026-08-01T09:01:00Z",
                    "monitorIndex": 1,
                    "dpiScale": 1.5,
                    "region": {"x": 10, "y": 20, "width": 300, "height": 200},
                    "windowTitle": "Helmsly",
                    "processName": "helmsly.exe",
                    "screenResolution": "2496x1664",
                    "text": "button clipped",
                    "severity": "high",
                    "area": "layout",
                    "dropped": false
                }
            ]
        }"#;
        fs::write(dir.join("sweep.json"), json).unwrap();
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
    fn evidence_file_names_follow_the_tag_number() {
        assert_eq!(tag_context_image_name(3), "tag-03-context.png");
        assert_eq!(tag_attachment_name(3, 1), "tag-03-a1.png");
        assert_eq!(tag_attachment_name(12, 2), "tag-12-a2.png");
        assert_eq!(tag_before_image_name(7), "tag-07-before.png");
        assert_eq!(tag_quote_image_name(7), "tag-07-quote.png");
    }

    #[test]
    fn next_attachment_name_counts_from_one() {
        let mut tag = sample_tag(4);
        assert_eq!(tag.next_attachment_name(), "tag-04-a1.png");
        tag.attachments.push(Attachment {
            image: "tag-04-a1.png".into(),
            region: Rect { x: 0, y: 0, width: 10, height: 10 },
            captured_utc: "2026-08-13T10:00:00Z".into(),
            label: LABEL_COMPARE.into(),
        });
        assert_eq!(tag.next_attachment_name(), "tag-04-a2.png");
    }

    #[test]
    fn schema_version_is_written() {
        let sweep = Sweep::new("s", "2026-08-13T10:00:00Z");
        let json = serde_json::to_string(&sweep).unwrap();
        assert!(json.contains("\"schemaVersion\":2"));
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
    fn v1_tag_without_any_v2_field_loads_with_defaults() {
        let json = r#"{
            "number": 1, "image": "tag-01.png",
            "capturedUtc": "2026-08-13T10:00:00Z", "monitorIndex": 0,
            "dpiScale": 1.0,
            "region": {"x":0,"y":0,"width":10,"height":10},
            "windowTitle": "t", "processName": "p.exe",
            "screenResolution": "800x600",
            "text": "note", "severity": "high", "area": "layout",
            "dropped": false
        }"#;
        let tag: Tag = serde_json::from_str(json).unwrap();
        // The v1 shape survives: image and region are still there.
        assert_eq!(tag.image.as_deref(), Some("tag-01.png"));
        assert_eq!(tag.image_name(), "tag-01.png");
        assert_eq!(tag.region_or_zero().width, 10);
        // And every v2 field takes its default.
        assert_eq!(tag.kind, KIND_REGION);
        assert!(!tag.is_quote());
        assert_eq!(tag.context_image, None);
        assert_eq!(tag.quote, "");
        assert_eq!(tag.quote_html, None);
        assert_eq!(tag.url, "");
        assert_eq!(tag.element, "");
        assert_eq!(tag.target, "");
        assert!(tag.attachments.is_empty());
        assert_eq!(tag.carried_from, None);
    }

    #[test]
    fn v1_sweep_file_loads_unchanged() {
        let store = SweepStore::new(tmp_root("v1"));
        write_v1_sweep(&store, "2026-08-01-legacy");
        let sweep = store.load_sweep("2026-08-01-legacy").unwrap();
        assert_eq!(sweep.schema_version, 1);
        assert_eq!(sweep.tags.len(), 1);
        assert_eq!(sweep.tags[0].kind, KIND_REGION);
        assert_eq!(sweep.tags[0].image_name(), "tag-01.png");
        // Saving it back writes the current schema version.
        let mut sweep = sweep;
        sweep.schema_version = SCHEMA_VERSION;
        store.save_sweep("2026-08-01-legacy", &sweep).unwrap();
        assert_eq!(store.load_sweep("2026-08-01-legacy").unwrap(), sweep);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn mixed_sweep_round_trips_through_json() {
        let mut sweep = Sweep::new("mixed", "2026-08-13T10:00:00Z");
        sweep.tags.push(sample_tag(1));
        sweep.tags.push(quote_tag(2, "the relay never moves"));
        let mut carried = sample_tag(3);
        carried.image = None;
        carried.context_image = Some(tag_context_image_name(3));
        carried.element = "button 'Deploy'".into();
        carried.target = "helmsly".into();
        carried.carried_from = Some(CarriedFrom {
            sweep: "2026-08-01-legacy".into(),
            number: 4,
            image: Some(tag_before_image_name(3)),
            text: "button clipped".into(),
        });
        sweep.tags.push(carried);

        let json = serde_json::to_string_pretty(&sweep).unwrap();
        let back: Sweep = serde_json::from_str(&json).unwrap();
        assert_eq!(sweep, back);
        // Field names stay camelCase on the wire.
        assert!(json.contains("\"quoteHtml\""));
        assert!(json.contains("\"contextImage\""));
        assert!(json.contains("\"carriedFrom\""));
        assert_eq!(back.tags[1].kind, KIND_QUOTE);
        assert_eq!(back.tags[1].region, None);
        assert_eq!(back.tags[1].image, None);
        assert_eq!(back.tags[1].quote, "the relay never moves");
    }

    #[test]
    fn attachments_round_trip() {
        let store = SweepStore::new(tmp_root("attach"));
        let (name, _) = store.create_sweep("s", "2026-08-13T10:00:00Z").unwrap();
        store.append_tag(&name, sample_tag(1)).unwrap();
        let attachment = Attachment {
            image: tag_attachment_name(1, 1),
            region: Rect { x: 5, y: 6, width: 70, height: 80 },
            captured_utc: "2026-08-13T10:02:00Z".into(),
            label: LABEL_COMPARE.into(),
        };
        store.append_attachment(&name, 1, attachment.clone()).unwrap();
        store
            .append_attachment(
                &name,
                1,
                Attachment {
                    image: tag_attachment_name(1, 2),
                    label: LABEL_AFTER.into(),
                    ..attachment.clone()
                },
            )
            .unwrap();
        let sweep = store.load_sweep(&name).unwrap();
        assert_eq!(sweep.tags[0].attachments.len(), 2);
        assert_eq!(sweep.tags[0].attachments[0], attachment);
        assert_eq!(sweep.tags[0].attachments[1].label, LABEL_AFTER);
        assert_eq!(sweep.tags[0].next_attachment_name(), "tag-01-a3.png");
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn append_attachment_missing_number_errors() {
        let store = SweepStore::new(tmp_root("attach-miss"));
        let (name, _) = store.create_sweep("s", "2026-08-13T10:00:00Z").unwrap();
        let err = store
            .append_attachment(&name, 9, Attachment::default())
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn set_carried_from_records_and_clears() {
        let store = SweepStore::new(tmp_root("carried-set"));
        let (name, _) = store.create_sweep("s", "2026-08-13T10:00:00Z").unwrap();
        store.append_tag(&name, sample_tag(1)).unwrap();
        let carried = CarriedFrom {
            sweep: "2026-08-01-old".into(),
            number: 2,
            image: Some("tag-01-before.png".into()),
            text: "still broken".into(),
        };
        store.set_carried_from(&name, 1, Some(carried.clone())).unwrap();
        assert_eq!(
            store.load_sweep(&name).unwrap().tags[0].carried_from,
            Some(carried)
        );
        store.set_carried_from(&name, 1, None).unwrap();
        assert_eq!(store.load_sweep(&name).unwrap().tags[0].carried_from, None);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn carry_forward_copies_text_chips_and_crop() {
        let store = SweepStore::new(tmp_root("carry"));
        let (old_name, _) = store.create_sweep("old", "2026-08-12T10:00:00Z").unwrap();
        let mut source = sample_tag(2);
        source.target = "helmsly".into();
        source.url = "https://acme.on.slobal.com/".into();
        source.element = "button 'Deploy'".into();
        store.append_tag(&old_name, source).unwrap();
        // A real crop file, so the copy has something to copy.
        fs::write(store.root().join(&old_name).join("tag-02.png"), b"png bytes").unwrap();

        let (new_name, _) = store.create_sweep("new", "2026-08-13T10:00:00Z").unwrap();
        store.append_tag(&new_name, sample_tag(1)).unwrap();

        let carried = store
            .carry_forward(&old_name, 2, &new_name, "2026-08-13T11:00:00Z")
            .unwrap();

        // A new number in the destination sequence, not the old one.
        assert_eq!(carried.number, 2);
        assert_eq!(carried.text, "button misaligned");
        assert_eq!(carried.severity, "high");
        assert_eq!(carried.area, "layout");
        assert_eq!(carried.target, "helmsly");
        assert_eq!(carried.url, "https://acme.on.slobal.com/");
        assert_eq!(carried.element, "button 'Deploy'");
        assert_eq!(carried.captured_utc, "2026-08-13T11:00:00Z");
        // No crop of its own until an "after" is taken.
        assert_eq!(carried.image, None);
        let from = carried.carried_from.clone().unwrap();
        assert_eq!(from.sweep, old_name);
        assert_eq!(from.number, 2);
        assert_eq!(from.image.as_deref(), Some("tag-02-before.png"));
        assert_eq!(from.text, "button misaligned");

        // The crop landed in the new sweep under the before name.
        let before = store.root().join(&new_name).join("tag-02-before.png");
        assert_eq!(fs::read(&before).unwrap(), b"png bytes");

        // The destination holds both tags; the source is untouched.
        let new_sweep = store.load_sweep(&new_name).unwrap();
        assert_eq!(new_sweep.tags.len(), 2);
        let old_sweep = store.load_sweep(&old_name).unwrap();
        assert_eq!(old_sweep.tags.len(), 1);
        assert_eq!(old_sweep.tags[0].number, 2);
        assert_eq!(old_sweep.tags[0].carried_from, None);
        assert_eq!(old_sweep.tags[0].image.as_deref(), Some("tag-02.png"));
        assert!(store.root().join(&old_name).join("tag-02.png").exists());
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn carry_forward_survives_a_missing_crop_file() {
        let store = SweepStore::new(tmp_root("carry-nofile"));
        let (old_name, _) = store.create_sweep("old", "2026-08-12T10:00:00Z").unwrap();
        store.append_tag(&old_name, quote_tag(1, "quoted line")).unwrap();
        let (new_name, _) = store.create_sweep("new", "2026-08-13T10:00:00Z").unwrap();

        let carried = store
            .carry_forward(&old_name, 1, &new_name, "2026-08-13T11:00:00Z")
            .unwrap();
        assert_eq!(carried.number, 1);
        assert_eq!(carried.kind, KIND_QUOTE);
        assert_eq!(carried.quote, "quoted line");
        assert_eq!(carried.carried_from.unwrap().image, None);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn carry_forward_missing_number_errors() {
        let store = SweepStore::new(tmp_root("carry-miss"));
        let (old_name, _) = store.create_sweep("old", "2026-08-12T10:00:00Z").unwrap();
        let (new_name, _) = store.create_sweep("new", "2026-08-13T10:00:00Z").unwrap();
        let err = store
            .carry_forward(&old_name, 9, &new_name, "2026-08-13T11:00:00Z")
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn sweep_exists_for_date_drives_the_day_rollover() {
        let store = SweepStore::new(tmp_root("rollover"));
        assert!(!store.sweep_exists_for_date("2026-08-13").unwrap());
        store.create_sweep("login", "2026-08-12T10:00:00Z").unwrap();
        assert!(store.sweep_exists_for_date("2026-08-12").unwrap());
        // Yesterday's sweep does not count as today's.
        assert!(!store.sweep_exists_for_date("2026-08-13").unwrap());
        store.create_sweep("default", "2026-08-13T09:00:00Z").unwrap();
        assert!(store.sweep_exists_for_date("2026-08-13").unwrap());
        let _ = fs::remove_dir_all(store.root());
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
    fn creating_a_sweep_makes_it_active() {
        let store = SweepStore::new(tmp_root("active-marker"));
        let (name, _) = store.create_sweep("login", "2026-08-13T10:00:00Z").unwrap();
        assert_eq!(store.marked_active_sweep().as_deref(), Some(name.as_str()));
        assert_eq!(store.active_sweep("2026-08-13T11:00:00Z").unwrap().0, name);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn the_marker_beats_the_name_order() {
        // Created second but sorts first: only the marker can tell.
        let store = SweepStore::new(tmp_root("active-order"));
        store.create_sweep("zulu", "2026-08-13T10:00:00Z").unwrap();
        let (alpha, _) = store.create_sweep("alpha", "2026-08-13T11:00:00Z").unwrap();
        assert_eq!(store.list_sweeps().unwrap()[0].0, "2026-08-13-zulu");
        assert_eq!(store.active_sweep("2026-08-13T12:00:00Z").unwrap().0, alpha);
        // And it can be pointed back at the older one.
        store.set_active_sweep("2026-08-13-zulu").unwrap();
        assert_eq!(
            store.active_sweep("2026-08-13T12:00:00Z").unwrap().0,
            "2026-08-13-zulu"
        );
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn a_marker_naming_a_deleted_sweep_falls_back_to_the_newest() {
        let store = SweepStore::new(tmp_root("active-stale"));
        store.create_sweep("alpha", "2026-08-12T10:00:00Z").unwrap();
        let (gone, _) = store.create_sweep("gone", "2026-08-13T10:00:00Z").unwrap();
        fs::remove_dir_all(store.root().join(&gone)).unwrap();
        assert_eq!(store.marked_active_sweep(), None);
        assert_eq!(
            store.active_sweep("2026-08-13T11:00:00Z").unwrap().0,
            "2026-08-12-alpha"
        );
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn a_marker_with_a_path_in_it_is_ignored() {
        let store = SweepStore::new(tmp_root("active-path"));
        store.create_sweep("alpha", "2026-08-12T10:00:00Z").unwrap();
        fs::write(store.active_marker_path(), "..\\..\\elsewhere").unwrap();
        assert_eq!(store.marked_active_sweep(), None);
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn the_marker_file_is_not_mistaken_for_a_sweep() {
        let store = SweepStore::new(tmp_root("active-list"));
        store.create_sweep("alpha", "2026-08-12T10:00:00Z").unwrap();
        assert!(store.active_marker_path().exists());
        assert_eq!(store.list_sweeps().unwrap().len(), 1);
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
        store
            .update_tag(&name, 1, "new text", "low", "copy", "helmsly")
            .unwrap();
        let sweep = store.load_sweep(&name).unwrap();
        assert_eq!(sweep.tags[0].text, "new text");
        assert_eq!(sweep.tags[0].severity, "low");
        assert_eq!(sweep.tags[0].area, "copy");
        assert_eq!(sweep.tags[0].target, "helmsly");
        let _ = fs::remove_dir_all(store.root());
    }

    #[test]
    fn update_tag_missing_number_errors() {
        let store = SweepStore::new(tmp_root("update-miss"));
        let (name, _) = store.create_sweep("s", "2026-08-13T10:00:00Z").unwrap();
        let err = store
            .update_tag(&name, 9, "t", "high", "layout", "")
            .unwrap_err();
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

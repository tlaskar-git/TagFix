// Fix list export: fixlist.md, fixlist.html (single file, images inlined as
// base64) and brief.md (agent brief). Pure rendering functions; nothing here
// touches the screen or the Tauri runtime.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;

use crate::store::{Sweep, Tag};

fn live_tags(sweep: &Sweep) -> impl Iterator<Item = &Tag> {
    sweep.tags.iter().filter(|t| !t.dropped)
}

fn first_line(text: &str) -> &str {
    let line = text.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        "untitled"
    } else {
        line
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Full evidence ledger, one section per tag, images by relative path.
pub fn render_fixlist_md(sweep: &Sweep, dir_name: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Fix list: {}\n\n", dir_name));
    out.push_str(&format!(
        "Sweep created {}. {} tags.\n\n",
        sweep.created_utc,
        live_tags(sweep).count()
    ));
    for tag in live_tags(sweep) {
        out.push_str(&format!("## Tag {:02}: {}\n\n", tag.number, first_line(&tag.text)));
        if tag.text.trim().lines().count() > 1 {
            out.push_str(tag.text.trim());
            out.push_str("\n\n");
        }
        out.push_str(&format!("- Severity: {}\n", tag.severity));
        out.push_str(&format!("- Area: {}\n", tag.area));
        out.push_str(&format!("- Captured: {}\n", tag.captured_utc));
        out.push_str(&format!(
            "- Window: {} ({})\n",
            tag.window_title, tag.process_name
        ));
        out.push_str(&format!(
            "- Monitor {} at {} scale {}, region {},{} {}x{}\n\n",
            tag.monitor_index,
            tag.screen_resolution,
            tag.dpi_scale,
            tag.region.x,
            tag.region.y,
            tag.region.width,
            tag.region.height
        ));
        out.push_str(&format!("![tag {:02}]({})\n\n", tag.number, tag.image));
    }
    out
}

/// Acceptance criterion for one task, derived from text, severity and area.
pub fn acceptance_for(tag: &Tag) -> String {
    let subject = first_line(&tag.text);
    let check = match tag.area.as_str() {
        "layout" => "elements in the captured region are aligned and sized as described",
        "copy" => "the text in the captured region reads as described",
        "a11y" => "the captured region passes the described accessibility expectation",
        "behaviour" => "the described interaction behaves as expected",
        _ => "the captured region no longer shows the described problem",
    };
    let bar = match tag.severity.as_str() {
        "high" => "Blocking: this must pass before the fix list is considered done.",
        "medium" => "Should pass in this round.",
        _ => "Nice to have; fix if the change is cheap.",
    };
    format!(
        "After the fix, {} (\"{}\"), verified against {} at DPI scale {}. {}",
        check, subject, tag.screen_resolution, tag.dpi_scale, bar
    )
}

/// Agent brief: scope plus one task per tag with an acceptance criterion.
pub fn render_brief_md(sweep: &Sweep, dir_name: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Agent brief: {}\n\n", dir_name));

    let mut apps: Vec<String> = live_tags(sweep)
        .map(|t| {
            if t.window_title.is_empty() {
                t.process_name.clone()
            } else {
                format!("{} ({})", t.window_title, t.process_name)
            }
        })
        .collect();
    apps.sort();
    apps.dedup();

    out.push_str("## Scope\n\n");
    out.push_str(
        "Fix the issues listed below. Evidence images live next to this file; \
         fixlist.md holds the full ledger. Do not widen scope beyond these tasks.\n\n",
    );
    if !apps.is_empty() {
        out.push_str("Surfaces under test:\n\n");
        for a in &apps {
            out.push_str(&format!("- {}\n", a));
        }
        out.push('\n');
    }

    out.push_str("## Tasks\n\n");
    for (i, tag) in live_tags(sweep).enumerate() {
        out.push_str(&format!(
            "### Task {}: {}\n\n",
            i + 1,
            first_line(&tag.text)
        ));
        if tag.text.trim().lines().count() > 1 {
            out.push_str(tag.text.trim());
            out.push_str("\n\n");
        }
        out.push_str(&format!(
            "- Severity: {} / Area: {}\n- Evidence: {} (region {},{} {}x{})\n",
            tag.severity,
            tag.area,
            tag.image,
            tag.region.x,
            tag.region.y,
            tag.region.width,
            tag.region.height
        ));
        out.push_str(&format!("- Acceptance: {}\n\n", acceptance_for(tag)));
    }
    out
}

/// Single-file HTML: images inlined as base64 data URIs, zero external
/// requests. `load_image` returns the PNG bytes for a tag image name.
pub fn render_fixlist_html<F>(sweep: &Sweep, dir_name: &str, load_image: F) -> String
where
    F: Fn(&str) -> Option<Vec<u8>>,
{
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str(&format!("<title>Fix list: {}</title>\n", escape_html(dir_name)));
    out.push_str("<style>\n");
    out.push_str(
        "body{font-family:Segoe UI,sans-serif;max-width:900px;margin:2rem auto;\
         padding:0 1rem;color:#1c1c1f;background:#fff}\n\
         img{max-width:100%;border:1px solid #ccc;border-radius:4px}\n\
         .meta{color:#555;font-size:0.9rem}\n\
         .sev-high{color:#b00020;font-weight:600}\n\
         .sev-medium{color:#a15c00;font-weight:600}\n\
         .sev-low{color:#2e6e2e;font-weight:600}\n\
         section{border-top:1px solid #ddd;margin-top:2rem;padding-top:1rem}\n",
    );
    out.push_str("</style>\n</head>\n<body>\n");
    out.push_str(&format!("<h1>Fix list: {}</h1>\n", escape_html(dir_name)));
    out.push_str(&format!(
        "<p class=\"meta\">Sweep created {}. {} tags.</p>\n",
        escape_html(&sweep.created_utc),
        live_tags(sweep).count()
    ));

    for tag in live_tags(sweep) {
        out.push_str("<section>\n");
        out.push_str(&format!(
            "<h2>Tag {:02}: {}</h2>\n",
            tag.number,
            escape_html(first_line(&tag.text))
        ));
        if tag.text.trim().lines().count() > 1 {
            out.push_str(&format!(
                "<p>{}</p>\n",
                escape_html(tag.text.trim()).replace('\n', "<br>")
            ));
        }
        out.push_str(&format!(
            "<p class=\"meta\"><span class=\"sev-{}\">{}</span> / {} <br>Captured {} from {} ({})<br>Monitor {} at {} scale {}, region {},{} {}x{}</p>\n",
            escape_html(&tag.severity),
            escape_html(&tag.severity),
            escape_html(&tag.area),
            escape_html(&tag.captured_utc),
            escape_html(&tag.window_title),
            escape_html(&tag.process_name),
            tag.monitor_index,
            escape_html(&tag.screen_resolution),
            tag.dpi_scale,
            tag.region.x,
            tag.region.y,
            tag.region.width,
            tag.region.height
        ));
        match load_image(&tag.image) {
            Some(bytes) => {
                out.push_str(&format!(
                    "<img alt=\"tag {:02}\" src=\"data:image/png;base64,{}\">\n",
                    tag.number,
                    B64.encode(&bytes)
                ));
            }
            None => {
                out.push_str(&format!(
                    "<p class=\"meta\">image {} missing at export time</p>\n",
                    escape_html(&tag.image)
                ));
            }
        }
        out.push_str("</section>\n");
    }
    out.push_str("</body>\n</html>\n");
    out
}

/// Write fixlist.md, fixlist.html and brief.md into the sweep folder.
/// Returns the one line pointer for the clipboard.
pub fn export_sweep_files(root: &std::path::Path, dir_name: &str) -> std::io::Result<String> {
    let store = crate::store::SweepStore::new(root.to_path_buf());
    let sweep = store.load_sweep(dir_name)?;
    let dir = root.join(dir_name);
    std::fs::write(dir.join("fixlist.md"), render_fixlist_md(&sweep, dir_name))?;
    let html = render_fixlist_html(&sweep, dir_name, |img| std::fs::read(dir.join(img)).ok());
    std::fs::write(dir.join("fixlist.html"), html)?;
    std::fs::write(dir.join("brief.md"), render_brief_md(&sweep, dir_name))?;
    Ok(format!(
        "TagFix agent brief: {}",
        dir.join("brief.md").display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Rect, Sweep};

    fn tag(number: u32, text: &str, severity: &str, area: &str, dropped: bool) -> Tag {
        Tag {
            number,
            image: crate::store::tag_image_name(number),
            captured_utc: "2026-08-13T10:00:00Z".into(),
            monitor_index: 1,
            dpi_scale: 1.5,
            region: Rect { x: 100, y: 200, width: 300, height: 150 },
            window_title: "Helmsly - Settings".into(),
            process_name: "helmsly.exe".into(),
            screen_resolution: "2496x1664".into(),
            text: text.into(),
            severity: severity.into(),
            area: area.into(),
            dropped,
        }
    }

    fn sweep_with(tags: Vec<Tag>) -> Sweep {
        let mut s = Sweep::new("demo", "2026-08-13T09:00:00Z");
        s.tags = tags;
        s
    }

    #[test]
    fn fixlist_md_has_section_per_live_tag() {
        let s = sweep_with(vec![
            tag(1, "button clipped", "high", "layout", false),
            tag(2, "typo in header", "low", "copy", false),
        ]);
        let md = render_fixlist_md(&s, "2026-08-13-demo");
        assert!(md.contains("## Tag 01: button clipped"));
        assert!(md.contains("## Tag 02: typo in header"));
        assert!(md.contains("# Fix list: 2026-08-13-demo"));
    }

    #[test]
    fn fixlist_md_references_images_by_relative_path() {
        let s = sweep_with(vec![tag(3, "x", "medium", "other", false)]);
        let md = render_fixlist_md(&s, "d");
        assert!(md.contains("![tag 03](tag-03.png)"));
        assert!(!md.contains("data:image"));
    }

    #[test]
    fn fixlist_md_skips_dropped_tags() {
        let s = sweep_with(vec![
            tag(1, "keep me", "high", "layout", false),
            tag(2, "dropped one", "low", "copy", true),
        ]);
        let md = render_fixlist_md(&s, "d");
        assert!(md.contains("keep me"));
        assert!(!md.contains("dropped one"));
        assert!(md.contains("1 tags"));
    }

    #[test]
    fn fixlist_md_records_capture_metadata() {
        let s = sweep_with(vec![tag(1, "x", "high", "layout", false)]);
        let md = render_fixlist_md(&s, "d");
        assert!(md.contains("Monitor 1 at 2496x1664 scale 1.5"));
        assert!(md.contains("region 100,200 300x150"));
        assert!(md.contains("Helmsly - Settings (helmsly.exe)"));
    }

    #[test]
    fn brief_has_task_per_live_tag_with_acceptance() {
        let s = sweep_with(vec![
            tag(1, "button clipped", "high", "layout", false),
            tag(2, "typo", "low", "copy", false),
            tag(3, "gone", "low", "copy", true),
        ]);
        let brief = render_brief_md(&s, "2026-08-13-demo");
        assert!(brief.contains("### Task 1: button clipped"));
        assert!(brief.contains("### Task 2: typo"));
        assert!(!brief.contains("Task 3"));
        assert_eq!(brief.matches("- Acceptance:").count(), 2);
    }

    #[test]
    fn brief_scope_lists_surfaces_once() {
        let s = sweep_with(vec![
            tag(1, "a", "high", "layout", false),
            tag(2, "b", "low", "copy", false),
        ]);
        let brief = render_brief_md(&s, "d");
        assert_eq!(
            brief.matches("Helmsly - Settings (helmsly.exe)").count(),
            1
        );
    }

    #[test]
    fn acceptance_reflects_severity_and_area() {
        let high_layout = acceptance_for(&tag(1, "button clipped", "high", "layout", false));
        assert!(high_layout.contains("aligned"));
        assert!(high_layout.contains("Blocking"));
        assert!(high_layout.contains("button clipped"));

        let low_copy = acceptance_for(&tag(2, "typo", "low", "copy", false));
        assert!(low_copy.contains("reads as described"));
        assert!(low_copy.contains("Nice to have"));
    }

    #[test]
    fn acceptance_handles_empty_text() {
        let a = acceptance_for(&tag(1, "", "medium", "other", false));
        assert!(a.contains("untitled"));
    }

    #[test]
    fn html_inlines_images_as_base64() {
        let s = sweep_with(vec![tag(1, "x", "high", "layout", false)]);
        let html = render_fixlist_html(&s, "d", |_| Some(vec![1, 2, 3, 4]));
        assert!(html.contains("data:image/png;base64,AQIDBA=="));
        assert!(!html.contains("src=\"tag-01.png\""));
    }

    #[test]
    fn html_makes_no_external_requests() {
        let s = sweep_with(vec![
            tag(1, "x", "high", "layout", false),
            tag(2, "y", "low", "copy", false),
        ]);
        let html = render_fixlist_html(&s, "d", |_| Some(vec![9, 9]));
        for needle in ["http://", "https://", "src=\"//", "@import", "url("] {
            assert!(!html.contains(needle), "found {}", needle);
        }
    }

    #[test]
    fn html_escapes_user_text() {
        let s = sweep_with(vec![tag(1, "<script>alert(1)</script>", "high", "layout", false)]);
        let html = render_fixlist_html(&s, "d", |_| None);
        assert!(!html.contains("<script>alert"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn html_notes_missing_images() {
        let s = sweep_with(vec![tag(1, "x", "high", "layout", false)]);
        let html = render_fixlist_html(&s, "d", |_| None);
        assert!(html.contains("missing at export time"));
    }

    #[test]
    fn html_skips_dropped_tags() {
        let s = sweep_with(vec![
            tag(1, "visible", "high", "layout", false),
            tag(2, "hidden entry", "low", "copy", true),
        ]);
        let html = render_fixlist_html(&s, "d", |_| Some(vec![1]));
        assert!(html.contains("visible"));
        assert!(!html.contains("hidden entry"));
    }

    #[test]
    fn five_tag_sweep_exports_all_three_files() {
        use crate::store::SweepStore;
        let root = std::env::temp_dir().join(format!("tagfix-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = SweepStore::new(root.clone());
        let (name, _) = store.create_sweep("five", "2026-08-13T10:00:00Z").unwrap();
        for n in 1..=5 {
            let t = tag(n, &format!("issue {}", n), "medium", "layout", false);
            std::fs::write(
                root.join(&name).join(&t.image),
                [137, 80, 78, 71, n as u8],
            )
            .unwrap();
            store.append_tag(&name, t).unwrap();
        }

        let pointer = export_sweep_files(&root, &name).unwrap();
        assert!(pointer.contains("brief.md"));
        for f in ["fixlist.md", "fixlist.html", "brief.md"] {
            assert!(root.join(&name).join(f).exists(), "{} missing", f);
        }
        let html = std::fs::read_to_string(root.join(&name).join("fixlist.html")).unwrap();
        assert_eq!(html.matches("data:image/png;base64,").count(), 5);
        let brief = std::fs::read_to_string(root.join(&name).join("brief.md")).unwrap();
        assert_eq!(brief.matches("### Task").count(), 5);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn exports_contain_no_em_or_en_dashes() {
        let s = sweep_with(vec![tag(1, "plain text", "high", "layout", false)]);
        let all = format!(
            "{}{}{}",
            render_fixlist_md(&s, "d"),
            render_brief_md(&s, "d"),
            render_fixlist_html(&s, "d", |_| Some(vec![1]))
        );
        assert!(!all.contains('\u{2014}'));
        assert!(!all.contains('\u{2013}'));
    }
}

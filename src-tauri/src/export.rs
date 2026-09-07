// Fix list export: fixlist.md, fixlist.html (single file, images inlined as
// base64), brief.md (agent brief) and the feedback sheet (feedback.md and
// feedback.txt) that goes straight into a chat window. Pure rendering
// functions; nothing here touches the screen or the Tauri runtime.

use std::collections::BTreeSet;
use std::path::Path;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde::Serialize;

use crate::settings::Target;
use crate::store::{Sweep, Tag, LABEL_AFTER};

/// The five renderings the review window can copy, open or save.
pub const EXPORT_FILES: [&str; 5] = [
    "fixlist.md",
    "fixlist.html",
    "brief.md",
    "feedback.md",
    "feedback.txt",
];

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

/// Chips are free text on disk, so an empty one has to read as something.
fn or_unset(value: &str) -> &str {
    let v = value.trim();
    if v.is_empty() {
        "unset"
    } else {
        v
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The target worth printing, if any. Both an empty target and the literal
/// "other" mean "not filed under a product", and the popover writes the
/// literal one, so the two have to read the same everywhere.
fn target_label(tag: &Tag) -> Option<&str> {
    let name = tag.target.trim();
    if name.is_empty() || name == UNTARGETED {
        None
    } else {
        Some(name)
    }
}

/// The "after" attachment of a carried tag, when one has been captured.
fn after_image(tag: &Tag) -> Option<&str> {
    tag.attachments
        .iter()
        .find(|a| a.label == LABEL_AFTER)
        .map(|a| a.image.as_str())
}

/// The copied original crop of a carried tag.
fn before_image(tag: &Tag) -> Option<&str> {
    tag.carried_from
        .as_ref()
        .and_then(|c| c.image.as_deref())
        .filter(|s| !s.is_empty())
}

/// "Re-report of 2026-09-01-round-97 tag 04", the one line that tells a
/// reader this is not a fresh finding.
fn re_report_line(tag: &Tag) -> Option<String> {
    tag.carried_from
        .as_ref()
        .map(|c| format!("Re-report of {} tag {:02}", c.sweep, c.number))
}

/// Every image file a tag points at, in the order a reader meets them.
/// Used by the per target export copies, which have to carry the pixels
/// as well as the words.
pub fn images_referenced(tag: &Tag) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(image) = tag.image.as_ref().filter(|s| !s.is_empty()) {
        out.push(image.clone());
    }
    if let Some(context) = tag.context_image.as_ref().filter(|s| !s.is_empty()) {
        out.push(context.clone());
    }
    if let Some(before) = before_image(tag) {
        out.push(before.to_string());
    }
    for a in &tag.attachments {
        if !a.image.is_empty() {
            out.push(a.image.clone());
        }
    }
    out
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
        // Schema v2 makes the crop optional; a quote tag has no rectangle.
        let region = tag.region_or_zero();
        out.push_str(&format!("## Tag {:02}: {}\n\n", tag.number, first_line(&tag.text)));
        if tag.text.trim().lines().count() > 1 {
            out.push_str(tag.text.trim());
            out.push_str("\n\n");
        }
        out.push_str(&format!("- Severity: {}\n", tag.severity));
        out.push_str(&format!("- Area: {}\n", tag.area));
        if let Some(name) = target_label(tag) {
            out.push_str(&format!("- Target: {}\n", name));
        }
        out.push_str(&format!("- Captured: {}\n", tag.captured_utc));
        out.push_str(&format!(
            "- Window: {} ({})\n",
            tag.window_title, tag.process_name
        ));
        if !tag.url.trim().is_empty() {
            out.push_str(&format!("- URL: {}\n", tag.url.trim()));
        }
        if !tag.element.trim().is_empty() {
            out.push_str(&format!("- Element: {}\n", tag.element.trim()));
        }
        out.push_str(&format!(
            "- Monitor {} at {} scale {}, region {},{} {}x{}\n\n",
            tag.monitor_index,
            tag.screen_resolution,
            tag.dpi_scale,
            region.x,
            region.y,
            region.width,
            region.height
        ));

        if let Some(line) = re_report_line(tag) {
            out.push_str(&line);
            out.push_str("\n\n");
        }

        // A quote tag carries words where a region tag carries pixels.
        if tag.is_quote() && !tag.quote.trim().is_empty() {
            for line in tag.quote.trim().lines() {
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
            out.push('\n');
        }

        if !tag.image_name().is_empty() {
            out.push_str(&format!("![tag {:02}]({})\n\n", tag.number, tag.image_name()));
        }
        if let Some(context) = tag.context_image.as_ref().filter(|s| !s.is_empty()) {
            // The context frame is the wider shot; in Markdown a second
            // image line is all "reduced width" can mean.
            out.push_str(&format!("![context {:02}]({})\n\n", tag.number, context));
        }

        // Carried tags put before and after next to each other. Everything
        // else lists its attachments with their labels.
        if tag.carried_from.is_some() {
            let mut line = String::new();
            if let Some(before) = before_image(tag) {
                line.push_str(&format!("![before]({})", before));
            }
            if let Some(after) = after_image(tag) {
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(&format!("![after]({})", after));
            }
            if !line.is_empty() {
                out.push_str(&line);
                out.push_str("\n\n");
            }
            for a in tag.attachments.iter().filter(|a| a.label != LABEL_AFTER) {
                out.push_str(&format!("![{}]({})\n", a.label, a.image));
            }
            if tag.attachments.iter().any(|a| a.label != LABEL_AFTER) {
                out.push('\n');
            }
        } else if !tag.attachments.is_empty() {
            for a in &tag.attachments {
                out.push_str(&format!("![{}]({})\n", a.label, a.image));
            }
            out.push('\n');
        }
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

/// The evidence bullet of a brief task.
fn brief_evidence(tag: &Tag) -> String {
    // A carried tag's evidence is the pair of shots, whatever its kind.
    if tag.carried_from.is_some() {
        return if after_image(tag).is_some() {
            "Re-report; before and after images attached".to_string()
        } else {
            "Re-report; before image attached".to_string()
        };
    }
    if tag.is_quote() {
        return "quote".to_string();
    }
    let region = tag.region_or_zero();
    format!(
        "{} (region {},{} {}x{})",
        tag.image_name(),
        region.x,
        region.y,
        region.width,
        region.height
    )
}

/// One task block of the brief. `index` is the global task number, which
/// stays in sweep order however the tasks are grouped.
fn brief_task(out: &mut String, index: usize, tag: &Tag) {
    out.push_str(&format!("### Task {}: {}\n\n", index, first_line(&tag.text)));
    if tag.text.trim().lines().count() > 1 {
        out.push_str(tag.text.trim());
        out.push_str("\n\n");
    }
    if tag.is_quote() && !tag.quote.trim().is_empty() {
        for line in tag.quote.trim().lines() {
            out.push_str("> ");
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n');
    }
    let chips = match target_label(tag) {
        Some(name) => format!(
            "- Severity: {} / Area: {} / Target: {}\n",
            tag.severity, tag.area, name
        ),
        None => format!("- Severity: {} / Area: {}\n", tag.severity, tag.area),
    };
    out.push_str(&chips);
    out.push_str(&format!("- Evidence: {}\n", brief_evidence(tag)));
    if !tag.url.trim().is_empty() {
        out.push_str(&format!("- URL: {}\n", tag.url.trim()));
    }
    if !tag.element.trim().is_empty() {
        out.push_str(&format!("- Element: {}\n", tag.element.trim()));
    }
    out.push_str(&format!("- Acceptance: {}\n\n", acceptance_for(tag)));
}

/// The name a task group is filed under. An empty target is "other" so a
/// half targeted sweep still lands every task somewhere.
const UNTARGETED: &str = "other";

/// Target headings in sweep order of first appearance, with "other" last.
fn target_order(sweep: &Sweep) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut has_untargeted = false;
    for tag in live_tags(sweep) {
        match target_label(tag) {
            None => has_untargeted = true,
            Some(name) => {
                if !out.iter().any(|n| n == name) {
                    out.push(name.to_string());
                }
            }
        }
    }
    if has_untargeted {
        out.push(UNTARGETED.to_string());
    }
    out
}

/// Agent brief: scope plus one task per tag with an acceptance criterion.
/// Tasks are grouped by target as soon as any live tag names one.
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

    // Task numbers are global and follow sweep order, so a task keeps its
    // number whether or not the sweep happens to be grouped.
    let numbered: Vec<(usize, &Tag)> = live_tags(sweep).enumerate().map(|(i, t)| (i + 1, t)).collect();
    let grouped = numbered.iter().any(|(_, t)| target_label(t).is_some());

    if !grouped {
        out.push_str("## Tasks\n\n");
        for (index, tag) in &numbered {
            brief_task(&mut out, *index, tag);
        }
        return out;
    }

    for group in target_order(sweep) {
        out.push_str(&format!("## Target: {}\n\n", group));
        for (index, tag) in &numbered {
            let belongs = match target_label(tag) {
                None => group == UNTARGETED,
                Some(name) => name == group,
            };
            if belongs {
                brief_task(&mut out, *index, tag);
            }
        }
    }
    out
}

/// Where the notes were taken, deduplicated. A tag with a URL names the
/// page; anything else names the window.
fn sources(sweep: &Sweep) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in live_tags(sweep) {
        let subject = if !tag.url.trim().is_empty() {
            tag.url.trim()
        } else {
            tag.window_title.trim()
        };
        let process = tag.process_name.trim();
        let line = if subject.is_empty() {
            process.to_string()
        } else if process.is_empty() {
            subject.to_string()
        } else {
            format!("{} ({})", subject, process)
        };
        if line.is_empty() {
            continue;
        }
        if !out.iter().any(|l| l == &line) {
            out.push(line);
        }
    }
    out
}

/// "(screenshot tag-02.png, button 'Deploy')": how a reader of the sheet
/// finds the pixels for a note that is not a quote.
fn screenshot_clause(tag: &Tag) -> String {
    if tag.is_quote() {
        return String::new();
    }
    let image = if !tag.image_name().is_empty() {
        tag.image_name().to_string()
    } else {
        before_image(tag).unwrap_or("").to_string()
    };
    if image.is_empty() {
        return String::new();
    }
    let element = tag.element.trim();
    if element.is_empty() {
        format!(" (screenshot {})", image)
    } else {
        format!(" (screenshot {}, {})", image, element)
    }
}

fn feedback_header(sweep: &Sweep, dir_name: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Feedback on {} ({} notes)\n",
        dir_name,
        live_tags(sweep).count()
    ));
    let sources = sources(sweep);
    if !sources.is_empty() {
        out.push_str(&format!("Sources: {}\n", sources.join("; ")));
    }
    out.push('\n');
    out
}

/// The chat ready sheet: numbered notes, quotes as blockquotes, region tags
/// pointing at their PNG. Markdown flavour.
pub fn render_feedback_md(sweep: &Sweep, dir_name: &str) -> String {
    let mut out = feedback_header(sweep, dir_name);
    for (i, tag) in live_tags(sweep).enumerate() {
        let chips = match target_label(tag) {
            Some(name) => format!(
                "[{} / {} / {}]",
                or_unset(&tag.severity),
                or_unset(&tag.area),
                name
            ),
            None => format!("[{} / {}]", or_unset(&tag.severity), or_unset(&tag.area)),
        };
        out.push_str(&format!(
            "{}. (tag {:02}) {}{}\n",
            i + 1,
            tag.number,
            chips,
            screenshot_clause(tag)
        ));
        if let Some(line) = re_report_line(tag) {
            out.push_str(&line);
            out.push('\n');
        }
        if !tag.quote.trim().is_empty() {
            for line in tag.quote.trim().lines() {
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
        }
        if !tag.text.trim().is_empty() {
            out.push('\n');
            out.push_str(tag.text.trim());
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

/// The same sheet for a window that does not render Markdown: no ">" and no
/// "[ ]", indented quotes and a spelled out chip line.
pub fn render_feedback_txt(sweep: &Sweep, dir_name: &str) -> String {
    let mut out = feedback_header(sweep, dir_name);
    for (i, tag) in live_tags(sweep).enumerate() {
        out.push_str(&format!(
            "{}. (tag {:02}){}\n",
            i + 1,
            tag.number,
            screenshot_clause(tag)
        ));
        let mut chips = format!(
            "Severity: {}, area: {}",
            or_unset(&tag.severity),
            or_unset(&tag.area)
        );
        if let Some(name) = target_label(tag) {
            chips.push_str(&format!(", target: {}", name));
        }
        out.push_str(&chips);
        out.push('\n');
        if let Some(line) = re_report_line(tag) {
            out.push_str(&line);
            out.push('\n');
        }
        if !tag.quote.trim().is_empty() {
            for line in tag.quote.trim().lines() {
                out.push_str("    ");
                out.push_str(line);
                out.push('\n');
            }
        }
        if !tag.text.trim().is_empty() {
            out.push('\n');
            out.push_str(tag.text.trim());
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

/// One inlined image, or a note saying the file was gone at export time.
fn html_image<F>(out: &mut String, alt: &str, name: &str, class: &str, load_image: &F)
where
    F: Fn(&str) -> Option<Vec<u8>>,
{
    let class_attr = if class.is_empty() {
        String::new()
    } else {
        format!(" class=\"{}\"", class)
    };
    match load_image(name) {
        Some(bytes) => out.push_str(&format!(
            "<img{} alt=\"{}\" src=\"data:image/png;base64,{}\">\n",
            class_attr,
            escape_html(alt),
            B64.encode(&bytes)
        )),
        None => out.push_str(&format!(
            "<p class=\"meta\">image {} missing at export time</p>\n",
            escape_html(name)
        )),
    }
}

/// A labelled figure inside a side by side row.
fn html_figure<F>(out: &mut String, label: &str, name: &str, load_image: &F)
where
    F: Fn(&str) -> Option<Vec<u8>>,
{
    out.push_str("<figure>\n");
    html_image(out, label, name, "", load_image);
    out.push_str(&format!("<figcaption>{}</figcaption>\n", escape_html(label)));
    out.push_str("</figure>\n");
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
         section{border-top:1px solid #ddd;margin-top:2rem;padding-top:1rem}\n\
         blockquote.quote{margin:1rem 0;padding:0.6rem 1rem;border-left:4px solid #9aa0a6;\
         background:#f4f4f6;white-space:pre-wrap;font-family:Consolas,monospace;font-size:0.92rem}\n\
         img.context{max-width:60%}\n\
         .side-by-side{display:flex;flex-wrap:wrap;gap:12px;align-items:flex-start}\n\
         .side-by-side figure{margin:0;flex:1 1 300px}\n\
         .side-by-side figcaption{color:#555;font-size:0.85rem;margin-top:4px}\n\
         .re-report{color:#555;font-size:0.9rem;font-style:italic}\n",
    );
    out.push_str("</style>\n</head>\n<body>\n");
    out.push_str(&format!("<h1>Fix list: {}</h1>\n", escape_html(dir_name)));
    out.push_str(&format!(
        "<p class=\"meta\">Sweep created {}. {} tags.</p>\n",
        escape_html(&sweep.created_utc),
        live_tags(sweep).count()
    ));

    for tag in live_tags(sweep) {
        // Schema v2 makes the crop optional; a quote tag has no rectangle.
        let region = tag.region_or_zero();
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
        let target_bit = match target_label(tag) {
            Some(name) => format!(" / {}", escape_html(name)),
            None => String::new(),
        };
        out.push_str(&format!(
            "<p class=\"meta\"><span class=\"sev-{}\">{}</span> / {}{} <br>Captured {} from {} ({})<br>Monitor {} at {} scale {}, region {},{} {}x{}",
            escape_html(&tag.severity),
            escape_html(&tag.severity),
            escape_html(&tag.area),
            target_bit,
            escape_html(&tag.captured_utc),
            escape_html(&tag.window_title),
            escape_html(&tag.process_name),
            tag.monitor_index,
            escape_html(&tag.screen_resolution),
            tag.dpi_scale,
            region.x,
            region.y,
            region.width,
            region.height
        ));
        if !tag.url.trim().is_empty() {
            out.push_str(&format!("<br>URL: {}", escape_html(tag.url.trim())));
        }
        if !tag.element.trim().is_empty() {
            out.push_str(&format!("<br>Element: {}", escape_html(tag.element.trim())));
        }
        out.push_str("</p>\n");

        if let Some(line) = re_report_line(tag) {
            out.push_str(&format!("<p class=\"re-report\">{}</p>\n", escape_html(&line)));
        }

        if tag.is_quote() && !tag.quote.trim().is_empty() {
            out.push_str(&format!(
                "<blockquote class=\"quote\">{}</blockquote>\n",
                escape_html(tag.quote.trim())
            ));
        }

        if !tag.image_name().is_empty() {
            html_image(
                &mut out,
                &format!("tag {:02}", tag.number),
                tag.image_name(),
                "",
                &load_image,
            );
        } else if !tag.is_quote() && tag.carried_from.is_none() {
            // A region tag with no crop on disk: say so rather than hide it.
            out.push_str("<p class=\"meta\">image  missing at export time</p>\n");
        }

        if let Some(context) = tag.context_image.as_ref().filter(|s| !s.is_empty()) {
            html_image(
                &mut out,
                &format!("context {:02}", tag.number),
                context,
                "context",
                &load_image,
            );
        }

        if tag.carried_from.is_some() {
            let before = before_image(tag);
            let after = after_image(tag);
            if before.is_some() || after.is_some() {
                out.push_str("<div class=\"side-by-side\">\n");
                if let Some(name) = before {
                    html_figure(&mut out, "before", name, &load_image);
                }
                if let Some(name) = after {
                    html_figure(&mut out, "after", name, &load_image);
                }
                out.push_str("</div>\n");
            }
            let others: Vec<&crate::store::Attachment> = tag
                .attachments
                .iter()
                .filter(|a| a.label != LABEL_AFTER)
                .collect();
            if !others.is_empty() {
                out.push_str("<div class=\"side-by-side\">\n");
                for a in others {
                    html_figure(&mut out, &a.label, &a.image, &load_image);
                }
                out.push_str("</div>\n");
            }
        } else if !tag.attachments.is_empty() {
            out.push_str("<div class=\"side-by-side\">\n");
            for a in &tag.attachments {
                html_figure(&mut out, &a.label, &a.image, &load_image);
            }
            out.push_str("</div>\n");
        }

        out.push_str("</section>\n");
    }
    out.push_str("</body>\n</html>\n");
    out
}

/// Any of the five renderings by file name, so the review window can ask
/// for one without a match statement of its own. None for a name that is
/// not an export.
pub fn render_named<F>(
    sweep: &Sweep,
    dir_name: &str,
    file_name: &str,
    load_image: F,
) -> Option<String>
where
    F: Fn(&str) -> Option<Vec<u8>>,
{
    match file_name {
        "fixlist.md" => Some(render_fixlist_md(sweep, dir_name)),
        "fixlist.html" => Some(render_fixlist_html(sweep, dir_name, load_image)),
        "brief.md" => Some(render_brief_md(sweep, dir_name)),
        "feedback.md" => Some(render_feedback_md(sweep, dir_name)),
        "feedback.txt" => Some(render_feedback_txt(sweep, dir_name)),
        _ => None,
    }
}

/// A copy of the sweep holding only the live tags of one target. The
/// untargeted name collects tags with no target at all.
pub fn sweep_for_target(sweep: &Sweep, target: &str) -> Sweep {
    let mut out = sweep.clone();
    out.tags = sweep
        .tags
        .iter()
        .filter(|t| !t.dropped)
        .filter(|t| match target_label(t) {
            None => target == UNTARGETED,
            Some(name) => name == target,
        })
        .cloned()
        .collect();
    out
}

/// What an export wrote: the clipboard pointer and every target folder that
/// received a copy, so the review window can name them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub pointer: String,
    pub target_dirs: Vec<String>,
}

/// Write the five renderings into the sweep folder, then a filtered copy
/// into every configured target export directory that has tags.
pub fn export_sweep_files(
    root: &Path,
    dir_name: &str,
    targets: &[Target],
) -> std::io::Result<ExportResult> {
    let store = crate::store::SweepStore::new(root.to_path_buf());
    let sweep = store.load_sweep(dir_name)?;
    let dir = root.join(dir_name);
    let load = |name: &str| std::fs::read(dir.join(name)).ok();

    std::fs::write(dir.join("fixlist.md"), render_fixlist_md(&sweep, dir_name))?;
    std::fs::write(
        dir.join("fixlist.html"),
        render_fixlist_html(&sweep, dir_name, &load),
    )?;
    std::fs::write(dir.join("brief.md"), render_brief_md(&sweep, dir_name))?;
    std::fs::write(
        dir.join("feedback.md"),
        render_feedback_md(&sweep, dir_name),
    )?;
    std::fs::write(
        dir.join("feedback.txt"),
        render_feedback_txt(&sweep, dir_name),
    )?;

    let mut target_dirs: Vec<String> = Vec::new();
    for target in targets {
        let Some(export_dir) = target.export_dir.as_ref().filter(|d| !d.trim().is_empty()) else {
            continue;
        };
        let filtered = sweep_for_target(&sweep, target.name.trim());
        if filtered.tags.is_empty() {
            continue;
        }
        let out_dir = Path::new(export_dir.trim()).join(dir_name);
        std::fs::create_dir_all(&out_dir)?;
        std::fs::write(
            out_dir.join("fixlist.md"),
            render_fixlist_md(&filtered, dir_name),
        )?;
        std::fs::write(
            out_dir.join("brief.md"),
            render_brief_md(&filtered, dir_name),
        )?;
        std::fs::write(
            out_dir.join("feedback.md"),
            render_feedback_md(&filtered, dir_name),
        )?;
        // The words are useless without the pixels they point at. A file
        // that has already been removed is skipped, not fatal.
        let mut wanted: BTreeSet<String> = BTreeSet::new();
        for tag in &filtered.tags {
            for image in images_referenced(tag) {
                wanted.insert(image);
            }
        }
        for image in wanted {
            let src = dir.join(&image);
            if src.exists() {
                std::fs::copy(&src, out_dir.join(&image))?;
            }
        }
        target_dirs.push(out_dir.display().to_string());
    }

    Ok(ExportResult {
        pointer: format!("TagFix agent brief: {}", dir.join("brief.md").display()),
        target_dirs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Attachment, CarriedFrom, Rect, Sweep, KIND_QUOTE, LABEL_COMPARE};

    fn tag(number: u32, text: &str, severity: &str, area: &str, dropped: bool) -> Tag {
        Tag {
            number,
            image: Some(crate::store::tag_image_name(number)),
            captured_utc: "2026-08-13T10:00:00Z".into(),
            monitor_index: 1,
            dpi_scale: 1.5,
            region: Some(Rect { x: 100, y: 200, width: 300, height: 150 }),
            window_title: "Helmsly - Settings".into(),
            process_name: "helmsly.exe".into(),
            screen_resolution: "2496x1664".into(),
            text: text.into(),
            severity: severity.into(),
            area: area.into(),
            dropped,
            ..Tag::default()
        }
    }

    fn quote_tag(number: u32, quote: &str, text: &str) -> Tag {
        Tag {
            kind: KIND_QUOTE.into(),
            image: None,
            region: None,
            quote: quote.into(),
            ..tag(number, text, "high", "copy", false)
        }
    }

    fn attachment(image: &str, label: &str) -> Attachment {
        Attachment {
            image: image.into(),
            region: Rect { x: 0, y: 0, width: 10, height: 10 },
            captured_utc: "2026-08-13T11:00:00Z".into(),
            label: label.into(),
        }
    }

    fn carried_tag(number: u32, text: &str, with_after: bool) -> Tag {
        let mut t = tag(number, text, "medium", "layout", false);
        t.image = None;
        t.carried_from = Some(CarriedFrom {
            sweep: "2026-08-01-round-97".into(),
            number: 4,
            image: Some(crate::store::tag_before_image_name(number)),
            text: text.into(),
        });
        if with_after {
            t.attachments
                .push(attachment(&crate::store::tag_attachment_name(number, 1), "after"));
        }
        t
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

    // Round 02: quote tags, side by side evidence, carried tags.

    #[test]
    fn quote_tag_renders_as_a_blockquote_in_md() {
        let s = sweep_with(vec![quote_tag(
            1,
            "the save button sits below the fold\non a phone",
            "wrong, it is pinned to the footer",
        )]);
        let md = render_fixlist_md(&s, "d");
        assert!(md.contains("> the save button sits below the fold\n> on a phone\n"));
        // No pixels: a quote tag has no crop to point at.
        assert!(!md.contains("![tag 01]"));
    }

    #[test]
    fn quote_tag_renders_as_a_styled_blockquote_in_html() {
        let s = sweep_with(vec![quote_tag(1, "the save button sits below the fold", "wrong")]);
        let html = render_fixlist_html(&s, "d", |_| Some(vec![1]));
        assert!(html.contains("<blockquote class=\"quote\">the save button sits below the fold</blockquote>"));
        assert!(html.contains("blockquote.quote{"));
        assert!(!html.contains("<img"));
    }

    #[test]
    fn region_tag_renders_crop_then_context_then_attachments() {
        let mut t = tag(2, "misaligned", "medium", "layout", false);
        t.context_image = Some(crate::store::tag_context_image_name(2));
        t.attachments.push(attachment("tag-02-a1.png", LABEL_COMPARE));
        let s = sweep_with(vec![t]);

        let md = render_fixlist_md(&s, "d");
        let crop = md.find("![tag 02](tag-02.png)").unwrap();
        let context = md.find("![context 02](tag-02-context.png)").unwrap();
        let compare = md.find("![compare](tag-02-a1.png)").unwrap();
        assert!(crop < context && context < compare);

        let html = render_fixlist_html(&s, "d", |_| Some(vec![7]));
        assert!(html.contains("img.context{max-width:60%}"));
        assert!(html.contains("class=\"context\""));
        assert!(html.contains("<div class=\"side-by-side\">"));
        assert!(html.contains("<figcaption>compare</figcaption>"));
        assert!(html.contains(".side-by-side{display:flex"));
    }

    #[test]
    fn carried_tag_renders_before_and_after_side_by_side() {
        let s = sweep_with(vec![carried_tag(3, "still broken", true)]);
        let md = render_fixlist_md(&s, "d");
        assert!(md.contains("Re-report of 2026-08-01-round-97 tag 04"));
        assert!(md.contains("![before](tag-03-before.png) ![after](tag-03-a1.png)"));

        let html = render_fixlist_html(&s, "d", |_| Some(vec![3]));
        assert!(html.contains("Re-report of 2026-08-01-round-97 tag 04"));
        assert!(html.contains("<figcaption>before</figcaption>"));
        assert!(html.contains("<figcaption>after</figcaption>"));
    }

    #[test]
    fn carried_tag_without_an_after_shows_only_before() {
        let s = sweep_with(vec![carried_tag(3, "still broken", false)]);
        let md = render_fixlist_md(&s, "d");
        assert!(md.contains("![before](tag-03-before.png)"));
        assert!(!md.contains("![after]"));
        let brief = render_brief_md(&s, "d");
        assert!(brief.contains("- Evidence: Re-report; before image attached"));
    }

    #[test]
    fn fixlist_metadata_carries_url_and_element_when_present() {
        let mut t = tag(1, "x", "high", "layout", false);
        t.url = "slobal.com/portal".into();
        t.element = "button 'Deploy'".into();
        let s = sweep_with(vec![t]);
        let md = render_fixlist_md(&s, "d");
        assert!(md.contains("- URL: slobal.com/portal"));
        assert!(md.contains("- Element: button 'Deploy'"));
        let html = render_fixlist_html(&s, "d", |_| Some(vec![1]));
        assert!(html.contains("URL: slobal.com/portal"));
        assert!(html.contains("Element: button &#039;Deploy&#039;") || html.contains("Element: button 'Deploy'"));
    }

    #[test]
    fn brief_groups_by_target_and_keeps_global_numbering() {
        let mut a = tag(1, "helmsly one", "high", "layout", false);
        a.target = "helmsly".into();
        let b = tag(2, "no target", "low", "copy", false);
        let mut c = tag(3, "helmsly two", "medium", "layout", false);
        c.target = "helmsly".into();
        let s = sweep_with(vec![a, b, c]);
        let brief = render_brief_md(&s, "d");

        assert!(brief.contains("## Target: helmsly"));
        assert!(brief.contains("## Target: other"));
        assert!(!brief.contains("## Tasks"));
        // Numbering follows sweep order, not group order.
        assert!(brief.contains("### Task 1: helmsly one"));
        assert!(brief.contains("### Task 2: no target"));
        assert!(brief.contains("### Task 3: helmsly two"));
        // The untargeted group comes last.
        assert!(brief.find("## Target: helmsly").unwrap() < brief.find("## Target: other").unwrap());
        // Task 3 sits with task 1 under helmsly, above the other group.
        assert!(brief.find("### Task 3").unwrap() < brief.find("## Target: other").unwrap());
        assert!(brief.contains("- Severity: high / Area: layout / Target: helmsly"));
    }

    #[test]
    fn brief_says_evidence_quote_for_quote_tags() {
        let s = sweep_with(vec![quote_tag(1, "some words", "wrong")]);
        let brief = render_brief_md(&s, "d");
        assert!(brief.contains("- Evidence: quote"));
    }

    #[test]
    fn brief_says_re_report_for_carried_tags() {
        let s = sweep_with(vec![carried_tag(2, "still broken", true)]);
        let brief = render_brief_md(&s, "d");
        assert!(brief.contains("- Evidence: Re-report; before and after images attached"));
    }

    fn mixed_sweep() -> Sweep {
        let mut q = quote_tag(
            1,
            "The Save button sits below the fold;\nthe form needs a scroll before it can be sent.",
            "Wrong: it is pinned to the footer at every width.",
        );
        q.target = "helmsly".into();
        q.window_title = "Claude - Helmsly round 98".into();
        q.process_name = "claude.exe".into();

        let mut r = tag(2, "The Deploy button is enabled too early.", "medium", "behaviour", false);
        r.target = "helmsly".into();
        r.element = "button 'Deploy'".into();
        r.url = "slobal.com/portal".into();
        r.process_name = "msedge.exe".into();

        let dropped = tag(3, "gone for good", "low", "copy", true);
        sweep_with(vec![q, r, dropped])
    }

    #[test]
    fn feedback_md_matches_the_agreed_shape() {
        let md = render_feedback_md(&mixed_sweep(), "2026-09-07-claude-round-98");
        assert!(md.starts_with("Feedback on 2026-09-07-claude-round-98 (2 notes)\n"));
        assert!(md.contains(
            "Sources: Claude - Helmsly round 98 (claude.exe); slobal.com/portal (msedge.exe)"
        ));
        assert!(md.contains("1. (tag 01) [high / copy / helmsly]\n"));
        assert!(md.contains("> The Save button sits below the fold;\n"));
        assert!(md.contains("Wrong: it is pinned to the footer at every width."));
        assert!(md.contains(
            "2. (tag 02) [medium / behaviour / helmsly] (screenshot tag-02.png, button 'Deploy')"
        ));
        assert!(!md.contains("gone for good"));
    }

    #[test]
    fn feedback_txt_drops_the_markdown_syntax() {
        let txt = render_feedback_txt(&mixed_sweep(), "2026-09-07-claude-round-98");
        assert!(txt.contains("1. (tag 01)\n"));
        assert!(txt.contains("Severity: high, area: copy, target: helmsly\n"));
        assert!(txt.contains("    The Save button sits below the fold;\n"));
        assert!(!txt.contains("> "));
        assert!(!txt.contains("["));
        assert!(txt.contains(
            "2. (tag 02) (screenshot tag-02.png, button 'Deploy')"
        ));
        assert!(!txt.contains("gone for good"));
    }

    #[test]
    fn feedback_numbers_entries_and_names_the_tag() {
        // Entry order is 1..N; the tag number is what finds the PNG, and
        // the two part company as soon as a tag is dropped.
        let mut s = sweep_with(vec![
            tag(1, "dropped", "low", "copy", true),
            tag(2, "first live", "high", "layout", false),
            tag(3, "second live", "low", "copy", false),
        ]);
        s.tags[1].target = "helmsly".into();
        let md = render_feedback_md(&s, "d");
        assert!(md.contains("1. (tag 02) [high / layout / helmsly]"));
        assert!(md.contains("2. (tag 03) [low / copy]"));
    }

    #[test]
    fn the_literal_other_target_reads_as_untargeted() {
        // The popover writes "other" as a chip value, so it has to mean the
        // same thing as a tag that was never filed under a product.
        let mut a = tag(1, "helmsly one", "high", "layout", false);
        a.target = "helmsly".into();
        let mut b = tag(2, "unfiled", "low", "copy", false);
        b.target = "other".into();
        let s = sweep_with(vec![a, b]);

        let feedback = render_feedback_md(&s, "d");
        assert!(feedback.contains("2. (tag 02) [low / copy]"));
        assert!(!feedback.contains("/ other]"));

        let brief = render_brief_md(&s, "d");
        assert!(brief.contains("## Target: other"));
        assert!(brief.find("### Task 2").unwrap() > brief.find("## Target: other").unwrap());
        assert!(brief.find("### Task 1").unwrap() < brief.find("## Target: other").unwrap());

        assert_eq!(sweep_for_target(&s, "other").tags.len(), 1);
        assert_eq!(sweep_for_target(&s, "helmsly").tags.len(), 1);
    }

    #[test]
    fn render_named_serves_every_export_and_nothing_else() {
        let s = mixed_sweep();
        for name in EXPORT_FILES {
            let rendered = render_named(&s, "d", name, |_| Some(vec![1])).unwrap();
            assert!(!rendered.is_empty(), "{} rendered empty", name);
        }
        assert!(render_named(&s, "d", "sweep.json", |_| None).is_none());
    }

    #[test]
    fn five_tag_sweep_exports_every_file() {
        use crate::store::SweepStore;
        let root = std::env::temp_dir().join(format!("tagfix-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = SweepStore::new(root.clone());
        let (name, _) = store.create_sweep("five", "2026-08-13T10:00:00Z").unwrap();
        for n in 1..=5 {
            let t = tag(n, &format!("issue {}", n), "medium", "layout", false);
            std::fs::write(
                root.join(&name).join(t.image_name()),
                [137, 80, 78, 71, n as u8],
            )
            .unwrap();
            store.append_tag(&name, t).unwrap();
        }

        let result = export_sweep_files(&root, &name, &[]).unwrap();
        assert!(result.pointer.contains("brief.md"));
        assert!(result.target_dirs.is_empty());
        for f in EXPORT_FILES {
            assert!(root.join(&name).join(f).exists(), "{} missing", f);
        }
        let html = std::fs::read_to_string(root.join(&name).join("fixlist.html")).unwrap();
        assert_eq!(html.matches("data:image/png;base64,").count(), 5);
        let brief = std::fs::read_to_string(root.join(&name).join("brief.md")).unwrap();
        assert_eq!(brief.matches("### Task").count(), 5);
        let feedback = std::fs::read_to_string(root.join(&name).join("feedback.md")).unwrap();
        assert!(feedback.contains("(5 notes)"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn target_export_writes_only_that_targets_tags_and_images() {
        use crate::store::SweepStore;
        let root = std::env::temp_dir().join(format!("tagfix-tgt-{}", std::process::id()));
        let out = std::env::temp_dir().join(format!("tagfix-tgt-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&out);
        let store = SweepStore::new(root.clone());
        let (name, _) = store.create_sweep("targets", "2026-09-07T10:00:00Z").unwrap();

        let mut a = tag(1, "helmsly issue", "high", "layout", false);
        a.target = "helmsly".into();
        a.context_image = Some(crate::store::tag_context_image_name(1));
        let mut b = tag(2, "slobal issue", "low", "copy", false);
        b.target = "slobal.com".into();
        for t in [&a, &b] {
            std::fs::write(root.join(&name).join(t.image_name()), [1, 2, 3]).unwrap();
        }
        std::fs::write(
            root.join(&name).join(crate::store::tag_context_image_name(1)),
            [4, 5],
        )
        .unwrap();
        store.append_tag(&name, a).unwrap();
        store.append_tag(&name, b).unwrap();

        let targets = vec![
            Target {
                name: "helmsly".into(),
                hosts: vec![],
                export_dir: Some(out.display().to_string()),
            },
            // No export directory: nothing is written for this one.
            Target {
                name: "slobal.com".into(),
                hosts: vec![],
                export_dir: None,
            },
        ];
        let result = export_sweep_files(&root, &name, &targets).unwrap();
        assert_eq!(result.target_dirs.len(), 1);

        let copy = out.join(&name);
        for f in ["fixlist.md", "brief.md", "feedback.md"] {
            assert!(copy.join(f).exists(), "{} missing in the target copy", f);
        }
        assert!(!copy.join("fixlist.html").exists());
        assert!(copy.join("tag-01.png").exists());
        assert!(copy.join("tag-01-context.png").exists());
        assert!(!copy.join("tag-02.png").exists());
        let md = std::fs::read_to_string(copy.join("fixlist.md")).unwrap();
        assert!(md.contains("helmsly issue"));
        assert!(!md.contains("slobal issue"));

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn exports_contain_no_em_or_en_dashes() {
        let s = mixed_sweep();
        let mut all = String::new();
        for name in EXPORT_FILES {
            all.push_str(&render_named(&s, "d", name, |_| Some(vec![1])).unwrap());
        }
        all.push_str(&render_fixlist_md(&sweep_with(vec![carried_tag(4, "again", true)]), "d"));
        assert!(!all.contains('\u{2014}'));
        assert!(!all.contains('\u{2013}'));
    }
}

//! `[[Wiki links]]`: parsing, resolution, backlinks and the ranges the editor
//! uses to paint links differently from prose.
//!
//! The syntax is deliberately tiny and lives in the page text itself — no extra
//! header, no side table — so a link survives any editor that keeps the body
//! text intact. A link target is the page *subject* (case-insensitive, trimmed)
//! or, when the author needs to be exact, the Message-ID in angle brackets:
//! `[[<alpha@vault.local>]]`.

use serde_json::Value;

use crate::mime::Capsule;
use crate::theme;

/// One `[[…]]` occurrence: byte range of the whole marker plus its target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkSpan {
    pub start: usize,
    pub end: usize,
    pub target: String,
}

/// Rich inline markdown token kinds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineKind {
    Bold,
    Italic,
    Code,
    WikiLink,
    ExternalLink { label_len: usize },
}

/// One parsed inline formatting token: byte range, delimiter length, and token kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineSpan {
    pub start: usize,
    pub end: usize,
    pub delim_len: usize,
    pub kind: InlineKind,
}

/// Find every rich inline formatting span in `text` (bold, italic, code, wikilinks, urls).
pub fn inline_spans(text: &str) -> Vec<InlineSpan> {
    let mut spans = Vec::new();
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // 1. Wikilink: [[...]]
        if i + 1 < len && bytes[i] == b'[' && bytes[i + 1] == b'[' {
            if let Some(close) = text[i + 2..].find("]]") {
                let end = i + 2 + close + 2;
                let raw = &text[i + 2..i + 2 + close];
                if !raw.contains('[') && !raw.contains(']') && !raw.trim().is_empty() {
                    spans.push(InlineSpan {
                        start: i,
                        end,
                        delim_len: 2,
                        kind: InlineKind::WikiLink,
                    });
                    i = end;
                    continue;
                }
            }
        }

        // 2. Inline code: `...`
        if bytes[i] == b'`' {
            if let Some(close) = text[i + 1..].find('`') {
                let end = i + 1 + close + 1;
                let raw = &text[i + 1..i + 1 + close];
                if !raw.is_empty() && !raw.contains('\n') {
                    spans.push(InlineSpan {
                        start: i,
                        end,
                        delim_len: 1,
                        kind: InlineKind::Code,
                    });
                    i = end;
                    continue;
                }
            }
        }

        // 3. External link: [label](url)
        if bytes[i] == b'[' {
            if let Some(close_bracket) = text[i + 1..].find(']') {
                let bracket_end = i + 1 + close_bracket;
                if bracket_end + 1 < len && bytes[bracket_end + 1] == b'(' {
                    if let Some(close_paren) = text[bracket_end + 2..].find(')') {
                        let end = bracket_end + 2 + close_paren + 1;
                        let label = &text[i + 1..bracket_end];
                        let url = &text[bracket_end + 2..end - 1];
                        if !label.is_empty() && !url.contains('\n') && !label.contains('\n') {
                            spans.push(InlineSpan {
                                start: i,
                                end,
                                delim_len: 1,
                                kind: InlineKind::ExternalLink { label_len: label.len() },
                            });
                            i = end;
                            continue;
                        }
                    }
                }
            }
        }

        // 4. Bold: **...** or __...__
        if (i + 1 < len && bytes[i] == b'*' && bytes[i + 1] == b'*')
            || (i + 1 < len && bytes[i] == b'_' && bytes[i + 1] == b'_')
        {
            let marker = if bytes[i] == b'*' { "**" } else { "__" };
            if let Some(close) = text[i + 2..].find(marker) {
                let end = i + 2 + close + 2;
                let inner = &text[i + 2..i + 2 + close];
                if !inner.is_empty() && !inner.trim().is_empty() && !inner.contains('\n') {
                    spans.push(InlineSpan {
                        start: i,
                        end,
                        delim_len: 2,
                        kind: InlineKind::Bold,
                    });
                    i = end;
                    continue;
                }
            }
        }

        // 5. Italic: *...* or _..._
        if bytes[i] == b'*' || bytes[i] == b'_' {
            let marker_char = bytes[i] as char;
            if i + 1 < len && bytes[i + 1] != b' ' && bytes[i + 1] != b'\t' && bytes[i + 1] != bytes[i] {
                if let Some(close) = text[i + 1..].find(marker_char) {
                    let end = i + 1 + close + 1;
                    let inner = &text[i + 1..i + 1 + close];
                    if !inner.is_empty() && !inner.ends_with(' ') && !inner.contains('\n') && !inner.contains(marker_char) {
                        spans.push(InlineSpan {
                            start: i,
                            end,
                            delim_len: 1,
                            kind: InlineKind::Italic,
                        });
                        i = end;
                        continue;
                    }
                }
            }
        }

        if let Some(ch) = text[i..].chars().next() {
            i += ch.len_utf8();
        } else {
            i += 1;
        }
    }

    spans
}

/// Find every `[[target]]` span in `text`.
///
/// Unclosed `[[`, empty `[[ ]]` and nested brackets are ignored: a half-typed
/// link must not make the editor highlight everything after it.
pub fn spans(text: &str) -> Vec<LinkSpan> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(open) = text[cursor..].find("[[") {
        let start = cursor + open;
        let inner_start = start + 2;
        let Some(close) = text[inner_start..].find("]]") else {
            break;
        };
        let end = inner_start + close + 2;
        let raw = &text[inner_start..inner_start + close];
        if !raw.contains('[') && !raw.contains(']') {
            let target = raw.trim();
            if !target.is_empty() {
                out.push(LinkSpan {
                    start,
                    end,
                    target: target.to_string(),
                });
            }
        }
        cursor = end;
        if cursor >= bytes.len() {
            break;
        }
    }
    out
}

/// Unique targets in order of first appearance.
pub fn targets(text: &str) -> Vec<String> {
    let mut seen = Vec::new();
    for span in spans(text) {
        if !seen.iter().any(|existing: &String| existing.eq_ignore_ascii_case(&span.target)) {
            seen.push(span.target);
        }
    }
    seen
}

/// Everything a page says, as one string, for link scanning.
///
/// Structural keys (`id`, `type`) are skipped; every other string in a block's
/// `data` counts, including todo item text and callout bodies. Keeping this in
/// one place means a new block type is link-aware without touching the parser.
pub fn page_text(capsule: &Capsule) -> String {
    let mut text = String::new();
    text.push_str(&capsule.subject);
    text.push('\n');
    for block in &capsule.blocks {
        if let Some(data) = block.get("data") {
            collect_strings(data, &mut text);
        }
    }
    text
}

fn collect_strings(value: &Value, out: &mut String) {
    match value {
        Value::String(text) => {
            out.push_str(text);
            out.push('\n');
        }
        Value::Array(items) => {
            for item in items {
                collect_strings(item, out);
            }
        }
        Value::Object(map) => {
            for (key, value) in map {
                if key == "id" || key == "type" {
                    continue;
                }
                collect_strings(value, out);
            }
        }
        _ => {}
    }
}

/// Links written on a page, in document order.
pub fn page_targets(capsule: &Capsule) -> Vec<String> {
    targets(&page_text(capsule))
}

/// Compare titles the way a human would: case- and whitespace-insensitive.
pub fn normalise(title: &str) -> String {
    title
        .trim()
        .trim_matches(|ch| ch == '<' || ch == '>')
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Resolve a link target to a page index. Message-ID links win over titles,
/// because a title can be renamed while the id cannot.
pub fn resolve(target: &str, capsules: &[Capsule]) -> Option<usize> {
    let wanted = normalise(target);
    if wanted.is_empty() {
        return None;
    }
    if let Some(index) = capsules.iter().position(|capsule| {
        normalise(&capsule.message_id) == wanted
    }) {
        return Some(index);
    }
    capsules
        .iter()
        .position(|capsule| normalise(&capsule.subject) == wanted)
}

/// Pages that link to `index`, excluding the page itself. Sorted by subject so
/// the list does not shuffle when a page is edited.
pub fn backlinks(capsules: &[Capsule], index: usize) -> Vec<usize> {
    let Some(target) = capsules.get(index) else {
        return Vec::new();
    };
    let subject = normalise(&target.subject);
    let id = normalise(&target.message_id);
    let mut found: Vec<usize> = capsules
        .iter()
        .enumerate()
        .filter(|(other, capsule)| {
            *other != index
                && page_targets(capsule)
                    .iter()
                    .any(|link| normalise(link) == subject || normalise(link) == id)
        })
        .map(|(other, _)| other)
        .collect();
    found.sort_by(|a, b| {
        capsules[*a]
            .subject
            .to_lowercase()
            .cmp(&capsules[*b].subject.to_lowercase())
    });
    found
}

/// Lay out text with rich markdown formatting (bold, italic, code, wikilinks, urls)
/// rendered inline with subtle delimiter styling so the document looks formatted
/// immediately while remaining 100% byte-exact for editing.
pub fn layout(ui: &egui::Ui, text: &str, size: f32, wrap: f32) -> std::sync::Arc<egui::Galley> {
    let font = egui::FontId::new(size, egui::FontFamily::Proportional);
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap;
    let mut cursor = 0usize;

    let ghost_color = theme::fade(theme::FAINT, 0.22);
    let ghost_delim_font = egui::FontId::new(size * 0.85, egui::FontFamily::Proportional);

    for span in inline_spans(text) {
        if span.start > cursor {
            job.append(&text[cursor..span.start], 0.0, plain(&font));
        }

        match span.kind {
            InlineKind::WikiLink => {
                // Opening [[
                job.append(
                    &text[span.start..span.start + 2],
                    0.0,
                    egui::TextFormat {
                        font_id: ghost_delim_font.clone(),
                        color: theme::fade(theme::ACCENT, 0.4),
                        ..Default::default()
                    },
                );
                // Inner target
                job.append(
                    &text[span.start + 2..span.end - 2],
                    0.0,
                    egui::TextFormat {
                        font_id: font.clone(),
                        color: theme::ACCENT_INK,
                        underline: egui::Stroke::new(1.0, theme::ACCENT),
                        ..Default::default()
                    },
                );
                // Closing ]]
                job.append(
                    &text[span.end - 2..span.end],
                    0.0,
                    egui::TextFormat {
                        font_id: ghost_delim_font.clone(),
                        color: theme::fade(theme::ACCENT, 0.4),
                        ..Default::default()
                    },
                );
            }
            InlineKind::Bold => {
                job.append(
                    &text[span.start..span.start + span.delim_len],
                    0.0,
                    egui::TextFormat {
                        font_id: ghost_delim_font.clone(),
                        color: ghost_color,
                        ..Default::default()
                    },
                );
                job.append(
                    &text[span.start + span.delim_len..span.end - span.delim_len],
                    0.0,
                    egui::TextFormat {
                        font_id: egui::FontId::new(size, theme::semibold()),
                        color: theme::INK,
                        ..Default::default()
                    },
                );
                job.append(
                    &text[span.end - span.delim_len..span.end],
                    0.0,
                    egui::TextFormat {
                        font_id: ghost_delim_font.clone(),
                        color: ghost_color,
                        ..Default::default()
                    },
                );
            }
            InlineKind::Italic => {
                job.append(
                    &text[span.start..span.start + span.delim_len],
                    0.0,
                    egui::TextFormat {
                        font_id: ghost_delim_font.clone(),
                        color: ghost_color,
                        ..Default::default()
                    },
                );
                job.append(
                    &text[span.start + span.delim_len..span.end - span.delim_len],
                    0.0,
                    egui::TextFormat {
                        font_id: font.clone(),
                        italics: true,
                        color: theme::INK,
                        ..Default::default()
                    },
                );
                job.append(
                    &text[span.end - span.delim_len..span.end],
                    0.0,
                    egui::TextFormat {
                        font_id: ghost_delim_font.clone(),
                        color: ghost_color,
                        ..Default::default()
                    },
                );
            }
            InlineKind::Code => {
                job.append(
                    &text[span.start..span.start + 1],
                    0.0,
                    egui::TextFormat {
                        font_id: egui::FontId::new(size * 0.85, theme::mono()),
                        color: ghost_color,
                        ..Default::default()
                    },
                );
                job.append(
                    &text[span.start + 1..span.end - 1],
                    0.0,
                    egui::TextFormat {
                        font_id: egui::FontId::new(size * 0.9, theme::mono()),
                        color: theme::ACCENT_INK,
                        background: theme::SUNKEN,
                        ..Default::default()
                    },
                );
                job.append(
                    &text[span.end - 1..span.end],
                    0.0,
                    egui::TextFormat {
                        font_id: egui::FontId::new(size * 0.85, theme::mono()),
                        color: ghost_color,
                        ..Default::default()
                    },
                );
            }
            InlineKind::ExternalLink { label_len } => {
                job.append(
                    &text[span.start..span.start + 1],
                    0.0,
                    egui::TextFormat {
                        font_id: ghost_delim_font.clone(),
                        color: ghost_color,
                        ..Default::default()
                    },
                );
                job.append(
                    &text[span.start + 1..span.start + 1 + label_len],
                    0.0,
                    egui::TextFormat {
                        font_id: font.clone(),
                        color: theme::ACCENT_INK,
                        underline: egui::Stroke::new(1.0, theme::ACCENT),
                        ..Default::default()
                    },
                );
                let mid_start = span.start + 1 + label_len;
                job.append(
                    &text[mid_start..mid_start + 2],
                    0.0,
                    egui::TextFormat {
                        font_id: ghost_delim_font.clone(),
                        color: ghost_color,
                        ..Default::default()
                    },
                );
                job.append(
                    &text[mid_start + 2..span.end - 1],
                    0.0,
                    egui::TextFormat {
                        font_id: egui::FontId::new(size * 0.85, theme::mono()),
                        color: theme::fade(theme::FAINT, 0.5),
                        ..Default::default()
                    },
                );
                job.append(
                    &text[span.end - 1..span.end],
                    0.0,
                    egui::TextFormat {
                        font_id: ghost_delim_font.clone(),
                        color: ghost_color,
                        ..Default::default()
                    },
                );
            }
        }
        cursor = span.end;
    }

    if cursor < text.len() || text.is_empty() {
        job.append(&text[cursor.min(text.len())..], 0.0, plain(&font));
    }
    ui.painter().layout_job(job)
}

fn plain(font: &egui::FontId) -> egui::TextFormat {
    egui::TextFormat {
        font_id: font.clone(),
        color: theme::INK,
        ..Default::default()
    }
}

/// Rewrite every `[[old title]]` in these blocks to `[[new title]]`.
///
/// Renaming a page must not silently break the links that point at it. The
/// match is the same case/whitespace-insensitive comparison used for
/// resolution; the replacement keeps the author's brackets and nothing else.
/// Returns the number of links rewritten.
pub fn rewrite_targets(blocks: &mut [Value], old: &str, new: &str) -> usize {
    let old_norm = normalise(old);
    if old_norm.is_empty() || normalise(new).is_empty() || normalise(new) == old_norm {
        return 0;
    }
    let mut count = 0;
    for block in blocks.iter_mut() {
        if let Some(data) = block.get_mut("data") {
            count += rewrite_value(data, &old_norm, new);
        }
    }
    count
}

fn rewrite_value(value: &mut Value, old_norm: &str, new: &str) -> usize {
    match value {
        Value::String(text) => {
            let (rewritten, count) = rewrite_text(text, old_norm, new);
            if count > 0 {
                *text = rewritten;
            }
            count
        }
        Value::Array(items) => items
            .iter_mut()
            .map(|item| rewrite_value(item, old_norm, new))
            .sum(),
        Value::Object(map) => map
            .iter_mut()
            .filter(|(key, _)| key.as_str() != "id" && key.as_str() != "type")
            .map(|(_, value)| rewrite_value(value, old_norm, new))
            .sum(),
        _ => 0,
    }
}

fn rewrite_text(text: &str, old_norm: &str, new: &str) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    let mut count = 0usize;
    for span in spans(text) {
        if normalise(&span.target) != old_norm {
            continue;
        }
        out.push_str(&text[cursor..span.start]);
        out.push_str("[[");
        out.push_str(new);
        out.push_str("]]");
        cursor = span.end;
        count += 1;
    }
    if count == 0 {
        return (text.to_string(), 0);
    }
    out.push_str(&text[cursor..]);
    (out, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_simple_spans() {
        let found = spans("see [[Alpha]] and [[ Beta ]].");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].target, "Alpha");
        assert_eq!(found[1].target, "Beta");
        assert_eq!(&"see [[Alpha]] and [[ Beta ]]."[found[1].start..found[1].end], "[[ Beta ]]");
    }

    #[test]
    fn ignores_unfinished_and_empty_links() {
        assert!(spans("[[ unclosed").is_empty());
        assert!(spans("[[   ]]").is_empty());
        assert!(spans("[[a[b]]").is_empty());
        assert_eq!(spans("[[ok]] [[x]]").len(), 2);
    }

    #[test]
    fn targets_are_unique_and_ordered() {
        assert_eq!(
            targets("[[b]] then [[A]] then [[b]]"),
            vec!["b".to_string(), "A".to_string()]
        );
    }

    fn capsule(id: &str, subject: &str, body: &str) -> Capsule {
        let mut capsule = Capsule::blank(id.to_string(), format!("{id}.eml"), subject.to_string());
        capsule.blocks = vec![serde_json::json!({
            "id": "paragraph-1",
            "type": "paragraph",
            "data": { "text": body }
        })];
        capsule
    }

    #[test]
    fn resolves_titles_case_insensitively() {
        let capsules = vec![
            capsule("<a@v>", "Memory map", ""),
            capsule("<b@v>", "Zero DB", ""),
        ];
        assert_eq!(resolve("  memory MAP ", &capsules), Some(0));
        assert_eq!(resolve("zero db", &capsules), Some(1));
        assert_eq!(resolve("missing", &capsules), None);
    }

    #[test]
    fn resolves_message_ids_exactly() {
        let capsules = vec![capsule("<a@v>", "Renamed page", "")];
        assert_eq!(resolve("<a@v>", &capsules), Some(0));
        assert_eq!(resolve("a@v", &capsules), Some(0));
    }

    #[test]
    fn backlinks_point_at_the_linking_pages() {
        let capsules = vec![
            capsule("<a@v>", "Alpha", "links to [[Gamma]]"),
            capsule("<b@v>", "Beta", "also [[gamma]] and [[Alpha]]"),
            capsule("<c@v>", "Gamma", "self [[Gamma]]"),
        ];
        // Alpha is linked from Beta only; Gamma is linked from Alpha, Beta and
        // never from itself.
        assert_eq!(backlinks(&capsules, 0), vec![1]);
        assert_eq!(backlinks(&capsules, 2), vec![0, 1]);
    }

    #[test]
    fn page_text_covers_nested_block_data() {
        let mut capsule = Capsule::blank("<x@v>".into(), "x.eml".into(), "Notes".into());
        capsule.blocks = vec![serde_json::json!({
            "id": "todo-1",
            "type": "todo_list",
            "data": {
                "title": "Work",
                "items": [{ "id": "item-1", "text": "read [[Alpha]]" }]
            }
        })];
        assert_eq!(page_targets(&capsule), vec!["Alpha".to_string()]);
    }

    fn paragraph(text: &str) -> Value {
        serde_json::json!({
            "id": "paragraph-1",
            "type": "paragraph",
            "data": { "text": text }
        })
    }

    #[test]
    fn rewrite_updates_matching_links_only() {
        let mut blocks = vec![
            paragraph("see [[Old Title]] and [[Other]] and [[old title]]"),
            serde_json::json!({
                "id": "todo-1",
                "type": "todo_list",
                "data": { "items": [{ "id": "i", "text": "finish [[OLD TITLE]]" }] }
            }),
        ];
        let count = rewrite_targets(&mut blocks, "old title", "New Title");
        assert_eq!(count, 3);
        assert_eq!(
            blocks[0]["data"]["text"].as_str().unwrap(),
            "see [[New Title]] and [[Other]] and [[New Title]]"
        );
        assert_eq!(
            blocks[1]["data"]["items"][0]["text"].as_str().unwrap(),
            "finish [[New Title]]"
        );
    }

    #[test]
    fn rewrite_ignores_same_or_empty_titles() {
        let mut blocks = vec![paragraph("[[Alpha]]")];
        assert_eq!(rewrite_targets(&mut blocks, "Alpha", " alpha "), 0);
        assert_eq!(rewrite_targets(&mut blocks, "Alpha", "   "), 0);
        assert_eq!(blocks[0]["data"]["text"].as_str().unwrap(), "[[Alpha]]");
    }

    #[test]
    fn rewrite_keeps_surrounding_prose() {
        let mut blocks = vec![paragraph("before [[A]] after [[B]] tail")];
        assert_eq!(rewrite_targets(&mut blocks, "A", "C"), 1);
        assert_eq!(
            blocks[0]["data"]["text"].as_str().unwrap(),
            "before [[C]] after [[B]] tail"
        );
    }

    #[test]
    fn renaming_keeps_backlinks_alive() {
        let mut capsules = vec![
            capsule("<a@v>", "Old", ""),
            capsule("<b@v>", "Other", "points at [[Old]]"),
        ];
        assert_eq!(backlinks(&capsules, 0), vec![1]);
        // Rename the page, then rewrite the links that pointed at the old title.
        capsules[0].subject = "Fresh".to_string();
        assert_eq!(rewrite_targets(&mut capsules[1].blocks, "Old", "Fresh"), 1);
        assert_eq!(page_targets(&capsules[1]), vec!["Fresh".to_string()]);
        assert_eq!(backlinks(&capsules, 0), vec![1]);
        assert_eq!(resolve("fresh", &capsules), Some(0));
        assert_eq!(resolve("old", &capsules), None);
    }

    #[test]
    fn test_inline_spans_detection() {
        let text = "Text with **bold**, *italic*, `code`, [[wiki]] and [site](https://test.com).";
        let spans = inline_spans(text);
        assert_eq!(spans.len(), 5);

        assert_eq!(spans[0].kind, InlineKind::Bold);
        assert_eq!(&text[spans[0].start..spans[0].end], "**bold**");

        assert_eq!(spans[1].kind, InlineKind::Italic);
        assert_eq!(&text[spans[1].start..spans[1].end], "*italic*");

        assert_eq!(spans[2].kind, InlineKind::Code);
        assert_eq!(&text[spans[2].start..spans[2].end], "`code`");

        assert_eq!(spans[3].kind, InlineKind::WikiLink);
        assert_eq!(&text[spans[3].start..spans[3].end], "[[wiki]]");

        assert!(matches!(spans[4].kind, InlineKind::ExternalLink { .. }));
        assert_eq!(&text[spans[4].start..spans[4].end], "[site](https://test.com)");
    }

    #[test]
    fn test_inline_spans_cyrillic() {
        let text = "Пишите **жирный** текст, *курсив* и `код`.";
        let spans = inline_spans(text);
        assert_eq!(spans.len(), 3);
        assert_eq!(&text[spans[0].start..spans[0].end], "**жирный**");
        assert_eq!(&text[spans[1].start..spans[1].end], "*курсив*");
        assert_eq!(&text[spans[2].start..spans[2].end], "`код`");
    }
}

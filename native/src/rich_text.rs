//! Rich text editing helpers for native egui:
//! - Hotkeys interception (`Ctrl+B`, `Ctrl+I`, `Ctrl+K`)
//! - Floating bubble toolbar for text selections (Notion-style)
//! - Markdown token wrapping & unwrapping
//! - Smart transform detection (`# `, `## `, `[] `, `> `)

use egui::{
    pos2, vec2, Area, Color32, CornerRadius, Frame, Margin, Order,
    Rect, RichText, Stroke, Ui,
};

use crate::i18n::Locale;
use crate::theme;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SmartTransform {
    Heading(u8, String),
    Tasks(String),
    Callout(String),
    Code(String),
}

/// Detect if text starts with a markdown shortcut prefix on line 1.
pub fn detect_smart_transform(text: &str) -> Option<SmartTransform> {
    let first_line = text.lines().next().unwrap_or(text);
    let trimmed = first_line.trim_start();
    if trimmed.starts_with("### ") {
        Some(SmartTransform::Heading(3, trimmed[4..].to_string()))
    } else if trimmed.starts_with("## ") {
        Some(SmartTransform::Heading(2, trimmed[3..].to_string()))
    } else if trimmed.starts_with("# ") {
        Some(SmartTransform::Heading(1, trimmed[2..].to_string()))
    } else if trimmed.starts_with("[] ") {
        Some(SmartTransform::Tasks(trimmed[3..].to_string()))
    } else if trimmed.starts_with("- [ ] ") || trimmed.starts_with("* [ ] ") {
        Some(SmartTransform::Tasks(trimmed[6..].to_string()))
    } else if trimmed.starts_with("- [x] ") || trimmed.starts_with("* [x] ") {
        Some(SmartTransform::Tasks(trimmed[6..].to_string()))
    } else if trimmed.starts_with("> ") {
        Some(SmartTransform::Callout(trimmed[2..].to_string()))
    } else if trimmed.starts_with("```") {
        Some(SmartTransform::Code(trimmed[3..].to_string()))
    } else {
        None
    }
}

/// Helper to wrap or unwrap selection in `text` with prefix and suffix tokens.
/// Returns the new `(start, end)` selection bounds.
pub fn toggle_wrap(
    text: &mut String,
    mut start: usize,
    mut end: usize,
    prefix: &str,
    suffix: &str,
) -> (usize, usize) {
    if start > end {
        std::mem::swap(&mut start, &mut end);
    }
    // Snap to valid UTF-8 boundaries
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }
    while end < text.len() && !text.is_char_boundary(end) {
        end += 1;
    }

    let p_len = prefix.len();
    let s_len = suffix.len();

    // Check if the selected text itself starts with prefix and ends with suffix
    if (end - start) >= (p_len + s_len)
        && text[start..end].starts_with(prefix)
        && text[start..end].ends_with(suffix)
    {
        // Unwrap inside selection
        let inner = text[start + p_len..end - s_len].to_string();
        text.replace_range(start..end, &inner);
        return (start, start + inner.len());
    }

    // Check if prefix is immediately before start and suffix is immediately after end
    if start >= p_len && (end + s_len) <= text.len() {
        if &text[start - p_len..start] == prefix && &text[end..end + s_len] == suffix {
            // Unwrap surrounding
            text.drain(end..end + s_len);
            text.drain(start - p_len..start);
            return (start - p_len, end - p_len);
        }
    }

    // Otherwise, wrap selection with prefix and suffix
    let selected = text[start..end].to_string();
    let replacement = format!("{prefix}{selected}{suffix}");
    text.replace_range(start..end, &replacement);
    (start, start + replacement.len())
}

/// Convert character indices to UTF-8 byte indices in `s`.
pub fn char_range_to_byte_range(s: &str, char_start: usize, char_end: usize) -> (usize, usize) {
    let mut byte_start = s.len();
    let mut byte_end = s.len();
    let mut char_count = 0;

    for (b_idx, _) in s.char_indices() {
        if char_count == char_start {
            byte_start = b_idx;
        }
        if char_count == char_end {
            byte_end = b_idx;
            break;
        }
        char_count += 1;
    }

    if char_start >= char_count {
        byte_start = s.len();
    }
    if char_end >= char_count {
        byte_end = s.len();
    }

    (byte_start.min(byte_end), byte_start.max(byte_end))
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SavedSelection {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
    pub b_start: usize,
    pub b_end: usize,
}

impl SavedSelection {
    pub fn new(rect: Rect, b_start: usize, b_end: usize) -> Self {
        Self {
            min_x: rect.min.x,
            min_y: rect.min.y,
            max_x: rect.max.x,
            max_y: rect.max.y,
            b_start,
            b_end,
        }
    }

    pub fn rect(&self) -> Rect {
        Rect::from_min_max(pos2(self.min_x, self.min_y), pos2(self.max_x, self.max_y))
    }
}

pub fn toolbar_rect(ui: &Ui, menu_id: egui::Id) -> Option<Rect> {
    ui.data(|d| d.get_temp(menu_id.with("rect")))
}

fn bubble_btn(ui: &mut Ui, label: RichText, tooltip: &str) -> bool {
    let button = egui::Button::new(label)
        .fill(Color32::TRANSPARENT)
        .frame(true)
        .corner_radius(CornerRadius::same(4))
        .min_size(vec2(24.0, 22.0));
    let resp = ui.add(button);
    if !tooltip.is_empty() {
        resp.on_hover_text(tooltip).clicked()
    } else {
        resp.clicked()
    }
}

fn menu_item(ui: &mut Ui, badge: &str, title: &str) -> bool {
    let button = egui::Button::new(
        RichText::new(format!("{badge:<3} {title}"))
            .size(11.5)
            .color(theme::INK),
    )
    .fill(Color32::TRANSPARENT)
    .frame(false);
    let resp = ui.add(button);
    resp.clicked()
}

pub enum BubbleAction {
    FormatBold,
    FormatItalic,
    FormatCode,
    FormatLink,
    TransformBlock(&'static str),
}

/// Draw a floating bubble menu above an active text selection.
pub fn draw_bubble_toolbar(
    ui: &mut Ui,
    menu_id: egui::Id,
    anchor_rect: Rect,
    _locale: Locale,
) -> Option<BubbleAction> {
    let mut action = None;

    let dropdown_key = menu_id.with("dropdown-open");
    let mut dropdown_open: bool = ui.data(|d| d.get_temp(dropdown_key).unwrap_or(false));

    // Calculate position: absolute screen coordinates centered right above the text
    let bubble_width = 175.0;
    let bubble_height = 30.0;
    let screen_rect = ui.ctx().viewport_rect();
    let center_x = anchor_rect.center().x;
    let x = (center_x - bubble_width * 0.5)
        .clamp(16.0, (screen_rect.width() - bubble_width - 16.0).max(16.0));
    let y = if anchor_rect.top() > 44.0 {
        anchor_rect.top() - bubble_height - 6.0
    } else {
        anchor_rect.bottom() + 6.0
    };

    let area_resp = Area::new(menu_id)
        .order(Order::Foreground)
        .fixed_pos(pos2(x, y))
        .show(ui.ctx(), |ui| {
            Frame::new()
                .fill(Color32::from_rgb(22, 27, 34))
                .stroke(Stroke::new(1.0, Color32::from_rgb(56, 64, 76)))
                .corner_radius(CornerRadius::same(7))
                .inner_margin(Margin::symmetric(4, 3))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 4],
                    blur: 14,
                    spread: 0,
                    color: Color32::from_black_alpha(160),
                })
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = vec2(2.0, 0.0);

                            // [ B ] Bold
                            if bubble_btn(ui, RichText::new("B").family(theme::semibold()).size(13.0), "Ctrl+B") {
                                action = Some(BubbleAction::FormatBold);
                            }

                            // [ I ] Italic
                            if bubble_btn(ui, RichText::new("I").italics().size(13.0), "Ctrl+I") {
                                action = Some(BubbleAction::FormatItalic);
                            }

                            // [ </> ] Inline Code
                            if bubble_btn(ui, RichText::new("</>").family(theme::mono()).size(11.0), "Code") {
                                action = Some(BubbleAction::FormatCode);
                            }

                            // [ [[ ]] ] Wikilink
                            if bubble_btn(ui, RichText::new("[[ ]]").family(theme::mono()).size(10.5).color(theme::ACCENT_INK), "Ctrl+K") {
                                action = Some(BubbleAction::FormatLink);
                            }

                            ui.add_space(2.0);
                            ui.painter().line_segment(
                                [
                                    pos2(ui.cursor().min.x, ui.cursor().min.y + 3.0),
                                    pos2(ui.cursor().min.x, ui.cursor().min.y + 19.0),
                                ],
                                Stroke::new(1.0, Color32::from_rgb(56, 64, 76)),
                            );
                            ui.add_space(3.0);

                            // Block type selector dropdown toggle
                            let drop_label = if dropdown_open { "¶ ▴" } else { "¶ ▾" };
                            let block_type_btn = egui::Button::new(
                                RichText::new(drop_label)
                                    .size(11.5)
                                    .color(if dropdown_open { theme::ACCENT_INK } else { theme::MUTED }),
                            )
                            .fill(Color32::TRANSPARENT)
                            .frame(true)
                            .corner_radius(CornerRadius::same(4))
                            .min_size(vec2(24.0, 22.0));

                            if ui.add(block_type_btn).on_hover_text("Turn into...").clicked() {
                                dropdown_open = !dropdown_open;
                                ui.data_mut(|d| d.insert_temp(dropdown_key, dropdown_open));
                            }
                        });

                        // If dropdown is open, render options directly inside the panel
                        if dropdown_open {
                            ui.add_space(3.0);
                            ui.painter().line_segment(
                                [
                                    pos2(ui.cursor().min.x - 2.0, ui.cursor().min.y),
                                    pos2(ui.cursor().min.x + bubble_width - 8.0, ui.cursor().min.y),
                                ],
                                Stroke::new(1.0, Color32::from_rgb(48, 54, 61)),
                            );
                            ui.add_space(3.0);

                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing = vec2(0.0, 2.0);
                                if menu_item(ui, "H1", "Heading 1") {
                                    action = Some(BubbleAction::TransformBlock("heading_1"));
                                    dropdown_open = false;
                                    ui.data_mut(|d| d.insert_temp(dropdown_key, false));
                                }
                                if menu_item(ui, "H2", "Heading 2") {
                                    action = Some(BubbleAction::TransformBlock("heading_2"));
                                    dropdown_open = false;
                                    ui.data_mut(|d| d.insert_temp(dropdown_key, false));
                                }
                                if menu_item(ui, "H3", "Heading 3") {
                                    action = Some(BubbleAction::TransformBlock("heading_3"));
                                    dropdown_open = false;
                                    ui.data_mut(|d| d.insert_temp(dropdown_key, false));
                                }
                                if menu_item(ui, "[ ]", "To-Do List") {
                                    action = Some(BubbleAction::TransformBlock("todo_list"));
                                    dropdown_open = false;
                                    ui.data_mut(|d| d.insert_temp(dropdown_key, false));
                                }
                                if menu_item(ui, "[!]", "Callout Note") {
                                    action = Some(BubbleAction::TransformBlock("callout"));
                                    dropdown_open = false;
                                    ui.data_mut(|d| d.insert_temp(dropdown_key, false));
                                }
                                if menu_item(ui, "{}", "Code Snippet") {
                                    action = Some(BubbleAction::TransformBlock("code_repl"));
                                    dropdown_open = false;
                                    ui.data_mut(|d| d.insert_temp(dropdown_key, false));
                                }
                            });
                        }
                    });
                });
        });

    ui.data_mut(|d| d.insert_temp(menu_id.with("rect"), area_resp.response.rect));

    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smart_transform_detection() {
        assert_eq!(
            detect_smart_transform("# Hello World"),
            Some(SmartTransform::Heading(1, "Hello World".to_string()))
        );
        assert_eq!(
            detect_smart_transform("## Subheading"),
            Some(SmartTransform::Heading(2, "Subheading".to_string()))
        );
        assert_eq!(
            detect_smart_transform("### Third level"),
            Some(SmartTransform::Heading(3, "Third level".to_string()))
        );
        assert_eq!(
            detect_smart_transform("[] New task"),
            Some(SmartTransform::Tasks("New task".to_string()))
        );
        assert_eq!(
            detect_smart_transform("- [ ] Another task"),
            Some(SmartTransform::Tasks("Another task".to_string()))
        );
        assert_eq!(
            detect_smart_transform("> A callout quote"),
            Some(SmartTransform::Callout("A callout quote".to_string()))
        );
        assert_eq!(
            detect_smart_transform("```rust"),
            Some(SmartTransform::Code("rust".to_string()))
        );
        assert_eq!(detect_smart_transform("Plain text without prefix"), None);
    }

    #[test]
    fn test_toggle_wrap_bold() {
        let mut text = "hello world".to_string();
        // Wrap "world"
        let (s, e) = toggle_wrap(&mut text, 6, 11, "**", "**");
        assert_eq!(text, "hello **world**");
        assert_eq!(&text[s..e], "**world**");

        // Unwrap "world"
        let (s2, e2) = toggle_wrap(&mut text, s, e, "**", "**");
        assert_eq!(text, "hello world");
        assert_eq!(&text[s2..e2], "world");
    }

    #[test]
    fn test_toggle_wrap_cyrillic() {
        let mut text = "привет мир".to_string();
        let mir_start = "привет ".len();
        let mir_end = text.len();
        let (s, e) = toggle_wrap(&mut text, mir_start, mir_end, "**", "**");
        assert_eq!(text, "привет **мир**");

        let (s2, e2) = toggle_wrap(&mut text, s, e, "**", "**");
        assert_eq!(text, "привет мир");
        assert_eq!(&text[s2..e2], "мир");
    }
}

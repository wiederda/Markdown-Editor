use eframe::egui::{
    text::{LayoutJob, TextFormat},
    Color32, FontId, Stroke, Visuals,
};

use crate::doc::DocKind;

/// Farben für die Syntaxhervorhebung, abhängig vom hellen/dunklen Design.
#[derive(Clone, Copy)]
pub struct HighlightColors {
    pub text: Color32,
    pub weak: Color32,
    pub strong: Color32,
    pub heading: Color32,
    pub code: Color32,
    pub code_bg: Color32,
    pub link: Color32,
    pub quote: Color32,
    pub marker: Color32,
    pub tag: Color32,
    pub attribute: Color32,
    pub string: Color32,
    pub comment: Color32,
}

impl HighlightColors {
    pub fn from_visuals(visuals: &Visuals) -> Self {
        if visuals.dark_mode {
            Self {
                text: visuals.text_color(),
                weak: Color32::from_rgb(138, 146, 158),
                strong: Color32::from_rgb(240, 224, 190),
                heading: Color32::from_rgb(230, 170, 100),
                code: Color32::from_rgb(150, 206, 160),
                code_bg: Color32::from_rgba_unmultiplied(90, 120, 100, 30),
                link: Color32::from_rgb(110, 180, 255),
                quote: Color32::from_rgb(160, 170, 150),
                marker: Color32::from_rgb(118, 128, 144),
                tag: Color32::from_rgb(120, 180, 255),
                attribute: Color32::from_rgb(224, 180, 120),
                string: Color32::from_rgb(160, 210, 140),
                comment: Color32::from_rgb(120, 132, 148),
            }
        } else {
            Self {
                text: visuals.text_color(),
                weak: Color32::from_rgb(120, 126, 136),
                strong: Color32::from_rgb(40, 40, 44),
                heading: Color32::from_rgb(168, 84, 10),
                code: Color32::from_rgb(30, 110, 70),
                code_bg: Color32::from_rgba_unmultiplied(80, 140, 100, 28),
                link: Color32::from_rgb(24, 96, 190),
                quote: Color32::from_rgb(92, 104, 88),
                marker: Color32::from_rgb(150, 150, 158),
                tag: Color32::from_rgb(24, 86, 176),
                attribute: Color32::from_rgb(158, 92, 18),
                string: Color32::from_rgb(34, 120, 60),
                comment: Color32::from_rgb(132, 138, 146),
            }
        }
    }
}

pub struct Highlighter {
    pub colors: HighlightColors,
    pub font: FontId,
    job: LayoutJob,
}

impl Highlighter {
    fn new(visuals: &Visuals, font_size: f32, wrap_width: f32) -> Self {
        let mut job = LayoutJob::default();
        job.wrap.max_width = wrap_width;
        Self {
            colors: HighlightColors::from_visuals(visuals),
            font: FontId::monospace(font_size),
            job,
        }
    }

    fn format(&self, color: Color32) -> TextFormat {
        TextFormat {
            font_id: self.font.clone(),
            color,
            ..Default::default()
        }
    }

    fn append(&mut self, text: &str, color: Color32) {
        if text.is_empty() {
            return;
        }
        self.job.append(text, 0.0, self.format(color));
    }

    fn append_styled(&mut self, text: &str, format: TextFormat) {
        if text.is_empty() {
            return;
        }
        self.job.append(text, 0.0, format);
    }

    fn finish(self) -> LayoutJob {
        self.job
    }
}

/// Erzeugt ein LayoutJob mit Syntaxhervorhebung für Markdown- oder HTML-Text.
pub fn layout_job(
    kind: DocKind,
    text: &str,
    visuals: &Visuals,
    font_size: f32,
    wrap_width: f32,
    enabled: bool,
) -> LayoutJob {
    let mut h = Highlighter::new(visuals, font_size, wrap_width);
    if !enabled {
        h.append(text, h.colors.text);
        return h.finish();
    }
    match kind {
        DocKind::Markdown => markdown(&mut h, text),
        DocKind::Html => html(&mut h, text),
    }
    h.finish()
}

// ---------------------------------------------------------------------------
// Markdown
// ---------------------------------------------------------------------------

fn markdown(h: &mut Highlighter, text: &str) {
    let mut in_fence = false;
    let mut pos = 0;
    while pos < text.len() {
        let rest = &text[pos..];
        let line_end = rest.find('\n').map(|i| pos + i + 1).unwrap_or(text.len());
        let line = &text[pos..line_end];
        let (content, newline) = match line.strip_suffix('\n') {
            Some(line) => (line, "\n"),
            None => (line, ""),
        };

        let trimmed = content.trim_start();
        let is_fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");

        if in_fence {
            if is_fence {
                h.append(content, h.colors.marker);
                in_fence = false;
            } else {
                let mut format = h.format(h.colors.code);
                format.background = h.colors.code_bg;
                h.append_styled(content, format);
            }
        } else if is_fence {
            h.append(content, h.colors.marker);
            in_fence = true;
        } else {
            markdown_line(h, content);
        }
        h.append(newline, h.colors.text);

        pos = line_end;
    }
}

fn markdown_line(h: &mut Highlighter, line: &str) {
    let indent_len = line.len() - line.trim_start().len();
    if indent_len > 0 {
        h.append(&line[..indent_len], h.colors.text);
    }
    let rest = &line[indent_len..];

    // Überschriften
    if rest.starts_with('#') {
        let hashes = rest.chars().take_while(|c| *c == '#').count();
        let after = &rest[hashes..];
        if after.starts_with(' ') || after.is_empty() {
            let mut heading_format = h.format(h.colors.heading);
            heading_format.extra_letter_spacing = 0.0;
            h.append(&rest[..hashes], h.colors.marker);
            h.append_styled(" ", heading_format.clone());
            let level = hashes.min(6) as f32;
            heading_format.font_id = FontId::monospace(h.font.size + (7.0 - level) * 1.0);
            h.append_styled(after.trim_start(), heading_format);
            return;
        }
    }

    // Zitate
    if let Some(after) = rest.strip_prefix('>') {
        h.append(">", h.colors.marker);
        let mut quote_format = h.format(h.colors.quote);
        quote_format.italics = true;
        h.append_styled(after, quote_format);
        return;
    }

    // Listen
    if let Some((marker, after)) = list_marker(rest) {
        h.append(marker, h.colors.heading);
        h.append(after, h.colors.text);
        return;
    }

    // Trennlinie
    let t = rest.trim();
    if t.len() >= 3
        && (t.chars().all(|c| c == '-')
            || t.chars().all(|c| c == '*')
            || t.chars().all(|c| c == '_'))
        && !t.contains(' ')
    {
        h.append(rest, h.colors.marker);
        return;
    }

    markdown_inline(h, rest);
}

fn list_marker(line: &str) -> Option<(&str, &str)> {
    for marker in ["- ", "* ", "+ "] {
        if line.starts_with(marker) {
            return Some((&line[..marker.len()], &line[marker.len()..]));
        }
    }
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && line[digits..].starts_with(". ") {
        return Some((&line[..digits + 2], &line[digits + 2..]));
    }
    None
}

fn markdown_inline(h: &mut Highlighter, line: &str) {
    let mut pos = 0;
    let mut plain = 0;
    while pos < line.len() {
        let rest = &line[pos..];
        let mut handled = false;

        // Bilder und Links
        let link_start = if rest.starts_with("![") {
            Some(2)
        } else if rest.starts_with('[') {
            Some(1)
        } else {
            None
        };
        if let Some(open) = link_start {
            if let Some(close_rel) = rest[open..].find(']') {
                let close = open + close_rel;
                if rest[close..].starts_with("](") {
                    if let Some(end_rel) = rest[close + 2..].find(')') {
                        let end = close + 2 + end_rel;
                        if plain < pos {
                            h.append(&line[plain..pos], h.colors.text);
                        }
                        if open == 2 {
                            h.append("![", h.colors.marker);
                        } else {
                            h.append("[", h.colors.marker);
                        }
                        let mut link_format = h.format(h.colors.link);
                        link_format.underline = Stroke::new(1.0_f32, h.colors.link);
                        h.append_styled(&rest[open..close], link_format);
                        h.append("](", h.colors.marker);
                        h.append(&rest[close + 2..end], h.colors.weak);
                        h.append(")", h.colors.marker);
                        pos += end + 1;
                        plain = pos;
                        handled = true;
                    }
                }
            }
        }

        if !handled {
            let pair = if rest.starts_with("**") {
                Some(("**", "**", true, false))
            } else if rest.starts_with("__") {
                Some(("__", "__", true, false))
            } else if rest.starts_with("~~") {
                Some(("~~", "~~", false, true))
            } else if rest.starts_with('`') {
                Some(("`", "`", false, false))
            } else if rest.starts_with('*') {
                Some(("*", "*", false, false))
            } else if rest.starts_with('_') {
                Some(("_", "_", false, false))
            } else {
                None
            };

            if let Some((open, close, bold, strike)) = pair {
                let after_open = open.len();
                if let Some(close_rel) = rest[after_open..].find(close) {
                    let close_at = after_open + close_rel;
                    if close_at > after_open {
                        if plain < pos {
                            h.append(&line[plain..pos], h.colors.text);
                        }
                        h.append(open, h.colors.marker);
                        let mut inner = h.format(if bold {
                            h.colors.strong
                        } else if strike || open == "`" {
                            h.colors.code
                        } else {
                            h.colors.text
                        });
                        if open == "`" {
                            inner.background = h.colors.code_bg;
                        } else if strike {
                            inner.strikethrough = Stroke::new(1.0_f32, h.colors.weak);
                        } else if !bold {
                            inner.italics = true;
                        }
                        h.append_styled(&rest[after_open..close_at], inner);
                        h.append(close, h.colors.marker);
                        pos += close_at + close.len();
                        plain = pos;
                        handled = true;
                    }
                }
            }
        }

        if !handled {
            let ch = rest.chars().next().unwrap();
            pos += ch.len_utf8();
        }
    }
    if plain < line.len() {
        h.append(&line[plain..], h.colors.text);
    }
}

// ---------------------------------------------------------------------------
// HTML
// ---------------------------------------------------------------------------

fn html(h: &mut Highlighter, text: &str) {
    let mut pos = 0;
    while pos < text.len() {
        let rest = &text[pos..];
        if rest.starts_with("<!--") {
            let end = rest.find("-->").map(|i| i + 3).unwrap_or(rest.len());
            h.append(&rest[..end], h.colors.comment);
            pos += end;
            continue;
        }
        if rest.starts_with('<') {
            // Script- und Style-Inhalte als Text behandeln.
            let lower = rest.to_ascii_lowercase();
            if let Some(tag) = script_or_style(&lower) {
                let close = format!("</{tag}");
                let end = lower
                    .find(&close)
                    .map(|i| i + close.len())
                    .unwrap_or(rest.len());
                html_tag(h, &rest[..end.min(rest.len())]);
                pos += end;
                continue;
            }
            if let Some(end) = find_tag_end(rest) {
                html_tag(h, &rest[..end]);
                pos += end;
                continue;
            }
        }
        let end = rest.find('<').unwrap_or(rest.len());
        h.append(&rest[..end], h.colors.text);
        pos += end;
    }
}

fn script_or_style(lower: &str) -> Option<&'static str> {
    if lower.starts_with("<script") {
        Some("script")
    } else if lower.starts_with("<style") {
        Some("style")
    } else {
        None
    }
}

fn find_tag_end(text: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, ch) in text.char_indices() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                }
            }
            None => {
                if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                } else if ch == '>' {
                    return Some(i + 1);
                }
            }
        }
    }
    None
}

fn html_tag(h: &mut Highlighter, tag: &str) {
    let mut pos = 0;
    let mut plain = 0;
    let bytes = tag.as_bytes();
    while pos < bytes.len() {
        let ch = tag[pos..].chars().next().unwrap();
        if ch == '"' || ch == '\'' {
            let quote = ch;
            let search_start = pos + quote.len_utf8();
            let end = tag[search_start..]
                .find(quote)
                .map(|i| search_start + i + quote.len_utf8())
                .unwrap_or(tag.len());
            if plain < pos {
                h.append(&tag[plain..pos], h.colors.text);
            }
            h.append(&tag[pos..end], h.colors.string);
            pos = end;
            plain = pos;
            continue;
        }
        if ch.is_ascii_alphabetic() || ch == '=' || ch == '-' || ch == '/' || ch == '<' {
            // Tag-Name am Anfang
            if pos == 0 && ch == '<' {
                let name_end = tag[1..]
                    .find(|c: char| !c.is_ascii_alphanumeric())
                    .map(|i| i + 1)
                    .unwrap_or(tag.len());
                if plain < pos {
                    h.append(&tag[plain..pos], h.colors.text);
                }
                h.append(&tag[..name_end], h.colors.tag);
                pos = name_end;
                plain = pos;
                continue;
            }
            // Attributname, gefolgt von '='
            if ch.is_ascii_alphabetic() {
                let rest = &tag[pos..];
                let name_len = rest
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == ':' || c == '_'))
                    .unwrap_or(rest.len());
                let name = &rest[..name_len];
                let after = &rest[name_len..];
                if after.trim_start().starts_with('=') {
                    if plain < pos {
                        h.append(&tag[plain..pos], h.colors.text);
                    }
                    h.append(name, h.colors.attribute);
                    pos += name_len;
                    plain = pos;
                    continue;
                }
            }
        }
        pos += ch.len_utf8();
    }
    if plain < tag.len() {
        h.append(&tag[plain..], h.colors.text);
    }
}

use crate::doc::DocKind;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FormatAction {
    Bold,
    Italic,
    Strikethrough,
    InlineCode,
    CodeBlock,
    Heading1,
    Heading2,
    Heading3,
    BulletList,
    NumberedList,
    Quote,
    Link,
    Image,
    Rule,
    Table,
}

impl FormatAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Bold => "Fett",
            Self::Italic => "Kursiv",
            Self::Strikethrough => "Durchgestrichen",
            Self::InlineCode => "Inline-Code",
            Self::CodeBlock => "Codeblock",
            Self::Heading1 => "Überschrift 1",
            Self::Heading2 => "Überschrift 2",
            Self::Heading3 => "Überschrift 3",
            Self::BulletList => "Aufzählungsliste",
            Self::NumberedList => "Nummerierte Liste",
            Self::Quote => "Zitat",
            Self::Link => "Link",
            Self::Image => "Bild",
            Self::Rule => "Trennlinie",
            Self::Table => "Tabelle",
        }
    }
}

pub fn char_to_byte(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len())
}

/// Ersetzt den Bereich `[start, end)` (Zeichenindizes) durch `replacement`
/// und liefert den Zeichenindex, an dem die Ersetzung beginnt.
pub fn replace_range(text: &mut String, start: usize, end: usize, replacement: &str) -> usize {
    let (start, end) = (start.min(end), start.max(end));
    let start_byte = char_to_byte(text, start);
    let end_byte = char_to_byte(text, end);

    let mut result = String::with_capacity(text.len() + replacement.len());
    result.push_str(&text[..start_byte]);
    let replaced_at = result.len();
    result.push_str(replacement);
    result.push_str(&text[end_byte..]);
    *text = result;

    text[..replaced_at].chars().count()
}

/// Umschließt die Auswahl mit `before`/`after` und liefert die neue Auswahl
/// (bzw. Cursorposition, wenn nichts ausgewählt war).
pub fn wrap_selection(
    text: &mut String,
    start: usize,
    end: usize,
    before: &str,
    after: &str,
) -> (usize, usize) {
    let (start, end) = (start.min(end), start.max(end));
    let start_byte = char_to_byte(text, start);
    let end_byte = char_to_byte(text, end);
    let selected = text[start_byte..end_byte].to_string();

    let mut result = String::with_capacity(text.len() + before.len() + after.len());
    result.push_str(&text[..start_byte]);
    result.push_str(before);
    let inner_start_byte = result.len();
    result.push_str(&selected);
    result.push_str(after);
    result.push_str(&text[end_byte..]);
    *text = result;

    let inner_start = text[..inner_start_byte].chars().count();
    let inner_end = inner_start + selected.chars().count();
    (inner_start, inner_end)
}

/// Stellt jeder Zeile der Auswahl `prefix` voran.
pub fn prefix_lines(text: &mut String, start: usize, end: usize, prefix: &str) -> (usize, usize) {
    let (start, end) = (start.min(end), start.max(end));
    let start_byte = char_to_byte(text, start);
    let end_byte = char_to_byte(text, end);

    let line_start_byte = text[..start_byte].rfind('\n').map_or(0, |i| i + 1);
    let line_end_byte = if end_byte > line_start_byte && text[..end_byte].ends_with('\n') {
        end_byte
    } else {
        text[end_byte..]
            .find('\n')
            .map_or(text.len(), |i| end_byte + i)
    };

    let block = text[line_start_byte..line_end_byte].to_string();
    let mut new_block = String::with_capacity(block.len() + prefix.len() * 8);
    for (index, line) in block.split('\n').enumerate() {
        if index > 0 {
            new_block.push('\n');
        }
        new_block.push_str(prefix);
        new_block.push_str(line);
    }

    let mut result = String::with_capacity(text.len() + new_block.len());
    result.push_str(&text[..line_start_byte]);
    let block_start_byte = result.len();
    result.push_str(&new_block);
    result.push_str(&text[line_end_byte..]);
    *text = result;

    let block_start = text[..block_start_byte].chars().count();
    let block_end = block_start + new_block.chars().count();
    (block_start, block_end)
}

/// Fügt einen Markdown-Link ein und wählt den sinnvollen Teil aus.
pub fn insert_link(text: &mut String, start: usize, end: usize) -> (usize, usize) {
    let (start, end) = (start.min(end), start.max(end));
    let start_byte = char_to_byte(text, start);
    let end_byte = char_to_byte(text, end);
    let selected = text[start_byte..end_byte].to_string();

    if selected.is_empty() {
        let pos = replace_range(text, start, end, "[Linktext](url)");
        let text_start = pos + 1;
        (text_start, text_start + "Linktext".chars().count())
    } else {
        let replacement = format!("[{selected}](url)");
        let pos = replace_range(text, start, end, &replacement);
        let url_start = pos + 1 + selected.chars().count() + 2;
        (url_start, url_start + 3)
    }
}

/// Fügt eine Markdown-Tabelle ein und markiert den ersten Spaltenkopf.
pub fn insert_table(text: &mut String, start: usize, end: usize) -> (usize, usize) {
    let table = "| Spalte 1 | Spalte 2 |\n| --- | --- |\n| Text | Text |";
    let pos = replace_range(text, start, end, table);
    let cell_start = pos + 2;
    (cell_start, cell_start + "Spalte 1".chars().count())
}

/// Fügt `![alt](pfad)` ein und markiert den Alternativtext zum direkten Überschreiben.
pub fn insert_image(
    text: &mut String,
    start: usize,
    end: usize,
    alt: &str,
    dest: &str,
) -> (usize, usize) {
    let (start, end) = (start.min(end), start.max(end));
    let (sb, eb) = (char_to_byte(text, start), char_to_byte(text, end));
    let selected = text[sb..eb].replace('\n', " ");
    let alt = if selected.trim().is_empty() {
        alt.to_string()
    } else {
        selected
    };

    let pos = replace_range(text, start, end, &format!("![{alt}]({dest})"));
    let alt_start = pos + 2;
    (alt_start, alt_start + alt.chars().count())
}

// ---------------------------------------------------------------------------
// Automatische Einrückung
// ---------------------------------------------------------------------------

/// Liefert den Text, der nach einem Zeilenumbruch automatisch eingefügt wird.
pub fn auto_indent(kind: DocKind, previous_line: &str) -> String {
    let indent: String = previous_line
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let rest = &previous_line[indent.len()..];
    let trimmed = rest.trim_end();

    match kind {
        DocKind::Markdown => {
            if trimmed.is_empty() {
                return String::new();
            }
            // Aufzählungsliste fortsetzen
            for marker in ["- ", "* ", "+ "] {
                if rest.starts_with(marker) {
                    return format!("{indent}{marker}");
                }
            }
            // Nummerierte Liste erhöhen
            let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
            if digits > 0 && rest[digits..].starts_with(". ") {
                let number: u64 = rest[..digits].parse().unwrap_or(1);
                return format!("{indent}{}. ", number + 1);
            }
            // Zitat fortsetzen
            if rest.starts_with('>') {
                return format!("{indent}> ");
            }
            // In einer offenen Codeumgebung nicht einrücken
            if rest.trim_start().starts_with("```") || rest.trim_start().starts_with("~~~") {
                return String::new();
            }
            indent
        }
        DocKind::Html => {
            if trimmed.is_empty() {
                return String::new();
            }
            let lower = trimmed.to_ascii_lowercase();
            if lower.starts_with("</") {
                return indent;
            }
            if lower.ends_with('>') && !lower.ends_with("/>") {
                let name: String = lower
                    .trim_start_matches('<')
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                let void = matches!(
                    name.as_str(),
                    "area"
                        | "base"
                        | "br"
                        | "col"
                        | "embed"
                        | "hr"
                        | "img"
                        | "input"
                        | "link"
                        | "meta"
                        | "param"
                        | "source"
                        | "track"
                        | "wbr"
                        | "!doctype"
                );
                if !void {
                    return format!("{indent}    ");
                }
            }
            indent
        }
    }
}

// ---------------------------------------------------------------------------
// Suchen
// ---------------------------------------------------------------------------

fn chars_equal(a: char, b: char, case_sensitive: bool) -> bool {
    if case_sensitive {
        a == b
    } else {
        a.to_lowercase().eq(b.to_lowercase())
    }
}

/// Sucht `needle` ab `from_char` (mit Umbruch am Dokumentende).
pub fn find_next(
    text: &str,
    needle: &str,
    case_sensitive: bool,
    from_char: usize,
) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    let hay: Vec<char> = text.chars().collect();
    let pat: Vec<char> = needle.chars().collect();
    if pat.len() > hay.len() {
        return None;
    }
    let n = hay.len();
    for offset in 0..=n {
        let start = (from_char + offset) % (n + 1);
        if start + pat.len() > n {
            continue;
        }
        if hay[start..start + pat.len()]
            .iter()
            .zip(&pat)
            .all(|(a, b)| chars_equal(*a, *b, case_sensitive))
        {
            return Some((start, start + pat.len()));
        }
    }
    None
}

pub fn find_all(text: &str, needle: &str, case_sensitive: bool) -> Vec<(usize, usize)> {
    let mut result = Vec::new();
    if needle.is_empty() {
        return result;
    }
    let hay: Vec<char> = text.chars().collect();
    let pat: Vec<char> = needle.chars().collect();
    if pat.len() > hay.len() {
        return result;
    }
    let n = hay.len();
    let mut start = 0;
    while start + pat.len() <= n {
        if hay[start..start + pat.len()]
            .iter()
            .zip(&pat)
            .all(|(a, b)| chars_equal(*a, *b, case_sensitive))
        {
            result.push((start, start + pat.len()));
            start += pat.len();
        } else {
            start += 1;
        }
    }
    result
}

// ---------------------------------------------------------------------------
// HTML-Formatierung
// ---------------------------------------------------------------------------

fn is_block_tag(name: &str) -> bool {
    matches!(
        name,
        "html"
            | "head"
            | "body"
            | "div"
            | "p"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "ul"
            | "ol"
            | "li"
            | "dl"
            | "dt"
            | "dd"
            | "table"
            | "thead"
            | "tbody"
            | "tfoot"
            | "tr"
            | "td"
            | "th"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "nav"
            | "aside"
            | "main"
            | "figure"
            | "figcaption"
            | "form"
            | "fieldset"
            | "blockquote"
            | "pre"
            | "hr"
            | "script"
            | "style"
            | "link"
            | "meta"
            | "title"
            | "colgroup"
            | "caption"
    )
}

fn is_void_tag(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
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

/// Formatiert HTML mit einheitlicher Einrückung (4 Leerzeichen).
pub fn format_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + input.len() / 4);
    let mut depth = 0usize;
    let mut line = String::new();
    let mut pos = 0;

    let flush_line = |out: &mut String, line: &mut String, depth: usize| {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            out.push_str(&"    ".repeat(depth));
            out.push_str(trimmed);
            out.push('\n');
        }
        line.clear();
    };

    while pos < input.len() {
        let rest = &input[pos..];

        // Kommentare
        if rest.starts_with("<!--") {
            let end = rest.find("-->").map(|i| i + 3).unwrap_or(rest.len());
            flush_line(&mut out, &mut line, depth);
            out.push_str(&"    ".repeat(depth));
            out.push_str(rest[..end].trim());
            out.push('\n');
            pos += end;
            continue;
        }

        // Rohe Inhalte (script, style, pre, textarea)
        let lower = rest.to_ascii_lowercase();
        let raw_tag = ["script", "style", "pre", "textarea"]
            .iter()
            .find(|tag| lower.starts_with(&format!("<{tag}")))
            .copied();
        if let Some(tag) = raw_tag {
            if let Some(open_end) = find_tag_end(rest) {
                let open = &rest[..open_end];
                let close = format!("</{tag}>");
                let inner_start = open_end;
                let lower_rest = &lower[inner_start..];
                let inner_end = lower_rest
                    .find(&close)
                    .map(|i| inner_start + i)
                    .unwrap_or(input.len() - pos);
                flush_line(&mut out, &mut line, depth);
                out.push_str(&"    ".repeat(depth));
                out.push_str(open.trim());
                out.push('\n');
                let inner = input[pos + inner_start..pos + inner_end].trim_matches('\n');
                if !inner.trim().is_empty() {
                    out.push_str(inner);
                    out.push('\n');
                }
                out.push_str(&"    ".repeat(depth));
                out.push_str(&close);
                out.push('\n');
                pos += inner_end + close.len();
                continue;
            }
        }

        // Tags
        if rest.starts_with('<') {
            let end = find_tag_end(rest).unwrap_or(rest.len());
            let tag = &rest[..end];
            let closing = tag.starts_with("</");
            let name: String = tag
                .trim_start_matches('<')
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '!')
                .collect::<String>()
                .to_ascii_lowercase();
            let self_closing = tag.trim_end().ends_with("/>");
            if is_block_tag(&name) || tag.starts_with("<!") {
                flush_line(&mut out, &mut line, depth);
                if closing {
                    depth = depth.saturating_sub(1);
                }
                out.push_str(&"    ".repeat(depth));
                out.push_str(tag.trim());
                out.push('\n');
                if !closing && !self_closing && !is_void_tag(&name) && !tag.starts_with("<!") {
                    depth += 1;
                }
            } else {
                line.push_str(tag);
            }
            pos += end;
            continue;
        }

        // Text
        let end = rest.find('<').unwrap_or(rest.len());
        let raw = &rest[..end];
        let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if !(line.is_empty() && collapsed.is_empty()) {
            line.push_str(&collapsed);
            let ends_with_space = raw.chars().last().map_or(true, |c| c.is_whitespace());
            if !ends_with_space && !collapsed.is_empty() {
                line.push(' ');
            }
        }
        pos += end;
    }

    flush_line(&mut out, &mut line, depth);
    out.trim_end().to_string()
}

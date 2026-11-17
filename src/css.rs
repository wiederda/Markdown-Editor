use std::path::{Path, PathBuf};

use eframe::egui::{Align, Color32};

/// Vereinfachte CSS-Deklarationen, die die Vorschau unterstützt.
#[derive(Clone, Default, Debug)]
pub struct Declarations {
    pub color: Option<Color32>,
    pub background: Option<Color32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    pub mono: Option<bool>,
    pub font_size: Option<f32>,
    pub align: Option<Align>,
    pub margin_left: Option<f32>,
    pub margin_top: Option<f32>,
    pub margin_bottom: Option<f32>,
}

impl Declarations {
    pub fn merge(&mut self, other: &Declarations) {
        macro_rules! merge_fields {
            ($($field:ident),* $(,)?) => {
                $(if other.$field.is_some() { self.$field = other.$field; })*
            };
        }
        merge_fields!(
            color,
            background,
            bold,
            italic,
            underline,
            strike,
            mono,
            font_size,
            align,
            margin_left,
            margin_top,
            margin_bottom,
        );
    }

    fn is_empty(&self) -> bool {
        self.color.is_none()
            && self.background.is_none()
            && self.bold.is_none()
            && self.italic.is_none()
            && self.underline.is_none()
            && self.strike.is_none()
            && self.mono.is_none()
            && self.font_size.is_none()
            && self.align.is_none()
            && self.margin_left.is_none()
            && self.margin_top.is_none()
            && self.margin_bottom.is_none()
    }
}

#[derive(Clone, Debug)]
struct Selector {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
}

#[derive(Clone, Debug)]
struct Rule {
    selector: Selector,
    declarations: Declarations,
    dark_only: bool,
    light_only: bool,
}

#[derive(Default, Debug)]
pub struct Stylesheet {
    rules: Vec<Rule>,
}

impl Stylesheet {
    /// Liest `<style>`-Blöcke und verlinkte CSS-Dateien aus einem HTML-Dokument.
    pub fn load(html: &str, base_dir: Option<&Path>, dark: bool) -> Self {
        let mut rules = Vec::new();
        for css in style_blocks(html) {
            parse_rules(&css, false, false, &mut rules);
        }
        for href in stylesheet_links(html) {
            if let Some(path) = resolve_href(base_dir, &href) {
                if let Ok(css) = std::fs::read_to_string(&path) {
                    parse_rules(&css, false, false, &mut rules);
                }
            }
        }
        let mut sheet = Self { rules };
        if !dark {
            sheet.rules.retain(|rule| !rule.dark_only);
        } else {
            sheet.rules.retain(|rule| !rule.light_only);
        }
        sheet
    }

    /// Liefert die zusammengeführten Deklarationen für ein Element.
    pub fn declarations_for(
        &self,
        tag: &str,
        id: Option<&str>,
        classes: &[&str],
        dark: bool,
    ) -> Declarations {
        let mut result = Declarations::default();
        for rule in &self.rules {
            if rule.dark_only && !dark {
                continue;
            }
            if rule.light_only && dark {
                continue;
            }
            if selector_matches(&rule.selector, tag, id, classes) {
                result.merge(&rule.declarations);
            }
        }
        result
    }
}

fn selector_matches(
    selector: &Selector,
    tag: &str,
    id: Option<&str>,
    classes: &[&str],
) -> bool {
    if let Some(expected) = &selector.tag {
        if !expected.eq_ignore_ascii_case(tag) {
            return false;
        }
    }
    if let Some(expected) = &selector.id {
        if id != Some(expected.as_str()) {
            return false;
        }
    }
    for class in &selector.classes {
        if !classes.iter().any(|c| c.eq_ignore_ascii_case(class)) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// HTML-Extraktion
// ---------------------------------------------------------------------------

fn style_blocks(html: &str) -> Vec<String> {
    let mut result = Vec::new();
    let lower = html.to_ascii_lowercase();
    let mut pos = 0;
    while let Some(start) = lower[pos..].find("<style") {
        let start = pos + start;
        let Some(open_end) = lower[start..].find('>').map(|i| start + i + 1) else {
            break;
        };
        let Some(close) = lower[open_end..].find("</style") else {
            break;
        };
        let close = open_end + close;
        result.push(html[open_end..close].to_owned());
        pos = close + "</style".len();
    }
    result
}

fn stylesheet_links(html: &str) -> Vec<String> {
    let mut result = Vec::new();
    let lower = html.to_ascii_lowercase();
    let mut pos = 0;
    while let Some(start) = lower[pos..].find("<link") {
        let start = pos + start;
        let Some(end) = html[start..].find('>').map(|i| start + i + 1) else {
            break;
        };
        let tag = &html[start..end];
        let tag_lower = tag.to_ascii_lowercase();
        if tag_lower.contains("stylesheet") {
            if let Some(href) = attribute(tag, "href") {
                if !href.is_empty() {
                    result.push(href);
                }
            }
        }
        pos = end;
    }
    result
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut search = 0;
    while let Some(found) = lower[search..].find(name) {
        let index = search + found;
        let before_ok = index == 0
            || lower.as_bytes()[index - 1].is_ascii_whitespace()
            || lower.as_bytes()[index - 1] == b'<';
        let after = index + name.len();
        if before_ok && lower[after..].trim_start().starts_with('=') {
            let rest = &tag[after..];
            let eq = rest.find('=')?;
            let rest = rest[eq + 1..].trim_start();
            let quote = rest.chars().next()?;
            if quote == '"' || quote == '\'' {
                let inner = &rest[quote.len_utf8()..];
                let end = inner.find(quote)?;
                return Some(inner[..end].to_owned());
            } else {
                let end = rest
                    .find(|c: char| c.is_whitespace() || c == '>')
                    .unwrap_or(rest.len());
                return Some(rest[..end].to_owned());
            }
        }
        search = index + name.len();
    }
    None
}

fn resolve_href(base_dir: Option<&Path>, href: &str) -> Option<PathBuf> {
    let href = href.trim();
    if href.starts_with("http://") || href.starts_with("https://") || href.starts_with("data:") {
        return None;
    }
    let cleaned = href
        .trim_start_matches("file://")
        .split(['?', '#'])
        .next()
        .unwrap_or(href);
    let decoded = percent_decode(cleaned);
    let path = PathBuf::from(decoded);
    if path.is_absolute() {
        Some(path)
    } else {
        base_dir.map(|base| base.join(path))
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(value) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------------------------------------------------------------------------
// CSS-Parser
// ---------------------------------------------------------------------------

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut pos = 0;
    while let Some(start) = css[pos..].find("/*") {
        let start = pos + start;
        out.push_str(&css[pos..start]);
        if let Some(end) = css[start + 2..].find("*/") {
            pos = start + 2 + end + 2;
        } else {
            pos = css.len();
            break;
        }
    }
    out.push_str(&css[pos..]);
    out
}

fn find_matching_brace(css: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, ch) in css[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_rules(css: &str, dark: bool, light: bool, rules: &mut Vec<Rule>) {
    let css = strip_comments(css);
    let mut pos = 0;
    while pos < css.len() {
        let Some(open) = css[pos..].find('{').map(|i| pos + i) else {
            break;
        };
        let selector_text = css[pos..open].trim().to_owned();
        let Some(close) = find_matching_brace(&css, open) else {
            break;
        };
        let body = &css[open + 1..close];
        let next = close + 1;

        if selector_text.starts_with('@') {
            let lower = selector_text.to_ascii_lowercase();
            let (next_dark, next_light) = if lower.contains("prefers-color-scheme") {
                (
                    dark || lower.contains("dark"),
                    light || lower.contains("light"),
                )
            } else if lower.starts_with("@media") {
                // Andere Medien (z. B. print) ignorieren.
                (true, true)
            } else {
                (dark, light)
            };
            if !(selector_text.starts_with("@media") && !lower.contains("prefers-color-scheme")) {
                parse_rules(body, next_dark, next_light, rules);
            }
            pos = next;
            continue;
        }

        let declarations = parse_declarations(body);
        if !declarations.is_empty() {
            for part in selector_text.split(',') {
                if let Some(selector) = parse_selector(part) {
                    rules.push(Rule {
                        selector,
                        declarations: declarations.clone(),
                        dark_only: dark && !light,
                        light_only: light && !dark,
                    });
                }
            }
        }
        pos = next;
    }
}

fn parse_selector(text: &str) -> Option<Selector> {
    let text = text.trim();
    if text.is_empty()
        || text.contains(' ')
        || text.contains('>')
        || text.contains('+')
        || text.contains('~')
        || text.contains('[')
        || text.contains(':')
        || text.contains('*')
    {
        return None;
    }
    let mut selector = Selector {
        tag: None,
        id: None,
        classes: Vec::new(),
    };
    let mut pos = 0;
    let bytes = text.as_bytes();
    while pos < bytes.len() {
        match bytes[pos] {
            b'#' => {
                let end = text[pos + 1..]
                    .find(['.', '#'])
                    .map(|i| pos + 1 + i)
                    .unwrap_or(text.len());
                selector.id = Some(text[pos + 1..end].to_owned());
                pos = end;
            }
            b'.' => {
                let end = text[pos + 1..]
                    .find(['.', '#'])
                    .map(|i| pos + 1 + i)
                    .unwrap_or(text.len());
                selector.classes.push(text[pos + 1..end].to_owned());
                pos = end;
            }
            _ => {
                let end = text[pos..]
                    .find(['.', '#'])
                    .map(|i| pos + i)
                    .unwrap_or(text.len());
                selector.tag = Some(text[pos..end].to_owned());
                pos = end;
            }
        }
    }
    Some(selector)
}

/// Nur die Deklarationen aus einem `style="…"`-Attribut.
pub fn parse_inline_declarations(style: &str) -> Declarations {
    parse_declarations(style)
}

fn parse_declarations(body: &str) -> Declarations {
    let mut declarations = Declarations::default();
    for part in body.split(';') {
        let Some(colon) = part.find(':') else {
            continue;
        };
        let name = part[..colon].trim().to_ascii_lowercase();
        let value = part[colon + 1..].trim();
        if value.is_empty() {
            continue;
        }
        match name.as_str() {
            "color" => {
                if let Some(color) = parse_color(value) {
                    declarations.color = Some(color);
                }
            }
            "background" | "background-color" => {
                if let Some(color) = parse_color(value) {
                    declarations.background = Some(color);
                }
            }
            "font-weight" => {
                let bold = value.eq_ignore_ascii_case("bold")
                    || value.eq_ignore_ascii_case("bolder")
                    || value.parse::<u32>().map_or(false, |v| v >= 600);
                declarations.bold = Some(bold);
            }
            "font-style" => {
                declarations.italic = Some(value.eq_ignore_ascii_case("italic"));
            }
            "text-decoration" | "text-decoration-line" => {
                let lower = value.to_ascii_lowercase();
                declarations.underline = Some(lower.contains("underline"));
                declarations.strike = Some(lower.contains("line-through"));
            }
            "font-family" => {
                let lower = value.to_ascii_lowercase();
                declarations.mono = Some(
                    lower.contains("mono") || lower.contains("courier") || lower.contains("consolas"),
                );
            }
            "font-size" => {
                declarations.font_size = parse_length(value, 16.0);
            }
            "text-align" => {
                declarations.align = match value.to_ascii_lowercase().as_str() {
                    "left" | "start" => Some(Align::LEFT),
                    "center" => Some(Align::Center),
                    "right" | "end" => Some(Align::RIGHT),
                    _ => None,
                };
            }
            "margin-left" | "padding-left" => {
                if let Some(length) = parse_length(value, 16.0) {
                    declarations.margin_left = Some(length);
                }
            }
            "margin-top" | "padding-top" => {
                if let Some(length) = parse_length(value, 16.0) {
                    declarations.margin_top = Some(length);
                }
            }
            "margin-bottom" | "padding-bottom" => {
                if let Some(length) = parse_length(value, 16.0) {
                    declarations.margin_bottom = Some(length);
                }
            }
            _ => {}
        }
    }
    declarations
}

fn parse_length(value: &str, base: f32) -> Option<f32> {
    let value = value.trim();
    let number_end = value
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(value.len());
    let number: f32 = value[..number_end].parse().ok()?;
    let unit = value[number_end..].trim().to_ascii_lowercase();
    Some(match unit.as_str() {
        "px" | "" => number,
        "pt" => number * 4.0 / 3.0,
        "em" | "rem" => number * base,
        "%" => number / 100.0 * base,
        "cm" => number * 37.8,
        "mm" => number * 3.78,
        "in" => number * 96.0,
        _ => number,
    })
}

fn parse_color(value: &str) -> Option<Color32> {
    let value = value.trim().to_ascii_lowercase();
    if let Some(hex) = value.strip_prefix('#') {
        return match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
                let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
                let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
                Some(Color32::from_rgb(r, g, b))
            }
            4 => {
                let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
                let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
                let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
                let a = u8::from_str_radix(&hex[3..4], 16).ok()? * 17;
                Some(Color32::from_rgba_unmultiplied(r, g, b, a))
            }
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Color32::from_rgb(r, g, b))
            }
            8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                Some(Color32::from_rgba_unmultiplied(r, g, b, a))
            }
            _ => None,
        };
    }
    if let Some(inner) = value
        .strip_prefix("rgb(")
        .or_else(|| value.strip_prefix("rgba("))
        .and_then(|v| v.strip_suffix(')'))
    {
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        if parts.len() >= 3 {
            let r: u8 = parts[0].parse().ok()?;
            let g: u8 = parts[1].parse().ok()?;
            let b: u8 = parts[2].parse().ok()?;
            let a = if parts.len() >= 4 {
                (parts[3].parse::<f32>().ok()? * 255.0).round() as u8
            } else {
                255
            };
            return Some(Color32::from_rgba_unmultiplied(r, g, b, a));
        }
        return None;
    }
    let color = match value.as_str() {
        "black" => Color32::BLACK,
        "white" => Color32::WHITE,
        "red" => Color32::from_rgb(200, 0, 0),
        "green" => Color32::from_rgb(0, 128, 0),
        "blue" => Color32::from_rgb(0, 0, 200),
        "gray" | "grey" => Color32::from_rgb(128, 128, 128),
        "silver" => Color32::from_rgb(192, 192, 192),
        "yellow" => Color32::from_rgb(230, 220, 0),
        "orange" => Color32::from_rgb(230, 140, 0),
        "purple" => Color32::from_rgb(128, 0, 128),
        "navy" => Color32::from_rgb(0, 0, 128),
        "teal" => Color32::from_rgb(0, 128, 128),
        "maroon" => Color32::from_rgb(128, 0, 0),
        "olive" => Color32::from_rgb(128, 128, 0),
        "lime" => Color32::from_rgb(0, 200, 0),
        "aqua" | "cyan" => Color32::from_rgb(0, 200, 200),
        "fuchsia" | "magenta" => Color32::from_rgb(200, 0, 200),
        "transparent" => Color32::TRANSPARENT,
        _ => return None,
    };
    Some(color)
}

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eframe::egui::{
    self,
    text::{LayoutJob, TextFormat},
    Align, Color32, FontId, Label, RichText, Stroke, TextStyle, TextureHandle, TextureOptions,
};
use tl::{HTMLTag, NodeHandle, Parser};

use crate::css::{self, Declarations, Stylesheet};

// ---------------------------------------------------------------------------
// Bild-Cache
// ---------------------------------------------------------------------------

struct ImageEntry {
    modified: Option<SystemTime>,
    texture: Option<TextureHandle>,
}

#[derive(Default)]
pub struct ImageCache {
    entries: HashMap<PathBuf, ImageEntry>,
}

impl ImageCache {
    pub fn texture(&mut self, ctx: &egui::Context, path: &Path) -> Option<TextureHandle> {
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        if let Some(entry) = self.entries.get(path) {
            if entry.modified == modified {
                return entry.texture.clone();
            }
        }
        let texture = load_texture(ctx, path);
        self.entries.insert(
            path.to_path_buf(),
            ImageEntry {
                modified,
                texture: texture.clone(),
            },
        );
        texture
    }
}

fn load_texture(ctx: &egui::Context, path: &Path) -> Option<TextureHandle> {
    let bytes = std::fs::read(path).ok()?;
    let image = image::load_from_memory(&bytes).ok()?;
    let image = if image.width() > 2400 || image.height() > 2400 {
        image.thumbnail(2400, 2400)
    } else {
        image
    };
    let rgba = image.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    Some(ctx.load_texture(
        path.to_string_lossy(),
        color_image,
        TextureOptions::LINEAR,
    ))
}

/// Löst einen `src`/`href`-Wert relativ zum Dokumentordner auf.
pub fn resolve_resource(base_dir: Option<&Path>, value: &str) -> Option<PathBuf> {
    let value = value.trim();
    if value.is_empty()
        || value.starts_with("http://")
        || value.starts_with("https://")
        || value.starts_with("data:")
        || value.starts_with('#')
    {
        return None;
    }
    let cleaned = value
        .trim_start_matches("file://")
        .split(['?', '#'])
        .next()
        .unwrap_or(value);
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
// Stile
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct ComputedStyle {
    color: Color32,
    background: Option<Color32>,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    mono: bool,
    size: f32,
    align: Align,
    margin_left: f32,
    space_before: f32,
    space_after: f32,
}

impl ComputedStyle {
    fn apply(&mut self, declarations: &Declarations, dark: bool) {
        if let Some(color) = declarations.color {
            self.color = color;
        }
        if let Some(background) = declarations.background {
            self.background = Some(background);
        }
        if let Some(bold) = declarations.bold {
            self.bold = bold;
        }
        if let Some(italic) = declarations.italic {
            self.italic = italic;
        }
        if let Some(underline) = declarations.underline {
            self.underline = underline;
        }
        if let Some(strike) = declarations.strike {
            self.strike = strike;
        }
        if let Some(mono) = declarations.mono {
            self.mono = mono;
        }
        if let Some(size) = declarations.font_size {
            self.size = size.clamp(6.0, 96.0);
        }
        if let Some(align) = declarations.align {
            self.align = align;
        }
        if let Some(margin) = declarations.margin_left {
            self.margin_left = margin.max(0.0).min(200.0);
        }
        if let Some(margin) = declarations.margin_top {
            self.space_before = margin.max(0.0).min(120.0);
        }
        if let Some(margin) = declarations.margin_bottom {
            self.space_after = margin.max(0.0).min(120.0);
        }
        let _ = dark;
    }

    fn font_id(&self) -> FontId {
        if self.mono {
            FontId::monospace(self.size)
        } else {
            FontId::proportional(self.size)
        }
    }

    fn text_format(&self) -> TextFormat {
        TextFormat {
            font_id: self.font_id(),
            color: self.color,
            background: self.background.unwrap_or(Color32::TRANSPARENT),
            italics: self.italic,
            underline: if self.underline {
                Stroke::new(1.0_f32, self.color)
            } else {
                Stroke::NONE
            },
            strikethrough: if self.strike {
                Stroke::new(1.0_f32, self.color)
            } else {
                Stroke::NONE
            },
            ..Default::default()
        }
    }
}

#[derive(Clone)]
struct Run {
    text: String,
    style: ComputedStyle,
    link: bool,
}

enum InlineItem {
    Run(Run),
    Break,
    Image { src: String, alt: String },
}

// ---------------------------------------------------------------------------
// Renderer
// ---------------------------------------------------------------------------

struct Renderer<'a, 'p> {
    parser: &'p Parser<'a>,
    sheet: Stylesheet,
    base_dir: Option<PathBuf>,
    images: &'p mut ImageCache,
    dark: bool,
    hyperlink: Color32,
    weak: Color32,
    code_bg: Color32,
}

/// Rendert HTML in egui-Widgets (Vorschau für HTML-Dokumente).
pub fn render(
    ui: &mut egui::Ui,
    html: &str,
    base_dir: Option<&Path>,
    images: &mut ImageCache,
) {
    let Ok(dom) = tl::parse(html, tl::ParserOptions::default()) else {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "Das HTML-Dokument konnte nicht gelesen werden.",
        );
        return;
    };
    let parser = dom.parser();
    let dark = ui.visuals().dark_mode;
    let sheet = Stylesheet::load(html, base_dir, dark);

    let base_size = ui
        .style()
        .text_styles
        .get(&TextStyle::Body)
        .map(|font| font.size)
        .unwrap_or(14.0);
    let base_style = ComputedStyle {
        color: ui.visuals().text_color(),
        background: None,
        bold: false,
        italic: false,
        underline: false,
        strike: false,
        mono: false,
        size: base_size,
        align: Align::LEFT,
        margin_left: 0.0,
        space_before: 0.0,
        space_after: 0.0,
    };

    let hyperlink = ui.visuals().hyperlink_color;
    let weak = ui.visuals().weak_text_color();
    let code_bg = if dark {
        Color32::from_rgba_unmultiplied(120, 140, 160, 30)
    } else {
        Color32::from_rgba_unmultiplied(60, 80, 100, 24)
    };

    let mut renderer = Renderer {
        parser,
        sheet,
        base_dir: base_dir.map(Path::to_path_buf),
        images,
        dark,
        hyperlink,
        weak,
        code_bg,
    };

    let body_handle = dom
        .query_selector("body")
        .and_then(|mut iter| iter.next())
        .and_then(|handle| handle.get(parser).and_then(|node| node.as_tag()))
        .map(|tag| (tag.name().as_utf8_str().into_owned(), tag));

    if let Some((_, body)) = body_handle.as_ref() {
        let style = renderer.style_for(body, &base_style, "body");
        if let Some(background) = style.background {
            ui.painter()
                .rect_filled(ui.max_rect(), 0.0, background);
        }
        renderer.render_nodes(ui, body.children().top().as_slice(), &style);
    } else {
        renderer.render_nodes(ui, dom.children(), &base_style);
    }
}

impl<'a, 'p> Renderer<'a, 'p> {
    fn style_for(&self, tag: &HTMLTag<'a>, inherited: &ComputedStyle, name: &str) -> ComputedStyle {
        let mut style = inherited.clone();
        style.background = None;
        style.margin_left = 0.0;
        style.space_before = 0.0;
        style.space_after = 0.0;

        match name {
            "h1" => {
                style.size = inherited.size * 1.9;
                style.bold = true;
                style.space_before = 14.0;
                style.space_after = 6.0;
            }
            "h2" => {
                style.size = inherited.size * 1.55;
                style.bold = true;
                style.space_before = 12.0;
                style.space_after = 5.0;
            }
            "h3" => {
                style.size = inherited.size * 1.3;
                style.bold = true;
                style.space_before = 10.0;
                style.space_after = 4.0;
            }
            "h4" | "h5" | "h6" => {
                style.size = inherited.size * 1.1;
                style.bold = true;
                style.space_before = 8.0;
                style.space_after = 3.0;
            }
            "p" => {
                style.space_after = 8.0;
            }
            "li" => {
                style.space_after = 2.0;
            }
            "blockquote" => {
                style.space_before = 6.0;
                style.space_after = 6.0;
            }
            "pre" => {
                style.mono = true;
                style.space_before = 6.0;
                style.space_after = 8.0;
            }
            "code" | "kbd" | "samp" | "var" => {
                style.mono = true;
                if style.background.is_none() {
                    style.background = Some(self.code_bg);
                }
            }
            "strong" | "b" => style.bold = true,
            "em" | "i" | "cite" | "dfn" => style.italic = true,
            "u" | "ins" => style.underline = true,
            "s" | "del" | "strike" => style.strike = true,
            "small" => style.size *= 0.85,
            "mark" => {
                style.background = Some(if self.dark {
                    Color32::from_rgb(120, 100, 20)
                } else {
                    Color32::from_rgb(255, 240, 140)
                });
            }
            "a" => {
                style.color = self.hyperlink;
                style.underline = true;
            }
            "th" => {
                style.bold = true;
            }
            _ => {}
        }

        let classes: Vec<String> = tag
            .attributes()
            .class()
            .map(|class| {
                class
                    .as_utf8_str()
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let class_refs: Vec<&str> = classes.iter().map(String::as_str).collect();
        let id = tag
            .attributes()
            .id()
            .map(|id| id.as_utf8_str().into_owned());
        let declarations = self
            .sheet
            .declarations_for(name, id.as_deref(), &class_refs, self.dark);
        style.apply(&declarations, self.dark);

        if let Some(inline) = attribute(tag, "style") {
            let declarations = css::parse_inline_declarations(&inline);
            style.apply(&declarations, self.dark);
        }
        style
    }

    fn render_nodes(
        &mut self,
        ui: &mut egui::Ui,
        nodes: &[NodeHandle],
        inherited: &ComputedStyle,
    ) {
        let mut inline: Vec<InlineItem> = Vec::new();
        for handle in nodes {
            let Some(node) = handle.get(self.parser) else {
                continue;
            };
            match node {
                tl::Node::Raw(text) => {
                    let text = decode_entities(&text.as_utf8_str());
                    push_text(&mut inline, text, inherited.clone(), false);
                }
                tl::Node::Comment(_) => {}
                tl::Node::Tag(tag) => {
                    let name = tag.name().as_utf8_str().to_lowercase();
                    if matches!(
                        name.as_str(),
                        "script" | "style" | "head" | "title" | "meta" | "link" | "base"
                    ) {
                        continue;
                    }
                    if matches!(name.as_str(), "br") {
                        inline.push(InlineItem::Break);
                        continue;
                    }
                    if name == "img" {
                        if let Some(src) = attribute(tag, "src") {
                            let alt = attribute(tag, "alt").unwrap_or_default();
                            inline.push(InlineItem::Image { src, alt });
                        }
                        continue;
                    }
                    let style = self.style_for(tag, inherited, &name);
                    if is_block(&name) {
                        self.flush_inline(ui, &mut inline);
                        self.render_block(ui, tag, &name, &style);
                    } else if name == "a" {
                        self.collect_inline(tag, style, &mut inline, true);
                    } else {
                        self.collect_inline(tag, style, &mut inline, false);
                    }
                }
            }
        }
        self.flush_inline(ui, &mut inline);
    }

    fn collect_inline(
        &mut self,
        tag: &HTMLTag<'a>,
        style: ComputedStyle,
        out: &mut Vec<InlineItem>,
        link: bool,
    ) {
        for handle in tag.children().top().iter() {
            let Some(node) = handle.get(self.parser) else {
                continue;
            };
            if let Some(text) = node.as_raw() {
                let text = decode_entities(&text.as_utf8_str());
                push_text(out, text, style.clone(), link);
            } else if let Some(child) = node.as_tag() {
                let name = child.name().as_utf8_str().to_lowercase();
                match name.as_str() {
                    "script" | "style" => {}
                    "br" => out.push(InlineItem::Break),
                    "img" => {
                        if let Some(src) = attribute(child, "src") {
                            let alt = attribute(child, "alt").unwrap_or_default();
                            out.push(InlineItem::Image { src, alt });
                        }
                    }
                    _ if is_block(&name) => {
                        out.push(InlineItem::Break);
                        let child_style = self.style_for(child, &style, &name);
                        self.collect_inline(child, child_style, out, link);
                        out.push(InlineItem::Break);
                    }
                    "a" => {
                        let child_style = self.style_for(child, &style, &name);
                        self.collect_inline(child, child_style, out, true);
                    }
                    _ => {
                        let child_style = self.style_for(child, &style, &name);
                        self.collect_inline(child, child_style, out, link);
                    }
                }
            }
        }
    }

    fn flush_inline(&mut self, ui: &mut egui::Ui, items: &mut Vec<InlineItem>) {
        if items.is_empty() {
            return;
        }
        let mut items = std::mem::take(items);
        trim_whitespace(&mut items);
        let mut index = 0;
        while index < items.len() {
            if let InlineItem::Image { src, alt } = &items[index] {
                let src = src.clone();
                let alt = alt.clone();
                self.render_image(ui, &src, &alt);
                index += 1;
                continue;
            }
            let start = index;
            while index < items.len() && !matches!(items[index], InlineItem::Image { .. }) {
                index += 1;
            }
            self.add_text_job(ui, &items[start..index]);
        }
    }

    fn add_text_job(&self, ui: &mut egui::Ui, items: &[InlineItem]) {
        if items.is_empty() {
            return;
        }
        let mut job = LayoutJob::default();
        job.wrap.max_width = ui.available_width().max(64.0);
        for item in items {
            match item {
                InlineItem::Run(run) => {
                    let mut format = run.style.text_format();
                    if run.link {
                        format.color = self.hyperlink;
                        format.underline = Stroke::new(1.0_f32, self.hyperlink);
                    }
                    job.append(&run.text, 0.0, format);
                }
                InlineItem::Break => {
                    let format = items
                        .iter()
                        .find_map(|item| match item {
                            InlineItem::Run(run) => Some(run.style.text_format()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    job.append("\n", 0.0, format);
                }
                InlineItem::Image { .. } => {}
            }
        }
        if job.text.trim().is_empty() {
            return;
        }
        ui.add(Label::new(job));
    }

    fn render_block(
        &mut self,
        ui: &mut egui::Ui,
        tag: &HTMLTag<'a>,
        name: &str,
        style: &ComputedStyle,
    ) {
        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p" | "div" | "section" | "article"
            | "header" | "footer" | "nav" | "aside" | "main" | "figure" | "figcaption" | "form"
            | "fieldset" | "details" | "summary" | "address" | "body" | "html" | "dl" | "dt"
            | "dd" | "center" => {
                if style.space_before > 0.0 {
                    ui.add_space(style.space_before);
                }
                if let Some(background) = style.background {
                    egui::Frame::new()
                        .fill(background)
                        .inner_margin(egui::Margin::same(6))
                        .show(ui, |ui| {
                            self.render_nodes(ui, tag.children().top().as_slice(), style);
                        });
                } else if style.margin_left > 0.0 {
                    ui.horizontal_top(|ui| {
                        ui.add_space(style.margin_left);
                        ui.vertical(|ui| {
                            self.render_nodes(ui, tag.children().top().as_slice(), style);
                        });
                    });
                } else {
                    self.render_nodes(ui, tag.children().top().as_slice(), style);
                }
                if style.space_after > 0.0 {
                    ui.add_space(style.space_after);
                }
            }
            "ul" | "ol" => self.render_list(ui, tag, name == "ol", style),
            "blockquote" => {
                if style.space_before > 0.0 {
                    ui.add_space(style.space_before);
                }
                let start_y = ui.cursor().top();
                let x = ui.available_rect_before_wrap().left() + 3.0;
                ui.horizontal_top(|ui| {
                    ui.add_space(14.0);
                    ui.vertical(|ui| {
                        self.render_nodes(ui, tag.children().top().as_slice(), style);
                    });
                });
                let end_y = ui.cursor().top();
                let color = if self.dark {
                    Color32::from_rgb(80, 90, 100)
                } else {
                    Color32::from_rgb(205, 210, 215)
                };
                ui.painter()
                    .vline(x, start_y..=end_y, Stroke::new(3.0_f32, color));
                if style.space_after > 0.0 {
                    ui.add_space(style.space_after);
                }
            }
            "pre" => {
                if style.space_before > 0.0 {
                    ui.add_space(style.space_before);
                }
                let text = tag.inner_text(self.parser);
                let mut format = style.text_format();
                format.background = Color32::TRANSPARENT;
                let job = LayoutJob::single_section(text.trim_matches('\n').to_owned(), format);
                egui::Frame::new()
                    .fill(self.code_bg)
                    .corner_radius(4)
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        egui::ScrollArea::horizontal().show(ui, |ui| {
                            ui.add(Label::new(job).wrap_mode(egui::TextWrapMode::Extend));
                        });
                    });
                if style.space_after > 0.0 {
                    ui.add_space(style.space_after);
                }
            }
            "hr" => {
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);
            }
            "table" => self.render_table(ui, tag, style),
            "script" | "style" | "head" | "title" => {}
            _ => {
                self.render_nodes(ui, tag.children().top().as_slice(), style);
            }
        }
    }

    fn render_list(
        &mut self,
        ui: &mut egui::Ui,
        tag: &HTMLTag<'a>,
        ordered: bool,
        style: &ComputedStyle,
    ) {
        if style.space_before > 0.0 {
            ui.add_space(style.space_before);
        }
        let mut number = 1usize;
        for handle in tag.children().top().iter() {
            let Some(node) = handle.get(self.parser) else {
                continue;
            };
            let Some(child) = node.as_tag() else {
                continue;
            };
            let child_name = child.name().as_utf8_str().to_lowercase();
            if child_name != "li" {
                if child_name == "ul" || child_name == "ol" {
                    self.render_list(ui, child, child_name == "ol", style);
                }
                continue;
            }
            let bullet = if ordered {
                let value = format!("{number}. ");
                number += 1;
                value
            } else {
                "• ".to_owned()
            };
            let item_style = self.style_for(child, style, "li");
            let mut inline: Vec<InlineItem> = Vec::new();
            let mut nested_handles: Vec<NodeHandle> = Vec::new();

            for li_child in child.children().top().iter() {
                let Some(li_node) = li_child.get(self.parser) else {
                    continue;
                };
                if let Some(t) = li_node.as_tag() {
                    let n = t.name().as_utf8_str().to_lowercase();
                    if (is_block(&n) && n != "p") || n == "ul" || n == "ol" {
                        nested_handles.push(*li_child);
                    } else {
                        let s = self.style_for(t, &item_style, &n);
                        self.collect_inline(t, s, &mut inline, false);
                    }
                } else if let Some(text) = li_node.as_raw() {
                    push_text(
                        &mut inline,
                        decode_entities(&text.as_utf8_str()),
                        item_style.clone(),
                        false,
                    );
                }
            }
            ui.horizontal_top(|ui| {
                ui.add_space(style.margin_left + 6.0);
                ui.label(
                    RichText::new(bullet)
                        .color(if self.dark {
                            Color32::from_rgb(160, 170, 180)
                        } else {
                            Color32::from_rgb(110, 120, 130)
                        })
                        .size(item_style.size),
                );
                ui.vertical(|ui| {
                    let mut inline = std::mem::take(&mut inline);
                    trim_whitespace(&mut inline);
                    self.add_text_job(ui, &inline);
                    for handle in &nested_handles {
                        if let Some(t) = handle.get(self.parser).and_then(|node| node.as_tag()) {
                            let n = t.name().as_utf8_str().to_lowercase();
                            let s = self.style_for(t, &item_style, &n);
                            self.render_block(ui, t, &n, &s);
                        }
                    }
                });
            });
            if item_style.space_after > 0.0 {
                ui.add_space(item_style.space_after);
            }
        }
    }

    fn render_table(&mut self, ui: &mut egui::Ui, tag: &HTMLTag<'a>, style: &ComputedStyle) {
        if style.space_before > 0.0 {
            ui.add_space(style.space_before);
        }
        let stroke_color = if self.dark {
            Color32::from_rgb(70, 78, 88)
        } else {
            Color32::from_rgb(205, 210, 215)
        };
        let grid_id = ui.next_auto_id();
        egui::Grid::new(grid_id)
            .striped(true)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                for row in table_rows(self.parser, tag) {
                    let Some(row_node) = row.get(self.parser) else {
                        continue;
                    };
                    let Some(row_tag) = row_node.as_tag() else {
                        continue;
                    };
                    let mut columns = 0;
                    for cell in row_tag.children().top().iter() {
                        let Some(cell_node) = cell.get(self.parser) else {
                            continue;
                        };
                        let Some(cell_tag) = cell_node.as_tag() else {
                            continue;
                        };
                        let cell_name = cell_tag.name().as_utf8_str().to_lowercase();
                        if cell_name != "td" && cell_name != "th" {
                            continue;
                        }
                        let cell_style = self.style_for(cell_tag, style, &cell_name);
                        egui::Frame::new()
                            .stroke(Stroke::new(1.0_f32, stroke_color))
                            .inner_margin(egui::Margin::symmetric(6, 3))
                            .show(ui, |ui| {
                                ui.set_min_width(48.0);
                                self.render_nodes(ui, cell_tag.children().top().as_slice(), &cell_style);
                            });
                        columns += 1;
                    }
                    if columns > 0 {
                        ui.end_row();
                    }
                }
            });
        if style.space_after > 0.0 {
            ui.add_space(style.space_after);
        }
    }

    fn render_image(&mut self, ui: &mut egui::Ui, src: &str, alt: &str) {
        // Entfernte Bilder (http/https) lädt egui über den HTTP-Loader.
        if src.starts_with("http://") || src.starts_with("https://") {
            let width = ui.available_width().max(64.0);
            ui.add(egui::Image::new(src).max_width(width));
            return;
        }
        let Some(path) = resolve_resource(self.base_dir.as_deref(), src) else {
            if !alt.is_empty() {
                ui.colored_label(self.weak, alt);
            }
            return;
        };
        if let Some(texture) = self.images.texture(ui.ctx(), &path) {
            let width = ui.available_width().max(64.0);
            let image = egui::Image::new(&texture).max_width(width);
            ui.add(image);
        } else {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                if alt.is_empty() {
                    format!("Bild nicht gefunden: {}", path.display())
                } else {
                    format!("{} (Bild nicht gefunden)", alt)
                },
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Helfer
// ---------------------------------------------------------------------------

fn attribute(tag: &HTMLTag<'_>, name: &str) -> Option<String> {
    tag.attributes()
        .get(name)
        .flatten()
        .map(|value| value.as_utf8_str().into_owned())
}

fn is_block(name: &str) -> bool {
    matches!(
        name,
        "html"
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
            | "caption"
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
            | "address"
            | "details"
            | "summary"
            | "center"
    )
}

fn table_rows<'a, 'p>(
    parser: &'p Parser<'a>,
    table: &'p HTMLTag<'a>,
) -> Vec<NodeHandle> {
    let mut rows = Vec::new();
    for handle in table.children().top().iter() {
        let Some(node) = handle.get(parser) else {
            continue;
        };
        let Some(tag) = node.as_tag() else {
            continue;
        };
        let name = tag.name().as_utf8_str().to_lowercase();
        if name == "tr" {
            rows.push(*handle);
        } else if matches!(name.as_str(), "thead" | "tbody" | "tfoot") {
            for child in tag.children().top().iter() {
                if let Some(tl::Node::Tag(child_tag)) = child.get(parser) {
                    if child_tag.name().as_utf8_str().eq_ignore_ascii_case("tr") {
                        rows.push(*child);
                    }
                }
            }
        }
    }
    rows
}

fn push_text(out: &mut Vec<InlineItem>, text: String, style: ComputedStyle, link: bool) {
    if text.is_empty() {
        return;
    }
    // HTML fasst aufeinanderfolgende Leerzeichen zusammen.
    let mut collapsed = String::with_capacity(text.len());
    let mut last_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !last_space {
                collapsed.push(' ');
                last_space = true;
            }
        } else {
            collapsed.push(ch);
            last_space = false;
        }
    }
    if collapsed.is_empty() {
        return;
    }
    out.push(InlineItem::Run(Run {
        text: collapsed,
        style,
        link,
    }));
}

fn trim_whitespace(items: &mut Vec<InlineItem>) {
    if let Some(InlineItem::Run(run)) = items.first_mut() {
        run.text = run.text.trim_start().to_owned();
    }
    if let Some(InlineItem::Run(run)) = items.last_mut() {
        run.text = run.text.trim_end().to_owned();
    }
    items.retain(|item| match item {
        InlineItem::Run(run) => !run.text.is_empty(),
        _ => true,
    });
}

fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some('\u{00a0}'),
            "ndash" => Some('–'),
            "mdash" => Some('—'),
            "hellip" => Some('…'),
            "copy" => Some('©'),
            "reg" => Some('®'),
            "trade" => Some('™'),
            "laquo" => Some('«'),
            "raquo" => Some('»'),
            "szlig" => Some('ß'),
            "auml" => Some('ä'),
            "ouml" => Some('ö'),
            "uuml" => Some('ü'),
            "Auml" => Some('Ä'),
            "Ouml" => Some('Ö'),
            "Uuml" => Some('Ü'),
            _ => {
                if let Some(hex) = entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X")) {
                    u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                } else if let Some(dec) = entity.strip_prefix('#') {
                    dec.parse::<u32>().ok().and_then(char::from_u32)
                } else {
                    None
                }
            }
        };
        match decoded {
            Some(ch) => out.push(ch),
            None => out.push_str(&rest[..=end]),
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

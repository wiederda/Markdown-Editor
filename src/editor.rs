use eframe::egui::{
    self,
    text::{CCursor, CCursorRange, LayoutJob},
    Align2, Color32, FontId, Key, Stroke, TextEdit, TextStyle,
};

use crate::doc::{DocKind, Document, ScrollMaster};
use crate::highlight;
use crate::textops::{self, FormatAction};

// ---------------------------------------------------------------------------
// Cursor- und Zustandshelfer
// ---------------------------------------------------------------------------

pub fn select_all(ctx: &egui::Context, doc: &Document) {
    let id = doc.editor_id();
    let Some(mut state) = TextEdit::load_state(ctx, id) else {
        return;
    };
    let len = doc.text.chars().count();
    state
        .cursor
        .set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(len))));
    TextEdit::store_state(ctx, id, state);
    ctx.memory_mut(|mem| mem.request_focus(id));
}

pub fn focus(ctx: &egui::Context, doc: &Document) {
    ctx.memory_mut(|mem| mem.request_focus(doc.editor_id()));
}

fn current_range(ctx: &egui::Context, doc: &Document) -> CCursorRange {
    let id = doc.editor_id();
    TextEdit::load_state(ctx, id)
        .and_then(|state| state.cursor.char_range())
        .unwrap_or_else(|| CCursorRange::one(CCursor::new(doc.text.chars().count())))
}

fn set_selection(ctx: &egui::Context, doc: &Document, start: usize, end: usize) {
    let id = doc.editor_id();
    let mut state = TextEdit::load_state(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(CCursorRange::two(
        CCursor::new(start),
        CCursor::new(end),
    )));
    TextEdit::store_state(ctx, id, state);
    ctx.memory_mut(|mem| mem.request_focus(id));
}

fn add_undo_point(doc: &Document, ctx: &egui::Context) {
    let id = doc.editor_id();
    let mut state = TextEdit::load_state(ctx, id).unwrap_or_default();
    let range = state
        .cursor
        .char_range()
        .unwrap_or_else(|| CCursorRange::one(CCursor::new(doc.text.chars().count())));
    let mut undoer = state.undoer();
    undoer.add_undo(&(range, doc.text.clone()));
    state.set_undoer(undoer);
    TextEdit::store_state(ctx, id, state);
}

pub fn undo(ctx: &egui::Context, doc: &mut Document) -> bool {
    let id = doc.editor_id();
    let Some(mut state) = TextEdit::load_state(ctx, id) else {
        return false;
    };
    let range = state
        .cursor
        .char_range()
        .unwrap_or_else(|| CCursorRange::one(CCursor::new(doc.text.chars().count())));
    let current = (range, doc.text.clone());
    let mut undoer = state.undoer();
    let Some(previous) = undoer.undo(&current).cloned() else {
        return false;
    };
    doc.text = previous.1;
    state.cursor.set_char_range(Some(previous.0));
    state.set_undoer(undoer);
    TextEdit::store_state(ctx, id, state);
    doc.dirty = true;
    true
}

pub fn redo(ctx: &egui::Context, doc: &mut Document) -> bool {
    let id = doc.editor_id();
    let Some(mut state) = TextEdit::load_state(ctx, id) else {
        return false;
    };
    let range = state
        .cursor
        .char_range()
        .unwrap_or_else(|| CCursorRange::one(CCursor::new(doc.text.chars().count())));
    let current = (range, doc.text.clone());
    let mut undoer = state.undoer();
    let Some(next) = undoer.redo(&current).cloned() else {
        return false;
    };
    doc.text = next.1;
    state.cursor.set_char_range(Some(next.0));
    state.set_undoer(undoer);
    TextEdit::store_state(ctx, id, state);
    doc.dirty = true;
    true
}

// ---------------------------------------------------------------------------
// Formatierung
// ---------------------------------------------------------------------------

fn pick_image(doc: &Document, status: &mut String) -> Option<(String, String)> {
    let mut dialog = rfd::FileDialog::new()
        .add_filter("Bilder", &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tiff"]);
    if let Some(dir) = doc.base_dir() {
        dialog = dialog.set_directory(dir);
    }
    let file = dialog.pick_file()?;

    let alt = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dest = match doc.base_dir().and_then(|dir| crate::doc::relative_path(dir, &file)) {
        Some(relative) => relative,
        None => file.clone(),
    };
    let mut dest = dest.to_string_lossy().replace('\\', "/");
    if dest.contains([' ', '(', ')']) {
        dest = format!("<{dest}>");
    } else if dest.is_empty() {
        status.push_str("Kein gültiger Bildpfad");
        return None;
    }
    Some((alt, dest))
}

pub fn apply_format(
    ctx: &egui::Context,
    doc: &mut Document,
    action: FormatAction,
    status: &mut String,
) {
    let image = if matches!(action, FormatAction::Image) {
        match pick_image(doc, status) {
            Some(image) => Some(image),
            None => return,
        }
    } else {
        None
    };

    let range = current_range(ctx, doc);
    let start = range.primary.index.min(range.secondary.index);
    let end = range.primary.index.max(range.secondary.index);

    add_undo_point(doc, ctx);

    let (new_start, new_end) = match action {
        FormatAction::Bold => textops::wrap_selection(&mut doc.text, start, end, "**", "**"),
        FormatAction::Italic => textops::wrap_selection(&mut doc.text, start, end, "*", "*"),
        FormatAction::Strikethrough => {
            textops::wrap_selection(&mut doc.text, start, end, "~~", "~~")
        }
        FormatAction::InlineCode => textops::wrap_selection(&mut doc.text, start, end, "`", "`"),
        FormatAction::CodeBlock => {
            textops::wrap_selection(&mut doc.text, start, end, "```\n", "\n```")
        }
        FormatAction::Heading1 => textops::prefix_lines(&mut doc.text, start, end, "# "),
        FormatAction::Heading2 => textops::prefix_lines(&mut doc.text, start, end, "## "),
        FormatAction::Heading3 => textops::prefix_lines(&mut doc.text, start, end, "### "),
        FormatAction::BulletList => textops::prefix_lines(&mut doc.text, start, end, "- "),
        FormatAction::NumberedList => textops::prefix_lines(&mut doc.text, start, end, "1. "),
        FormatAction::Quote => textops::prefix_lines(&mut doc.text, start, end, "> "),
        FormatAction::Link => textops::insert_link(&mut doc.text, start, end),
        FormatAction::Image => {
            let (alt, dest) = image.unwrap_or_default();
            textops::insert_image(&mut doc.text, start, end, &alt, &dest)
        }
        FormatAction::Rule => {
            let pos = textops::replace_range(&mut doc.text, start, end, "\n\n---\n\n");
            (pos + 7, pos + 7)
        }
        FormatAction::Table => textops::insert_table(&mut doc.text, start, end),
    };

    let id = doc.editor_id();
    let mut state = TextEdit::load_state(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(CCursorRange::two(
        CCursor::new(new_start),
        CCursor::new(new_end),
    )));
    TextEdit::store_state(ctx, id, state);
    doc.dirty = true;
    *status = format!("{} eingefügt", action.label());
}

pub fn format_document(ctx: &egui::Context, doc: &mut Document, status: &mut String) {
    match doc.kind {
        DocKind::Html => {
            let formatted = textops::format_html(&doc.text);
            if formatted == doc.text {
                *status = "HTML ist bereits formatiert".to_owned();
                return;
            }
            add_undo_point(doc, ctx);
            doc.text = formatted;
            set_selection(ctx, doc, 0, 0);
            doc.dirty = true;
            *status = "HTML formatiert".to_owned();
        }
        DocKind::Markdown => {
            *status = "Für Markdown stehen die Formatierungsbefehle im Menü „Format“ bereit"
                .to_owned();
        }
    }
}

// ---------------------------------------------------------------------------
// Suchen und Ersetzen
// ---------------------------------------------------------------------------

pub fn find_next(doc: &mut Document, ctx: &egui::Context, status: &mut String) {
    if doc.find.is_empty() {
        return;
    }
    let range = current_range(ctx, doc);
    let start = range.primary.index.max(range.secondary.index);
    match textops::find_next(&doc.text, &doc.find, doc.match_case, start) {
        Some((match_start, match_end)) => {
            set_selection(ctx, doc, match_start, match_end);
            *status = format!(
                "Treffer in Zeile {}",
                doc.text.chars().take(match_start).filter(|c| *c == '\n').count() + 1
            );
        }
        None => *status = format!("Keine Treffer für „{}“", doc.find),
    }
}

pub fn replace_current(doc: &mut Document, ctx: &egui::Context, status: &mut String) {
    if doc.find.is_empty() {
        return;
    }
    let range = current_range(ctx, doc);
    let start = range.primary.index.min(range.secondary.index);
    let end = range.primary.index.max(range.secondary.index);
    let selected: String = doc
        .text
        .chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect();

    let matches = if doc.match_case {
        selected == doc.find
    } else {
        selected.to_lowercase() == doc.find.to_lowercase()
    };

    if !matches {
        find_next(doc, ctx, status);
        return;
    }

    add_undo_point(doc, ctx);
    let replacement_chars = doc.replace.chars().count();
    textops::replace_range(&mut doc.text, start, end, &doc.replace);
    set_selection(ctx, doc, start + replacement_chars, start + replacement_chars);
    doc.dirty = true;
    *status = "Ersetzt".to_owned();
    find_next(doc, ctx, status);
}

pub fn replace_all(doc: &mut Document, ctx: &egui::Context, status: &mut String) {
    if doc.find.is_empty() {
        return;
    }
    let matches = textops::find_all(&doc.text, &doc.find, doc.match_case);
    if matches.is_empty() {
        *status = format!("Keine Treffer für „{}“", doc.find);
        return;
    }
    add_undo_point(doc, ctx);
    let mut result = String::with_capacity(doc.text.len());
    let mut cursor = 0;
    for (start, end) in &matches {
        let start_byte = textops::char_to_byte(&doc.text, *start);
        let from = textops::char_to_byte(&doc.text, cursor);
        result.push_str(&doc.text[from..start_byte]);
        result.push_str(&doc.replace);
        cursor = *end;
    }
    let from = textops::char_to_byte(&doc.text, cursor);
    result.push_str(&doc.text[from..]);
    doc.text = result;
    doc.dirty = true;
    set_selection(ctx, doc, 0, 0);
    *status = format!("{} Treffer ersetzt", matches.len());
}

// ---------------------------------------------------------------------------
// Editor-Oberfläche
// ---------------------------------------------------------------------------

pub struct EditorOptions {
    pub line_numbers: bool,
    pub syntax_highlighting: bool,
    pub auto_indent: bool,
    pub sync_scroll: bool,
}

pub fn show_editor(ui: &mut egui::Ui, doc: &mut Document, status: &mut String, options: &EditorOptions) {
    let ctx = ui.ctx().clone();

    find_bar(ui, doc, status);

    let text_id = doc.editor_id();
    let incoming_offset = if options.sync_scroll && doc.scroll_master == ScrollMaster::Preview {
        Some(doc.sync_ratio * doc.editor_max)
    } else {
        None
    };

    let total_lines = doc.text.chars().filter(|c| *c == '\n').count() + 1;
    let digits = total_lines.to_string().len().max(2);
    let font = ui
        .style()
        .text_styles
        .get(&TextStyle::Monospace)
        .cloned()
        .unwrap_or_else(|| FontId::monospace(13.0));
    let char_width = ui.fonts(|fonts| fonts.glyph_width(&font, '0'));
    let gutter_width = if options.line_numbers {
        char_width * digits as f32 + 14.0
    } else {
        0.0
    };

    let mut scroll_area = egui::ScrollArea::vertical()
        .id_salt(("editor_scroll", doc.id))
        .auto_shrink([false, false]);
    if let Some(offset) = incoming_offset {
        scroll_area = scroll_area.vertical_scroll_offset(offset);
    }

    let output = scroll_area.show(ui, |ui| {
        let height = ui.available_height().max(240.0);
        let mut result = None;
        ui.horizontal_top(|ui| {
            let gutter_left = ui.cursor().left();
            if gutter_width > 0.0 {
                ui.add_space(gutter_width);
            }

            let kind = doc.kind;
            let syntax = options.syntax_highlighting;
            let mono_size = ui
                .style()
                .text_styles
                .get(&TextStyle::Monospace)
                .map(|font| font.size)
                .unwrap_or(13.0);
            let mut layouter =
                move |ui: &egui::Ui, buffer: &dyn egui::TextBuffer, wrap_width: f32| {
                    let job: LayoutJob = highlight::layout_job(
                        kind,
                        buffer.as_str(),
                        ui.visuals(),
                        mono_size,
                        wrap_width,
                        syntax,
                    );
                    ui.fonts(|fonts| fonts.layout_job(job))
                };

            let hint = match kind {
                DocKind::Markdown => "Markdown hier schreiben …",
                DocKind::Html => "HTML hier schreiben …",
            };

            let text_output = egui::TextEdit::multiline(&mut doc.text)
                .id(text_id)
                .code_editor()
                .desired_width(f32::INFINITY)
                .desired_rows(24)
                .min_size(egui::vec2(0.0, height))
                .hint_text(hint)
                .layouter(&mut layouter)
                .show(ui);

            if text_output.response.changed() {
                doc.dirty = true;
            }

            text_output.response.context_menu(|ui| {
                let ctx = ui.ctx().clone();
                context_menu(ui, doc, status, &ctx);
            });

            result = Some((text_output, gutter_left));
        });
        result
    });

    let clip_rect = output.inner_rect;
    if let Some((text_output, gutter_left)) = output.inner {
        let painter = ui.painter().with_clip_rect(clip_rect);
        let galley = &text_output.galley;
        let galley_pos = text_output.galley_pos;

        let cursor_char = text_output
            .cursor_range
            .map(|range| range.primary.index)
            .unwrap_or(0);
        let mut line_index = 0usize;
        let mut column = 1usize;
        for ch in doc.text.chars().take(cursor_char) {
            if ch == '\n' {
                line_index += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        doc.cursor_line = line_index + 1;
        doc.cursor_column = column;

        let mut row_line = 0usize;
        let mut previous_ended_with_newline = true;
        let mut line_rows: Vec<egui::Rect> = Vec::new();
        let mut number_positions: Vec<(usize, egui::Pos2)> = Vec::new();
        for (row_index, placed) in galley.rows.iter().enumerate() {
            if row_index == 0 || previous_ended_with_newline {
                if row_index != 0 {
                    row_line += 1;
                }
                number_positions.push((row_line + 1, galley_pos + placed.pos.to_vec2()));
            }
            if row_line == line_index {
                line_rows.push(placed.rect().translate(galley_pos.to_vec2()));
            }
            previous_ended_with_newline = placed.row.ends_with_newline;
        }

        // Aktuelle Zeile hervorheben (dezent, über dem Text).
        if let (Some(first), Some(last)) = (line_rows.first(), line_rows.last()) {
            let rect = egui::Rect::from_min_max(
                egui::pos2(gutter_left + gutter_width, first.top() - 1.0),
                egui::pos2(
                    galley_pos.x + galley.rect.right(),
                    last.bottom() + 1.0,
                ),
            );
            let accent = ui.visuals().selection.bg_fill;
            painter.rect_filled(
                rect,
                3.0,
                Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 26),
            );
        }

        // Zeilennummern
        if options.line_numbers {
            let weak = ui.visuals().weak_text_color();
            let strong = ui.visuals().text_color();
            for (number, pos) in number_positions {
                let color = if number == line_index + 1 { strong } else { weak };
                painter.text(
                    egui::pos2(gutter_left + gutter_width - 7.0, pos.y),
                    Align2::RIGHT_TOP,
                    number.to_string(),
                    font.clone(),
                    color,
                );
            }
            painter.vline(
                gutter_left + gutter_width - 2.0,
                clip_rect.y_range(),
                Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color),
            );
        }

        if options.auto_indent && text_output.response.has_focus() {
            auto_indent_after_newline(&ctx, doc, &text_output);
        }
    }

    let scroll_offset = output.state.offset.y;
    let applied = incoming_offset.unwrap_or(doc.editor_offset);
    if (scroll_offset - applied).abs() > 0.5 {
        doc.editor_offset = scroll_offset;
        doc.editor_max = (output.content_size.y - output.inner_rect.height()).max(1.0);
        doc.sync_ratio = (scroll_offset / doc.editor_max).clamp(0.0, 1.0);
        doc.scroll_master = ScrollMaster::Editor;
    }
}

fn find_bar(ui: &mut egui::Ui, doc: &mut Document, status: &mut String) {
    if !doc.find_open {
        return;
    }
    let ctx = ui.ctx().clone();
    ui.horizontal_wrapped(|ui| {
        ui.label("Suchen:");
        let find_response = ui.add(
            egui::TextEdit::singleline(&mut doc.find)
                .hint_text("Text suchen")
                .desired_width(170.0),
        );
        ui.label("Ersetzen:");
        ui.add(
            egui::TextEdit::singleline(&mut doc.replace)
                .hint_text("Ersatztext")
                .desired_width(170.0),
        );
        if ui.button("Weiter").clicked() {
            find_next(doc, &ctx, status);
        }
        if ui.button("Ersetzen").clicked() {
            replace_current(doc, &ctx, status);
        }
        if ui.button("Alle ersetzen").clicked() {
            replace_all(doc, &ctx, status);
        }
        ui.checkbox(&mut doc.match_case, "Aa")
            .on_hover_text("Groß-/Kleinschreibung beachten");
        if ui.button("×").on_hover_text("Suchen schließen").clicked() {
            doc.find_open = false;
            ctx.memory_mut(|mem| mem.request_focus(doc.editor_id()));
        }

        if find_response.lost_focus()
            && ui.input(|i| i.key_pressed(Key::Enter))
            && doc.find_open
        {
            find_next(doc, &ctx, status);
        }
    });
}

fn context_menu(ui: &mut egui::Ui, doc: &mut Document, status: &mut String, ctx: &egui::Context) {
    match doc.kind {
        DocKind::Markdown => {
            format_item(ui, doc, status, ctx, "Fett", FormatAction::Bold);
            format_item(ui, doc, status, ctx, "Kursiv", FormatAction::Italic);
            format_item(ui, doc, status, ctx, "Inline-Code", FormatAction::InlineCode);
            format_item(ui, doc, status, ctx, "Codeblock", FormatAction::CodeBlock);
            ui.separator();
            format_item(ui, doc, status, ctx, "Link", FormatAction::Link);
            format_item(ui, doc, status, ctx, "Aufzählungsliste", FormatAction::BulletList);
            format_item(ui, doc, status, ctx, "Nummerierte Liste", FormatAction::NumberedList);
            format_item(ui, doc, status, ctx, "Zitat", FormatAction::Quote);
            format_item(ui, doc, status, ctx, "Tabelle", FormatAction::Table);
        }
        DocKind::Html => {
            if ui.button("HTML formatieren").clicked() {
                format_document(ctx, doc, status);
                ui.close();
            }
        }
    }
    ui.separator();
    if ui.button("Suchen und Ersetzen …").clicked() {
        doc.find_open = true;
        ui.close();
    }
}

fn format_item(
    ui: &mut egui::Ui,
    doc: &mut Document,
    status: &mut String,
    ctx: &egui::Context,
    label: &str,
    action: FormatAction,
) {
    if ui.button(label).clicked() {
        apply_format(ctx, doc, action, status);
        ui.close();
    }
}

fn auto_indent_after_newline(
    ctx: &egui::Context,
    doc: &mut Document,
    output: &egui::text_edit::TextEditOutput,
) {
    let enter_pressed = ctx.input(|input| {
        input.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key {
                    key: Key::Enter,
                    pressed: true,
                    ..
                }
            )
        })
    });
    if !enter_pressed {
        return;
    }
    let Some(range) = output.cursor_range else {
        return;
    };
    if range.primary.index != range.secondary.index {
        return;
    }
    let position = range.primary.index;
    if position == 0 {
        return;
    }
    let chars: Vec<char> = doc.text.chars().collect();
    let previous_line_start = chars[..position.saturating_sub(1)]
        .iter()
        .rposition(|c| *c == '\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let previous_line: String = chars[previous_line_start..position.saturating_sub(1)]
        .iter()
        .collect();
    let indent = textops::auto_indent(doc.kind, &previous_line);
    if indent.is_empty() {
        return;
    }
    let byte = textops::char_to_byte(&doc.text, position);
    doc.text.insert_str(byte, &indent);
    let mut state = output.state.clone();
    let new_position = position + indent.chars().count();
    state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(
        new_position,
    ))));
    TextEdit::store_state(ctx, doc.editor_id(), state);
    doc.dirty = true;
}

/// Zeigt eine Statuszeile mit Cursorposition und Dokumenttyp.
pub fn status_text(doc: &Document) -> String {
    format!(
        "{} · Zeile {} · Spalte {} · UTF-8",
        doc.kind.filter_name(),
        doc.cursor_line,
        doc.cursor_column
    )
}

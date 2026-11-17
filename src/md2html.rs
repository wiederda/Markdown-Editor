use std::path::{Path, PathBuf};

use pulldown_cmark::{html, Options, Parser};

use crate::doc::relative_path;

pub const DEFAULT_CSS: &str = r#"/* Standard-Stylesheet des MD Editors */
:root {
  color-scheme: light dark;
}
html {
  font-family: system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
  line-height: 1.6;
}
body {
  max-width: 860px;
  margin: 0 auto;
  padding: 2rem 1.25rem 4rem;
  color: #1f2328;
  background: #ffffff;
}
h1, h2, h3, h4, h5, h6 {
  line-height: 1.25;
  margin: 1.6em 0 0.6em;
}
h1 { font-size: 2em; border-bottom: 1px solid #d8dee4; padding-bottom: .3em; }
h2 { font-size: 1.6em; border-bottom: 1px solid #d8dee4; padding-bottom: .3em; }
h3 { font-size: 1.3em; }
a { color: #0969da; }
code, pre {
  font-family: ui-monospace, SFMono-Regular, Consolas, "Liberation Mono", monospace;
}
code {
  background: #f0f2f4;
  padding: .15em .35em;
  border-radius: 4px;
  font-size: .92em;
}
pre {
  background: #f6f8fa;
  padding: 1em;
  border-radius: 6px;
  overflow-x: auto;
}
pre code { background: none; padding: 0; }
blockquote {
  margin: 1em 0;
  padding: .2em 1em;
  border-left: 4px solid #d0d7de;
  color: #57606a;
}
table { border-collapse: collapse; margin: 1em 0; width: 100%; }
th, td { border: 1px solid #d0d7de; padding: .45em .7em; text-align: left; }
th { background: #f6f8fa; }
img { max-width: 100%; height: auto; }
hr { border: 0; border-top: 1px solid #d0d7de; margin: 2em 0; }
@media (prefers-color-scheme: dark) {
  body { color: #e6edf3; background: #0d1117; }
  h1, h2 { border-bottom-color: #30363d; }
  code, pre { background: #161b22; }
  a { color: #58a6ff; }
  blockquote { border-left-color: #3d444d; color: #9198a1; }
  th, td { border-color: #3d444d; }
  th { background: #161b22; }
}
"#;

#[derive(Clone, Default, PartialEq, Eq)]
pub enum CssChoice {
    #[default]
    Default,
    None,
    File(PathBuf),
}

impl CssChoice {
    pub fn label(&self) -> String {
        match self {
            Self::Default => "Standard-Stylesheet einbetten".to_owned(),
            Self::None => "Kein Stylesheet".to_owned(),
            Self::File(path) => format!("CSS-Datei: {}", path.display()),
        }
    }
}

#[derive(Clone)]
pub struct ExportOptions {
    pub full_document: bool,
    pub css: CssChoice,
    pub title: String,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            full_document: true,
            css: CssChoice::Default,
            title: String::new(),
        }
    }
}

pub fn markdown_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_SMART_PUNCTUATION
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_DEFINITION_LIST
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_SUBSCRIPT
}

/// Wandelt Markdown in einen HTML-Fragment um (ohne Head/Body).
pub fn markdown_to_html(markdown: &str) -> String {
    let parser = Parser::new_ext(markdown, markdown_options());
    let mut out = String::with_capacity(markdown.len() + markdown.len() / 2);
    html::push_html(&mut out, parser);
    out
}

fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

fn stylesheet_link(path: &Path, output_dir: &Path) -> String {
    let href = relative_path(output_dir, path)
        .filter(|p| !p.to_string_lossy().starts_with(".."))
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| {
            format!(
                "file:///{}",
                path.to_string_lossy().replace('\\', "/").trim_start_matches('/')
            )
        });
    format!(
        "  <link rel=\"stylesheet\" href=\"{}\">\n",
        escape_html(&href)
    )
}

/// Erzeugt die exportierbare HTML-Ausgabe.
pub fn build_html(
    markdown: &str,
    options: &ExportOptions,
    output_path: &Path,
) -> String {
    let body = markdown_to_html(markdown);
    if !options.full_document {
        return body;
    }

    let title = if options.title.trim().is_empty() {
        output_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Markdown-Dokument".to_owned())
    } else {
        options.title.trim().to_owned()
    };

    let output_dir = output_path.parent().unwrap_or(Path::new("."));
    let mut head_extra = String::new();
    match &options.css {
        CssChoice::Default => {
            head_extra.push_str("  <style>\n");
            head_extra.push_str(DEFAULT_CSS);
            head_extra.push_str("  </style>\n");
        }
        CssChoice::None => {}
        CssChoice::File(path) => head_extra.push_str(&stylesheet_link(path, output_dir)),
    }

    format!(
        "<!DOCTYPE html>\n<html lang=\"de\">\n<head>\n  <meta charset=\"utf-8\">\n  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n  <meta name=\"generator\" content=\"MD Editor\">\n  <title>{title}</title>\n{head_extra}</head>\n<body>\n{body}</body>\n</html>\n",
        title = escape_html(&title),
    )
}

//! Imágenes de la IA dentro del terminal, sin tapar nada.
//!
//! - **Etiquetas en línea**: el texto `[Image #N]` (imagen que le mandaste),
//!   la palabra `Read` de `Read(foto.png)` y el resumen `Read N files` de Claude
//!   Code se dibujan como una etiqueta con la imagen en pequeño. La etiqueta
//!   ocupa **exactamente** las celdas de ese texto: el terminal no tiene hueco
//!   para imágenes, así que nunca se sale de ahí.
//! - **Barra inferior**: las últimas imágenes que ha leído la IA, en una franja
//!   propia debajo del terminal (el terminal se encoge; no se superpone).
//!
//! Un clic en cualquier miniatura la abre en grande. Las imágenes salen del
//! transcript de la sesión (mismo lector que el panel «Agentes»), así que
//! funciona igual en local, WSL o SSH y sin leer el disco.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, StyledImage as _, div, img, px,
};
use gpui_component::ActiveTheme as _;
use nebula_terminal::grid::Dimensions as _;
use nebula_terminal::index::{Column, Line};

use super::TerminalView;
use crate::ai_usage::{ImageOrigin, SessionImage};

/// Padding del terminal dentro de su tarjeta (ver `render`): la rejilla
/// empieza aquí.
const GRID_PAD_X: f32 = 12.0;
const GRID_PAD_Y: f32 = 8.0;
/// Filas que puede crecer una etiqueta hacia abajo (por huecos vacíos).
const MAX_ROWS: usize = 4;
/// Imágenes convertidas que se guardan como mucho.
const CACHE_LIMIT: usize = 96;
/// Líneas por debajo de la vista que se recorren para numerar los resúmenes.
const SUMMARY_SCAN_LIMIT: i32 = 5_000;
/// Alto de la barra inferior y de sus miniaturas.
pub(super) const IMAGE_BAR_HEIGHT: f32 = 46.0;
const BAR_THUMB: f32 = 32.0;
/// Miniaturas en la barra.
const BAR_IMAGES: usize = 10;

/// Referencia a imágenes encontrada en una fila visible.
#[derive(Debug, PartialEq, Eq)]
struct Marker {
    row: usize,
    /// Celdas que ocupa la etiqueta: `start_col..end_col` (lo que tapa).
    start_col: usize,
    end_col: usize,
    kind: MarkerKind,
    /// Texto que se ve en la etiqueta ("#5", "Read 2 files"); None = solo imagen.
    caption: Option<String>,
}

/// Filas de alto que puede ocupar una etiqueta: la suya más las de debajo
/// que estén **vacías** en esas mismas columnas (máximo `MAX_ROWS`). Así la
/// miniatura crece solo por huecos y nunca tapa texto.
fn free_rows(rows: &[Vec<char>], row: usize, start: usize, end: usize) -> usize {
    let blank = |line: &Vec<char>| {
        (start..end.min(line.len())).all(|col| matches!(line[col], ' ' | '\0' | '\u{a0}'))
    };
    1 + rows[row + 1..].iter().take(MAX_ROWS - 1).take_while(|line| blank(line)).count()
}

#[derive(Debug, PartialEq, Eq)]
enum MarkerKind {
    /// `[Image #N]` → "Image #N".
    Attached(String),
    /// `Read(ruta.png)` → "ruta.png".
    ReadPath(String),
    /// Resumen `Read N files` de Claude Code: `reads` = N y `from_bottom` = cuántos
    /// resúmenes con lecturas hay por debajo (0 = el más reciente).
    ReadSummary { reads: usize, from_bottom: usize },
}

const IMAGE_EXTENSIONS: [&str; 6] = [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"];

fn is_word_start(text: &str, byte: usize) -> bool {
    text[..byte].chars().next_back().is_none_or(|c| !c.is_alphanumeric())
}

/// `Read 2 files` / `read 1 file` / `Reading 3 files…` dentro de una línea:
/// (byte inicial, byte final, N).
fn read_summary(text: &str) -> Option<(usize, usize, usize)> {
    let lower = text.to_lowercase();
    let mut search = 0;
    while let Some(start) = lower[search..].find("read").map(|at| at + search) {
        search = start + 4;
        if !is_word_start(&lower, start) {
            continue;
        }
        let mut rest = &lower[start + 4..];
        if let Some(after) = rest.strip_prefix("ing") {
            rest = after;
        }
        let Some(after) = rest.strip_prefix(' ') else { continue };
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            continue;
        }
        let after = &after[digits.len()..];
        let Some(after) = after.strip_prefix(" file") else { continue };
        let after = after.strip_prefix('s').unwrap_or(after);
        if after.chars().next().is_some_and(char::is_alphanumeric) {
            continue;
        }
        let end = lower.len() - after.len();
        return Some((start, end, digits.parse().ok()?));
    }
    None
}

/// Busca referencias a imágenes en el texto de una fila. `from_bottom` es el
/// número del resumen `Read N files` de esta fila, si lo tiene.
fn markers_in_line(row: usize, line: &[char], from_bottom: Option<usize>) -> Vec<Marker> {
    let text: String = line.iter().collect();
    let col_of = |byte: usize| text[..byte].chars().count();
    let mut found = Vec::new();
    let mut search = 0;
    while let Some(start) = text[search..].find("[Image #").map(|at| at + search) {
        let rest = &text[start + "[Image #".len()..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() && rest[digits.len()..].starts_with(']') {
            let end = start + "[Image #".len() + digits.len() + 1;
            found.push(Marker {
                row,
                start_col: col_of(start),
                end_col: col_of(end),
                kind: MarkerKind::Attached(format!("Image #{digits}")),
                caption: Some(format!("#{digits}")),
            });
        }
        search = start + 1;
    }
    let mut search = 0;
    while let Some(start) = text[search..].find("Read(").map(|at| at + search) {
        let inner_start = start + "Read(".len();
        if let Some(close) = text[inner_start..].find(')') {
            let inner = text[inner_start..inner_start + close].trim();
            let lower = inner.to_ascii_lowercase();
            if IMAGE_EXTENSIONS.iter().any(|ext| lower.ends_with(ext)) {
                // Solo se sustituye la palabra «Read»; la ruta sigue visible.
                let start_col = col_of(start);
                found.push(Marker {
                    row,
                    start_col,
                    end_col: start_col + "Read".len(),
                    kind: MarkerKind::ReadPath(inner.to_owned()),
                    caption: None,
                });
            }
        }
        search = start + 1;
    }
    if let (Some(from_bottom), Some((start, end, reads))) = (from_bottom, read_summary(&text)) {
        found.push(Marker {
            row,
            start_col: col_of(start),
            end_col: col_of(end),
            kind: MarkerKind::ReadSummary { reads, from_bottom },
            caption: Some(text[start..end].to_owned()),
        });
    }
    found
}

/// Imágenes del transcript que corresponden a una referencia. `groups` son los
/// bloques con lecturas, del más reciente al más antiguo.
fn images_for(
    images: &[SessionImage],
    groups: &[(usize, Vec<SessionImage>)],
    kind: &MarkerKind,
) -> Vec<SessionImage> {
    match kind {
        MarkerKind::Attached(label) => images
            .iter()
            .rev()
            .find(|image| image.label.as_deref() == Some(label.as_str()))
            .cloned()
            .into_iter()
            .collect(),
        MarkerKind::ReadPath(path) => {
            // La CLI muestra la ruta relativa o recortada; el transcript guarda
            // la absoluta. Se compara por el final, con barras normalizadas.
            let wanted = path.replace('\\', "/").trim_start_matches("./").to_ascii_lowercase();
            images
                .iter()
                .rev()
                .find(|image| {
                    image.origin == ImageOrigin::Read
                        && image.label.as_deref().is_some_and(|label| {
                            let label = label.replace('\\', "/").to_ascii_lowercase();
                            label == wanted || label.ends_with(&format!("/{wanted}"))
                        })
                })
                .cloned()
                .into_iter()
                .collect()
        },
        // Se empareja por orden desde abajo y solo si el número de lecturas
        // coincide: ante la duda, no se pone nada (nunca una imagen equivocada).
        MarkerKind::ReadSummary { reads, from_bottom } => match groups.get(*from_bottom) {
            Some((group_reads, images)) if group_reads == reads => images.clone(),
            _ => Vec::new(),
        },
    }
}

/// Imagen leída por la IA cuya ruta termina en `path` (para la vista previa).
pub(super) fn read_image_for_path(images: &[SessionImage], path: &str) -> Option<SessionImage> {
    images_for(images, &[], &MarkerKind::ReadPath(path.to_owned())).into_iter().next()
}

fn image_format(media_type: &str) -> gpui::ImageFormat {
    match media_type {
        "image/jpeg" | "image/jpg" => gpui::ImageFormat::Jpeg,
        "image/gif" => gpui::ImageFormat::Gif,
        "image/webp" => gpui::ImageFormat::Webp,
        "image/bmp" => gpui::ImageFormat::Bmp,
        _ => gpui::ImageFormat::Png,
    }
}

/// Estado de las miniaturas del panel.
#[derive(Default)]
pub(in crate::gpui_shell::terminal) struct InlineImageCache {
    /// Imágenes ya convertidas para pintar, por puntero de los bytes.
    pictures: HashMap<usize, Arc<gpui::Image>>,
    /// La barra se cerró cuando había este número de imágenes leídas; vuelve a
    /// salir cuando la IA lea otra.
    bar_hidden_at: Option<usize>,
}

impl TerminalView {
    /// Referencias a imágenes en las filas visibles ahora mismo.
    fn visible_markers(&self) -> Vec<(Marker, usize)> {
        let Some(session) = &self.session else { return Vec::new() };
        let term = session.term.lock();
        // Las filas que se acaban de pintar: durante un cambio de tamaño las
        // comprometidas (`self.rows`) aún no se han actualizado.
        let rows = if self.painted_rows > 0 { self.painted_rows } else { self.rows };
        let origin = term.viewport_origin_for(rows);
        let bottom = term.bottommost_line();
        let row_text = |line: Line| -> Vec<char> {
            let cells = &term.grid()[line];
            (0..term.columns()).map(|col| cells[Column(col)].c).collect()
        };
        // Resúmenes «Read N files» por debajo de la vista: dan el número de
        // orden (desde abajo) de los que sí se ven.
        let last_visible = origin + (rows.saturating_sub(1)) as i32;
        let mut below = 0;
        let mut line = bottom;
        while line > last_visible && (bottom - line).0 < SUMMARY_SCAN_LIMIT {
            let text: String = row_text(line).into_iter().collect();
            if read_summary(&text).is_some() {
                below += 1;
            }
            line = line - 1;
        }
        let texts: Vec<Vec<char>> = (0..rows)
            .map(|row| {
                let line = origin + row as i32;
                if line < term.topmost_line() || line > bottom {
                    Vec::new()
                } else {
                    row_text(line)
                }
            })
            .collect();
        let mut markers = Vec::new();
        for row in (0..rows).rev() {
            let chars = texts[row].clone();
            if chars.is_empty() {
                continue;
            }
            let text: String = chars.iter().collect();
            let summary = read_summary(&text).is_some();
            if !summary && !chars.contains(&'[') && !text.contains("Read(") {
                continue;
            }
            let from_bottom = summary.then_some(below);
            if summary {
                below += 1;
            }
            markers.extend(markers_in_line(row, &chars, from_bottom).into_iter().map(|marker| {
                let rows = free_rows(&texts, row, marker.start_col, marker.end_col);
                (marker, rows)
            }));
        }
        markers
    }

    fn picture(&mut self, image: &SessionImage) -> Arc<gpui::Image> {
        if self.session_thumbs.pictures.len() > CACHE_LIMIT {
            self.session_thumbs.pictures.clear();
        }
        self.session_thumbs
            .pictures
            .entry(Arc::as_ptr(&image.bytes) as usize)
            .or_insert_with(|| {
                Arc::new(gpui::Image::from_bytes(
                    image_format(&image.media_type),
                    image.bytes.to_vec(),
                ))
            })
            .clone()
    }

    /// Etiquetas en línea, dibujadas encima del texto que sustituyen.
    pub(super) fn render_inline_images(&mut self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let markers = self.visible_markers();
        if markers.is_empty() {
            return Vec::new();
        }
        let Some(tracker) = self.session_tracker() else { return Vec::new() };
        let Ok(guard) = tracker.try_lock() else { return Vec::new() };
        let groups: Vec<(usize, Vec<SessionImage>)> =
            guard.read_groups().rev().map(|group| (group.reads, group.images.clone())).collect();
        let mut missing = false;
        let mut found = Vec::new();
        for (marker, rows) in markers {
            let images = images_for(guard.images(), &groups, &marker.kind);
            if images.is_empty() {
                missing |= matches!(marker.kind, MarkerKind::Attached(_));
            } else {
                found.push((marker, rows, images));
            }
        }
        drop(guard);
        if missing {
            // Aún no está en lo leído del transcript: se relee en segundo plano
            // y la miniatura aparece en el siguiente pintado.
            self.poll_session_tracker(tracker, cx);
        }
        let cell = self.cell_width.as_f32();
        let line = self.line_height.as_f32();
        let theme = cx.theme();
        let (border, background, hover, muted) =
            (theme.border, theme.secondary, theme.accent_foreground, theme.muted_foreground);
        let mut elements = Vec::new();
        for (index, (marker, rows, images)) in found.into_iter().enumerate() {
            let left = GRID_PAD_X + marker.start_col as f32 * cell;
            let top = GRID_PAD_Y + marker.row as f32 * line;
            let width = (marker.end_col - marker.start_col) as f32 * cell;
            let height = rows as f32 * line - 2.0;
            let tall = rows > 1;
            let first = images[0].clone();
            let mut pill = div()
                .id(("inline-image", index))
                .absolute()
                .left(px(left))
                .top(px(top + 1.0))
                .w(px(width))
                .h(px(height))
                .flex()
                .items_center()
                .gap(px(3.0))
                .p(px(if tall { 3.0 } else { 1.5 }))
                .rounded(px(if tall { 8.0 } else { 4.0 }))
                .shadow_sm()
                .border_1()
                .border_color(border)
                .bg(background)
                .overflow_hidden()
                .cursor_pointer()
                .hover(move |style| style.border_color(hover))
                // El clic es de la etiqueta: ni selección ni ratón de la app.
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |view, _, _, cx| {
                    cx.stop_propagation();
                    view.show_session_image(&first, cx);
                }));
            let caption_size = (line * 0.58).max(9.0);
            if tall {
                // Grande: las imágenes llenan la etiqueta y el texto va como pie
                // de foto encima, para no perder qué era («#5», «Read 2 files»).
                let shown = images.len().min(3);
                let extra = images.len() - shown;
                for image in images.iter().take(shown) {
                    let picture = self.picture(image);
                    pill = pill.child(
                        img(picture)
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .rounded(px(5.0))
                            .object_fit(gpui::ObjectFit::Cover),
                    );
                }
                let mut caption = marker.caption.clone().unwrap_or_default();
                if extra > 0 {
                    if !caption.is_empty() {
                        caption.push_str(" · ");
                    }
                    caption.push_str(&format!("+{extra}"));
                }
                if !caption.is_empty() {
                    pill = pill.child(
                        div()
                            .absolute()
                            .left(px(6.0))
                            .bottom(px(5.0))
                            .max_w(px(width - 12.0))
                            .px(px(5.0))
                            .rounded(px(4.0))
                            .bg(gpui::black().opacity(0.62))
                            .text_size(px(caption_size))
                            .text_color(gpui::white())
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(caption),
                    );
                }
            } else {
                // Una línea: miniatura pequeña + «#5» / «+k» al lado.
                let thumb_h = height - 3.0;
                let thumb_w = (thumb_h * 1.5).max(18.0);
                let label = match &marker.kind {
                    MarkerKind::Attached(_) => marker.caption.clone(),
                    _ => None,
                };
                let label_w = if label.is_some() { cell * 3.0 } else { 0.0 };
                let fit = (((width - label_w - 3.0) / (thumb_w + 3.0)).floor() as usize).max(1);
                let shown = images.len().min(fit);
                let extra = images.len() - shown;
                for image in images.iter().take(shown) {
                    let picture = self.picture(image);
                    pill = pill.child(
                        img(picture)
                            .h(px(thumb_h))
                            .w(px(thumb_w))
                            .flex_shrink_0()
                            .rounded(px(3.0))
                            .object_fit(gpui::ObjectFit::Cover),
                    );
                }
                let text = match (label, extra) {
                    (Some(label), _) => Some(label),
                    (None, 0) => None,
                    (None, extra) => Some(format!("+{extra}")),
                };
                pill = pill.when_some(text, |pill, text| {
                    pill.child(
                        div()
                            .text_size(px(caption_size))
                            .text_color(muted)
                            .whitespace_nowrap()
                            .child(text),
                    )
                });
            }
            elements.push(pill.into_any_element());
        }
        elements
    }

    /// Barra inferior con las últimas imágenes que ha leído la IA. Va en su
    /// propia franja bajo el terminal: no tapa nada.
    pub(super) fn render_image_bar(&mut self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let tracker = self.session_tracker()?;
        let read: Vec<SessionImage> = {
            let guard = tracker.try_lock().ok()?;
            guard
                .images()
                .iter()
                .filter(|image| image.origin == ImageOrigin::Read)
                .cloned()
                .collect()
        };
        if read.is_empty() || self.session_thumbs.bar_hidden_at == Some(read.len()) {
            return None;
        }
        self.session_thumbs.bar_hidden_at = None;
        let total = read.len();
        let recent: Vec<SessionImage> = read.into_iter().rev().take(BAR_IMAGES).collect();
        let language = super::ui_language();
        let theme = cx.theme();
        let (border, muted, hover, surface) =
            (theme.border, theme.muted_foreground, theme.accent_foreground, theme.secondary);
        let mut row = div().flex().items_center().gap(px(6.0)).flex_1().min_w_0().overflow_hidden();
        for (index, image) in recent.into_iter().enumerate() {
            let picture = self.picture(&image);
            let tooltip: gpui::SharedString = image
                .label
                .clone()
                .map(|label| label.rsplit(['/', '\\']).next().unwrap_or(&label).to_owned())
                .unwrap_or_else(|| {
                    language.pick("AI 读取的图片", "Image read by the AI").to_owned()
                })
                .into();
            row = row.child(
                div()
                    .id(("ai-image-bar", index))
                    .size(px(BAR_THUMB))
                    .flex_shrink_0()
                    .rounded(px(6.0))
                    .overflow_hidden()
                    .border_1()
                    .border_color(border)
                    .cursor_pointer()
                    .hover(move |style| style.border_color(hover))
                    .tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
                    })
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.show_session_image(&image, cx);
                    }))
                    .child(img(picture).size_full().object_fit(gpui::ObjectFit::Cover)),
            );
        }
        let title = language.pick("AI 看过的图片", "Seen by the AI").to_owned();
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .h(px(IMAGE_BAR_HEIGHT))
                .flex_shrink_0()
                .px(px(10.0))
                .mx(px(-4.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(border)
                .bg(surface.opacity(0.55))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("🖼  {title} · {total}")),
                )
                .child(row)
                .child(
                    div()
                        .id("ai-image-bar-close")
                        .flex_shrink_0()
                        .px(px(6.0))
                        .text_color(muted)
                        .cursor_pointer()
                        .hover(move |style| style.text_color(hover))
                        .child("×")
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.session_thumbs.bar_hidden_at = Some(total);
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(text: &str) -> Vec<char> {
        text.chars().collect()
    }

    #[test]
    fn finds_attached_and_read_markers_covering_their_text() {
        let markers =
            markers_in_line(3, &chars("> mira [Image #5] y ● Read(docs/shot.png) ok"), None);
        assert_eq!(
            markers,
            vec![
                Marker {
                    row: 3,
                    start_col: 7,
                    end_col: 17,
                    kind: MarkerKind::Attached("Image #5".into()),
                    caption: Some("#5".into()),
                },
                Marker {
                    row: 3,
                    start_col: 22,
                    end_col: 26,
                    kind: MarkerKind::ReadPath("docs/shot.png".into()),
                    caption: None,
                },
            ]
        );
        assert!(markers_in_line(0, &chars("● Read(src/main.rs)"), None).is_empty());
        assert!(markers_in_line(0, &chars("[Image #x] [Image]"), None).is_empty());
    }

    #[test]
    fn detects_claude_code_read_summaries() {
        assert_eq!(read_summary("  Read 1 file"), Some((2, 13, 1)));
        assert_eq!(read_summary("Searched for 1 pattern, read 2 files"), Some((24, 36, 2)));
        assert_eq!(read_summary("● Reading 3 files…").map(|found| found.2), Some(3));
        assert_eq!(read_summary("thread 2 files"), None);
        assert_eq!(read_summary("Ran 2 shell commands"), None);
        assert_eq!(read_summary("Read the 2 files"), None);
        let markers = markers_in_line(4, &chars("  Read 2 files"), Some(1));
        assert_eq!(
            markers,
            vec![Marker {
                row: 4,
                start_col: 2,
                end_col: 14,
                kind: MarkerKind::ReadSummary { reads: 2, from_bottom: 1 },
                caption: Some("Read 2 files".into()),
            }]
        );
    }

    #[test]
    fn labels_grow_only_into_blank_cells() {
        let rows: Vec<Vec<char>> = ["  Read 1 file", "", "             ", "  ● texto", "      "]
            .iter()
            .map(|row| format!("{row:<20}").chars().collect())
            .collect();
        // Debajo de «Read 1 file» hay dos filas vacías y luego texto.
        assert_eq!(free_rows(&rows, 0, 2, 13), 3);
        // La fila 3 tiene texto en esas columnas: no crece.
        assert_eq!(free_rows(&rows, 2, 2, 13), 1);
    }

    fn image(origin: ImageOrigin, label: &str) -> SessionImage {
        SessionImage {
            origin,
            label: Some(label.into()),
            media_type: "image/png".into(),
            bytes: Arc::new(label.as_bytes().to_vec()),
        }
    }

    #[test]
    fn markers_resolve_to_the_right_images() {
        let shot = image(ImageOrigin::Read, r"C:\proyectos\web\docs\shot.png");
        let images = vec![image(ImageOrigin::Sent, "Image #5"), shot.clone()];
        let groups = vec![(1, vec![shot.clone()]), (2, vec![])];
        let read = images_for(&images, &groups, &MarkerKind::ReadPath("docs/shot.png".into()));
        assert_eq!(read.len(), 1);
        assert_eq!(images_for(&images, &groups, &MarkerKind::Attached("Image #5".into())).len(), 1);
        assert!(images_for(&images, &groups, &MarkerKind::ReadPath("hot.png".into())).is_empty());
        // Resúmenes: por orden desde abajo y solo si cuadra el número de lecturas.
        let newest = MarkerKind::ReadSummary { reads: 1, from_bottom: 0 };
        assert_eq!(images_for(&images, &groups, &newest).len(), 1);
        let mismatch = MarkerKind::ReadSummary { reads: 3, from_bottom: 0 };
        assert!(images_for(&images, &groups, &mismatch).is_empty());
        let beyond = MarkerKind::ReadSummary { reads: 1, from_bottom: 7 };
        assert!(images_for(&images, &groups, &beyond).is_empty());
    }
}

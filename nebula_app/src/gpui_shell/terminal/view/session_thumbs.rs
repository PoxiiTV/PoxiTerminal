//! Miniaturas fijas en el terminal: junto a cada `[Image #N]` (imagen que le
//! mandaste a la IA) y cada `Read(foto.png)` (imagen que leyó la IA) aparece
//! la imagen en pequeño, como en la app de escritorio. Un clic la abre en
//! grande en una pestaña.
//!
//! Las imágenes salen del transcript de la sesión (mismo lector que el panel
//! «Agentes»), así que funciona igual en local, WSL o SSH y sin leer el disco.
//! Son elementos propios encima del terminal: el clic lo recibe la miniatura,
//! no la aplicación (aunque Claude Code tenga el ratón capturado).

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, StyledImage as _, div, img, px,
};
use gpui_component::ActiveTheme as _;
use nebula_terminal::grid::Dimensions as _;
use nebula_terminal::index::Column;

use super::TerminalView;
use crate::ai_usage::{ImageOrigin, SessionImage};

/// Padding del terminal dentro de su tarjeta (ver `render`): la rejilla
/// empieza aquí.
const GRID_PAD_X: f32 = 12.0;
const GRID_PAD_Y: f32 = 8.0;
/// Alto de la miniatura en líneas del terminal.
const THUMB_LINES: f32 = 2.6;
/// Imágenes convertidas que se guardan como mucho.
const CACHE_LIMIT: usize = 64;

/// Referencia a una imagen encontrada en una fila visible.
#[derive(Debug, PartialEq, Eq)]
struct Marker {
    row: usize,
    /// Columna donde termina el texto de la referencia.
    end_col: usize,
    kind: MarkerKind,
}

#[derive(Debug, PartialEq, Eq)]
enum MarkerKind {
    /// `[Image #N]` → "Image #N".
    Attached(String),
    /// `Read(ruta.png)` → "ruta.png".
    Read(String),
}

const IMAGE_EXTENSIONS: [&str; 6] = [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"];

/// Busca referencias a imágenes en el texto de una fila.
fn markers_in_line(row: usize, line: &[char]) -> Vec<Marker> {
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
                end_col: col_of(end),
                kind: MarkerKind::Attached(format!("Image #{digits}")),
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
                found.push(Marker {
                    row,
                    end_col: col_of(inner_start + close + 1),
                    kind: MarkerKind::Read(inner.to_owned()),
                });
            }
        }
        search = start + 1;
    }
    found
}

/// La imagen del transcript que corresponde a una referencia.
fn find_image<'a>(images: &'a [SessionImage], kind: &MarkerKind) -> Option<&'a SessionImage> {
    match kind {
        MarkerKind::Attached(label) => {
            images.iter().rev().find(|image| image.label.as_deref() == Some(label.as_str()))
        },
        MarkerKind::Read(path) => {
            // La CLI muestra la ruta relativa o recortada; el transcript guarda
            // la absoluta. Se compara por el final, con barras normalizadas.
            let wanted = path.replace('\\', "/").trim_start_matches("./").to_ascii_lowercase();
            images.iter().rev().find(|image| {
                image.origin == ImageOrigin::Read
                    && image.label.as_deref().is_some_and(|label| {
                        let label = label.replace('\\', "/").to_ascii_lowercase();
                        label == wanted || label.ends_with(&format!("/{wanted}"))
                    })
            })
        },
    }
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

/// Caché de imágenes ya convertidas para pintar, por puntero de los bytes.
#[derive(Default)]
pub(in crate::gpui_shell::terminal) struct InlineImageCache(HashMap<usize, Arc<gpui::Image>>);

impl TerminalView {
    /// Referencias a imágenes en las filas visibles ahora mismo.
    fn visible_markers(&self) -> Vec<Marker> {
        let Some(session) = &self.session else { return Vec::new() };
        let term = session.term.lock();
        let origin = term.viewport_origin_for(self.rows);
        let mut markers = Vec::new();
        for row in 0..self.rows {
            let line = origin + row as i32;
            if line < term.topmost_line() || line > term.bottommost_line() {
                continue;
            }
            let cells = &term.grid()[line];
            let chars: Vec<char> = (0..term.columns()).map(|col| cells[Column(col)].c).collect();
            // Atajo barato: la mayoría de filas no tienen ni "[" ni "Read(".
            if !chars.contains(&'[') && !chars.windows(5).any(|w| w == ['R', 'e', 'a', 'd', '(']) {
                continue;
            }
            markers.extend(markers_in_line(row, &chars));
        }
        markers
    }

    /// Miniaturas a dibujar encima del terminal (posición absoluta en el pane).
    pub(super) fn render_inline_images(&mut self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let markers = self.visible_markers();
        if markers.is_empty() {
            return Vec::new();
        }
        let Some(tracker) = self.session_tracker() else { return Vec::new() };
        let Ok(guard) = tracker.try_lock() else { return Vec::new() };
        let mut missing = false;
        let mut found = Vec::new();
        for marker in markers {
            match find_image(guard.images(), &marker.kind) {
                Some(image) => found.push((marker, image.clone())),
                None => missing = true,
            }
        }
        drop(guard);
        if missing {
            // Aún no está en lo leído del transcript: se relee en segundo plano
            // y la miniatura aparece en el siguiente pintado.
            self.poll_session_tracker(tracker, cx);
        }
        if self.session_thumbs.0.len() > CACHE_LIMIT {
            self.session_thumbs.0.clear();
        }
        let cell = self.cell_width.as_f32();
        let line = self.line_height.as_f32();
        let height = (line * THUMB_LINES).max(28.0);
        let theme = cx.theme();
        let (border, background, hover) = (theme.border, theme.popover, theme.accent);
        found
            .into_iter()
            .enumerate()
            .map(|(index, (marker, image))| {
                let picture = self
                    .session_thumbs
                    .0
                    .entry(Arc::as_ptr(&image.bytes) as usize)
                    .or_insert_with(|| {
                        Arc::new(gpui::Image::from_bytes(
                            image_format(&image.media_type),
                            image.bytes.to_vec(),
                        ))
                    })
                    .clone();
                let left = GRID_PAD_X + (marker.end_col as f32 + 1.0) * cell;
                let top = GRID_PAD_Y + marker.row as f32 * line;
                div()
                    .id(("inline-image", index))
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .h(px(height))
                    .max_w(px(height * 3.0))
                    .p(px(2.0))
                    .rounded_md()
                    .border_1()
                    .border_color(border)
                    .bg(background)
                    .shadow_md()
                    .cursor_pointer()
                    .hover(move |style| style.border_color(hover))
                    // El clic es de la miniatura: ni selección ni ratón de la app.
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |view, _, _, cx| {
                        cx.stop_propagation();
                        view.open_session_image(&image, cx);
                    }))
                    .child(img(picture).h_full().rounded_sm().object_fit(gpui::ObjectFit::Contain))
                    .into_any_element()
            })
            .collect()
    }

    /// Abre una imagen de la sesión en grande en una pestaña.
    fn open_session_image(&self, image: &SessionImage, cx: &mut Context<Self>) {
        let extension = match image.media_type.as_str() {
            "image/jpeg" | "image/jpg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            _ => "png",
        };
        // Nombre estable por contenido: abrir la misma imagen no crea copias.
        let digest = {
            use std::hash::{Hash as _, Hasher as _};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            image.bytes.hash(&mut hasher);
            hasher.finish()
        };
        let path = std::env::temp_dir().join(format!("poxiterminal-img-{digest:016x}.{extension}"));
        if !path.exists() && std::fs::write(&path, image.bytes.as_slice()).is_err() {
            return;
        }
        let pane_id = self.pane_id;
        cx.defer(move |cx| {
            crate::gpui_shell::workspace::windowing::open_path_for_pane(pane_id, path, cx)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(text: &str) -> Vec<char> {
        text.chars().collect()
    }

    #[test]
    fn finds_attached_and_read_markers_with_their_end_column() {
        let markers = markers_in_line(3, &chars("> mira [Image #5] y ● Read(docs/shot.png) ok"));
        assert_eq!(
            markers,
            vec![
                Marker { row: 3, end_col: 17, kind: MarkerKind::Attached("Image #5".into()) },
                Marker { row: 3, end_col: 41, kind: MarkerKind::Read("docs/shot.png".into()) },
            ]
        );
        assert!(markers_in_line(0, &chars("● Read(src/main.rs)")).is_empty());
        assert!(markers_in_line(0, &chars("[Image #x] [Image]")).is_empty());
    }

    fn image(origin: ImageOrigin, label: &str) -> SessionImage {
        SessionImage {
            origin,
            label: Some(label.into()),
            media_type: "image/png".into(),
            bytes: Arc::new(vec![1, 2, 3]),
        }
    }

    #[test]
    fn read_markers_match_absolute_transcript_paths_by_suffix() {
        let images = vec![
            image(ImageOrigin::Sent, "Image #5"),
            image(ImageOrigin::Read, r"C:\proyectos\web\docs\shot.png"),
        ];
        let read = find_image(&images, &MarkerKind::Read("docs/shot.png".into())).unwrap();
        assert_eq!(read.origin, ImageOrigin::Read);
        let sent = find_image(&images, &MarkerKind::Attached("Image #5".into())).unwrap();
        assert_eq!(sent.origin, ImageOrigin::Sent);
        assert!(find_image(&images, &MarkerKind::Read("hot.png".into())).is_none());
        assert!(find_image(&images, &MarkerKind::Attached("Image #6".into())).is_none());
    }
}

//! Imágenes de la IA en el terminal: al pasar el ratón por `[Image #3]` (lo
//! que muestra Claude Code al adjuntar una imagen) o por la ruta de una imagen
//! (por ejemplo `Read(shot.png)`), aparece la miniatura. Ctrl+clic la abre en
//! grande en una pestaña.
//!
//! Las imágenes adjuntas salen del transcript de la sesión (`ai_usage`), que
//! es el mismo lector que usa el panel «Agentes».

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use gpui::{
    Context, IntoElement, ParentElement as _, Pixels, Point, Styled as _, StyledImage as _,
    anchored, deferred, div, img, px,
};
use gpui_component::ActiveTheme as _;
use nebula_terminal::grid::Dimensions as _;
use nebula_terminal::index::{Column, Point as TermPoint};

use super::TerminalView;
use crate::ai_usage::{SessionImage, UsageTracker};

/// Una imagen mayor que esto no se previsualiza al pasar el ratón.
const MAX_PREVIEW_BYTES: u64 = 25 * 1024 * 1024;

#[derive(Clone)]
pub(in crate::gpui_shell::terminal) struct ImageHover {
    /// Texto bajo el ratón que la originó (para no recalcular en cada píxel).
    key: String,
    label: String,
    image: Arc<gpui::Image>,
    /// Archivo en disco (rutas) o None (adjunta: se vuelca a un temporal al abrir).
    path: Option<PathBuf>,
    bytes: Arc<Vec<u8>>,
    media_type: String,
    position: Point<Pixels>,
}

/// Qué hay bajo el ratón que pueda ser una imagen.
#[derive(Debug, PartialEq, Eq)]
enum ImageRef {
    /// `[Image #3]` → 3.
    Attached(u32),
    /// Una ruta que acaba en extensión de imagen.
    Path(String),
}

const IMAGE_EXTENSIONS: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico"];

/// Busca `[Image #N]` o una ruta de imagen que cubra la columna `col` de `line`.
fn image_ref_at(line: &str, col: usize) -> Option<ImageRef> {
    let chars: Vec<char> = line.chars().collect();
    if col >= chars.len() {
        return None;
    }
    // `[Image #N]`
    let text: String = chars.iter().collect();
    let mut search = 0;
    while let Some(start) = text[search..].find("[Image #").map(|found| found + search) {
        let start_col = text[..start].chars().count();
        let rest = &text[start + "[Image #".len()..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() && rest[digits.len()..].starts_with(']') {
            let end_col = start_col + "[Image #".len() + digits.len();
            if (start_col..=end_col).contains(&col) {
                return digits.parse().ok().map(ImageRef::Attached);
            }
        }
        search = start + 1;
    }
    // Ruta: palabra bajo el cursor, cortada por espacios, comillas y paréntesis.
    let stop =
        |c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '(' | ')' | '`' | '<' | '>' | ',');
    if stop(chars[col]) {
        return None;
    }
    let mut start = col;
    while start > 0 && !stop(chars[start - 1]) {
        start -= 1;
    }
    let mut end = col;
    while end + 1 < chars.len() && !stop(chars[end + 1]) {
        end += 1;
    }
    let word: String = chars[start..=end].iter().collect();
    let word = word.trim_end_matches(['.', ':', ';']);
    let extension = Path::new(word).extension()?.to_str()?.to_ascii_lowercase();
    IMAGE_EXTENSIONS.contains(&extension.as_str()).then(|| ImageRef::Path(word.to_owned()))
}

/// Solo discos locales: nada de rutas UNC (\\servidor\…, //servidor/…).
fn is_local_path(path: &Path) -> bool {
    let text = path.to_string_lossy();
    !(text.starts_with("\\\\") || text.starts_with("//") || text.starts_with("\\/"))
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

fn media_type_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        _ => "image/png",
    }
}

impl TerminalView {
    /// Lector del transcript de la IA de este panel, compartido con el panel
    /// «Agentes». Solo existe si la CLI informó de su archivo y es accesible
    /// desde Windows.
    pub(crate) fn session_tracker(&mut self) -> Option<Arc<Mutex<UsageTracker>>> {
        let file = self.agent_session_file();
        // Solo en desarrollo: forzar un transcript para probar la interfaz.
        #[cfg(debug_assertions)]
        let file = file.or_else(|| std::env::var("POXITERMINAL_DEBUG_TRANSCRIPT").ok());
        let path = PathBuf::from(file?);
        let current = self.session_tracker.as_ref().is_some_and(|tracker| {
            tracker.try_lock().map_or(true, |tracker| tracker.path() == path)
        });
        if !current {
            if !path.is_file() {
                return None;
            }
            self.session_tracker = Some(Arc::new(Mutex::new(UsageTracker::new(path))));
        }
        self.session_tracker.clone()
    }

    /// Texto de la línea de la rejilla en `point`.
    fn grid_line_text(&self, point: TermPoint) -> Option<String> {
        let session = self.session.as_ref()?;
        let term = session.term.lock();
        if point.line < term.topmost_line() || point.line > term.bottommost_line() {
            return None;
        }
        let row = &term.grid()[point.line];
        Some((0..term.columns()).map(|col| row[Column(col)].c).collect())
    }

    /// Actualiza la miniatura según lo que haya bajo el ratón.
    pub(super) fn update_image_hover(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.image_mouse = position;
        let (point, _) = self.grid_point(position);
        let found = self
            .grid_line_text(point)
            .and_then(|line| image_ref_at(&line, point.column.0))
            .and_then(|found| self.resolve_image_ref(found, cx));
        match (found, &mut self.image_hover) {
            (Some(next), Some(current)) if current.key == next.key => {
                current.position = position;
                cx.notify();
            },
            (Some(mut next), _) => {
                next.position = position;
                self.image_hover = Some(next);
                cx.notify();
            },
            (None, Some(_)) => {
                self.image_hover = None;
                cx.notify();
            },
            (None, None) => {},
        }
        // Si el ratón ya no está sobre la imagen que se estaba leyendo, se olvida.
        if self.image_hover.is_none() {
            let (point, _) = self.grid_point(position);
            let still = self
                .grid_line_text(point)
                .and_then(|line| image_ref_at(&line, point.column.0))
                .is_some();
            if !still {
                self.image_loading = None;
            }
        }
    }

    pub(super) fn clear_image_hover(&mut self, cx: &mut Context<Self>) {
        self.image_loading = None;
        if self.image_hover.take().is_some() {
            cx.notify();
        }
    }

    fn resolve_image_ref(&mut self, found: ImageRef, cx: &mut Context<Self>) -> Option<ImageHover> {
        match found {
            ImageRef::Attached(number) => {
                let label = format!("Image #{number}");
                if let Some(current) = &self.image_hover {
                    if current.key == label {
                        return Some(current.clone());
                    }
                }
                let tracker = self.session_tracker()?;
                let image: Option<SessionImage> = tracker.try_lock().ok().and_then(|tracker| {
                    tracker
                        .images()
                        .iter()
                        .rev()
                        .find(|image| image.label.as_deref() == Some(label.as_str()))
                        .cloned()
                });
                let Some(image) = image else {
                    // Aún no leída: se relee el transcript en segundo plano y
                    // la miniatura aparece en el siguiente movimiento.
                    self.poll_session_tracker(tracker, cx);
                    return None;
                };
                Some(ImageHover {
                    key: label.clone(),
                    label: format!("[{label}]"),
                    image: Arc::new(gpui::Image::from_bytes(
                        image_format(&image.media_type),
                        image.bytes.to_vec(),
                    )),
                    path: None,
                    bytes: image.bytes,
                    media_type: image.media_type,
                    position: Point::default(),
                })
            },
            ImageRef::Path(text) => {
                // Primero, la imagen tal como la leyó la IA (transcript): vale
                // también en WSL/SSH, donde la ruta no existe en este PC.
                let key = format!("read:{text}");
                if let Some(current) = &self.image_hover {
                    if current.key == key {
                        return Some(current.clone());
                    }
                }
                let from_transcript = self.session_tracker().and_then(|tracker| {
                    let guard = tracker.try_lock().ok()?;
                    super::session_thumbs::read_image_for_path(guard.images(), &text)
                });
                if let Some(image) = from_transcript {
                    return Some(ImageHover {
                        key,
                        label: text.rsplit(['/', '\\']).next().unwrap_or(&text).to_owned(),
                        image: Arc::new(gpui::Image::from_bytes(
                            image_format(&image.media_type),
                            image.bytes.to_vec(),
                        )),
                        path: None,
                        bytes: image.bytes,
                        media_type: image.media_type,
                        position: Point::default(),
                    });
                }
                let mut path = PathBuf::from(&text);
                if path.is_relative() {
                    path = self.local_cwd()?.join(path);
                }
                // Rutas de red (\\servidor\… o //servidor/…): ni se tocan. Abrirlas
                // al pasar el ratón conectaría por SMB (y congelaría la ventana).
                if !is_local_path(&path) {
                    return None;
                }
                let key = path.to_string_lossy().into_owned();
                if let Some(current) = &self.image_hover {
                    if current.key == key {
                        return Some(current.clone());
                    }
                }
                if self.image_loading.as_deref() != Some(key.as_str()) {
                    self.load_image_path(key, path, cx);
                }
                None
            },
        }
    }

    /// Lee la imagen en segundo plano; si el ratón sigue encima al terminar,
    /// aparece la miniatura.
    fn load_image_path(&mut self, key: String, path: PathBuf, cx: &mut Context<Self>) {
        self.image_loading = Some(key.clone());
        let task = cx.background_executor().spawn(async move {
            let metadata = std::fs::metadata(&path).ok()?;
            if !metadata.is_file() || metadata.len() > MAX_PREVIEW_BYTES {
                return None;
            }
            let bytes = std::fs::read(&path).ok()?;
            let media_type = media_type_for(&path).to_owned();
            let image = Arc::new(gpui::Image::from_bytes(image_format(&media_type), bytes.clone()));
            let label = path.file_name()?.to_string_lossy().into_owned();
            Some(ImageHover {
                key,
                label,
                image,
                path: Some(path),
                bytes: Arc::new(bytes),
                media_type,
                position: Point::default(),
            })
        });
        cx.spawn(async move |this, cx| {
            let loaded = task.await;
            let _ = this.update(cx, |view, cx| {
                let Some(mut hover) = loaded else {
                    view.image_loading = None;
                    return;
                };
                if view.image_loading.as_deref() == Some(hover.key.as_str()) {
                    view.image_loading = None;
                    hover.position = view.image_mouse;
                    view.image_hover = Some(hover);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn poll_session_tracker(
        &mut self,
        tracker: Arc<Mutex<UsageTracker>>,
        cx: &mut Context<Self>,
    ) {
        if self.session_tracker_polling {
            return;
        }
        self.session_tracker_polling = true;
        let task = cx.background_executor().spawn(async move {
            if let Ok(mut tracker) = tracker.try_lock() {
                let _ = tracker.poll();
            }
        });
        cx.spawn(async move |this, cx| {
            task.await;
            let _ = this.update(cx, |view, cx| {
                view.session_tracker_polling = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// Ctrl+clic sobre una imagen: abrirla en grande sobre el panel.
    pub(super) fn open_hovered_image(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(hover) = self.image_hover.clone() else { return false };
        self.show_image(hover.image, hover.label, cx);
        true
    }

    pub(super) fn render_image_hover(&self, cx: &Context<Self>) -> Option<impl IntoElement> {
        let hover = self.image_hover.as_ref()?;
        let theme = cx.theme();
        let hint = super::ui_language().pick("Ctrl+单击放大", "Ctrl+click to enlarge");
        Some(
            deferred(
                anchored()
                    .position(hover.position)
                    .offset(gpui::point(px(14.0), px(18.0)))
                    .snap_to_window_with_margin(px(8.0))
                    .child(
                        div()
                            .p_1p5()
                            .rounded_lg()
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.popover)
                            .shadow_lg()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                img(hover.image.clone())
                                    .max_w(px(260.0))
                                    .max_h(px(180.0))
                                    .rounded_md()
                                    .object_fit(gpui::ObjectFit::Contain),
                            )
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .gap_3()
                                    .px_0p5()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(hover.label.clone())
                                    .child(hint.to_owned()),
                            ),
                    ),
            )
            .with_priority(3),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_attached_image_markers_under_the_cursor() {
        let line = "> mira esto [Image #12] y [Image #3]";
        let first = line.find("[Image #12]").unwrap();
        assert_eq!(image_ref_at(line, first), Some(ImageRef::Attached(12)));
        assert_eq!(image_ref_at(line, first + 10), Some(ImageRef::Attached(12)));
        assert_eq!(
            image_ref_at(line, line.find("[Image #3]").unwrap() + 4),
            Some(ImageRef::Attached(3))
        );
        assert_eq!(image_ref_at(line, 2), None);
    }

    #[test]
    fn network_paths_are_never_previewed() {
        assert!(!is_local_path(Path::new(r"\\attacker\share\x.png")));
        assert!(!is_local_path(Path::new("//attacker/share/x.png")));
        assert!(is_local_path(Path::new(r"C:\tmp\x.png")));
        assert!(is_local_path(Path::new("docs/x.png")));
    }

    #[test]
    fn finds_image_paths_inside_tool_calls() {
        let line = "● Read(docs/shot-17.png)";
        let col = line.find("shot").unwrap();
        assert_eq!(image_ref_at(line, col), Some(ImageRef::Path("docs/shot-17.png".to_owned())));
        assert_eq!(
            image_ref_at("guardado en C:\\tmp\\a.JPG.", 15),
            Some(ImageRef::Path("C:\\tmp\\a.JPG".to_owned()))
        );
        assert_eq!(image_ref_at("main.rs y notas.txt", 2), None);
    }
}

//! Visor de imágenes en grande **dentro del propio panel**: fondo oscurecido,
//! la imagen centrada y un pie con su nombre. Se cierra con un clic en
//! cualquier sitio o con cualquier tecla (Esc), y vuelves justo donde estabas;
//! no abre pestañas.

use std::sync::Arc;

use gpui::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _,
    StyledImage as _, div, img, px, relative,
};

use super::TerminalView;
use crate::ai_usage::SessionImage;

#[derive(Clone)]
pub(in crate::gpui_shell::terminal) struct Lightbox {
    image: Arc<gpui::Image>,
    caption: String,
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

impl TerminalView {
    /// Abre una imagen de la sesión en grande sobre este panel.
    pub(crate) fn show_session_image(&mut self, image: &SessionImage, cx: &mut Context<Self>) {
        let caption = image
            .label
            .as_deref()
            .map(|label| label.rsplit(['/', '\\']).next().unwrap_or(label).to_owned())
            .unwrap_or_default();
        self.show_image(
            Arc::new(gpui::Image::from_bytes(
                image_format(&image.media_type),
                image.bytes.to_vec(),
            )),
            caption,
            cx,
        );
    }

    pub(super) fn show_image(
        &mut self,
        image: Arc<gpui::Image>,
        caption: String,
        cx: &mut Context<Self>,
    ) {
        self.image_hover = None;
        self.lightbox = Some(Lightbox { image, caption });
        cx.notify();
    }

    /// Cierra el visor si está abierto. Devuelve si lo estaba.
    pub(super) fn close_lightbox(&mut self, cx: &mut Context<Self>) -> bool {
        let open = self.lightbox.take().is_some();
        if open {
            cx.notify();
        }
        open
    }

    pub(super) fn render_lightbox(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let lightbox = self.lightbox.as_ref()?;
        let hint = super::ui_language()
            .pick("单击任意处或按 Esc 关闭", "Click anywhere or press Esc to close");
        Some(
            div()
                .id("image-lightbox")
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.0))
                .p(px(24.0))
                .bg(gpui::black().opacity(0.78))
                .cursor_pointer()
                // Todo el panel es «cerrar»: ni selección ni ratón de la app.
                .on_mouse_down(gpui::MouseButton::Left, cx.listener(|view, _, _, cx| {
                    cx.stop_propagation();
                    view.close_lightbox(cx);
                }))
                .on_mouse_down(gpui::MouseButton::Right, |_, _, cx| cx.stop_propagation())
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .child(
                    img(lightbox.image.clone())
                        .max_w(relative(1.0))
                        .max_h(relative(0.86))
                        .rounded(px(8.0))
                        .shadow_lg()
                        .object_fit(gpui::ObjectFit::Contain),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(12.0))
                        .text_xs()
                        .text_color(gpui::white().opacity(0.8))
                        .child(lightbox.caption.clone())
                        .child(div().text_color(gpui::white().opacity(0.5)).child(hint.to_owned())),
                ),
        )
    }
}

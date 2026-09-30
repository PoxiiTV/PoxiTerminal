//! Visor de cambios: qué ha tocado la IA en un turno (o todo lo pendiente
//! de commit), archivo por archivo, con opción de descartar.
//!
//! La vista es una entidad propia que la pestaña de código incrusta (igual que
//! el modo de fusión), así el workspace no necesita un tipo de pestaña nuevo.

pub(crate) mod git;

use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, UniformListScrollHandle, Window, div, px,
    uniform_list,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _};

use self::git::{DiffLine, FileDiff, FileStatus, LineKind};
use crate::display::side_panel::GitLocation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiffMode {
    /// Solo lo que cambió desde la foto del inicio del turno.
    Turn,
    /// Todo lo pendiente respecto al último commit.
    Uncommitted,
}

enum LoadState {
    Loading,
    Ready { base: String, files: Vec<FileDiff> },
    Error(String),
}

pub(crate) struct DiffView {
    location: GitLocation,
    turn_base: Option<String>,
    mode: DiffMode,
    state: LoadState,
    selected: usize,
    /// Archivo cuyo «Descartar» espera la segunda pulsación.
    confirm_discard: Option<String>,
    /// Archivo a seleccionar cuando termine la carga (doble clic en el panel Git).
    preselect: Option<String>,
    notice: Option<String>,
    scroll: UniformListScrollHandle,
}

impl DiffView {
    pub(crate) fn new(
        location: GitLocation,
        turn_base: Option<String>,
        preselect: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mode = if turn_base.is_some() { DiffMode::Turn } else { DiffMode::Uncommitted };
        let mut view = Self {
            location,
            turn_base,
            mode,
            state: LoadState::Loading,
            selected: 0,
            confirm_discard: None,
            preselect,
            notice: None,
            scroll: UniformListScrollHandle::new(),
        };
        view.reload(cx);
        view
    }

    /// Nombre del repositorio para el título de la pestaña.
    pub(crate) fn repo_name(&self) -> String {
        let root = match &self.location {
            GitLocation::Local { root } => root.to_string_lossy().into_owned(),
            GitLocation::Wsl { root, .. } => root.clone(),
        };
        root.trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or(&root)
            .to_owned()
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.state = LoadState::Loading;
        self.confirm_discard = None;
        let location = self.location.clone();
        let base = match self.mode {
            DiffMode::Turn => self.turn_base.clone(),
            DiffMode::Uncommitted => None,
        };
        let task = cx.background_executor().spawn(async move {
            let base = base.unwrap_or_else(|| git::head_tree(&location));
            let current = git::snapshot_tree(&location)?;
            let files = git::diff_trees(&location, &base, &current)?;
            Ok::<_, String>((base, files))
        });
        cx.spawn(async move |this, cx| {
            let loaded = task.await;
            let _ = this.update(cx, |view, cx| {
                view.state = match loaded {
                    Ok((base, files)) => {
                        if let Some(path) = view.preselect.take() {
                            view.selected =
                                files.iter().position(|file| file.path == path).unwrap_or(0);
                        }
                        view.selected = view.selected.min(files.len().saturating_sub(1));
                        LoadState::Ready { base, files }
                    },
                    Err(error) => LoadState::Error(error),
                };
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn set_mode(&mut self, mode: DiffMode, cx: &mut Context<Self>) {
        if self.mode != mode {
            self.mode = mode;
            self.selected = 0;
            self.reload(cx);
        }
    }

    fn discard_selected(&mut self, cx: &mut Context<Self>) {
        let LoadState::Ready { base, files } = &self.state else { return };
        let Some(file) = files.get(self.selected).cloned() else { return };
        if self.confirm_discard.as_deref() != Some(file.path.as_str()) {
            self.confirm_discard = Some(file.path.clone());
            cx.notify();
            return;
        }
        self.confirm_discard = None;
        let location = self.location.clone();
        let base = base.clone();
        let unstage = self.mode == DiffMode::Uncommitted;
        let task = cx.background_executor().spawn(async move {
            git::discard(&location, &base, &file, unstage).map(|()| file.path)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |view, cx| {
                let language = crate::gpui_shell::config::ui_language(cx);
                view.notice = Some(match result {
                    Ok(path) => language
                        .pick("已放弃 {path} 的更改", "Discarded changes to {path}")
                        .replace("{path}", &path),
                    Err(error) => error,
                });
                view.reload(cx);
            });
        })
        .detach();
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let language = crate::gpui_shell::config::ui_language(cx);
        let mode_button = |id: &'static str, label: &'static str, mode: DiffMode, enabled: bool| {
            let active = self.mode == mode;
            Button::new(id)
                .xsmall()
                .label(label)
                .when(active, |button| button.primary())
                .when(!active, |button| button.ghost())
                .disabled(!enabled)
                .on_click(cx.listener(move |this, _, _, cx| this.set_mode(mode, cx)))
        };
        let files = match &self.state {
            LoadState::Ready { files, .. } => files.as_slice(),
            _ => &[],
        };
        let mut list = div().id("diff-files").flex().flex_col().flex_1().overflow_y_scroll();
        for (index, file) in files.iter().enumerate() {
            let selected = index == self.selected;
            let (badge, color) = status_badge(file.status, cx);
            let (dir, name) = split_path(&file.path);
            list = list.child(
                div()
                    .id(("diff-file", index))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .when(selected, |row| row.bg(theme.accent))
                    .when(!selected, |row| row.hover(|style| style.bg(theme.list_hover)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = index;
                        this.confirm_discard = None;
                        this.scroll.scroll_to_item(0, gpui::ScrollStrategy::Top);
                        cx.notify();
                    }))
                    .child(
                        div()
                            .w(px(14.0))
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(color)
                            .child(badge),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_sm()
                            .child(name.to_owned())
                            .when(!dir.is_empty(), |row| {
                                row.child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(dir.to_owned()),
                                )
                            }),
                    )
                    .child(counts(file, cx)),
            );
        }
        let empty = matches!(&self.state, LoadState::Ready { files, .. } if files.is_empty());
        div()
            .flex()
            .flex_col()
            .w(px(300.0))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .pt_3()
                    .pb_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(language.pick("更改", "Changes").to_owned())
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(self.repo_name()),
                            ),
                    )
                    .child(
                        Button::new("diff-refresh")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(IconName::Redo))
                            .tooltip(language.pick("刷新", "Refresh"))
                            .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_1()
                    .px_3()
                    .pb_2()
                    .child(mode_button(
                        "diff-mode-turn",
                        language.pick("本轮", "This turn"),
                        DiffMode::Turn,
                        self.turn_base.is_some(),
                    ))
                    .child(mode_button(
                        "diff-mode-all",
                        language.pick("全部未提交", "All uncommitted"),
                        DiffMode::Uncommitted,
                        true,
                    )),
            )
            .child(list.px_1())
            .when(empty, |sidebar| {
                sidebar.child(
                    div()
                        .p_3()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(language.pick("没有更改", "No changes").to_owned()),
                )
            })
    }

    fn render_body(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = cx.theme();
        let language = crate::gpui_shell::config::ui_language(cx);
        let file = match &self.state {
            LoadState::Loading => {
                return centered(language.pick("正在计算更改…", "Computing changes…"), cx);
            },
            LoadState::Error(error) => return centered(error, cx),
            LoadState::Ready { files, .. } => match files.get(self.selected) {
                Some(file) => file.clone(),
                None => return centered(language.pick("没有更改", "No changes"), cx),
            },
        };
        let confirming = self.confirm_discard.as_deref() == Some(file.path.as_str());
        let discard_label = if confirming {
            language.pick("确定放弃？", "Discard? Click again")
        } else {
            language.pick("放弃更改", "Discard")
        };
        let title = match &file.old_path {
            Some(old) => format!("{old} → {}", file.path),
            None => file.path.clone(),
        };
        let header = div()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(counts(&file, cx))
            .child(
                Button::new("diff-discard")
                    .xsmall()
                    .label(discard_label)
                    .when(confirming, |button| button.danger())
                    .when(!confirming, |button| button.ghost())
                    .on_click(cx.listener(|this, _, _, cx| this.discard_selected(cx))),
            );
        let body = if file.binary {
            centered(language.pick("二进制文件，无法显示差异", "Binary file: no text diff"), cx)
        } else {
            let lines: Rc<Vec<DiffLine>> = Rc::new(file.lines.clone());
            let mono = theme.mono_font_family.clone();
            let size = theme.mono_font_size;
            let colors = LineColors::new(cx);
            uniform_list("diff-lines", lines.len(), move |range, _, _| {
                range.map(|index| line_row(&lines[index], &mono, size, &colors)).collect()
            })
            .track_scroll(&self.scroll)
            .flex_1()
            .into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(header)
            .child(body)
            .when(file.truncated, |view| {
                view.child(div().px_3().py_1().text_xs().text_color(theme.warning).child(
                    language.pick("差异过长，已截断", "Diff too long: truncated").to_owned(),
                ))
            })
            .into_any_element()
    }
}

impl Render for DiffView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let notice = self.notice.clone();
        let muted = cx.theme().muted_foreground;
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_sidebar(cx))
                    .child(self.render_body(cx)),
            )
            .when_some(notice, |view, notice| {
                view.child(div().px_3().py_1().text_xs().text_color(muted).child(notice))
            })
    }
}

#[derive(Clone)]
struct LineColors {
    added: gpui::Hsla,
    removed: gpui::Hsla,
    hunk: gpui::Hsla,
    muted: gpui::Hsla,
    foreground: gpui::Hsla,
}

impl LineColors {
    fn new(cx: &Context<DiffView>) -> Self {
        let theme = cx.theme();
        Self {
            added: theme.success.opacity(0.16),
            removed: theme.danger.opacity(0.16),
            hunk: theme.accent.opacity(0.5),
            muted: theme.muted_foreground,
            foreground: theme.foreground,
        }
    }
}

fn line_row(
    line: &DiffLine,
    mono: &SharedString,
    size: gpui::Pixels,
    colors: &LineColors,
) -> gpui::AnyElement {
    let number = |value: Option<u32>| {
        div()
            .w(px(44.0))
            .flex_shrink_0()
            .pr_2()
            .text_right()
            .text_color(colors.muted)
            .child(value.map(|n| n.to_string()).unwrap_or_default())
    };
    let (background, sign) = match line.kind {
        LineKind::Added => (Some(colors.added), "+"),
        LineKind::Removed => (Some(colors.removed), "-"),
        LineKind::Hunk => (Some(colors.hunk), ""),
        LineKind::Context => (None, " "),
    };
    let text = line.text.replace('\t', "    ");
    div()
        .flex()
        .w_full()
        .font_family(mono.clone())
        .text_size(size)
        .whitespace_nowrap()
        .when_some(background, |row, color| row.bg(color))
        .child(number(line.old_no))
        .child(number(line.new_no))
        .child(div().w(px(16.0)).flex_shrink_0().text_color(colors.muted).child(sign))
        .child(
            div()
                .flex_1()
                .overflow_hidden()
                .text_color(if line.kind == LineKind::Hunk {
                    colors.muted
                } else {
                    colors.foreground
                })
                .child(text),
        )
        .into_any_element()
}

fn status_badge(status: FileStatus, cx: &Context<DiffView>) -> (&'static str, gpui::Hsla) {
    let theme = cx.theme();
    match status {
        FileStatus::Added => ("A", theme.success),
        FileStatus::Deleted => ("D", theme.danger),
        FileStatus::Modified => ("M", theme.warning),
        FileStatus::Renamed => ("R", theme.info),
    }
}

fn counts(file: &FileDiff, cx: &Context<DiffView>) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .flex()
        .gap_1()
        .flex_shrink_0()
        .text_xs()
        .when(file.added > 0, |row| {
            row.child(div().text_color(theme.success).child(format!("+{}", file.added)))
        })
        .when(file.removed > 0, |row| {
            row.child(div().text_color(theme.danger).child(format!("−{}", file.removed)))
        })
}

fn centered(text: &str, cx: &Context<DiffView>) -> gpui::AnyElement {
    div()
        .flex()
        .flex_1()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text.to_owned())
        .into_any_element()
}

/// `src/app/main.rs` → ("src/app", "main.rs").
fn split_path(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(split) => (&path[..split], &path[split + 1..]),
        None => ("", path),
    }
}

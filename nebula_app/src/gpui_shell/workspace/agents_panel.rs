//! Centro de control de IA: una tarjeta por agente abierto (Claude Code,
//! Codex…) con su estado en vivo, carpeta y rama, uso de contexto y coste
//! estimado. Un clic en la tarjeta lleva a su panel.
//!
//! El estado sale de los hooks que ya recibe cada panel; el uso de tokens se
//! lee del archivo de sesión de la CLI (`ai_usage`) en segundo plano.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{Animation, AnimationExt as _, StyledImage as _, img, pulsating_between};

use super::*;
use crate::ai_usage::{SessionImage, UsageSnapshot, UsageTracker};
use crate::display::side_panel::PanelView;
use crate::gpui_shell::terminal::view::TerminalView;
use crate::runtime_api::RuntimeTaskState;

/// Cada cuánto se releen transcripts y ramas mientras el panel está abierto.
const REFRESH: Duration = Duration::from_secs(2);
/// Miniaturas por tarjeta.
const THUMBS: usize = 6;

#[derive(Default)]
pub(crate) struct AgentsPanel {
    trackers: HashMap<u64, Arc<Mutex<UsageTracker>>>,
    usage: HashMap<u64, UsageSnapshot>,
    /// Últimas imágenes de cada sesión (enviadas y leídas por la IA).
    images: HashMap<u64, Vec<SessionImage>>,
    /// Miniaturas ya convertidas, por puntero de los bytes.
    thumbs: HashMap<usize, Arc<gpui::Image>>,
    /// Estado y desde cuándo, para «esperando hace 2 min».
    since: HashMap<u64, (RuntimeTaskState, Instant)>,
    /// Rama por carpeta (None = no es un repo).
    branches: HashMap<String, Option<String>>,
    polling: bool,
}

struct AgentCard {
    pane_id: u64,
    name: String,
    state: RuntimeTaskState,
    cwd: String,
    view: Entity<TerminalView>,
}

impl NebulaWorkspace {
    fn agent_cards(&self, cx: &App) -> Vec<AgentCard> {
        let mut cards = Vec::new();
        for tab in &self.tabs {
            let WorkspaceTab::Terminal { panes, .. } = tab else { continue };
            for pane in panes {
                let view = pane.view.read(cx);
                let Some(agent) = view.runtime_agent() else { continue };
                cards.push(AgentCard {
                    pane_id: pane.id,
                    name: agent.display_name,
                    state: view.runtime_task_state(),
                    cwd: view.cwd.clone(),
                    view: pane.view.clone(),
                });
            }
        }
        cards
    }

    pub(super) fn toggle_agents_panel(&mut self, cx: &mut Context<Self>) {
        self.details_panel.section = None;
        if self.side_panel.open && self.side_panel.view != PanelView::Agents {
            self.select_side_panel_view(PanelView::Agents, cx);
        } else {
            self.toggle_side_panel(PanelView::Agents, cx);
        }
        self.start_agents_polling(cx);
    }

    /// Bucle de refresco mientras la pestaña «Agentes» esté a la vista.
    pub(super) fn start_agents_polling(&mut self, cx: &mut Context<Self>) {
        if self.agents_panel.polling || !self.agents_panel_visible() {
            return;
        }
        self.agents_panel.polling = true;
        self.refresh_agents(cx);
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                executor.timer(REFRESH).await;
                let keep = this
                    .update(cx, |workspace, cx| {
                        if !workspace.agents_panel_visible() {
                            workspace.agents_panel.polling = false;
                            return false;
                        }
                        workspace.refresh_agents(cx);
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        })
        .detach();
    }

    fn agents_panel_visible(&self) -> bool {
        self.side_panel.open && self.side_panel.view == PanelView::Agents
    }

    /// Relee (en segundo plano) los transcripts y las ramas de cada agente.
    fn refresh_agents(&mut self, cx: &mut Context<Self>) {
        let cards = self.agent_cards(cx);
        let alive: std::collections::HashSet<u64> = cards.iter().map(|card| card.pane_id).collect();
        self.agents_panel.trackers.retain(|pane, _| alive.contains(pane));
        self.agents_panel.usage.retain(|pane, _| alive.contains(pane));
        self.agents_panel.images.retain(|pane, _| alive.contains(pane));
        let live: std::collections::HashSet<usize> = self
            .agents_panel
            .images
            .values()
            .flatten()
            .map(|image| Arc::as_ptr(&image.bytes) as usize)
            .collect();
        self.agents_panel.thumbs.retain(|key, _| live.contains(key));
        self.agents_panel.since.retain(|pane, _| alive.contains(pane));
        for card in &cards {
            // El lector es del propio panel: lo comparte con la vista previa de
            // imágenes del terminal, así el transcript se lee una sola vez.
            match card.view.update(cx, |view, _| view.session_tracker()) {
                Some(tracker) => {
                    self.agents_panel.trackers.insert(card.pane_id, tracker);
                },
                None => {
                    self.agents_panel.trackers.remove(&card.pane_id);
                },
            }
            if let Some(tracker) = self.agents_panel.trackers.get(&card.pane_id).cloned() {
                let pane_id = card.pane_id;
                let task = cx.background_executor().spawn(async move {
                    // try_lock: si el lector está ocupado (primera lectura de un
                    // transcript grande) se salta esta pasada sin esperar.
                    let mut tracker = tracker.try_lock().ok()?;
                    tracker.poll().ok()?;
                    let images = tracker.images();
                    let recent = images[images.len().saturating_sub(THUMBS)..].to_vec();
                    Some((tracker.snapshot().clone(), recent))
                });
                cx.spawn(async move |this, cx| {
                    if let Some((snapshot, images)) = task.await {
                        let _ = this.update(cx, |workspace, cx| {
                            let panel = &mut workspace.agents_panel;
                            let same_images = panel.images.get(&pane_id).is_some_and(|old| {
                                old.len() == images.len()
                                    && old
                                        .iter()
                                        .zip(&images)
                                        .all(|(a, b)| Arc::ptr_eq(&a.bytes, &b.bytes))
                            });
                            if panel.usage.get(&pane_id) != Some(&snapshot) || !same_images {
                                panel.usage.insert(pane_id, snapshot);
                                panel.images.insert(pane_id, images);
                                cx.notify();
                            }
                        });
                    }
                })
                .detach();
            }
            let cwd = card.cwd.clone();
            if !cwd.is_empty() && std::path::Path::new(&cwd).is_dir() {
                let task = cx.background_executor().spawn({
                    let cwd = cwd.clone();
                    async move { git_branch(&cwd) }
                });
                cx.spawn(async move |this, cx| {
                    let branch = task.await;
                    let _ = this.update(cx, |workspace, cx| {
                        if workspace.agents_panel.branches.get(&cwd) != Some(&branch) {
                            workspace.agents_panel.branches.insert(cwd, branch);
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
        }
        cx.notify();
    }

    pub(super) fn render_agents_panel(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        // El panel puede abrirse por muchos caminos (atajo, pestaña, reabrir el
        // lateral…): el propio render garantiza que el refresco está en marcha.
        self.start_agents_polling(cx);
        let language = crate::gpui_shell::config::ui_language(cx);
        let cards = self.agent_cards(cx);
        let now = Instant::now();
        for card in &cards {
            let entry = self.agents_panel.since.entry(card.pane_id).or_insert((card.state, now));
            if entry.0 != card.state {
                *entry = (card.state, now);
            }
        }
        let theme = cx.theme().clone();
        let waiting = cards.iter().filter(|card| card.state == RuntimeTaskState::Attention).count();
        let mut summary = language
            .pick("{count} 个智能体", "{count} agents")
            .replace("{count}", &cards.len().to_string());
        if waiting > 0 {
            summary.push_str(" · ");
            summary.push_str(
                &language
                    .pick("{count} 个等待你", "{count} waiting for you")
                    .replace("{count}", &waiting.to_string()),
            );
        }
        let mut list = v_flex().id("agents-panel").size_full().overflow_y_scroll().gap_2().p_2();
        list = list.child(div().px_1().text_xs().text_color(theme.muted_foreground).child(summary));
        if cards.is_empty() {
            list = list.child(div().p_3().text_sm().text_color(theme.muted_foreground).child(
                language.pick(
                    "没有打开的 AI 智能体。在终端里启动 Claude Code 或 Codex 即可在这里看到它们。",
                    "No AI agents open. Start Claude Code or Codex in a terminal to see them here.",
                ),
            ));
        }
        for card in cards {
            let since = self.agents_panel.since.get(&card.pane_id).map(|(_, at)| now - *at);
            let usage = self.agents_panel.usage.get(&card.pane_id).cloned();
            let branch = self.agents_panel.branches.get(&card.cwd).cloned().flatten();
            let has_changes = self.has_turn_baseline(card.pane_id);
            let images = self.agents_panel.images.get(&card.pane_id).cloned().unwrap_or_default();
            let thumbs: Vec<(Arc<gpui::Image>, SessionImage)> = images
                .into_iter()
                .map(|image| {
                    let key = Arc::as_ptr(&image.bytes) as usize;
                    let thumb = self
                        .agents_panel
                        .thumbs
                        .entry(key)
                        .or_insert_with(|| {
                            Arc::new(gpui::Image::from_bytes(
                                image_format(&image.media_type),
                                image.bytes.to_vec(),
                            ))
                        })
                        .clone();
                    (thumb, image)
                })
                .collect();
            list = list.child(self.render_agent_card(
                card,
                since,
                usage,
                branch,
                has_changes,
                thumbs,
                cx,
            ));
        }
        list.into_any_element()
    }

    fn render_agent_card(
        &self,
        card: AgentCard,
        since: Option<Duration>,
        usage: Option<UsageSnapshot>,
        branch: Option<String>,
        has_changes: bool,
        thumbs: Vec<(Arc<gpui::Image>, SessionImage)>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let language = crate::gpui_shell::config::ui_language(cx);
        let spanish = language == crate::i18n::UiLanguage::EsEs;
        let theme = cx.theme().clone();
        let (label, color) = match card.state {
            RuntimeTaskState::Running => (language.pick("工作中…", "Working…"), theme.info),
            RuntimeTaskState::Attention => {
                (language.pick("等待你", "Waiting for you"), theme.warning)
            },
            RuntimeTaskState::WaitingInput => {
                (language.pick("等待输入", "Waiting for input"), theme.warning)
            },
            RuntimeTaskState::Finished => (language.pick("已完成", "Done"), theme.success),
            RuntimeTaskState::Failed => (language.pick("出错", "Error"), theme.danger),
            RuntimeTaskState::Idle => (language.pick("空闲", "Idle"), theme.muted_foreground),
        };
        let pane_id = card.pane_id;
        let view = card.view.clone();
        let folder = card
            .cwd
            .trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default()
            .to_owned();
        let mut place = if folder.is_empty() { String::new() } else { format!("📁 {folder}") };
        if let Some(branch) = &branch {
            if !place.is_empty() {
                place.push_str("  ·  ");
            }
            place.push_str(&format!("🌿 {branch}"));
        }
        let elapsed = since.map(|since| {
            let minutes = since.as_secs() / 60;
            let text = match minutes {
                0 => language.pick("刚刚", "just now").to_owned(),
                1..=59 => language.pick("{n} 分钟", "{n} min").replace("{n}", &minutes.to_string()),
                _ => language
                    .pick("{h} 小时 {m} 分钟", "{h} h {m} min")
                    .replace("{h}", &(minutes / 60).to_string())
                    .replace("{m}", &(minutes % 60).to_string()),
            };
            text
        });
        let attention = card.state == RuntimeTaskState::Attention;
        let mut body = v_flex().gap_1p5().child(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(card.name.clone()),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .flex_shrink_0()
                        .px_2()
                        .py_0p5()
                        .rounded_full()
                        .bg(color.opacity(0.14))
                        .text_xs()
                        .text_color(color)
                        .child(div().size(px(6.0)).rounded_full().bg(color))
                        .child(label.to_owned()),
                ),
        );
        if !place.is_empty() {
            body = body
                .child(div().truncate().text_xs().text_color(theme.muted_foreground).child(place));
        }
        if let Some(usage) = &usage {
            body = body.child(usage_row(usage, spanish, &theme, language));
        }
        if !thumbs.is_empty() {
            let mut strip = h_flex().gap_1().pt_0p5();
            for (index, (thumb, image)) in thumbs.into_iter().enumerate() {
                let tooltip: SharedString = image
                    .label
                    .clone()
                    .unwrap_or_else(|| match image.origin {
                        crate::ai_usage::ImageOrigin::Sent => {
                            language.pick("你发送的图片", "Image you sent").to_owned()
                        },
                        crate::ai_usage::ImageOrigin::Read => {
                            language.pick("AI 读取的图片", "Image read by the AI").to_owned()
                        },
                    })
                    .into();
                strip = strip.child(
                    div()
                        .id(("agent-thumb", pane_id * 100 + index as u64))
                        .size(px(44.0))
                        .flex_shrink_0()
                        .rounded_md()
                        .overflow_hidden()
                        .border_1()
                        .border_color(theme.border)
                        .cursor_pointer()
                        .tooltip(move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
                        })
                        .on_click({
                            let view = view.clone();
                            cx.listener(move |_, _, _, cx| {
                                cx.stop_propagation();
                                view.update(cx, |view, cx| view.show_session_image(&image, cx));
                                // Lleva a ese panel para ver la imagen sobre él.
                                cx.defer(move |cx| {
                                    windowing::focus_notification(Some(pane_id), cx)
                                });
                            })
                        })
                        .child(img(thumb).size_full().object_fit(gpui::ObjectFit::Cover)),
                );
            }
            body = body.child(strip);
        }
        let footer_text = elapsed.map(|elapsed| match card.state {
            RuntimeTaskState::Running => {
                language.pick("已工作 {t}", "working for {t}").replace("{t}", &elapsed)
            },
            RuntimeTaskState::Attention | RuntimeTaskState::WaitingInput => {
                language.pick("已等待 {t}", "waiting for {t}").replace("{t}", &elapsed)
            },
            _ => language.pick("{t} 前", "{t} ago").replace("{t}", &elapsed),
        });
        body = body.child(
            h_flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(footer_text.unwrap_or_default()),
                )
                .when(has_changes, |row| {
                    row.child(
                        Button::new(("agent-view-changes", pane_id))
                            .xsmall()
                            .ghost()
                            .label(language.pick("查看更改", "View changes"))
                            .on_click(cx.listener(move |workspace, _, window, cx| {
                                cx.stop_propagation();
                                workspace.open_changes_tab(Some(pane_id), None, window, cx);
                            })),
                    )
                }),
        );
        let card_el = div()
            .id(("agent-card", pane_id))
            .p_3()
            .rounded_lg()
            .border_1()
            .border_color(if attention { theme.warning } else { theme.border })
            .bg(theme.secondary.opacity(0.5))
            .cursor_pointer()
            .hover(|style| style.bg(theme.list_hover))
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.defer(move |cx| windowing::focus_notification(Some(pane_id), cx));
            }))
            .child(body);
        if attention {
            // Pulso suave del borde: se ve de reojo sin molestar.
            card_el
                .with_animation(
                    ("agent-card-pulse", pane_id),
                    Animation::new(Duration::from_millis(1600))
                        .repeat()
                        .with_easing(pulsating_between(0.35, 1.0)),
                    move |card, t| card.border_color(theme.warning.opacity(t)),
                )
                .into_any_element()
        } else {
            card_el.into_any_element()
        }
    }
}

/// Barra de contexto + tokens + coste estimado.
fn usage_row(
    usage: &UsageSnapshot,
    spanish: bool,
    theme: &gpui_component::Theme,
    language: crate::i18n::UiLanguage,
) -> gpui::AnyElement {
    let percent = usage.context_percent();
    let bar_color = match percent {
        Some(p) if p >= 85 => theme.danger,
        Some(p) if p >= 60 => theme.warning,
        _ => theme.success,
    };
    let mut parts = Vec::new();
    if let Some(percent) = percent {
        parts.push(
            language.pick("{p}% 上下文", "{p}% context").replace("{p}", &percent.to_string()),
        );
    }
    parts.push(format!("{} tokens", compact_tokens(usage.context_tokens, spanish)));
    if let Some(cost) = usage.cost_usd {
        parts.push(format_cost(cost, spanish));
    }
    v_flex()
        .gap_1()
        .when_some(percent, |column, percent| {
            column.child(
                div().h(px(5.0)).w_full().rounded_full().bg(theme.muted).child(
                    div()
                        .h_full()
                        .rounded_full()
                        .bg(bar_color)
                        .w(gpui::relative(f32::from(percent) / 100.0)),
                ),
            )
        })
        .child(div().text_xs().text_color(theme.muted_foreground).child(parts.join(" · ")))
        .into_any_element()
}

/// 145000 → "145k"; 1250000 → "1,3M" (coma decimal en castellano).
fn compact_tokens(tokens: u64, spanish: bool) -> String {
    let text = if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{}k", (tokens as f64 / 1_000.0).round() as u64)
    } else {
        tokens.to_string()
    };
    if spanish { text.replace('.', ",") } else { text }
}

/// Coste estimado a precio de API: "≈ 1,20 $" en castellano, "≈ $1.20" en inglés.
fn format_cost(cost: f64, spanish: bool) -> String {
    if spanish {
        format!("≈ {} $", format!("{cost:.2}").replace('.', ","))
    } else {
        format!("≈ ${cost:.2}")
    }
}

fn image_format(media_type: &str) -> gpui::ImageFormat {
    match media_type {
        "image/jpeg" | "image/jpg" => gpui::ImageFormat::Jpeg,
        "image/gif" => gpui::ImageFormat::Gif,
        "image/webp" => gpui::ImageFormat::Webp,
        _ => gpui::ImageFormat::Png,
    }
}

/// Rama actual de la carpeta, o None si no es un repositorio.
fn git_branch(cwd: &str) -> Option<String> {
    let mut command = std::process::Command::new("git");
    let output = crate::platform::process::hidden_command(&mut command)
        .args(["--no-optional-locks", "-C", cwd, "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|branch| !branch.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_use_the_spanish_decimal_comma() {
        assert_eq!(compact_tokens(145_400, true), "145k");
        assert_eq!(compact_tokens(1_250_000, true), "1,2M");
        assert_eq!(compact_tokens(1_250_000, false), "1.2M");
        assert_eq!(compact_tokens(512, true), "512");
        assert_eq!(format_cost(1.2, true), "≈ 1,20 $");
        assert_eq!(format_cost(1.2, false), "≈ $1.20");
    }
}

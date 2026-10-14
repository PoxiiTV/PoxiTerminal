//! Tarjeta «Pool» del panel Agentes: el reparto de la cuota de Claude de un vistazo.
//!
//! Por cada ventana (5 h y semana): lo que te queda en grande, una barra partida con la parte de
//! cada persona (como la anilla del panel web), lo que le queda al otro, los préstamos y cuándo se
//! reinicia. Solo aparece si el módulo está activado (`poxiterminal pool setup`). Los números los
//! calcula el servidor; aquí únicamente se presentan.

use super::*;
use crate::i18n::UiLanguage;
use crate::pool::{
    self, KindState, Phase, ServerState, UserState, countdown_parts, display_name, fmt_points,
};

/// Colores fijos de cada persona, los mismos que en el panel web de Fifty (terracota y azul).
fn person_color(index: usize) -> gpui::Hsla {
    if index == 0 { gpui::rgb(0xd97757).into() } else { gpui::rgb(0x6fb3d2).into() }
}

fn level_color(theme: &gpui_component::Theme, level: &str) -> gpui::Hsla {
    match level {
        "aviso" | "importante" => theme.warning,
        "critico" | "agotado" => theme.danger,
        _ => theme.success,
    }
}

fn level_label(language: UiLanguage, level: &str) -> &'static str {
    match level {
        "aviso" => language.pick("提醒", "Pool: warning"),
        "importante" => language.pick("重要", "Pool: important"),
        "critico" => language.pick("危急", "Pool: critical"),
        "agotado" => language.pick("已用完", "Pool: exhausted"),
        _ => language.pick("正常", "Pool: normal"),
    }
}

fn countdown(language: UiLanguage, seconds: i64) -> String {
    let (d, h, m) = countdown_parts(seconds);
    if d > 0 {
        language
            .pick("{d} 天 {h} 小时", "Pool: {d} d {h} h")
            .replace("{d}", &d.to_string())
            .replace("{h}", &h.to_string())
    } else if h > 0 {
        language
            .pick("{h} 小时 {m} 分钟", "{h} h {m} min")
            .replace("{h}", &h.to_string())
            .replace("{m}", &m.to_string())
    } else {
        language.pick("{n} 分钟", "{n} min").replace("{n}", &m.max(1).to_string())
    }
}

fn pill(text: String, color: gpui::Hsla) -> gpui::AnyElement {
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
        .child(text)
        .into_any_element()
}

fn muted(theme: &gpui_component::Theme, text: String) -> gpui::AnyElement {
    div().text_xs().text_color(theme.muted_foreground).child(text).into_any_element()
}

pub(super) fn render_pool_card(
    language: UiLanguage,
    theme: &gpui_component::Theme,
) -> Option<gpui::AnyElement> {
    let status = pool::status();
    let (label, color) = match status.phase {
        Phase::Off => return None,
        Phase::Online => (language.pick("已连接", "Pool: connected"), theme.success),
        Phase::Connecting => (language.pick("连接中…", "Connecting…"), theme.info),
        Phase::Offline => (language.pick("离线", "Pool: offline"), theme.warning),
        Phase::NoToken => (language.pick("未配置", "Pool: not configured"), theme.warning),
        Phase::Unauthorized => (language.pick("令牌被拒绝", "Pool: token rejected"), theme.danger),
        Phase::HelperMissing => {
            (language.pick("缺少钩子程序", "Pool: hook helper missing"), theme.warning)
        },
    };
    let now_s = chrono::Utc::now().timestamp();

    let mut header = h_flex().gap_2().items_center().child(
        v_flex()
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(language.pick("Pool", "Pool")),
            )
            .children(status.last_sync_ms.map(|ms| {
                let ago = (now_s - ms / 1000).max(0);
                let text = if ago < 45 {
                    language.pick("刚刚", "just now").to_owned()
                } else {
                    language
                        .pick("{t} 前更新", "Pool: updated {t} ago")
                        .replace("{t}", &countdown(language, ago))
                };
                muted(theme, text)
            })),
    );
    header = header.child(pill(label.to_owned(), color));
    let mut body = v_flex().gap_3().child(header);

    match &status.state {
        Some(state) => {
            if !state.mixed.is_empty() {
                body = body.child(
                    div()
                        .px_2()
                        .py_1p5()
                        .rounded_md()
                        .bg(theme.danger.opacity(0.12))
                        .text_xs()
                        .text_color(theme.danger)
                        .child(
                            language
                                .pick("检测到两个不同的 Claude 账户", "Pool: mixed accounts")
                                .to_owned(),
                        ),
                );
            }
            body = body
                .child(window_section(
                    language,
                    theme,
                    state,
                    &state.five,
                    language.pick("5 小时", "Pool: 5 hours"),
                    now_s,
                ))
                .child(div().h(px(1.0)).w_full().bg(theme.border))
                .child(window_section(
                    language,
                    theme,
                    state,
                    &state.seven,
                    language.pick("本周", "Pool: week"),
                    now_s,
                ));
            if !state.desync.is_empty() {
                body = body.child(muted(
                    theme,
                    language.pick("有设备时钟不同步", "Pool: clock out of sync").to_owned(),
                ));
            }
        },
        None => {
            if let Some(detail) = &status.detail {
                body = body.child(muted(theme, detail.clone()));
            }
        },
    }
    if status.unsent_events > 0 {
        body = body.child(muted(
            theme,
            language
                .pick("{n} 个事件待同步", "Pool: {n} events waiting to sync")
                .replace("{n}", &status.unsent_events.to_string()),
        ));
    }

    Some(
        div()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .bg(theme.secondary.opacity(0.5))
            .child(body)
            .into_any_element(),
    )
}

/// Barra partida: un tramo por persona (proporcional a su parte) relleno con lo que lleva gastado.
/// En modo compartido es una sola bolsa que se llena por turnos.
fn split_bar(theme: &gpui_component::Theme, kind: &KindState) -> gpui::AnyElement {
    let track = h_flex().w_full().h(px(10.0)).justify_between();
    if kind.mode == "shared" {
        let mut fills = h_flex().h_full().w_full().rounded_full().bg(theme.muted).overflow_hidden();
        for (i, u) in kind.users.iter().enumerate() {
            let w = (u.used / 100.0).clamp(0.0, 1.0) as f32;
            fills = fills.child(div().h_full().w(gpui::relative(w)).bg(person_color(i)));
        }
        return track.child(fills).into_any_element();
    }
    let total: f64 = kind.users.iter().map(|u| u.limit.max(0.0)).sum::<f64>().max(1.0);
    let gap = if kind.users.len() > 1 { 0.012 } else { 0.0 };
    let mut bar = track;
    for (i, u) in kind.users.iter().enumerate() {
        let span = ((u.limit.max(0.0) / total) as f32 - gap).max(0.0);
        let fill = if u.limit > 0.0 { (u.used / u.limit).clamp(0.0, 1.0) as f32 } else { 0.0 };
        bar = bar.child(
            div()
                .h_full()
                .w(gpui::relative(span))
                .rounded_full()
                .bg(theme.muted)
                .overflow_hidden()
                .child(div().h_full().w(gpui::relative(fill)).rounded_full().bg(person_color(i))),
        );
    }
    bar.into_any_element()
}

/// Línea de una persona: color, nombre, lo que le queda y los préstamos.
fn person_line(
    language: UiLanguage,
    theme: &gpui_component::Theme,
    index: usize,
    user: &UserState,
    is_me: bool,
) -> gpui::AnyElement {
    let left = if is_me {
        language.pick("剩 {n} / {t}", "Pool: you have {n} of {t} left")
    } else {
        language.pick("剩 {n} / {t}", "Pool: has {n} of {t} left")
    }
    .replace("{n}", &fmt_points(user.remaining))
    .replace("{t}", &fmt_points(user.limit));
    let mut extra = Vec::new();
    if user.loans_out > 0.0 {
        extra.push(
            language.pick("借出 {n}", "Pool: lent {n}").replace("{n}", &fmt_points(user.loans_out)),
        );
    }
    if user.loans_in > 0.0 {
        extra.push(
            language
                .pick("借入 {n}", "Pool: borrowed {n}")
                .replace("{n}", &fmt_points(user.loans_in)),
        );
    }
    let name = if is_me {
        format!("{} ({})", display_name(&user.name), language.pick("你", "Pool: you"))
    } else {
        display_name(&user.name)
    };
    h_flex()
        .gap_2()
        .items_center()
        .text_xs()
        .child(div().size(px(8.0)).rounded_full().bg(person_color(index)).flex_shrink_0())
        .child(div().font_weight(gpui::FontWeight::MEDIUM).child(name))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(theme.muted_foreground)
                .child(extra.join(" · ")),
        )
        .child(div().text_color(theme.muted_foreground).child(left))
        .into_any_element()
}

/// Una ventana completa: título y cuenta atrás, lo que te queda en grande, la barra y las personas.
fn window_section(
    language: UiLanguage,
    theme: &gpui_component::Theme,
    state: &ServerState,
    kind: &KindState,
    title: &str,
    now_s: i64,
) -> gpui::AnyElement {
    let mut head = h_flex().items_center().justify_between().child(
        div()
            .text_xs()
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(theme.muted_foreground)
            .child(title.to_uppercase()),
    );
    if let Some(window) = &kind.window {
        head = head.child(muted(
            theme,
            language
                .pick("{t} 后重置", "Pool: resets in {t}")
                .replace("{t}", &countdown(language, window.resets_at - now_s)),
        ));
    }

    let me_index = kind.users.iter().position(|u| u.name == state.user);
    let (Some(index), Some(_)) = (me_index, kind.account_pct) else {
        return v_flex()
            .gap_1()
            .child(head)
            .child(muted(theme, language.pick("无数据", "Pool: no data").to_owned()))
            .into_any_element();
    };
    let me = &kind.users[index];

    let hero = h_flex()
        .gap_3()
        .items_end()
        .child(
            div()
                .text_3xl()
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(person_color(index))
                .line_height(px(32.0))
                .child(fmt_points(me.remaining)),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(language.pick("剩余", "Pool: left for you")),
                )
                .child(muted(
                    theme,
                    language
                        .pick("共 {t} · 已用 {u}", "Pool: of {t} · {u} used")
                        .replace("{t}", &fmt_points(me.limit))
                        .replace("{u}", &fmt_points(me.used)),
                )),
        )
        .child(pill(level_label(language, &me.level).to_owned(), level_color(theme, &me.level)));

    let mut section = v_flex().gap_2().child(head).child(hero).child(split_bar(theme, kind));
    for (i, u) in kind.users.iter().enumerate() {
        section = section.child(person_line(language, theme, i, u, u.name == state.user));
    }
    let mut foot = vec![
        language
            .pick("账户 {n}%", "Pool: account {n}%")
            .replace("{n}", &fmt_points(kind.account_pct.unwrap_or(0.0))),
    ];
    if kind.stale {
        foot.push(language.pick("数据过旧", "Pool: stale data").to_owned());
    }
    section.child(muted(theme, foot.join(" · "))).into_any_element()
}

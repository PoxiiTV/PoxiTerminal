//! Tarjeta «Pool» del panel Agentes: conexión con el servidor y reparto de la cuota (5 h y semana).
//!
//! Solo aparece si el módulo está activado (`poxiterminal pool setup`). Los números los calcula el
//! servidor; aquí únicamente se muestran, y la antigüedad del dato se hace visible.

use super::*;
use crate::i18n::UiLanguage;
use crate::pool::{self, Phase, level_tag};

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

    let mut body = v_flex().gap_1p5().child(
        h_flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(language.pick("Pool", "Pool")),
            )
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
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

    match &status.state {
        Some(state) => {
            body = body
                .child(window_row(
                    language,
                    theme,
                    language.pick("5 小时窗口", "Pool: 5 h window"),
                    state,
                    &state.five,
                ))
                .child(window_row(
                    language,
                    theme,
                    language.pick("每周窗口", "Pool: weekly window"),
                    state,
                    &state.seven,
                ));
            if !state.desync.is_empty() {
                body = body.child(muted(theme, "⚠ reloj desajustado"));
            }
        },
        None => {
            if let Some(detail) = &status.detail {
                body = body.child(muted(theme, detail));
            }
        },
    }
    if status.unsent_events > 0 {
        body = body.child(muted(
            theme,
            &language
                .pick("{n} 个事件待同步", "Pool: {n} events waiting to sync")
                .replace("{n}", &status.unsent_events.to_string()),
        ));
    }

    Some(
        div()
            .p_3()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .bg(theme.secondary.opacity(0.5))
            .child(body)
            .into_any_element(),
    )
}

fn muted(theme: &gpui_component::Theme, text: &str) -> gpui::AnyElement {
    div().text_xs().text_color(theme.muted_foreground).child(text.to_owned()).into_any_element()
}

/// Una ventana (5 h o semanal): tu consumo / tu límite, el del otro y una barra con tu proporción.
fn window_row(
    language: UiLanguage,
    theme: &gpui_component::Theme,
    title: &str,
    state: &pool::ServerState,
    kind: &pool::KindState,
) -> gpui::AnyElement {
    let Some(me) = state.me(kind).filter(|_| kind.account_pct.is_some()) else {
        return muted(theme, &format!("{title}: {}", language.pick("无数据", "Pool: no data")));
    };
    let color = match me.level.as_str() {
        "aviso" | "importante" => theme.warning,
        "critico" | "agotado" => theme.danger,
        _ => theme.success,
    };
    let mut parts =
        vec![format!("{} {:.0}/{:.0}", language.pick("你", "Pool: you"), me.used, me.limit)];
    if let Some(other) = state.other(kind) {
        parts.push(format!("{} {:.0}/{:.0}", other.name, other.used, other.limit));
    }
    if let Some(tag) = level_tag(&me.level) {
        parts.push(tag.to_owned());
    }
    if kind.stale {
        parts.push(language.pick("数据过旧", "Pool: stale data").to_owned());
    }
    let fraction = (me.pct / 100.0).clamp(0.0, 1.0) as f32;
    v_flex()
        .gap_1()
        .child(div().text_xs().font_weight(gpui::FontWeight::MEDIUM).child(title.to_owned()))
        .child(
            div().h(px(5.0)).w_full().rounded_full().bg(theme.muted).child(
                div().h_full().rounded_full().bg(color).w(gpui::relative(fraction)),
            ),
        )
        .child(muted(theme, &parts.join(" · ")))
        .into_any_element()
}

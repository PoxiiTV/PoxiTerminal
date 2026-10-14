//! Estado devuelto por el servidor (`GET /api/v1/state`) y la línea de texto que se pinta en la
//! statusline de Claude. El servidor es la fuente de verdad: aquí solo se presenta.

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct UserState {
    pub name: String,
    pub used: f64,
    pub limit: f64,
    pub remaining: f64,
    pub pct: f64,
    pub level: String,
    pub loans_in: f64,
    pub loans_out: f64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct WindowInfo {
    /// Segundos Unix en que se reinicia la ventana.
    pub resets_at: i64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct KindState {
    /// % oficial de la cuenta observado en la statusline; `None` sin ventana activa.
    pub account_pct: Option<f64>,
    pub status: String,
    pub stale: bool,
    pub mode: String,
    pub window: Option<WindowInfo>,
    pub users: Vec<UserState>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ServerState {
    pub user: String,
    pub five: KindState,
    pub seven: KindState,
    pub desync: Vec<String>,
    /// Equipos que parecen medir cuentas de Claude distintas.
    pub mixed: Vec<String>,
}

/// Puntos con coma decimal solo si hace falta: 6, 27,9.
pub fn fmt_points(value: f64) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if rounded.fract().abs() < f64::EPSILON {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.1}").replace('.', ",")
    }
}

/// Primera letra en mayúscula para los nombres de usuario.
pub fn display_name(name: &str) -> String {
    let mut chars = name.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// Cuenta atrás en (días, horas, minutos), sin negativos.
pub fn countdown_parts(seconds: i64) -> (i64, i64, i64) {
    let s = seconds.max(0);
    (s / 86_400, (s % 86_400) / 3_600, (s % 3_600) / 60)
}

impl ServerState {
    pub fn me<'a>(&self, kind: &'a KindState) -> Option<&'a UserState> {
        kind.users.iter().find(|user| user.name == self.user)
    }

    pub fn other<'a>(&self, kind: &'a KindState) -> Option<&'a UserState> {
        kind.users.iter().find(|user| user.name != self.user)
    }
}

/// Texto corto del nivel; `None` en "normal" para no ensuciar la línea.
pub fn level_tag(level: &str) -> Option<&'static str> {
    match level {
        "aviso" => Some("AVISO"),
        "importante" => Some("IMPORTANTE"),
        "critico" => Some("CRÍTICO"),
        "agotado" => Some("AGOTADO"),
        _ => None,
    }
}

fn kind_part(state: &ServerState, label: &str, kind: &KindState) -> String {
    let Some(me) = state.me(kind).filter(|_| kind.account_pct.is_some()) else {
        return format!("{label}: sin datos");
    };
    let mut text = format!("{label}: tú {:.0}/{:.0}", me.used, me.limit);
    if let Some(other) = state.other(kind) {
        text.push_str(&format!(" · {} {:.0}/{:.0}", other.name, other.used, other.limit));
    }
    if let Some(tag) = level_tag(&me.level) {
        text.push_str(&format!(" [{tag}]"));
    }
    if kind.stale {
        text.push_str(" (dato antiguo)");
    }
    text
}

/// Línea de la statusline: `Pool · 5h: tú 18/50 · kristian 12/50 · sem: tú 9/50 · kristian 6/50`.
pub fn format_line(state: &ServerState) -> String {
    let mut line = format!(
        "Pool · {} · {}",
        kind_part(state, "5h", &state.five),
        kind_part(state, "sem", &state.seven)
    );
    if !state.desync.is_empty() {
        line.push_str(" · reloj desajustado");
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(name: &str, used: f64, limit: f64, level: &str) -> UserState {
        UserState {
            name: name.into(),
            used,
            limit,
            remaining: limit - used,
            pct: used / limit * 100.0,
            level: level.into(),
            ..Default::default()
        }
    }

    fn state() -> ServerState {
        ServerState {
            user: "alexis".into(),
            five: KindState {
                account_pct: Some(30.0),
                status: "normal".into(),
                stale: false,
                users: vec![
                    user("alexis", 18.0, 50.0, "normal"),
                    user("kristian", 12.0, 50.0, "normal"),
                ],
                ..Default::default()
            },
            seven: KindState {
                account_pct: Some(15.0),
                status: "normal".into(),
                stale: false,
                users: vec![
                    user("alexis", 9.0, 50.0, "normal"),
                    user("kristian", 6.0, 50.0, "normal"),
                ],
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn line_shows_both_windows_for_both_users() {
        assert_eq!(
            format_line(&state()),
            "Pool · 5h: tú 18/50 · kristian 12/50 · sem: tú 9/50 · kristian 6/50"
        );
    }

    #[test]
    fn warnings_stale_data_and_clock_skew_are_visible() {
        let mut s = state();
        s.five.users[0] = user("alexis", 40.0, 50.0, "importante");
        s.seven.stale = true;
        s.desync = vec!["alexis/pc".into()];
        let line = format_line(&s);
        assert!(line.contains("tú 40/50 · kristian 12/50 [IMPORTANTE]"), "{line}");
        assert!(line.contains("(dato antiguo)"), "{line}");
        assert!(line.ends_with("reloj desajustado"), "{line}");
    }

    #[test]
    fn no_active_window_says_no_data_instead_of_inventing_numbers() {
        let mut s = state();
        s.five = KindState::default();
        assert!(format_line(&s).starts_with("Pool · 5h: sin datos · sem:"));
    }

    #[test]
    fn points_names_and_countdowns_are_formatted_for_the_card() {
        assert_eq!(fmt_points(6.0), "6");
        assert_eq!(fmt_points(27.94), "27,9");
        assert_eq!(fmt_points(18.04), "18");
        assert_eq!(display_name("kristian"), "Kristian");
        assert_eq!(display_name(""), "");
        assert_eq!(countdown_parts(2 * 3_600 + 13 * 60 + 5), (0, 2, 13));
        assert_eq!(countdown_parts(3 * 86_400 + 4 * 3_600), (3, 4, 0));
        assert_eq!(countdown_parts(-10), (0, 0, 0));
    }

    #[test]
    fn the_card_gets_reset_time_loans_mode_and_mixed_accounts_from_the_server() {
        let json = r#"{"user":"alexis","mixed":["alexis/pc","kristian/pc"],"five":{"account_pct":38,"mode":"hybrid","window":{"resets_at":1792012200,"start":1791994200},"users":[{"name":"alexis","used":38,"limit":44,"remaining":6,"pct":86,"level":"importante","loans_in":0,"loans_out":6}]}}"#;
        let parsed: ServerState = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.five.window.as_ref().unwrap().resets_at, 1_792_012_200);
        assert_eq!(parsed.five.mode, "hybrid");
        assert_eq!(parsed.five.users[0].loans_out, 6.0);
        assert_eq!(parsed.mixed.len(), 2);
    }

    #[test]
    fn server_json_with_missing_or_extra_fields_still_parses() {
        let json = r#"{"user":"alexis","five":{"account_pct":4,"status":"normal","users":[{"name":"alexis","used":1,"limit":50,"remaining":49,"pct":2,"level":"normal","share":50}]},"campo_nuevo":1}"#;
        let parsed: ServerState = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.me(&parsed.five).unwrap().used, 1.0);
        assert!(parsed.seven.account_pct.is_none());
    }
}

//! Payload de la statusline de Claude → muestra de cuota.
//!
//! `poxiterminal-hook claude-statusline` guarda el JSON crudo por sesión; aquí se interpreta.
//! Solo se extraen contadores y rutas: ni prompts ni contenido.

use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Window {
    pub used_percentage: u8,
    /// Segundos Unix en que se reinicia la ventana.
    pub resets_at: i64,
}

/// Lectura del % de la cuenta en un instante. Se serializa con los nombres que espera el servidor.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sample {
    /// Milisegundos del reloj de este dispositivo.
    pub ts: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub five_hour: Option<Window>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seven_day: Option<Window>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    /// `None` si el payload no trae `rate_limits` (p. ej. antes de la primera respuesta).
    pub sample: Option<Sample>,
    pub transcript: Option<PathBuf>,
}

pub fn parse(payload: &str, ts_ms: i64) -> Option<Parsed> {
    let value: Value = serde_json::from_str(payload).ok()?;
    let session_id = value["session_id"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 100)
        .map(str::to_owned);
    let limits = &value["rate_limits"];
    let five_hour = window(&limits["five_hour"]);
    let seven_day = window(&limits["seven_day"]);
    let sample = (five_hour.is_some() || seven_day.is_some()).then(|| Sample {
        ts: ts_ms,
        five_hour,
        seven_day,
        session_id,
    });
    let transcript = value["transcript_path"].as_str().filter(|p| !p.is_empty()).map(PathBuf::from);
    Some(Parsed { sample, transcript })
}

fn window(value: &Value) -> Option<Window> {
    let pct = value["used_percentage"].as_f64()?;
    let resets_at = resets_at(&value["resets_at"])?;
    Some(Window { used_percentage: pct.round().clamp(0.0, 100.0) as u8, resets_at })
}

/// Claude documenta `resets_at` como segundos Unix, pero hay informes de cadenas ISO: se aceptan
/// ambas formas para no perder muestras si cambia entre versiones.
fn resets_at(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64().or_else(|| number.as_f64().map(|f| f as i64)),
        Value::String(text) => text.trim().parse::<i64>().ok().or_else(|| {
            chrono::DateTime::parse_from_rfc3339(text.trim()).ok().map(|date| date.timestamp())
        }),
        _ => None,
    }
    .filter(|seconds| *seconds > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAYLOAD: &str = r#"{
        "session_id":"abc-1","transcript_path":"C:/Users/x/.claude/projects/p/abc-1.jsonl",
        "model":{"id":"claude-sonnet-5-5"},"cost":{"total_cost_usd":0.1},
        "rate_limits":{"five_hour":{"used_percentage":21,"resets_at":1792012200},
                       "seven_day":{"used_percentage":18,"resets_at":1792360800}}}"#;

    #[test]
    fn real_payload_shape_becomes_a_sample_and_a_transcript() {
        let parsed = parse(PAYLOAD, 1_000).unwrap();
        let sample = parsed.sample.unwrap();
        assert_eq!(sample.ts, 1_000);
        assert_eq!(
            sample.five_hour,
            Some(Window { used_percentage: 21, resets_at: 1_792_012_200 })
        );
        assert_eq!(
            sample.seven_day,
            Some(Window { used_percentage: 18, resets_at: 1_792_360_800 })
        );
        assert_eq!(sample.session_id.as_deref(), Some("abc-1"));
        assert!(parsed.transcript.unwrap().ends_with("abc-1.jsonl"));
    }

    #[test]
    fn without_rate_limits_there_is_no_sample_but_the_transcript_is_still_tracked() {
        let parsed = parse(r#"{"session_id":"s","transcript_path":"/t.jsonl"}"#, 1).unwrap();
        assert!(parsed.sample.is_none());
        assert!(parsed.transcript.is_some());
    }

    #[test]
    fn a_single_window_is_enough_and_percentages_are_clamped() {
        let json = r#"{"rate_limits":{"seven_day":{"used_percentage":140.4,"resets_at":99}}}"#;
        let sample = parse(json, 5).unwrap().sample.unwrap();
        assert!(sample.five_hour.is_none());
        assert_eq!(sample.seven_day.unwrap().used_percentage, 100);
    }

    #[test]
    fn resets_at_accepts_epoch_numbers_numeric_strings_and_iso_dates() {
        let at = |raw: &str| {
            let json = format!(
                r#"{{"rate_limits":{{"five_hour":{{"used_percentage":1,"resets_at":{raw}}}}}}}"#
            );
            parse(&json, 0).unwrap().sample.map(|s| s.five_hour.unwrap().resets_at)
        };
        assert_eq!(at("1792012200"), Some(1_792_012_200));
        assert_eq!(at(r#""1792012200""#), Some(1_792_012_200));
        assert_eq!(at(r#""2026-10-14T21:30:00Z""#), Some(1_792_013_400));
        assert_eq!(at("null"), None);
        assert_eq!(at("-5"), None);
    }

    #[test]
    fn garbage_is_ignored() {
        assert!(parse("no es json", 0).is_none());
        assert!(parse("[]", 0).unwrap().sample.is_none());
    }
}

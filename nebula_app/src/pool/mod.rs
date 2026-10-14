//! Módulo Pool: reparto de una suscripción de Claude entre dos personas (servidor CMKristian).
//!
//! Es opt-in (`poxiterminal pool setup`). Sin activarlo no se toca ninguna configuración ni se hace
//! ninguna petición de red.
//!
//! * La statusline de Claude (`poxiterminal-hook claude-statusline`) deja el payload con el % real
//!   de cuota en `<datos>/pool/statusline/<sesión>.json` y muestra la línea de estado.
//! * [`engine`] lee esos payloads y los transcripts, y los envía al servidor.
//! * Solo viajan contadores, modelos, ids y porcentajes: nunca prompts, respuestas ni código.

pub mod cli;
mod client;
mod config;
mod engine;
mod install;
mod samples;
mod state;
mod transcript;

pub use engine::{Phase, spawn, status};
pub use state::{KindState, ServerState, level_tag};

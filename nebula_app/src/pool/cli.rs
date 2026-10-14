//! `poxiterminal pool setup | status | off`.

use std::io::BufRead;

use clap::{Args, Subcommand};

use super::client::{Client, normalize_server};
use super::{config, install};

/// Reparto de una suscripción de Claude entre dos personas (servidor CMKristian).
#[derive(Args, Debug)]
pub struct Options {
    #[clap(subcommand)]
    pub action: Action,
}

#[derive(Subcommand, Debug)]
pub enum Action {
    /// Conecta este PC con el servidor, guarda el token en el almacén seguro del sistema e
    /// instala la statusline de Claude (encadenando la que ya tengas).
    Setup {
        /// Dirección del servidor (https://…; http solo en localhost).
        #[clap(long, value_name = "URL")]
        server: String,
        /// Token del dispositivo. Si se omite se lee de la entrada estándar, así no queda en el
        /// historial de la terminal.
        #[clap(long, value_name = "TOKEN")]
        token: Option<String>,
    },
    /// Muestra el estado del módulo y lo que responde el servidor.
    Status,
    /// Hace ahora un ciclo completo (leer, enviar y pedir el estado) y cuenta qué ha pasado. Sirve
    /// para diagnosticar sin abrir la ventana; no toca tu configuración de Claude.
    Sync,
    /// Desactiva el módulo y restaura tu statusline original.
    Off {
        /// Borra también el token guardado.
        #[clap(long)]
        forget: bool,
    },
}

pub fn run(options: Options) -> i32 {
    match options.action {
        Action::Setup { server, token } => setup(&server, token),
        Action::Status => status(),
        Action::Sync => sync(),
        Action::Off { forget } => off(forget),
    }
}

fn sync() -> i32 {
    let report = super::engine::sync_once();
    println!("Fase: {:?}", report.phase);
    if let Some(detail) = &report.detail {
        println!("Detalle: {detail}");
    }
    println!("Enviado: {} muestras de cuota y {} eventos de uso.", report.sent_samples, report.sent_events);
    match report.line {
        Some(line) => {
            println!("\n{line}");
            0
        },
        None => 1,
    }
}

fn settings_path() -> Option<std::path::PathBuf> {
    install::claude_dir().map(|dir| dir.join("settings.json"))
}

fn setup(server: &str, token: Option<String>) -> i32 {
    let server = match normalize_server(server) {
        Ok(url) => url,
        Err(reason) => {
            eprintln!("{reason}");
            return 2;
        },
    };
    let token = match token {
        Some(token) => token,
        None => {
            eprint!("Token del dispositivo: ");
            let mut line = String::new();
            let _ = std::io::stdin().lock().read_line(&mut line);
            line
        },
    };
    let token = token.trim();
    if !token.starts_with("cmk_") {
        eprintln!("El token debe empezar por cmk_ (lo genera `device:add` en el servidor).");
        return 2;
    }

    match Client::new(&server, token).me() {
        Ok((user, device)) => println!("Conectado como {user} (dispositivo {device})."),
        Err(error) => {
            eprintln!("No se pudo verificar el token: {error}");
            return 1;
        },
    }
    if let Err(error) = config::store_token(token) {
        eprintln!("No se pudo guardar el token en el almacén seguro: {error}");
        return 1;
    }
    if let Err(error) = config::save(true, Some(&server)) {
        eprintln!("No se pudo guardar la configuración: {error}");
        return 1;
    }

    let Some(settings) =
        settings_path().filter(|path| path.parent().is_some_and(|dir| dir.exists()))
    else {
        println!(
            "No se encontró la carpeta de Claude Code (~/.claude): la statusline se instalará cuando exista."
        );
        return 0;
    };
    let helper = crate::ai_hook::helper_command();
    match install::apply_at(&settings, &config::pool_dir(), true, helper.as_deref()) {
        Ok(true) => {
            println!("Statusline de Claude instalada (tu statusline anterior queda encadenada).")
        },
        Ok(false) => println!("La statusline de Claude ya estaba instalada."),
        Err(error) => {
            eprintln!("No se pudo instalar la statusline: {error}");
            return 1;
        },
    }
    println!("Listo. Si ya tenías Claude abierto, reinícialo para que use la statusline nueva.");
    0
}

fn status() -> i32 {
    let cfg = config::load();
    println!("Módulo Pool: {}", if cfg.enabled { "activado" } else { "desactivado" });
    println!("Servidor:    {}", cfg.server.as_deref().unwrap_or("(sin configurar)"));
    let token = config::token();
    println!("Token:       {}", if token.is_some() { "guardado" } else { "no guardado" });
    let installed = settings_path().is_some_and(|path| install::is_installed(&path));
    println!("Statusline:  {}", if installed { "instalada" } else { "no instalada" });

    let (Some(server), Some(token)) = (cfg.server, token) else { return 0 };
    let client = Client::new(&server, &token);
    match client.state() {
        Ok(state) => {
            println!("\n{}", super::state::format_line(&state));
            0
        },
        Err(error) => {
            eprintln!("\nServidor: {error}");
            1
        },
    }
}

fn off(forget: bool) -> i32 {
    if let Err(error) = config::save(false, None) {
        eprintln!("No se pudo guardar la configuración: {error}");
        return 1;
    }
    if let Some(settings) = settings_path() {
        match install::apply_at(&settings, &config::pool_dir(), false, None) {
            Ok(true) => println!("Tu statusline original ha sido restaurada."),
            Ok(false) => {},
            Err(error) => {
                eprintln!("No se pudo restaurar la statusline: {error}");
                return 1;
            },
        }
    }
    if forget {
        let _ = config::forget_token();
        println!("Token borrado.");
    }
    println!("Módulo Pool desactivado.");
    0
}

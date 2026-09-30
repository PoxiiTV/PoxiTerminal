<p align="center">
  <img src="extra/logo/nebula.png" alt="Icono de PoxiTerminal" width="128" height="128" />
</p>

<h1 align="center">PoxiTerminal</h1>

<p align="center">
  <strong>Terminal acelerado por GPU para Windows, en castellano.</strong><br />
  Pestañas y paneles divididos · SSH y SFTP · Sesiones de Claude Code y Codex · Lector de documentos
</p>

<p align="center">
  <strong>Español</strong> · <a href="#english">English</a>
</p>

---

## Español

PoxiTerminal es un terminal para Windows hecho en Rust con GPUI. Está basado en
[pebrel](https://github.com/Kuddev/pebrel) de Kuddev y tiene toda la interfaz en castellano:
menús, ajustes, avisos, notificaciones e instalador.

### Características

- **Terminal rápido por GPU**, con pestañas, paneles divididos y temas.
- **SSH y SFTP** integrados, con perfiles, túneles y explorador de archivos remoto.
- **Sesiones de IA** (Claude Code, Codex…) con avisos cuando terminan o piden aprobación.
- **Lector de documentos** (Markdown, fórmulas matemáticas y más).
- **Copias de seguridad y sincronización** de la configuración (WebDAV, S3…).
- **Actualizaciones automáticas** desde las releases de este repositorio.

### Instalación

Descarga el instalador o el ZIP portable desde
[Releases](https://github.com/PoxiiTV/PoxiTerminal/releases).
Requiere Windows 10 (1809) o superior.

La configuración se guarda en `%APPDATA%\PoxiTerminal`, así que puede convivir con pebrel.

### Compilar desde el código

Requisitos: [Rust](https://rustup.rs/), Visual Studio Build Tools (C++) e
[Inno Setup 6](https://jrsoftware.org/isinfo.php) (solo para el instalador).

| Script | Qué hace |
|---|---|
| `start.bat` | Compila y arranca PoxiTerminal en modo desarrollo. |
| `build.bat` | Genera el ZIP portable y el instalador en `dist\`. |

### Créditos y licencia

PoxiTerminal es un trabajo derivado de [pebrel](https://github.com/Kuddev/pebrel) (© Kuddev)
y se distribuye bajo la misma licencia, [GPL-3.0](LICENSE). Las licencias de terceros están en
[`THIRD-PARTY-NOTICES`](THIRD-PARTY-NOTICES) y en [`licenses/`](licenses/).

---

## English

PoxiTerminal is a GPU-accelerated Windows terminal built with Rust and GPUI. It is based on
[pebrel](https://github.com/Kuddev/pebrel) by Kuddev, with the whole interface translated to
Spanish (English is still available in the settings).

### Features

- **GPU-accelerated terminal** with tabs, split panes and themes.
- **Built-in SSH and SFTP** with profiles, tunnels and a remote file browser.
- **AI sessions** (Claude Code, Codex…) with notifications when they finish or need approval.
- **Document reader** (Markdown, math and more).
- **Settings backup and sync** (WebDAV, S3…).
- **Automatic updates** from this repository's releases.

### Install

Download the installer or the portable ZIP from
[Releases](https://github.com/PoxiiTV/PoxiTerminal/releases). Requires Windows 10 (1809) or later.

Settings live in `%APPDATA%\PoxiTerminal`, so it can run side by side with pebrel.

### Build from source

Requirements: [Rust](https://rustup.rs/), Visual Studio Build Tools (C++) and
[Inno Setup 6](https://jrsoftware.org/isinfo.php) (installer only).

| Script | What it does |
|---|---|
| `start.bat` | Builds and runs PoxiTerminal in development mode. |
| `build.bat` | Produces the portable ZIP and the installer in `dist\`. |

### Credits and license

PoxiTerminal is a derivative work of [pebrel](https://github.com/Kuddev/pebrel) (© Kuddev) and is
distributed under the same license, [GPL-3.0](LICENSE). Third-party licenses are listed in
[`THIRD-PARTY-NOTICES`](THIRD-PARTY-NOTICES) and [`licenses/`](licenses/).

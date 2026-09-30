# Changelog

## 0.2.0

### 🇪🇸 Español

**El terminal para trabajar con IA.**

- **Centro de control de IA** (`Ctrl+Shift+A`): una tarjeta por agente abierto (Claude Code, Codex…) con su estado en vivo (trabajando, te espera, terminado, error), carpeta y rama, tiempo en ese estado, **% de contexto, tokens y coste estimado** a precio de API. Borde pulsante cuando el agente te espera y clic para ir a su panel.
- **¿Qué ha tocado la IA?**: al empezar cada turno se toma una foto del repositorio (sin tocar tu índice ni tu stage) y el botón **Ver cambios** enseña solo lo que la IA cambió en esa respuesta, archivo por archivo. Modo *Todo sin commit* y **Descartar** por archivo (los archivos nuevos van a la Papelera).
- **Imágenes de la IA**: pasa el ratón por `[Image #N]` o por la ruta de una imagen para ver la miniatura; `Ctrl+clic` la abre en grande. Las tarjetas de agentes muestran las últimas imágenes de la sesión.
- **Buscar en el historial** (`Ctrl+F`): barra flotante con contador, resaltado de coincidencias, `Intro` / `Mayús+Intro` / `F3` para saltar. En vim, nano o htop `Ctrl+F` sigue siendo de la app.
- **Túneles SSH**: reenvío de puertos local y SOCKS por host guardado; se abren al conectar y se cierran con el panel.

### 🇬🇧 English

**The terminal for working with AI.**

- **AI control center** (`Ctrl+Shift+A`): one card per open agent (Claude Code, Codex…) with live state, folder and branch, time in state, **context %, tokens and estimated API cost**. Pulsing border when the agent is waiting for you; click to jump to its pane.
- **What did the AI change?**: a repository snapshot is taken when each turn starts (without touching your index or stage) and **View changes** shows only what the AI changed in that answer, file by file. *All uncommitted* mode and per-file **Discard** (new files go to the Recycle Bin).
- **AI images**: hover `[Image #N]` or an image path to preview it; `Ctrl+click` opens it full size. Agent cards show the latest session images.
- **Scrollback search** (`Ctrl+F`): floating bar with match counter, highlighting and `Enter` / `Shift+Enter` / `F3` navigation. In vim, nano or htop `Ctrl+F` still belongs to the app.
- **SSH tunnels**: local and SOCKS port forwarding per saved host; they open on connect and close with the pane.

## 0.1.0

### 🇪🇸 Español

- Primera versión: el terminal de [pebrel](https://github.com/Kuddev/pebrel) con marca propia y **toda la interfaz en castellano** (menús, ajustes, avisos, errores, notificaciones e instalador).
- Castellano por defecto aunque Windows esté en otro idioma; búsqueda en español en la paleta y los ajustes.
- Convive con pebrel: ajustes, credenciales e integraciones de IA propias.

### 🇬🇧 English

- First release: the [pebrel](https://github.com/Kuddev/pebrel) terminal with its own branding and **the whole interface in Spanish**.
- Spanish by default regardless of the Windows language; Spanish search in the palette and settings.
- Runs side by side with pebrel: separate settings, credentials and AI hooks.

# Changelog

## 0.3.0

### 🇪🇸 Español

- **Vista previa a ventana completa**: al pasar el ratón por una imagen de la IA (miniatura, `[Image #N]` o ruta de imagen) se ve **en grande ocupando toda la ventana** y se quita sola al apartar el ratón. `Ctrl+clic` la deja fija.
- **Toda la miniatura responde**: también la parte de abajo de las miniaturas altas y los resúmenes `Read N files`, que antes no tenían vista previa.
- **Corrección**: la vista previa ya no sale como una columna estrecha y estirada con la imagen diminuta.

### 🇬🇧 English

- **Full-window preview**: hovering an AI image (thumbnail, `[Image #N]` or image path) shows it **large, filling the whole window**, and it goes away when the mouse moves off. `Ctrl+click` pins it.
- **The whole thumbnail responds**: including the lower part of tall thumbnails and `Read N files` summaries, which had no preview before.
- **Fix**: the preview no longer shows up as a narrow stretched column with a tiny image.

## 0.2.0

### 🇪🇸 Español

**El terminal para trabajar con IA.**

- **Centro de control de IA** (`Ctrl+Shift+A`): una tarjeta por agente abierto (Claude Code, Codex…) con su estado en vivo (trabajando, te espera, terminado, error), carpeta y rama, tiempo en ese estado, **% de contexto, tokens y coste estimado** a precio de API. Borde pulsante cuando el agente te espera y clic para ir a su panel.
- **¿Qué ha tocado la IA?**: al empezar cada turno se toma una foto del repositorio (sin tocar tu índice ni tu stage) y el botón **Ver cambios** enseña solo lo que la IA cambió en esa respuesta, archivo por archivo. Modo *Todo sin commit* y **Descartar** por archivo (los archivos nuevos van a la Papelera).
- **Imágenes de la IA dentro del terminal**: `[Image #N]`, `Read(foto.png)` y los resúmenes `Read N files` de Claude Code se ven como miniaturas que ocupan justo el sitio de ese texto (crecen solo por huecos vacíos, nunca tapan nada). Barra **Vistas por la IA** bajo el terminal y tira de imágenes en las tarjetas de agentes. Un clic abre la imagen **en grande sobre el panel**; clic o `Esc` para cerrar.
- **Buscar en el historial** (`Ctrl+F`): barra flotante con contador, resaltado de coincidencias, `Intro` / `Mayús+Intro` / `F3` para saltar. En vim, nano o htop `Ctrl+F` sigue siendo de la app.
- **Túneles SSH**: reenvío de puertos local y SOCKS por host guardado; se abren al conectar y se cierran con el panel.
- **Correcciones**: el visor de cambios y el panel Git funcionan en WSL (y ya no pasan las rutas por el shell), textos del editor SSH traducidos y un cierre inesperado al terminar un turno de la IA.

### 🇬🇧 English

**The terminal for working with AI.**

- **AI control center** (`Ctrl+Shift+A`): one card per open agent (Claude Code, Codex…) with live state, folder and branch, time in state, **context %, tokens and estimated API cost**. Pulsing border when the agent is waiting for you; click to jump to its pane.
- **What did the AI change?**: a repository snapshot is taken when each turn starts (without touching your index or stage) and **View changes** shows only what the AI changed in that answer, file by file. *All uncommitted* mode and per-file **Discard** (new files go to the Recycle Bin).
- **AI images inside the terminal**: `[Image #N]`, `Read(photo.png)` and Claude Code's `Read N files` summaries become thumbnails that fit exactly over that text (they only grow into empty cells, never covering anything). A **Seen by the AI** bar below the terminal and image strips on agent cards. Click to open the image **full size over the pane**; click or `Esc` to close.
- **Scrollback search** (`Ctrl+F`): floating bar with match counter, highlighting and `Enter` / `Shift+Enter` / `F3` navigation. In vim, nano or htop `Ctrl+F` still belongs to the app.
- **SSH tunnels**: local and SOCKS port forwarding per saved host; they open on connect and close with the pane.
- **Fixes**: the diff viewer and Git panel work in WSL (paths no longer go through the shell), SSH editor texts translated, and a crash when an AI turn finished.

## 0.1.0

### 🇪🇸 Español

- Primera versión: el terminal de [pebrel](https://github.com/Kuddev/pebrel) con marca propia y **toda la interfaz en castellano** (menús, ajustes, avisos, errores, notificaciones e instalador).
- Castellano por defecto aunque Windows esté en otro idioma; búsqueda en español en la paleta y los ajustes.
- Convive con pebrel: ajustes, credenciales e integraciones de IA propias.

### 🇬🇧 English

- First release: the [pebrel](https://github.com/Kuddev/pebrel) terminal with its own branding and **the whole interface in Spanish**.
- Spanish by default regardless of the Windows language; Spanish search in the palette and settings.
- Runs side by side with pebrel: separate settings, credentials and AI hooks.

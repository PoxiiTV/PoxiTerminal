# Changelog

## 0.5.0

### 🇪🇸 Español

**PoxiTerminal llega a Mac.** Primera versión para macOS con Apple Silicon (macOS 14 o posterior).

- **Descarga `PoxiTerminal-v0.5.0-macos-arm64.dmg`**, ábrelo y arrastra PoxiTerminal a Aplicaciones.
- Todo en castellano, como en Windows: pestañas, paneles, SSH, SFTP, panel de Git y centro de control de IA.
- **Icono propio para Mac**, con el margen y las esquinas del resto de apps de macOS.
- **Actualizaciones automáticas también en Mac:** busca el `.dmg` nuevo en las releases y se instala sola cuando tú le dices.
- Convive con pebrel también en Mac: su propia carpeta de ajustes (`~/Library/Application Support/PoxiTerminal`) y su propio identificador.
- Para compilar en Mac basta con Rust y las Command Line Tools: `./build-mac.sh`.

> Aún no está firmada por Apple: la primera vez, ábrela, cierra el aviso y ve a **Ajustes del Sistema → Privacidad y seguridad → Abrir igualmente**.

### 🇬🇧 English

**PoxiTerminal comes to the Mac.** First release for Apple Silicon Macs (macOS 14 or later).

- **Download `PoxiTerminal-v0.5.0-macos-arm64.dmg`**, open it and drag PoxiTerminal to Applications.
- Fully in Spanish, as on Windows: tabs, panes, SSH, SFTP, Git panel and the AI control center.
- **Mac-specific icon**, with the margin and corners of other macOS apps.
- **Automatic updates on Mac too:** it looks for the new `.dmg` in releases and installs it when you say so.
- Runs side by side with pebrel on Mac too: its own settings folder and bundle identifier.
- Building on a Mac only needs Rust and the Command Line Tools: `./build-mac.sh`.

> Not signed by Apple yet: the first time, open it, close the warning and go to **System Settings → Privacy & Security → Open Anyway**.

## 0.4.1

### 🇪🇸 Español

**Tarjeta Pool renovada** en el centro de control de IA (`Ctrl+Shift+A`), para quien usa el módulo Pool con su servidor Fifty.

- **Lo que te queda, en grande**, por cada ventana (5 h y semana), con lo que llevas gastado y tu nivel (Normal, Aviso, Importante, Crítico, Agotado).
- **Barra partida**: un tramo por persona, proporcional a su parte, relleno con lo que ha gastado. En modo compartido, una sola bolsa.
- **Lo que le queda a la otra persona**, los préstamos que das o recibes y **cuánto falta para que se reinicie** cada ventana.
- **Avisos**: si dos equipos miden cuentas de Claude distintas, si un reloj está desajustado o si hay datos pendientes de enviar.
- El servidor del módulo pasa a llamarse **Fifty**.

### 🇬🇧 English

**Revamped Pool card** in the AI control center (`Ctrl+Shift+A`), for people using the Pool module with their Fifty server.

- **What you have left, big**, for each window (5 h and week), with what you have used and your level (Normal, Warning, Important, Critical, Exhausted).
- **Split bar**: one segment per person, sized by their share and filled with what they used. In shared mode, a single pool.
- **What the other person has left**, the loans you give or receive and **how long until each window resets**.
- **Warnings**: when two machines measure different Claude accounts, when a clock is off or when data is waiting to be sent.
- The module's server is now called **Fifty**.

## 0.4.0

### 🇪🇸 Español

**Repartir una suscripción de Claude entre dos personas (avanzado).** Módulo opcional **Pool**, pensado para quien comparte una suscripción: necesita un servidor propio compatible (no incluido) y, si no lo usas, no cambia nada. Mide quién gasta cuánta cuota y lo muestra en vivo.

- **Cuota real, no estimada**: usa el % de uso que muestra Claude Code (ventana de 5 h y semanal) y reparte cada subida entre las personas según sus tokens, **subagentes incluidos**.
- **Tarjeta «Pool»** en el centro de control de IA (`Ctrl+Shift+A`): tu consumo y el de la otra persona, con aviso al acercarte a tu parte.
- **Línea de estado en la statusline de Claude**. Si ya tenías una statusline (por ejemplo la de Orca), queda **encadenada** y se restaura exactamente al desactivar el módulo.
- **Opcional y seguro**: no hace nada hasta ejecutar `poxiterminal pool setup`. Solo envía contadores, modelos y porcentajes por HTTPS (nunca prompts, respuestas ni código) y el token se guarda en el almacén de credenciales del sistema.
- **Comandos nuevos**: `poxiterminal pool setup`, `pool status`, `pool sync` (un ciclo completo para diagnosticar sin abrir la ventana) y `pool off`.
- Necesita un servidor compatible (CMKristian, no incluido). Sin él el módulo no hace nada.

### 🇬🇧 English

**Share one Claude subscription between two people (advanced).** Optional **Pool** module, meant for people who share a subscription: it needs your own compatible server (not included) and changes nothing if you don't use it. It measures who uses how much quota and shows it live.

- **Real quota, not an estimate**: it uses the usage % that Claude Code shows (5-hour and weekly windows) and splits every increase between people by their tokens, **subagents included**.
- **"Pool" card** in the AI control center (`Ctrl+Shift+A`): your usage and the other person's, with a warning as you approach your share.
- **Status line in Claude's statusline**. If you already had a statusline (for example Orca's), it is **chained** and restored exactly when the module is turned off.
- **Optional and safe**: it does nothing until you run `poxiterminal pool setup`. It only sends counters, models and percentages over HTTPS (never prompts, answers or code) and the token is kept in the system credential store.
- **New commands**: `poxiterminal pool setup`, `pool status`, `pool sync` (one full cycle to diagnose without opening the window) and `pool off`.
- Requires a compatible server (CMKristian, not included). Without it the module does nothing.

## 0.3.1

### 🇪🇸 Español

- **Icono nuevo**: PoxiTerminal estrena icono con el estilo de la familia de programas de Poxi, una P pixelada con el `>_` del terminal.
- **Graphite por defecto**: el icono arranca en gris grafito. Los 25 colores siguen disponibles en **Configuración → Apariencia → Icono de la app**, Titanium incluido.

### 🇬🇧 English

- **New icon**: PoxiTerminal gets a new icon in the style of the Poxi app family, a pixel-art P with the terminal's `>_`.
- **Graphite by default**: the icon now starts in graphite gray. All 25 colors are still available under **Settings → Appearance → App icon**, Titanium included.

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

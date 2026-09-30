//! Visor de cambios desde el workspace: foto del repositorio al empezar cada
//! turno de la IA y apertura de la pestaña «Cambios».

use std::collections::HashMap;

use super::*;
use crate::display::side_panel::GitLocation;
use crate::gpui_shell::diff_view::git;

/// Foto del repositorio de un panel al empezar el último turno de su IA.
#[derive(Clone, Debug)]
pub(crate) struct TurnBaseline {
    pub location: GitLocation,
    pub tree: String,
}

/// Fotos por panel. Solo se guarda la del último turno de cada uno.
#[derive(Default)]
pub(crate) struct TurnBaselines(HashMap<u64, TurnBaseline>);

impl NebulaWorkspace {
    pub(crate) fn has_turn_baseline(&self, pane_id: u64) -> bool {
        self.turn_baselines.0.contains_key(&pane_id)
    }

    /// Carpeta de trabajo del panel como ubicación Git (local o dentro de WSL).
    fn pane_git_location(&self, pane_id: u64, cx: &App) -> Option<GitLocation> {
        let tab_ix = self.tab_of_pane(pane_id)?;
        let WorkspaceTab::Terminal { panes, .. } = self.tabs.get(tab_ix)? else { return None };
        let view = panes.iter().find(|pane| pane.id == pane_id)?.view.read(cx);
        if let Some(crate::session::LaunchSession::Shell { program, args, .. }) =
            self.meta(tab_ix).launch
        {
            if let Some(wsl) = crate::shell_detect::wsl_cwd(&view.cwd, &program, &args) {
                return Some(GitLocation::Wsl { distro: wsl.distro, root: wsl.guest });
            }
        }
        view.local_cwd().map(|root| GitLocation::Local { root })
    }

    /// Al enviar un prompt se hace la foto en segundo plano; así «Este turno»
    /// muestra solo lo que la IA toque a partir de aquí.
    pub(crate) fn capture_turn_baseline(&mut self, pane_id: u64, cx: &mut Context<Self>) {
        let Some(location) = self.pane_git_location(pane_id, cx) else { return };
        let task = cx.background_executor().spawn(async move {
            let root = git::repo_root(&location)?;
            let tree = git::snapshot_tree(&root)?;
            Ok::<_, String>(TurnBaseline { location: root, tree })
        });
        cx.spawn(async move |this, cx| {
            if let Ok(baseline) = task.await {
                let _ = this.update(cx, |workspace, _| {
                    workspace.turn_baselines.0.insert(pane_id, baseline);
                });
            }
        })
        .detach();
    }

    /// Abre (o reutiliza) la pestaña «Cambios». Con `pane` usa la foto de su
    /// último turno; sin foto, o sin panel, abre «Todo sin commit».
    pub(crate) fn open_changes_tab(
        &mut self,
        pane: Option<u64>,
        preselect: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(baseline) = pane.and_then(|pane| self.turn_baselines.0.get(&pane).cloned()) {
            self.show_changes_tab(baseline.location, Some(baseline.tree), preselect, window, cx);
            return;
        }
        let location = pane
            .and_then(|pane| self.pane_git_location(pane, cx))
            .or_else(|| self.side_panel.git_location());
        let Some(location) = location else { return };
        // Sin foto hay que subir hasta la raíz del repo. Es un proceso git (o
        // wsl.exe, que puede tardar si la distro está parada): en segundo plano.
        let task = cx.background_executor().spawn(async move { git::repo_root(&location) });
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = window_handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |workspace, cx| match result {
                    Ok(root) => workspace.show_changes_tab(root, None, preselect, window, cx),
                    Err(error) => crate::gpui_shell::toast::toast(
                        window,
                        cx,
                        crate::gpui_shell::toast::ToastKind::Warning,
                        error,
                    ),
                });
            });
        })
        .detach();
    }

    fn show_changes_tab(
        &mut self,
        location: GitLocation,
        tree: Option<String>,
        preselect: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root_key = match &location {
            GitLocation::Local { root } => root.clone(),
            GitLocation::Wsl { distro, root } => {
                std::path::PathBuf::from(format!("{distro}:{root}"))
            },
        };
        // Una pestaña por repo: si ya existe se cierra y se abre otra con la
        // foto buena (la del turno puede haber cambiado).
        if let Some(ix) = self.tabs.iter().position(|tab| {
            matches!(tab, WorkspaceTab::Code { view, .. } if view.read(cx).is_diff_of(&root_key))
        }) {
            self.close_tab(ix, window, cx);
        }
        let view = cx.new(|cx| {
            crate::gpui_shell::code_tab::CodeTabView::new_diff(
                location, tree, preselect, window, cx,
            )
        });
        let subscription = cx.subscribe(&view, Self::on_code_tab_event);
        self.insert_new_tab(WorkspaceTab::Code { view, _subscription: subscription });
        self.focus_active(window, cx);
        cx.notify();
    }
}

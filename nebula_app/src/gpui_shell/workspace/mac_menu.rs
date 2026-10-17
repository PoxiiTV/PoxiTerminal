//! Barra de menús de macOS, en el idioma de la interfaz.
//!
//! Sin `set_menus` la app solo enseñaba su nombre arriba, sin «Salir», «Ocultar»
//! ni «Edición». Los menús reutilizan las acciones de los atajos, así que macOS
//! muestra al lado de cada opción el atajo con ⌘ que tenga asignado.

use gpui::{App, Menu, MenuItem, OsAction, SystemMenuType};

use super::keyboard_bindings::QuitApp;
use super::{
    CloseActiveTerminal, CopySelection, DecreaseFontSize, IncreaseFontSize, NewTerminal,
    NewWindow, OpenQuickJump, OpenSettings, PasteClipboard, ResetFontSize, SelectNextTab,
    SelectPreviousTab, SplitDown, SplitRight, ToggleAgentsPanel, ToggleCommandPalette,
    ToggleFileTree, ToggleFullscreen, ToggleGitPanel, ToggleSidebar, ToggleZoom,
};
use crate::display::UiLanguage;

gpui::actions!(
    nebula_mac_menu,
    [HideApp, HideOtherApps, ShowAllApps, MinimizeWindow, ZoomWindow]
);

/// Instala la barra de menús y las acciones propias de macOS (ocultar, minimizar…).
pub(super) fn init(cx: &mut App) {
    cx.on_action(|_: &HideApp, cx: &mut App| cx.hide());
    cx.on_action(|_: &HideOtherApps, cx: &mut App| cx.hide_other_apps());
    cx.on_action(|_: &ShowAllApps, cx: &mut App| cx.unhide_other_apps());
    cx.on_action(|_: &MinimizeWindow, cx: &mut App| {
        if let Some(window) = cx.active_window() {
            let _ = window.update(cx, |_, window, _| window.minimize_window());
        }
    });
    cx.on_action(|_: &ZoomWindow, cx: &mut App| {
        if let Some(window) = cx.active_window() {
            let _ = window.update(cx, |_, window, _| window.zoom_window());
        }
    });
    let language = crate::gpui_shell::config::ui_language(cx);
    cx.set_menus(menus(language));
}

/// Castellano salvo que la interfaz esté en inglés (o en chino, que no tiene textos
/// de menú propios: mejor inglés que mezclar idiomas).
fn menus(language: UiLanguage) -> Vec<Menu> {
    let es = !matches!(language, UiLanguage::EnUs | UiLanguage::ZhCn);
    let t = |es_text: &'static str, en_text: &'static str| if es { es_text } else { en_text };
    vec![
        Menu::new("PoxiTerminal").items([
            MenuItem::action(t("Configuración…", "Settings…"), OpenSettings),
            MenuItem::separator(),
            MenuItem::os_submenu(t("Servicios", "Services"), SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action(t("Ocultar PoxiTerminal", "Hide PoxiTerminal"), HideApp),
            MenuItem::action(t("Ocultar otros", "Hide Others"), HideOtherApps),
            MenuItem::action(t("Mostrar todo", "Show All"), ShowAllApps),
            MenuItem::separator(),
            MenuItem::action(t("Salir de PoxiTerminal", "Quit PoxiTerminal"), QuitApp),
        ]),
        Menu::new(t("Archivo", "File")).items([
            MenuItem::action(t("Nueva pestaña", "New Tab"), NewTerminal),
            MenuItem::action(t("Nueva ventana", "New Window"), NewWindow),
            MenuItem::separator(),
            MenuItem::action(t("Cerrar pestaña", "Close Tab"), CloseActiveTerminal),
        ]),
        Menu::new(t("Edición", "Edit")).items([
            MenuItem::os_action(t("Copiar", "Copy"), CopySelection, OsAction::Copy),
            MenuItem::os_action(t("Pegar", "Paste"), PasteClipboard, OsAction::Paste),
        ]),
        Menu::new(t("Visualización", "View")).items([
            MenuItem::action(t("Paleta de comandos", "Command Palette"), ToggleCommandPalette),
            MenuItem::action(t("Salto rápido", "Quick Jump"), OpenQuickJump),
            MenuItem::separator(),
            MenuItem::action(t("Barra lateral", "Sidebar"), ToggleSidebar),
            MenuItem::action(t("Panel de archivos", "Files Panel"), ToggleFileTree),
            MenuItem::action(t("Panel de Git", "Git Panel"), ToggleGitPanel),
            MenuItem::action(t("Centro de control de IA", "AI Control Center"), ToggleAgentsPanel),
            MenuItem::separator(),
            MenuItem::action(t("Aumentar texto", "Bigger Text"), IncreaseFontSize),
            MenuItem::action(t("Reducir texto", "Smaller Text"), DecreaseFontSize),
            MenuItem::action(t("Tamaño original", "Actual Size"), ResetFontSize),
            MenuItem::separator(),
            MenuItem::action(t("Pantalla completa", "Full Screen"), ToggleFullscreen),
        ]),
        Menu::new(t("Ventana", "Window")).items([
            MenuItem::action(t("Minimizar", "Minimize"), MinimizeWindow),
            MenuItem::action(t("Zoom", "Zoom"), ZoomWindow),
            MenuItem::separator(),
            MenuItem::action(t("Dividir a la derecha", "Split Right"), SplitRight),
            MenuItem::action(t("Dividir abajo", "Split Down"), SplitDown),
            MenuItem::action(t("Ampliar panel", "Zoom Pane"), ToggleZoom),
            MenuItem::separator(),
            MenuItem::action(t("Pestaña siguiente", "Next Tab"), SelectNextTab),
            MenuItem::action(t("Pestaña anterior", "Previous Tab"), SelectPreviousTab),
        ]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(menu: &Menu) -> Vec<String> {
        menu.items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Action { name, .. } => Some(name.to_string()),
                MenuItem::SystemMenu(menu) => Some(menu.name.to_string()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn mac_menu_bar_speaks_the_interface_language() {
        let es = menus(UiLanguage::EsEs);
        let names: Vec<_> = es.iter().map(|menu| menu.name.to_string()).collect();
        assert_eq!(names, ["PoxiTerminal", "Archivo", "Edición", "Visualización", "Ventana"]);
        assert!(labels(&es[0]).contains(&"Salir de PoxiTerminal".to_owned()));
        assert!(labels(&es[0]).contains(&"Servicios".to_owned()));
        assert_eq!(labels(&es[2]), ["Copiar", "Pegar"]);

        let en = menus(UiLanguage::EnUs);
        assert_eq!(en[1].name.to_string(), "File");
        assert!(labels(&en[0]).contains(&"Quit PoxiTerminal".to_owned()));
        // Ningún texto en castellano se cuela en la versión inglesa.
        for menu in &en {
            for label in labels(menu) {
                assert!(!label.contains('ñ') && !label.contains("ción"), "{label}");
            }
        }
    }
}

//! 快速终端全局热键。
//!
//! 进程内持有一扇独立窗口，切换时保留 PTY 与终端状态。
//! 窗口系统决定显示位置和动画能力；全局键注册与窗口生命周期分开管理。
//!
//! `global_hotkey` 的事件走进程级 channel，而 GPUI 主线程不能阻塞。一个小线程阻塞
//! 在那个 channel 上（不耗 CPU），再通过异步 channel 交给 GPUI：主线程只在真的按下
//! 组合键时才醒。以前每 16 ms 轮询一次，空闲时每秒唤醒主线程 60 次。

use std::time::Duration;

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use gpui::{App, Context, Global, Window};

use super::NebulaWorkspace;

/// Linux portal 仍按一帧的节奏排空（它的 channel 不能阻塞等待）。
#[cfg(target_os = "linux")]
const POLL_INTERVAL: Duration = Duration::from_millis(16);

#[cfg(windows)]
pub(super) use crate::platform::quick_window::*;

/// 每多少次轮询回读一次设置里的组合键（Linux portal，约 2 秒）。
///
/// 设置页改键后要立刻生效，但它的持久化路径（`settings_pane/keymap.rs::persist`）没有
/// 面向进程级单例的通知通道，而 GPUI 的内存全局 `config::Settings` 里也不存这个键。
/// 与其为一个低频动作新加一条事件链，这里在后台按固定节拍回读——它不在渲染路径上，
/// 每次只读一个不到 1KB 的设置文件。
#[cfg(target_os = "linux")]
const RESYNC_EVERY: u32 = 125;

/// 回读设置里组合键的节拍（改键后最多 2 秒生效）。
const RESYNC_INTERVAL: Duration = Duration::from_secs(2);

/// 当前注册状态。`GlobalHotKeyManager` 必须活到进程结束：它的 `Drop` 会向系统注销热键。
struct QuickTerminalHotkey {
    manager: GlobalHotKeyManager,
    /// `None` = 组合键合法但系统拒绝注册（通常是被别的应用占用）。
    hotkey: Option<HotKey>,
    /// 已注册的原始字符串，用来判断设置是否变过。
    combo: String,
}

impl Global for QuickTerminalHotkey {}

#[cfg(target_os = "linux")]
struct PortalHotkey(crate::platform::global_shortcut::Portal);
#[cfg(target_os = "linux")]
impl Global for PortalHotkey {}

impl NebulaWorkspace {
    /// 注册快速终端热键并启动事件泵。只应由初始窗口调用一次。
    pub(super) fn start_quick_terminal_hotkey(window: &Window, cx: &mut Context<Self>) {
        #[cfg(target_os = "linux")]
        if crate::platform::window_visibility::is_wayland(window) {
            Self::start_portal_hotkey(cx);
            return;
        }
        if !crate::platform::CAPABILITIES.quick_terminal_hotkey
            || cx.has_global::<QuickTerminalHotkey>()
        {
            return;
        }
        let manager = match GlobalHotKeyManager::new() {
            Ok(manager) => manager,
            Err(err) => {
                // 非致命：热键不可用时终端其余功能完好，不值得弹提示打扰用户。
                log::warn!("quick terminal disabled: global hotkey init failed: {err}");
                return;
            },
        };
        let combo = current_combo();
        let hotkey = register(&manager, &combo);
        cx.set_global(QuickTerminalHotkey { manager, hotkey, combo });

        // 线程阻塞在全局热键 channel 上，有事件才转发；GPUI 侧 await，无需轮询。
        let (events_tx, mut events_rx) = futures::channel::mpsc::unbounded::<GlobalHotKeyEvent>();
        if let Err(error) =
            std::thread::Builder::new().name("poxiterminal-quick-hotkey".into()).spawn(move || {
                while let Ok(event) = GlobalHotKeyEvent::receiver().recv() {
                    if events_tx.unbounded_send(event).is_err() {
                        break;
                    }
                }
            })
        {
            log::warn!("quick terminal disabled: hotkey thread failed: {error}");
            return;
        }
        cx.spawn(async move |_this, cx| {
            use futures::StreamExt as _;
            while let Some(first) = events_rx.next().await {
                // 同一批的多次按下合并成一次切换，避免窗口来回闪。
                let mut events = vec![first];
                while let Ok(event) = events_rx.try_recv() {
                    events.push(event);
                }
                let pressed = cx.update(|cx| {
                    let id = cx.global::<QuickTerminalHotkey>().hotkey.map(|hotkey| hotkey.id());
                    pressed_in(&events, id)
                });
                if pressed {
                    cx.update(super::windowing::toggle_quick_terminal_window);
                }
            }
        })
        .detach();

        let executor = cx.background_executor().clone();
        cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(RESYNC_INTERVAL).await;
                cx.update(resync_combo);
            }
        })
        .detach();
    }
}

#[cfg(target_os = "linux")]
impl NebulaWorkspace {
    fn start_portal_hotkey(cx: &mut Context<Self>) {
        if cx.has_global::<PortalHotkey>() {
            return;
        }
        let portal = match crate::platform::global_shortcut::Portal::start(current_combo()) {
            Ok(portal) => portal,
            Err(error) => {
                log::warn!("Could not start shortcut portal: {error}");
                return;
            },
        };
        cx.set_global(PortalHotkey(portal));
        cx.spawn(async move |_, cx| {
            let mut ticks = 0u32;
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                ticks = ticks.wrapping_add(1);
                cx.update(|cx| {
                    let portal = &cx.global::<PortalHotkey>().0;
                    if ticks % RESYNC_EVERY == 0 {
                        portal.set_combo(current_combo());
                    }
                    if portal.take_pressed() {
                        super::windowing::toggle_quick_terminal_window(cx);
                    }
                });
            }
        })
        .detach();
    }
}

/// 设置里持久化的组合键；缺失或非法时回落到与旧壳同一个默认值。
fn current_combo() -> String {
    nebula_settings::RuntimeSettings::load().quick_terminal_hotkey
}

/// 注册一个组合键。返回 `None` 表示解析或注册失败——两者都不影响其余功能。
fn register(manager: &GlobalHotKeyManager, combo: &str) -> Option<HotKey> {
    if combo.trim().is_empty() {
        return None;
    }
    let hotkey = match combo.parse::<HotKey>() {
        Ok(hotkey) => hotkey,
        Err(err) => {
            log::warn!("quick terminal hotkey {combo:?} is not a valid combo: {err}");
            return None;
        },
    };
    match manager.register(hotkey) {
        Ok(()) => Some(hotkey),
        Err(err) => {
            // 开发期常见：上一个实例被硬杀、Drop 没跑，键还被系统占着。
            log::debug!("quick terminal hotkey {combo:?} not registered: {err}");
            None
        },
    }
}

/// 设置改过就换键。先注册新键再注销旧键：系统拒绝新键时旧键仍然可用，
/// 不会出现「改了一个冲突的键，结果连原来那个也没了」。
fn resync_combo(cx: &mut App) {
    let latest = current_combo();
    let state = cx.global::<QuickTerminalHotkey>();
    if state.combo == latest && (state.hotkey.is_some() || latest.trim().is_empty()) {
        return;
    }
    let state = cx.global_mut::<QuickTerminalHotkey>();
    if latest.trim().is_empty() {
        if let Some(previous) = state.hotkey {
            if let Err(error) = state.manager.unregister(previous) {
                log::warn!("could not clear quick terminal shortcut: {error}");
                return;
            }
        }
        state.hotkey = None;
        state.combo = latest;
        return;
    }
    let replacement = register(&state.manager, &latest);
    if replacement.is_some() {
        if let Some(previous) = state.hotkey.take() {
            let _ = state.manager.unregister(previous);
        }
        state.hotkey = replacement;
    }
    state.combo = latest;
}

/// ¿Hay en este lote alguna pulsación (no suelta) de nuestro atajo?
///
/// Se mira el lote entero: varias pulsaciones seguidas cuentan como una sola
/// para que la ventana no se abra y cierre sola.
fn pressed_in(events: &[GlobalHotKeyEvent], id: Option<u32>) -> bool {
    id.is_some_and(|id| {
        events.iter().any(|event| event.id == id && event.state == HotKeyState::Pressed)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: u32, state: HotKeyState) -> GlobalHotKeyEvent {
        GlobalHotKeyEvent { id, state }
    }

    /// Varias pulsaciones del mismo lote cuentan como una sola (no hace parpadear la
    /// ventana); soltar la tecla o la de otro atajo no cuentan.
    #[test]
    fn only_a_press_of_our_shortcut_toggles_once_per_batch() {
        let ours = 7;
        assert!(!pressed_in(&[], Some(ours)));
        assert!(!pressed_in(&[event(ours, HotKeyState::Released)], Some(ours)));
        assert!(!pressed_in(&[event(9, HotKeyState::Pressed)], Some(ours)));
        assert!(!pressed_in(&[event(ours, HotKeyState::Pressed)], None));
        assert!(pressed_in(
            &[event(ours, HotKeyState::Pressed), event(ours, HotKeyState::Released),
              event(ours, HotKeyState::Pressed)],
            Some(ours)
        ));
    }
}

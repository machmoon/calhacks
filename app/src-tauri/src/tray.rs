//! The menu bar (tray) icon: Wright up beside Cold Turkey, Claude and Box.
//!
//! One status item carrying the quick actions: left click toggles the
//! overlay, right click opens a menu with show/hide, the dashboard, about and
//! quit.
//!
//! The window's visibility is read from the window (or, on Windows, from the
//! `display:none` workaround state), never assumed from the last click.
//!
//! The icon is `include_bytes!` so the binary needs no resource lookup at the
//! moment the tray is built; see `icons/tray/gen.py` for how it was drawn.
//! On macOS it is a **template image** (black on transparent) and the system
//! tints it for a light or dark menu bar. Everywhere else the coloured
//! (copper) variant is used, since template rendering is a macOS-only concept.

use std::sync::Mutex;

use tauri::{
    image::Image,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime,
};

/// The tray icon's id (one per app; `Manager::tray_by_id`).
pub const TRAY_ID: &str = "wright";

pub const ID_SHOW: &str = "tray-show";
pub const ID_DASHBOARD: &str = "tray-dashboard";
pub const ID_ABOUT: &str = "tray-about";
pub const ID_QUIT: &str = "tray-quit";

pub const LABEL_SHOW: &str = "Show Wright";
pub const LABEL_HIDE: &str = "Hide Wright";
pub const LABEL_DASHBOARD: &str = "Open dashboard";
pub const LABEL_ABOUT: &str = "About Wright";
pub const LABEL_QUIT: &str = "Quit Wright";

#[cfg(target_os = "macos")]
const ICON: &[u8] = include_bytes!("../icons/tray/idleTemplate@2x.png");
#[cfg(not(target_os = "macos"))]
const ICON: &[u8] = include_bytes!("../icons/tray/idle-colour.png");

/// The tray's handles, managed on the app so the shortcut path and the
/// `tray_set_state` command can reach the same items.
pub struct TrayState<R: Runtime> {
    show: MenuItem<R>,
    visible: Mutex<bool>,
}

/// The status item's glyph.
pub fn icon() -> tauri::Result<Image<'static>> {
    Image::from_bytes(ICON)
}

/// The label of the show/hide item for a window state.
pub fn show_label(visible: bool) -> &'static str {
    if visible {
        LABEL_HIDE
    } else {
        LABEL_SHOW
    }
}

/// The tooltip: the app and its version, so hovering answers "which build".
pub fn tooltip() -> String {
    format!("Wright {}", env!("CARGO_PKG_VERSION"))
}

/// Build the status item and its menu, and manage the handles on the app.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, ID_SHOW, LABEL_HIDE, true, None::<&str>)?;
    let dashboard = MenuItem::with_id(app, ID_DASHBOARD, LABEL_DASHBOARD, true, None::<&str>)?;
    let about = MenuItem::with_id(app, ID_ABOUT, LABEL_ABOUT, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, ID_QUIT, LABEL_QUIT, true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &show,
            &dashboard,
            &PredefinedMenuItem::separator(app)?,
            &about,
            &quit,
        ],
    )?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon()?)
        // A no-op off macOS; there the coloured PNG is used as drawn.
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip(tooltip())
        .menu(&menu)
        // Left click is the overlay toggle; the menu is on the right button.
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                crate::shortcuts::toggle_overlay(app);
                sync_visible(app);
            }
        })
        .build(app)?;

    app.manage(TrayState {
        show,
        visible: Mutex::new(true),
    });
    // The overlay starts visible in tauri.conf.json, but read it rather than
    // assume it, so the first label is right on a platform that differs.
    sync_visible(app);
    Ok(())
}

fn on_menu_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    match event.id().as_ref() {
        ID_SHOW => {
            crate::shortcuts::toggle_overlay(app);
            sync_visible(app);
        }
        ID_DASHBOARD => {
            if let Err(e) = crate::window::show_dashboard_window(app) {
                eprintln!("[tray] could not open the dashboard: {e}");
            }
        }
        ID_ABOUT => show_about(app),
        ID_QUIT => app.exit(0),
        _ => {}
    }
}

fn show_about<R: Runtime>(app: &AppHandle<R>) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    app.dialog()
        .message(format!(
            "Wright {}\n\nDescribe a vehicle; an agent team designs it and checks every claim.\nA GPL-3.0 fork of Pluely.",
            env!("CARGO_PKG_VERSION")
        ))
        .title(LABEL_ABOUT)
        .kind(MessageDialogKind::Info)
        .show(|_| {});
}

/// Push the overlay's visibility onto the show/hide label.
pub fn apply<R: Runtime>(app: &AppHandle<R>, visible: bool) -> Result<(), String> {
    let Some(state) = app.try_state::<TrayState<R>>() else {
        // No tray was built (a headless test, or the build failed and was
        // reported at startup): nothing to update, and not an error.
        return Ok(());
    };
    *state
        .visible
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = visible;
    state
        .show
        .set_text(show_label(visible))
        .map_err(|e| format!("tray show item: {e}"))
}

/// Whether the overlay is on screen, read the way the platform hides it: the
/// window itself everywhere but Windows, where the hide shortcut leaves the
/// window shown and blanks the webview instead (`WindowVisibility`).
pub fn overlay_visible<R: Runtime>(app: &AppHandle<R>) -> bool {
    #[cfg(target_os = "windows")]
    {
        if let Some(state) = app.try_state::<crate::shortcuts::WindowVisibility>() {
            let hidden = state
                .is_hidden
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            return !*hidden;
        }
    }
    app.get_webview_window("main")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(true)
}

/// Re-read the window's visibility and update the show/hide label; called
/// after anything that hides or shows the overlay natively.
pub fn sync_visible<R: Runtime>(app: &AppHandle<R>) {
    if let Err(e) = apply(app, overlay_visible(app)) {
        eprintln!("[tray] could not sync visibility: {e}");
    }
}

/// The front end's report of whether, on Windows, the webview is blanked
/// (`useTrayState`).
#[tauri::command]
pub fn tray_set_state<R: Runtime>(app: AppHandle<R>, visible: bool) -> Result<(), String> {
    // Off Windows the window's own visibility is the truth and the front end
    // cannot see a native hide, so it is read here rather than trusted.
    let visible = if cfg!(target_os = "windows") {
        visible
    } else {
        overlay_visible(&app)
    };
    apply(&app, visible)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every menu id, in menu order.
    const MENU_IDS: [&str; 4] = [ID_SHOW, ID_DASHBOARD, ID_ABOUT, ID_QUIT];

    #[test]
    fn menu_ids_are_distinct_and_prefixed() {
        let mut seen = std::collections::HashSet::new();
        for id in MENU_IDS {
            assert!(id.starts_with("tray-"), "{id}");
            assert!(seen.insert(id), "duplicate menu id {id}");
        }
    }

    #[test]
    fn labels_name_the_app() {
        assert_eq!(show_label(true), "Hide Wright");
        assert_eq!(show_label(false), "Show Wright");
        assert_eq!(LABEL_DASHBOARD, "Open dashboard");
        assert_eq!(LABEL_ABOUT, "About Wright");
        assert_eq!(LABEL_QUIT, "Quit Wright");
        assert!(tooltip().starts_with("Wright "));
    }

    #[test]
    fn icon_decodes() {
        let image = icon().expect("icon decodes");
        assert!(image.width() > 0 && image.height() > 0);
    }

    /// The template-image contract: every visible pixel is black, so macOS
    /// can tint the glyph for a light or dark menu bar. A coloured pixel would
    /// render as a grey smudge.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_icon_is_a_template_image() {
        let image = icon().unwrap();
        for px in image.rgba().chunks(4) {
            if px[3] > 0 {
                assert_eq!(&px[..3], &[0, 0, 0], "non-black pixel in a template icon");
            }
        }
    }
}

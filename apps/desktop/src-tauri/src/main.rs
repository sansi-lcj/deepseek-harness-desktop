//! DeepSeek Harness desktop shell: a Tauri 2 window that boots the dsh web
//! server and hosts its browser UI.
//!
//! Lifecycle: the shell spawns the built dsh server ('apps/cli/lib/bin.js'),
//! watches its stdout for the readiness URL line, then navigates the main
//! window from the bundled splash page to the live server URL. On quit the
//! server is signalled first, then the shell exits. External http(s) links
//! open in the default browser instead of unmanaged child webviews.

mod paths;
mod server;

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use tauri::webview::NewWindowResponse;
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_single_instance::init as single_instance;
use tauri_plugin_updater::UpdaterExt;

use server::ServerManager;

/// Set once a deliberate shutdown has begun, so the second ExitRequested
/// raised by AppHandle::exit is not prevented again.
static EXITING: AtomicBool = AtomicBool::new(false);

/// Smoke mode (DSH_DESKTOP_SMOKE=1): report once the server page loads, then exit.
fn smoke_enabled() -> bool {
    std::env::var("DSH_DESKTOP_SMOKE").is_ok_and(|value| value == "1")
}

/// The main window: splash-first, with the external-link policy attached.
pub(crate) fn main_window_builder<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    url: WebviewUrl,
) -> WebviewWindowBuilder<'_, R, tauri::AppHandle<R>> {
    WebviewWindowBuilder::new(app, "main", url)
        .title("DeepSeek Harness")
        .inner_size(1280.0, 832.0)
        .min_inner_size(960.0, 600.0)
        .on_new_window(move |url, _features| {
            let text = url.to_string();
            let loopback = text.contains("127.0.0.1") || text.contains("localhost");
            let web = text.starts_with("http://") || text.starts_with("https://");
            if web && !loopback {
                let _ = open::that(text);
                NewWindowResponse::Deny
            } else {
                NewWindowResponse::Allow
            }
        })
}

/// Startup update check: asks, downloads, installs, and restarts when a
/// newer release is published at the configured endpoint.
fn check_for_updates(app: tauri::AppHandle) {
    let Ok(updater) = app.updater() else {
        eprintln!("[dsh-desktop] updater unavailable");
        return;
    };
    match tauri::async_runtime::block_on(async { updater.check().await }) {
        Ok(Some(update)) => {
            eprintln!("[dsh-desktop] update available: {}", update.version);
            let confirmed = app
                .dialog()
                .message(format!(
                    "DeepSeek Harness {} is available (current {}). Install and restart now?",
                    update.version, update.current_version
                ))
                .title("Update available")
                .buttons(tauri_plugin_dialog::MessageDialogButtons::OkCancelCustom(
                    "Install".to_string(),
                    "Later".to_string(),
                ))
                .blocking_show();
            if !confirmed {
                return;
            }
            eprintln!("[dsh-desktop] downloading update {}...", update.version);
            let installed = tauri::async_runtime::block_on(async {
                update
                    .download_and_install(
                        |_chunk, _total| {},
                        || eprintln!("[dsh-desktop] update downloaded; installing"),
                    )
                    .await
            });
            match installed {
                Ok(()) => {
                    eprintln!("[dsh-desktop] update installed; restarting");
                    app.request_restart();
                }
                Err(error) => eprintln!("[dsh-desktop] update install failed: {error}"),
            }
        }
        Ok(None) => eprintln!("[dsh-desktop] no update available"),
        Err(error) => eprintln!("[dsh-desktop] update check failed: {error}"),
    }
}

fn main() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(single_instance(|app, _args, _cwd| {
            eprintln!("[dsh-desktop] single-instance: focusing the running window");
            // A second launch focuses the running instance's window.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .on_window_event(|window, event| {
            match event {
                tauri::WindowEvent::CloseRequested { .. } => {
                    eprintln!("[dsh-desktop] window CloseRequested label={}", window.label())
                }
                tauri::WindowEvent::Destroyed => {
                    eprintln!("[dsh-desktop] window Destroyed label={}", window.label())
                }
                _ => {}
            }
        })
        .on_page_load(|window, payload| {
            eprintln!("[dsh-desktop] page-load url={}", payload.url());
            if smoke_enabled()
                && payload.url().scheme() == "http"
                && payload.url().host_str() == Some("127.0.0.1")
            {
                eprintln!("[dsh-desktop] SMOKE_OK url={}", payload.url());
                window.app_handle().exit(0);
            }
        });

    // macOS-only: surface webview content-process crashes in the shell log.
    #[cfg(target_os = "macos")]
    let builder = builder.on_web_content_process_terminate(|webview| {
        eprintln!(
            "[dsh-desktop] webview content process terminated label={}",
            webview.label()
        );
    });

    builder
        .setup(|app| {
            let window = main_window_builder(app.handle(), WebviewUrl::App("index.html".into()))
                .build()
                .expect("failed to build the main window");
            eprintln!(
                "[dsh-desktop] window ready url={:?} visible={}",
                window.url(),
                window.is_visible().unwrap_or(false)
            );
            app.manage(ServerManager::start(app.handle().clone()));
            if !smoke_enabled() {
                let handle = app.handle().clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_secs(6));
                    check_for_updates(handle);
                });
            }
            if smoke_enabled() {
                let handle = app.handle().clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_secs(120));
                    eprintln!("[dsh-desktop] SMOKE_TIMEOUT");
                    handle.exit(2);
                });
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build the dsh-desktop shell")
        .run(|app_handle, event| {
            if let RunEvent::ExitRequested { api, code, .. } = event {
                eprintln!("[dsh-desktop] exit requested code={code:?}");
                if !EXITING.swap(true, Ordering::SeqCst) {
                    api.prevent_exit();
                    let manager: ServerManager = app_handle.state::<ServerManager>().inner().clone();
                    let handle = app_handle.clone();
                    let restarting = code == Some(i32::MAX);
                    thread::spawn(move || {
                        manager.shutdown();
                        eprintln!("[dsh-desktop] server stopped; terminating the process");
                        if restarting {
                            // Preserve the restart exit code so tauri relaunches
                            // the app after the event loop winds down.
                            handle.exit(i32::MAX);
                        } else {
                            std::process::exit(0);
                        }
                    });
                }
            }
        });
}

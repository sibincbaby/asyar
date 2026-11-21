//! Platform-specific bootstrap module.
//!
//! Handles OS-specific initialization (Linux D-Bus service, single-instance routing, deep links, diagnostics panic hooks,
//! and autostart registration).

use std::sync::Arc;
use tauri::Manager;

pub fn init_early(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let launcher_coordinator = app
        .state::<Arc<crate::launcher::LauncherCoordinator>>()
        .inner()
        .clone();
    launcher_coordinator
        .attach_app(app.handle().clone())
        .map_err(|error| format!("failed to attach launcher coordinator: {error}"))?;

    #[cfg(target_os = "linux")]
    match crate::platform::linux::launcher_dbus::start(
        &app.config().identifier,
        &launcher_coordinator,
    ) {
        Ok(service) => {
            app.manage(service);
        }
        Err(error) => {
            log::warn!("[launcher-dbus] {error}");
        }
    }

    let initial_args = std::env::args().collect::<Vec<_>>();
    if let Some(action) = crate::launcher::classify_initial_launch(
        &initial_args,
        crate::deeplink::deep_link_scheme(app.handle()),
    ) {
        launcher_coordinator
            .request(action)
            .map_err(|error| format!("failed to queue initial launcher action: {error}"))?;
    }

    Ok(())
}

pub fn init_diagnostics(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Install a panic hook that emits a `feedback:report` event before the process unwinds.
    {
        let app_handle = app.handle().clone();
        std::panic::set_hook(Box::new(move |info| {
            let location = info
                .location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_else(|| "unknown".into());
            let detail = info.to_string();
            let payload = serde_json::json!({
                "source": "rust",
                "kind": "panic",
                "severity": "fatal",
                "retryable": false,
                "context": { "location": location },
                "developerDetail": detail,
            });
            let _ = tauri::Emitter::emit(&app_handle, "feedback:report", payload);
            log::error!("panic: {info}");
        }));
    }

    // Long-poll IPC bridge: queue Rust->JS events for `bridge_poll`.
    app.manage(Arc::new(crate::event_bridge::EventBridge::default()));

    Ok(())
}

pub fn init_deeplinks(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Notification-action registry
    {
        let registry = Arc::new(crate::notifications::NotificationActionRegistry::new());
        let backend = crate::notifications::build_default_backend(
            app.handle().clone(),
            Arc::clone(&registry),
        );
        app.manage(registry);
        app.manage(backend);
    }

    // Extension-tray manager
    {
        let lookup: Arc<dyn crate::extension_tray::icon::ExtensionDirLookup + Send + Sync> =
            Arc::new(
                crate::extension_tray::extension_lookup::AppHandleExtensionDirLookup::new(
                    app.handle().clone(),
                ),
            );
        let backend =
            crate::extension_tray::backend::TauriTrayBackend::new(app.handle().clone(), lookup);
        app.manage(crate::extension_tray::ExtensionTrayManager::new(Box::new(
            backend,
        )));
    }

    // Deep link handler
    {
        use tauri_plugin_deep_link::DeepLinkExt;
        let scheme = crate::deeplink::deep_link_scheme(app.handle());

        let handle = app.handle().clone();
        app.deep_link().on_open_url(move |event| {
            for url in event.urls() {
                crate::deeplink::dispatch_url(&handle, scheme, url.as_str());
            }
        });

        match app.deep_link().get_current() {
            Ok(Some(urls)) => {
                let pending = app.state::<crate::deeplink::PendingDeeplinks>();
                for url in urls {
                    pending.push(url.to_string());
                }
            }
            Ok(None) => {}
            Err(err) => log::warn!("[Deeplink] get_current failed: {err}"),
        }
    }

    Ok(())
}

pub fn init_autostart(app: &tauri::AppHandle) {
    #[cfg(desktop)]
    {
        use tauri_plugin_autostart::MacosLauncher;
        use tauri_plugin_autostart::ManagerExt;

        let _ = app.plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ));

        let autostart_manager = app.autolaunch();
        log::info!(
            "current autostart status: {}",
            autostart_manager.is_enabled().unwrap_or(false)
        );
    }
}

pub fn init_late(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let launcher_coordinator = app
        .state::<Arc<crate::launcher::LauncherCoordinator>>()
        .inner()
        .clone();
    launcher_coordinator
        .mark_ready()
        .map_err(|error| format!("failed to mark launcher coordinator ready: {error}"))?;
    Ok(())
}

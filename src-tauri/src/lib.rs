use serde::Serialize;
use tauri::{Manager, Runtime};
use tauri_plugin_updater::UpdaterExt;

#[derive(Serialize)]
struct PlatformInfo {
    os: &'static str,
    arch: &'static str,
    family: &'static str,
    app_version: &'static str,
}

#[tauri::command]
fn platform_info() -> PlatformInfo {
    PlatformInfo {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        family: std::env::consts::FAMILY,
        app_version: env!("CARGO_PKG_VERSION"),
    }
}

#[tauri::command]
async fn set_invisible_mode<R: Runtime>(
    app: tauri::AppHandle<R>,
    enabled: bool,
) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window not found".to_string())?;
    window.set_always_on_top(enabled).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn window_minimize<R: Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window not found".to_string())?;
    window.minimize().map_err(|e| e.to_string())
}

#[tauri::command]
async fn window_toggle_maximize<R: Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window not found".to_string())?;
    let is_max = window.is_maximized().map_err(|e| e.to_string())?;
    if is_max {
        window.unmaximize().map_err(|e| e.to_string())
    } else {
        window.maximize().map_err(|e| e.to_string())
    }
}

#[derive(Serialize)]
struct UpdateInfo {
    available: bool,
    version: Option<String>,
    current_version: &'static str,
    notes: Option<String>,
}

#[tauri::command]
async fn check_for_update<R: Runtime>(app: tauri::AppHandle<R>) -> Result<UpdateInfo, String> {
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    match update {
        Some(u) => Ok(UpdateInfo {
            available: true,
            version: Some(u.version.clone()),
            current_version: env!("CARGO_PKG_VERSION"),
            notes: u.body.clone(),
        }),
        None => Ok(UpdateInfo {
            available: false,
            version: None,
            current_version: env!("CARGO_PKG_VERSION"),
            notes: None,
        }),
    }
}

#[tauri::command]
async fn install_update<R: Runtime>(app: tauri::AppHandle<R>) -> Result<(), String> {
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "nenhuma atualização disponível".to_string())?;
    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|_app| {
            // Linux/webkit2gtk nega getUserMedia por padrão. Interceptamos a
            // PermissionRequest e permitimos mic (necessário pra chamadas).
            #[cfg(target_os = "linux")]
            {
                if let Some(window) = _app.get_webview_window("main") {
                    let _ = window.with_webview(|wv| {
                        use webkit2gtk::{PermissionRequestExt, WebViewExt};
                        let inner: webkit2gtk::WebView = wv.inner();
                        inner.connect_permission_request(|_wv, request| {
                            request.allow();
                            true
                        });
                    });
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            platform_info,
            set_invisible_mode,
            window_minimize,
            window_toggle_maximize,
            check_for_update,
            install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

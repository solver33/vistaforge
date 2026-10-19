// No console window behind the app in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! The Vistaforge window: a thin Tauri shell over `vistaforge-bridge`. Each command here is
//! also served by the development server, with the same names and arguments
//! (`app/bridge/src/bin/devserver.rs`).

use std::path::PathBuf;
use std::sync::Arc;

use tauri::ipc::{Channel, InvokeBody, Request, Response};
use tauri::{AppHandle, Manager, State};
use vistaforge_bridge::config::{self, AppConfig};
use vistaforge_bridge::paths::{self, Instance};
use vistaforge_bridge::render;
use vistaforge_bridge::{AppInfo, Bridge, CatalogInfo, ExportOutcome};
use vistaforge_core::engine::Progress;
use vistaforge_core::settings::WorldSettings;

type Shared<'a> = State<'a, Arc<Bridge>>;

/// Runs slow work on a worker thread so the window stays responsive.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn app_info() -> AppInfo {
    vistaforge_bridge::app_info()
}

#[tauri::command]
async fn detect_instances() -> Result<Vec<Instance>, String> {
    blocking(|| Ok(paths::detect())).await
}

#[tauri::command(rename_all = "snake_case")]
async fn load_catalog(bridge: Shared<'_>, path: PathBuf) -> Result<CatalogInfo, String> {
    let bridge = bridge.inner().clone();
    blocking(move || bridge.load_catalog(&path)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn preview(
    bridge: Shared<'_>,
    settings: WorldSettings,
    res: u32,
) -> Result<Response, String> {
    let bridge = bridge.inner().clone();
    blocking(move || bridge.preview(&settings, res))
        .await
        .map(Response::new)
}

/// The scene render mode traces; `on_progress` gets (parts done, parts in all).
#[tauri::command(rename_all = "snake_case")]
async fn render_scene(
    bridge: Shared<'_>,
    settings: WorldSettings,
    request: render::SceneRequest,
    on_progress: Channel<[usize; 2]>,
) -> Result<Response, String> {
    let bridge = bridge.inner().clone();
    blocking(move || {
        bridge.render_scene(&settings, &request, &|done, total| {
            on_progress.send([done, total]).ok();
        })
    })
    .await
    .map(Response::new)
}

/// Writes a rendered picture: the PNG is the raw body, the path its `path` header in hex
/// (headers can't carry every file name as it is).
#[tauri::command]
fn save_image(request: Request<'_>) -> Result<(), String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("no image data".into());
    };
    let hex = request
        .headers()
        .get("path")
        .and_then(|v| v.to_str().ok())
        .ok_or("no file name")?;
    let path = render::hex_path(hex).ok_or("bad file name")?;
    std::fs::write(&path, bytes).map_err(|e| format!("saving {}: {e}", path.display()))
}

#[tauri::command(rename_all = "snake_case")]
fn world_exists(saves_dir: PathBuf, name: String) -> bool {
    paths::world_exists(&saves_dir, &name)
}

#[tauri::command(rename_all = "snake_case")]
async fn export_world(
    bridge: Shared<'_>,
    settings: WorldSettings,
    saves_dir: PathBuf,
    overwrite: bool,
    on_progress: Channel<Progress>,
) -> Result<ExportOutcome, String> {
    let bridge = bridge.inner().clone();
    blocking(move || {
        bridge.export(&settings, &saves_dir, overwrite, &|p| {
            on_progress.send(p).ok();
        })
    })
    .await
}

#[tauri::command]
fn cancel_export(bridge: Shared<'_>) {
    bridge.cancel_export();
}

#[tauri::command(rename_all = "snake_case")]
async fn read_settings(path: PathBuf) -> Result<WorldSettings, String> {
    blocking(move || vistaforge_bridge::read_settings(&path)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn write_settings(path: PathBuf, settings: WorldSettings) -> Result<(), String> {
    blocking(move || vistaforge_bridge::write_settings(&path, &settings)).await
}

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|dir| dir.join("config.json"))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn load_config(app: AppHandle) -> Result<AppConfig, String> {
    Ok(config::load(&config_path(&app)?))
}

#[tauri::command(rename_all = "snake_case")]
fn save_config(app: AppHandle, config: AppConfig) -> Result<(), String> {
    config::save(&config_path(&app)?, &config).map_err(|e| format!("{e:#}"))
}

#[tauri::command(rename_all = "snake_case")]
fn reveal(path: PathBuf) -> Result<(), String> {
    paths::reveal(&path).map_err(|e| format!("{e:#}"))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(Bridge::new()))
        .invoke_handler(tauri::generate_handler![
            app_info,
            detect_instances,
            load_catalog,
            preview,
            render_scene,
            save_image,
            world_exists,
            export_world,
            cancel_export,
            read_settings,
            write_settings,
            load_config,
            save_config,
            reveal,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Vistaforge");
}

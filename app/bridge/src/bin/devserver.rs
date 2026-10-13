//! Serves the app's commands over HTTP so the UI can run in a browser against the real core.
//! For development only: `npm run dev:real` starts it together with Vite, which forwards
//! `/api/*` here.
//!
//! Every command is `POST /api/<name>` with the same JSON arguments the Tauri app takes.
//! Export runs in the background: `export_world` starts it and `export_status` reports on it.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tiny_http::{Header, Request, Response, Server};
use vistaforge_bridge::config::{self, AppConfig};
use vistaforge_bridge::{paths, Bridge, ExportOutcome};
use vistaforge_core::engine::Progress;
use vistaforge_core::settings::WorldSettings;

#[derive(Default, Serialize)]
struct ExportStatus {
    running: bool,
    progress: Option<Progress>,
    outcome: Option<ExportOutcome>,
    error: Option<String>,
}

struct State {
    bridge: Bridge,
    config_path: PathBuf,
    export: Mutex<ExportStatus>,
}

#[derive(Deserialize)]
struct PathArg {
    path: PathBuf,
}

#[derive(Deserialize)]
struct PreviewArgs {
    settings: WorldSettings,
    res: u32,
}

#[derive(Deserialize)]
struct ExistsArgs {
    saves_dir: PathBuf,
    name: String,
}

#[derive(Deserialize)]
struct ExportArgs {
    settings: WorldSettings,
    saves_dir: PathBuf,
    overwrite: bool,
}

#[derive(Deserialize)]
struct WriteArgs {
    path: PathBuf,
    settings: WorldSettings,
}

#[derive(Deserialize)]
struct ConfigArgs {
    config: AppConfig,
}

enum Reply {
    Json(serde_json::Value),
    Bytes(Vec<u8>),
}

fn json<T: Serialize>(value: T) -> Result<Reply> {
    Ok(Reply::Json(serde_json::to_value(value)?))
}

fn args<T: DeserializeOwned>(body: &str) -> Result<T> {
    serde_json::from_str(if body.trim().is_empty() { "{}" } else { body }).context("bad arguments")
}

fn handle(state: &Arc<State>, command: &str, body: &str) -> Result<Reply> {
    match command {
        "app_info" => json(vistaforge_bridge::app_info()),
        "detect_instances" => json(paths::detect()),
        "load_catalog" => json(state.bridge.load_catalog(&args::<PathArg>(body)?.path)?),
        "preview" => {
            let a: PreviewArgs = args(body)?;
            Ok(Reply::Bytes(state.bridge.preview(&a.settings, a.res)?))
        }
        "world_exists" => {
            let a: ExistsArgs = args(body)?;
            json(paths::world_exists(&a.saves_dir, &a.name))
        }
        "export_world" => {
            let a: ExportArgs = args(body)?;
            *state.export.lock().unwrap() = ExportStatus {
                running: true,
                ..ExportStatus::default()
            };
            let state = state.clone();
            std::thread::spawn(move || {
                let report = |p: Progress| state.export.lock().unwrap().progress = Some(p);
                let result = state
                    .bridge
                    .export(&a.settings, &a.saves_dir, a.overwrite, &report);
                let mut status = state.export.lock().unwrap();
                status.running = false;
                match result {
                    Ok(outcome) => status.outcome = Some(outcome),
                    Err(e) => status.error = Some(format!("{e:#}")),
                }
            });
            json(())
        }
        "export_status" => json(&*state.export.lock().unwrap()),
        "cancel_export" => {
            state.bridge.cancel_export();
            json(())
        }
        "read_settings" => json(vistaforge_bridge::read_settings(
            &args::<PathArg>(body)?.path,
        )?),
        "write_settings" => {
            let a: WriteArgs = args(body)?;
            vistaforge_bridge::write_settings(&a.path, &a.settings)?;
            json(())
        }
        "load_config" => json(config::load(&state.config_path)),
        "save_config" => {
            config::save(&state.config_path, &args::<ConfigArgs>(body)?.config)?;
            json(())
        }
        "reveal" => {
            println!("reveal {}", args::<PathArg>(body)?.path.display());
            json(())
        }
        _ => anyhow::bail!("unknown command {command}"),
    }
}

fn respond(state: &Arc<State>, mut request: Request) {
    let mut body = String::new();
    let read = request.as_reader().read_to_string(&mut body);
    let url = request.url().to_string();
    let result = match (read, url.strip_prefix("/api/")) {
        (Ok(_), Some(command)) => handle(state, command, &body),
        (Err(e), _) => Err(e.into()),
        (_, None) => Err(anyhow::anyhow!("not found: {url}")),
    };
    let header = |v: &str| Header::from_bytes("Content-Type", v).unwrap();
    let sent = match result {
        Ok(Reply::Json(v)) => request.respond(
            Response::from_data(serde_json::to_vec(&v).unwrap())
                .with_header(header("application/json")),
        ),
        Ok(Reply::Bytes(b)) => {
            request.respond(Response::from_data(b).with_header(header("application/octet-stream")))
        }
        Err(e) => request.respond(
            Response::from_string(format!("{e:#}"))
                .with_status_code(400)
                .with_header(header("text/plain; charset=utf-8")),
        ),
    };
    if let Err(e) = sent {
        eprintln!("{url}: {e}");
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let port: u16 = flag("--port")
        .map(|p| p.parse())
        .transpose()?
        .unwrap_or(1421);
    let config_path = flag("--config")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("vistaforge-dev-config.json"));
    let state = Arc::new(State {
        bridge: Bridge::new(),
        config_path,
        export: Mutex::new(ExportStatus::default()),
    });
    if let Some(catalog) = flag("--catalog") {
        let info = state.bridge.load_catalog(catalog.as_ref())?;
        println!("catalog: {} blocks", info.blocks);
    }
    let server = Server::http(("127.0.0.1", port)).map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("vistaforge devserver on http://127.0.0.1:{port}");
    for request in server.incoming_requests() {
        let state = state.clone();
        std::thread::spawn(move || respond(&state, request));
    }
    Ok(())
}

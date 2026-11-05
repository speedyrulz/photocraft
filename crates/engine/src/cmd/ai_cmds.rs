//! The AI menu: commands that run the image through a ComfyUI server.
//!
//! `ai.upscale` flattens the active document to an sRGB PNG, sends it through the bundled upscale
//! workflow ([`photocraft_comfy::workflow`]) and opens what the workflow's Output Image node
//! produced as a new document, so the source is never changed. It is a background job that leaves
//! the document free: a run takes minutes, and the worker holds a copy-on-write snapshot. Cancel
//! takes the job off the server. `url` has no default, so nothing reaches a server by accident.
//!
//! `ai.comfyInfo` reports what a server offers: its models and the other choices of each option,
//! and which of the workflow's nodes it lacks.
//!
//! Untrusted automation may only use these with a server on this machine
//! (`authorize_engine_command`): the commands send the document's pixels to the address given.

use serde_json::{Map, Value, json};

use photocraft_comfy::workflow::{self, Kind};

use crate::commands::CommandSpec;
use crate::{EngineError, Result, Session};

/// The command the AI menu runs.
pub const UPSCALE: &str = "ai.upscale";
const INFO: &str = "ai.comfyInfo";

/// Parameters of `ai.upscale` that aren't in [`workflow::OPTIONS`].
const UPSCALE_KEYS: &[&str] = &["url", "seed", "sizeMode", "megapixels", "scale", "coalesce", "target"];

fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.into(), msg: msg.into() }
}

fn url_param<'a>(p: &'a Value, cmd: &str) -> Result<&'a str> {
    p.get("url").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`url` must be the address of a ComfyUI server"))
}

/// Whether `url` is a server on this machine.
pub fn is_local_server(url: &str) -> bool {
    photocraft_comfy::is_loopback_url(url)
}

/// The options of a ComfyUI workflow, for an agent choosing params.
fn options_json() -> Value {
    let defaults = workflow::defaults();
    let list: Vec<Value> = workflow::OPTIONS
        .iter()
        .map(|o| {
            let kind = match o.kind {
                Kind::Bool => "bool",
                Kind::Int { .. } => "int",
                Kind::Float { .. } => "float",
                Kind::Text | Kind::Multiline => "text",
                Kind::Choice => "choice",
            };
            json!({"key": o.key, "label": o.label, "section": o.section.label(), "kind": kind, "default": defaults.get(o.key)})
        })
        .collect();
    Value::Array(list)
}

#[cfg(not(target_arch = "wasm32"))]
fn comfy_error(e: photocraft_comfy::ComfyError) -> EngineError {
    match e {
        photocraft_comfy::ComfyError::Cancelled => EngineError::Cancelled,
        other => EngineError::Other(other.to_string()),
    }
}

fn enabled(s: &Session) -> std::result::Result<(), String> {
    if cfg!(target_arch = "wasm32") {
        return Err("AI commands need the desktop app".into());
    }
    s.active().map(|_| ()).ok_or_else(|| "no document open".to_string())
}

fn upscale(s: &mut Session, p: &Value) -> Result<Value> {
    let empty = Map::new();
    let params = p.as_object().unwrap_or(&empty);
    if let Some(key) = params.keys().find(|k| !UPSCALE_KEYS.contains(&k.as_str()) && workflow::option(k).is_none()) {
        return Err(bad(UPSCALE, format!("unknown parameter `{key}`")));
    }
    let url = photocraft_comfy::normalize_url(url_param(p, UPSCALE)?).map_err(|e| bad(UPSCALE, e.to_string()))?;
    let st = s.active().ok_or(EngineError::NoDocument)?;
    let doc = st.doc.clone();
    let (width, height) = (doc.size.width, doc.size.height);
    if u64::from(width) * u64::from(height) > photocraft_compose::MAX_RENDER_PIXELS {
        return Err(bad(UPSCALE, format!("the document's {width}×{height} canvas is too large to flatten")));
    }
    // Everything wrong with the params shows now, not after the upload.
    workflow::build(params, "input.png", width, height).map_err(|e| bad(UPSCALE, e.to_string()))?;
    run_job(s, doc, url, params.clone())
}

#[cfg(target_arch = "wasm32")]
fn run_job(_: &mut Session, _: std::sync::Arc<photocraft_doc::Document>, _: String, _: Map<String, Value>) -> Result<Value> {
    Err(EngineError::Other("AI Upscale needs the desktop app".into()))
}

/// The upload, the run and the download, on a worker; the new document on the UI thread.
#[cfg(not(target_arch = "wasm32"))]
fn run_job(s: &mut Session, doc: std::sync::Arc<photocraft_doc::Document>, url: String, params: Map<String, Value>) -> Result<Value> {
    use photocraft_comfy::client::{Client, Update};

    let name = format!("{} upscaled", crate::file_cmds::stem(&doc.name));
    let import = crate::jobs::import_options(s.prefs());
    let (width, height) = (doc.size.width, doc.size.height);
    crate::jobs::run(
        s,
        "AI Upscale",
        false,
        move |ctx| {
            let client = Client::new(&url).map_err(comfy_error)?;
            ctx.progress(0.0, "Preparing the image");
            let png = photocraft_io::export_srgb_png(&doc).map_err(|e| EngineError::Other(format!("could not prepare the image: {e}")))?;
            ctx.check()?;
            ctx.progress(0.02, "Sending the image to ComfyUI");
            let image = client.upload_png(&png).map_err(comfy_error)?;
            drop(png);
            ctx.check()?;
            let graph = workflow::build(&params, &image, width, height).map_err(comfy_error)?;
            let progress = client.open_progress();
            let id = client.queue(&graph).map_err(comfy_error)?;
            let mut report = |update: Update| match update {
                Update::Queued { ahead: 0 } => ctx.progress(0.05, "Starting on ComfyUI"),
                Update::Queued { ahead } => ctx.progress(0.05, &format!("In the ComfyUI queue: {ahead} ahead")),
                Update::Running { node, done, total } => {
                    let (fraction, message) = workflow::stage(&node, done, total);
                    ctx.progress(fraction, &message);
                }
            };
            let output = client.wait(&id, workflow::OUTPUT_NODE, progress, &|| ctx.cancelled(), &mut report).map_err(comfy_error)?;
            ctx.progress(0.95, "Fetching the result");
            let bytes = client.download(&output).map_err(comfy_error)?;
            ctx.check()?;
            ctx.progress(0.98, "Opening the result");
            let imported =
                photocraft_io::import_with_options(&output.filename, &bytes, &import).map_err(|e| crate::error::io_open_error(&output.filename, e))?;
            Ok(imported.document)
        },
        move |s, mut result| {
            result.name = name;
            let (w, h) = (result.size.width, result.size.height);
            let (document, _) = s.open_document(result, None);
            Ok(json!({"document": document, "width": w, "height": h}))
        },
    )
}

#[cfg(target_arch = "wasm32")]
fn info(_: &mut Session, _: &Value) -> Result<Value> {
    Err(EngineError::Other("AI commands need the desktop app".into()))
}

#[cfg(not(target_arch = "wasm32"))]
fn info(_: &mut Session, p: &Value) -> Result<Value> {
    use photocraft_comfy::client::Client;
    let client = Client::new(url_param(p, INFO)?).map_err(|e| bad(INFO, e.to_string()))?;
    let info = client.probe().map_err(comfy_error)?;
    Ok(json!({
        "url": client.base(),
        "version": info.version,
        "choices": info.choices,
        "missing": info.missing,
        "options": options_json(),
    }))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: UPSCALE,
            label: "AI Upscale…",
            menu: &["AI"],
            shortcut: None,
            params: "{\"url\": string (required: the ComfyUI address), \"seed\": 0-4294967295, \"sizeMode\": \"megapixels\"|\"scale\", \"megapixels\": 0.01-1000, \"scale\": 0.01-100, …the option keys `ai.comfyInfo` lists} opens the result as a new document",
            enabled,
            run: upscale,
            journal: true,
        },
        CommandSpec {
            id: INFO,
            label: "ComfyUI Server Info",
            menu: &[],
            shortcut: None,
            params: "{\"url\": string (required)} returns {version, choices, missing, options}",
            enabled: |_| if cfg!(target_arch = "wasm32") { Err("AI commands need the desktop app".into()) } else { Ok(()) },
            run: info,
            journal: false,
        },
    ]
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
#[path = "ai_cmds/tests.rs"]
mod tests;

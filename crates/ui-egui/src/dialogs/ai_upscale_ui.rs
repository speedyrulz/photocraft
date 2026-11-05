//! AI › AI Upscale…: a form over the engine's `ai.upscale`, which sends the image through the
//! bundled ComfyUI workflow.
//!
//! What most runs change is in front: where ComfyUI is, the output size, the seed and the models.
//! Everything else the workflow offers ([`workflow::OPTIONS`]) waits under Advanced, in groups, so
//! the table of options decides what's here. Fields are the command's parameters, so automation
//! can read and set them; `__status`, `__choices` and `__missing` hold what the server said.
//!
//! Opening the dialog asks the server what it has, on a worker so a server that doesn't answer
//! can't hold up a frame. The lists fill in when it answers, and without an answer the dialog still
//! works with the workflow's own values. The choices are remembered across runs (`prefs.dialogs`).

use std::sync::{Arc, Mutex, PoisonError};

use egui::{FontId, Sense, pos2, vec2};
use photocraft_comfy::workflow::{self, Kind, Opt, Section, Size};
use photocraft_engine::ai_cmds::UPSCALE as COMMAND;
use serde_json::{Map, Value, json};

use crate::PhotocraftApp;
use crate::state::DialogKind;
use crate::theme::Tokens;

const MARK: &str = "__aiUpscale";
const URL: &str = "url";
const RANDOMIZE: &str = "randomizeSeed";
const LABEL_W: f32 = 118.0;
const FIELD_W: f32 = 400.0;
const ADVANCED_H: f32 = 300.0;

pub fn owns(f: &Map<String, Value>) -> bool {
    f.contains_key(MARK)
}

/// The dialog for a parameterless `ai.upscale`; a call with params runs without one.
pub fn menu(app: &mut PhotocraftApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if id != COMMAND || params.as_object().is_some_and(|m| m.keys().any(|k| !k.starts_with("__"))) {
        return None;
    }
    Some(Ok(json!({ "dialog": open(app) })))
}

/// The defaults under what was chosen last time.
fn starting_values(app: &PhotocraftApp) -> Map<String, Value> {
    let mut f = workflow::defaults();
    f.insert(URL.into(), json!(photocraft_comfy::DEFAULT_URL));
    f.insert(RANDOMIZE.into(), json!(true));
    if let Some(Value::Object(saved)) = app.session.prefs().dialogs.get(COMMAND) {
        for (key, value) in saved {
            // Settings from a version with other options drop out.
            if f.contains_key(key) {
                f.insert(key.clone(), value.clone());
            }
        }
    }
    f
}

pub fn open(app: &mut PhotocraftApp) -> u64 {
    let mut f = starting_values(app);
    f.insert("__command".into(), json!(COMMAND));
    f.insert("__label".into(), json!("AI Upscale…"));
    f.insert(MARK.into(), json!(true));
    let size = app.session.active().map(|st| (st.doc.size.width, st.doc.size.height)).unwrap_or((0, 0));
    f.insert("__width".into(), json!(size.0));
    f.insert("__height".into(), json!(size.1));
    app.ui.open_dialog(DialogKind::Command, f)
}

/// Alt+Cancel: the workflow's own values. The server stays, and so does what it listed.
pub fn reset(f: &mut Map<String, Value>) {
    let url = f.get(URL).cloned();
    f.retain(|k, _| k.starts_with("__"));
    f.extend(workflow::defaults());
    f.insert(URL.into(), url.unwrap_or(json!(photocraft_comfy::DEFAULT_URL)));
    f.insert(RANDOMIZE.into(), json!(true));
}

/// The command's parameters: the dialog's fields without its own.
fn params(f: &Map<String, Value>) -> Map<String, Value> {
    f.iter().filter(|(k, _)| !k.starts_with("__") && k.as_str() != RANDOMIZE).map(|(k, v)| (k.clone(), v.clone())).collect()
}

pub fn confirm(app: &mut PhotocraftApp, f: &Map<String, Value>) -> Result<Value, String> {
    let mut f = f.clone();
    if f.get(RANDOMIZE).and_then(Value::as_bool).unwrap_or(false) {
        f.insert("seed".into(), json!(new_seed()));
    }
    let result = app.run(COMMAND, Value::Object(params(&f)));
    if result.is_ok() {
        // With the seed this run used, so a good result can be repeated.
        let mut remembered = params(&f);
        remembered.insert(RANDOMIZE.into(), f.get(RANDOMIZE).cloned().unwrap_or(json!(false)));
        app.session.prefs.edit(|p| p.dialogs.insert(COMMAND.into(), Value::Object(remembered)));
    }
    result
}

#[cfg(not(target_arch = "wasm32"))]
fn new_seed() -> u64 {
    photocraft_comfy::client::random_seed()
}

#[cfg(target_arch = "wasm32")]
fn new_seed() -> u64 {
    0
}

/// A probe's answer, filled in by its worker.
type Slot = Arc<Mutex<Option<Value>>>;

fn slot_id() -> egui::Id {
    egui::Id::unique("ai-upscale-probe")
}

/// Ask the server at the `url` field what it has. Replaces a probe still running: a late answer
/// for the old address lands in a slot nobody reads.
#[cfg(not(target_arch = "wasm32"))]
fn start_probe(ctx: &egui::Context, f: &mut Map<String, Value>) {
    let url = text(f, URL);
    let slot: Slot = Arc::new(Mutex::new(None));
    let (done, repaint) = (slot.clone(), ctx.clone());
    let spawned = std::thread::Builder::new().name("comfy-probe".into()).spawn(move || {
        let answer = photocraft_comfy::client::Client::new(&url).and_then(|c| c.probe());
        let answer = match answer {
            Ok(info) => json!({"version": info.version, "choices": info.choices, "missing": info.missing}),
            Err(e) => json!({"error": e.to_string()}),
        };
        *done.lock().unwrap_or_else(PoisonError::into_inner) = Some(answer);
        repaint.request_repaint();
    });
    match spawned {
        Ok(_) => {
            ctx.data_mut(|d| d.insert_temp(slot_id(), slot));
            f.insert("__probing".into(), json!(true));
            f.insert("__status".into(), json!(tl!("Connecting…")));
            f.remove("__ok");
        }
        Err(e) => {
            f.insert("__status".into(), json!(e.to_string()));
            f.insert("__ok".into(), json!(false));
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn start_probe(_: &egui::Context, f: &mut Map<String, Value>) {
    f.insert("__status".into(), json!(tl!("AI commands need the desktop app")));
    f.insert("__ok".into(), json!(false));
}

/// Take in a probe's answer when it has come.
fn finish_probe(ctx: &egui::Context, f: &mut Map<String, Value>) {
    if !f.contains_key("__probing") {
        return;
    }
    let slot: Option<Slot> = ctx.data(|d| d.get_temp(slot_id()));
    let answer = slot.and_then(|s| s.lock().unwrap_or_else(PoisonError::into_inner).take());
    let Some(answer) = answer else { return };
    f.remove("__probing");
    if let Some(error) = answer.get("error").and_then(Value::as_str) {
        f.insert("__status".into(), json!(error));
        f.insert("__ok".into(), json!(false));
        f.remove("__choices");
        f.remove("__missing");
        return;
    }
    let version = answer.get("version").and_then(Value::as_str).unwrap_or("");
    let template = "Connected to ComfyUI {version}";
    f.insert("__status".into(), json!(crate::i18n::fmt(tl!(template), &[("version", version)])));
    f.insert("__ok".into(), json!(true));
    f.insert("__choices".into(), answer.get("choices").cloned().unwrap_or(json!({})));
    f.insert("__missing".into(), answer.get("missing").cloned().unwrap_or(json!([])));
}

fn text(f: &Map<String, Value>, key: &str) -> String {
    f.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn number(f: &Map<String, Value>, key: &str) -> f64 {
    f.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn label(ui: &mut egui::Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(LABEL_W, 22.0), Sense::hover());
    // A long translation is cut, never run into the field.
    let galley = crate::widgets::tab_strip::elided(ui, text, FontId::proportional(12.0), t.text_dim, LABEL_W - 6.0);
    let at = pos2(r.right() - 6.0 - galley.size().x, r.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, t.text_dim);
}

fn tip(response: egui::Response, o: &Opt) -> egui::Response {
    if o.tip.is_empty() { response } else { response.on_hover_text(tl!(o.tip)) }
}

/// What the server lists for option `key`, with the current value first when it isn't listed.
fn choices(f: &Map<String, Value>, key: &str, current: &str) -> Vec<String> {
    let listed = f.get("__choices").and_then(|c| c.get(key)).and_then(Value::as_array);
    let mut list: Vec<String> = listed.map(|l| l.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    if !list.iter().any(|c| c == current) {
        list.insert(0, current.to_string());
    }
    list
}

/// Choices set to something the connected server doesn't list.
fn absent_choices(f: &Map<String, Value>) -> Vec<String> {
    let Some(listed) = f.get("__choices").and_then(Value::as_object) else { return Vec::new() };
    let absent = |o: &&Opt| {
        o.kind == Kind::Choice
            && listed.get(o.key).and_then(Value::as_array).is_some_and(|l| !l.iter().any(|c| c.as_str() == f.get(o.key).and_then(Value::as_str)))
    };
    workflow::OPTIONS.iter().filter(absent).map(|o| text(f, o.key)).collect()
}

/// A value as the field shows it: three decimals are all any option uses, and an `f32` field would
/// otherwise store 0.95 as 0.949999988.
fn tidy(value: f32) -> f64 {
    (f64::from(value) * 1000.0).round() / 1000.0
}

/// A whole number too large for an `f32` field.
fn long_int_field(ui: &mut egui::Ui, f: &mut Map<String, Value>, key: &str, max: u64, width: f32) -> egui::Response {
    let id = egui::Id::unique(("ai-upscale-int", key));
    let stored = f.get(key).and_then(Value::as_u64).unwrap_or(0);
    let held: Option<(u64, String)> = ui.ctx().data(|d| d.get_temp(id));
    let mut shown = held.filter(|(value, _)| *value == stored).map_or_else(|| stored.to_string(), |(_, text)| text);
    let response = crate::widgets::text_field(ui, id.with("field"), &mut shown, "", width, true);
    if response.changed() {
        shown.retain(|c| c.is_ascii_digit());
        if let Ok(value) = shown.parse::<u64>() {
            f.insert(key.into(), json!(value.min(max)));
        }
        let value = f.get(key).and_then(Value::as_u64).unwrap_or(0);
        ui.ctx().data_mut(|d| d.insert_temp(id, (value, shown)));
    }
    response
}

/// One option's row: its label and the editor its kind calls for.
fn option_row(ui: &mut egui::Ui, f: &mut Map<String, Value>, o: &Opt) {
    if o.kind == Kind::Bool {
        ui.horizontal(|ui| {
            ui.add_space(LABEL_W + ui.spacing().item_spacing.x);
            let mut on = f.get(o.key).and_then(Value::as_bool).unwrap_or(false);
            if tip(crate::widgets::checkbox(ui, &mut on, tl!(o.label)), o).changed() {
                f.insert(o.key.into(), json!(on));
            }
        });
        return;
    }
    ui.horizontal(|ui| {
        label(ui, tl!(o.label));
        // Advanced's rows are indented: they fit the column that's left.
        let width = ui.available_width().min(FIELD_W);
        match o.kind {
            Kind::Choice => {
                let mut current = text(f, o.key);
                let list = choices(f, o.key, &current);
                let options: Vec<(String, &str)> = list.iter().map(|c| (c.clone(), c.as_str())).collect();
                // The list shows a whole file name; the closed button stays in the column.
                let (rect, _) = ui.allocate_exact_size(vec2(width, 24.0), Sense::hover());
                let mut field = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
                field.set_clip_rect(rect.intersect(ui.clip_rect()));
                if crate::widgets::dropdown_literal(&mut field, &format!("ai-upscale-{}", o.key), tl!(o.label), &mut current, &options, width) {
                    f.insert(o.key.into(), json!(current));
                }
            }
            Kind::Int { max, .. } if max > 1 << 24 => {
                long_int_field(ui, f, o.key, max as u64, 180.0);
            }
            Kind::Int { min, max } => {
                let mut value = number(f, o.key) as f32;
                if tip(crate::widgets::value_field_named(ui, tl!(o.label), &mut value, min as f32..=max as f32, "", 84.0), o).changed() {
                    f.insert(o.key.into(), json!(value.round() as i64));
                }
            }
            Kind::Float { min, max } => {
                let mut value = number(f, o.key) as f32;
                if tip(crate::widgets::value_field_named(ui, tl!(o.label), &mut value, min as f32..=max as f32, "", 84.0), o).changed() {
                    f.insert(o.key.into(), json!(tidy(value)));
                }
            }
            Kind::Text => {
                let mut value = text(f, o.key);
                let response = crate::widgets::text_field(ui, egui::Id::unique(("ai-upscale", o.key)), &mut value, "", width, false);
                if tip(response, o).changed() {
                    f.insert(o.key.into(), json!(value));
                }
            }
            Kind::Multiline => {
                let mut value = text(f, o.key);
                let response = ui.add(egui::TextEdit::multiline(&mut value).desired_width(width).desired_rows(3));
                crate::field_tab::register(ui.ctx(), response.id);
                if tip(response, o).changed() {
                    f.insert(o.key.into(), json!(value));
                }
            }
            Kind::Bool => {}
        }
    });
}

fn server_rows(ui: &mut egui::Ui, f: &mut Map<String, Value>) {
    let t = Tokens::get(ui.ctx());
    let mut connect = false;
    ui.horizontal(|ui| {
        label(ui, tl!("ComfyUI address:"));
        let mut url = text(f, URL);
        let response = crate::widgets::text_field(ui, egui::Id::unique("ai-upscale-url"), &mut url, photocraft_comfy::DEFAULT_URL, FIELD_W - 92.0, false);
        if response.changed() {
            f.insert(URL.into(), json!(url));
            f.insert("__urlEdited".into(), json!(true));
        }
        if response.lost_focus() && f.remove("__urlEdited").is_some() {
            connect = true;
        }
        let probing = f.contains_key("__probing");
        connect |= ui.add_enabled_ui(!probing, |ui| crate::widgets::secondary_button(ui, tl!("Connect"), 84.0)).inner.clicked();
    });
    if connect {
        start_probe(ui.ctx(), f);
    }
    let status = text(f, "__status");
    let color = match f.get("__ok").and_then(Value::as_bool) {
        Some(true) => t.text_dim,
        Some(false) => t.danger,
        None => t.text_faint,
    };
    ui.horizontal(|ui| {
        ui.add_space(LABEL_W + ui.spacing().item_spacing.x);
        ui.set_max_width(FIELD_W);
        ui.add(egui::Label::new(egui::RichText::new(status).font(FontId::proportional(11.5)).color(color)).wrap());
    });
    let missing: Vec<String> = f.get("__missing").and_then(Value::as_array).map(|l| l.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    let absent = absent_choices(f);
    let warning = if !missing.is_empty() {
        let template = "This server lacks nodes the workflow needs: {nodes}";
        Some(crate::i18n::fmt(tl!(template), &[("nodes", missing.join(", ").as_str())]))
    } else if !absent.is_empty() {
        let template = "Not on this server: {names}";
        Some(crate::i18n::fmt(tl!(template), &[("names", absent.join(", ").as_str())]))
    } else {
        None
    };
    if let Some(warning) = warning {
        ui.horizontal(|ui| {
            ui.add_space(LABEL_W + ui.spacing().item_spacing.x);
            ui.set_max_width(FIELD_W);
            ui.add(egui::Label::new(egui::RichText::new(warning).font(FontId::proportional(11.5)).color(t.danger)).wrap());
        });
    }
}

fn output_rows(ui: &mut egui::Ui, f: &mut Map<String, Value>) {
    let t = Tokens::get(ui.ctx());
    crate::widgets::section_label(ui, tl!("Output"));
    crate::widgets::hairline(ui);
    ui.horizontal(|ui| {
        label(ui, tl!("Size:"));
        let mut mode = text(f, "sizeMode");
        let modes = [("megapixels".to_string(), tl!("Megapixels")), ("scale".to_string(), tl!("Multiple of source"))];
        if crate::widgets::dropdown(ui, "ai-upscale-size-mode", tl!("Size"), &mut mode, &modes, 150.0) {
            f.insert("sizeMode".into(), json!(mode));
        }
        if mode == "scale" {
            let mut value = number(f, "scale") as f32;
            let max = workflow::MAX_SCALE as f32;
            if crate::widgets::value_field_named(ui, tl!("Multiple of source"), &mut value, 0.01..=max, "×", 84.0).changed() {
                f.insert("scale".into(), json!(tidy(value)));
            }
        } else {
            let mut value = number(f, "megapixels") as f32;
            let max = workflow::MAX_MEGAPIXELS as f32;
            if crate::widgets::value_field_named(ui, tl!("Megapixels"), &mut value, 0.01..=max, "MP", 84.0).changed() {
                f.insert("megapixels".into(), json!(tidy(value)));
            }
        }
    });
    let (width, height) = (number(f, "__width") as u32, number(f, "__height") as u32);
    if let Ok(size) = Size::from_params(f) {
        let (w, h) = size.output(width, height, number(f, "divisibleBy").clamp(0.0, 512.0) as u32);
        let template = "{source} source → {output} px";
        let line = crate::i18n::fmt(tl!(template), &[("source", format!("{width} × {height}").as_str()), ("output", format!("{w} × {h}").as_str())]);
        ui.horizontal(|ui| {
            ui.add_space(LABEL_W + ui.spacing().item_spacing.x);
            ui.label(egui::RichText::new(line).font(FontId::proportional(11.5)).color(t.text_dim));
        });
    }
    ui.horizontal(|ui| {
        label(ui, tl!("Seed:"));
        let mut randomize = f.get(RANDOMIZE).and_then(Value::as_bool).unwrap_or(false);
        ui.add_enabled_ui(!randomize, |ui| long_int_field(ui, f, "seed", workflow::MAX_SEED, 120.0));
        if crate::widgets::checkbox(ui, &mut randomize, tl!("New seed each run")).changed() {
            f.insert(RANDOMIZE.into(), json!(randomize));
        }
    });
}

fn advanced_rows(ui: &mut egui::Ui, f: &mut Map<String, Value>) {
    egui::CollapsingHeader::new(tl!("Advanced")).id_salt("ai-upscale-advanced").default_open(false).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("ai-upscale-advanced-scroll").max_height(ADVANCED_H).auto_shrink([false, true]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            for section in Section::ADVANCED {
                egui::CollapsingHeader::new(tl!(section.label())).id_salt(("ai-upscale-section", section.label())).default_open(false).show(ui, |ui| {
                    for o in workflow::OPTIONS.iter().filter(|o| o.section == section) {
                        option_row(ui, f, o);
                    }
                });
            }
        });
    });
}

pub fn body(ui: &mut egui::Ui, f: &mut Map<String, Value>) {
    ui.spacing_mut().item_spacing.y = 6.0;
    ui.set_width(LABEL_W + FIELD_W + 12.0);
    finish_probe(ui.ctx(), f);
    if !f.contains_key("__status") {
        start_probe(ui.ctx(), f);
    }
    server_rows(ui, f);
    ui.add_space(4.0);
    output_rows(ui, f);
    ui.add_space(4.0);
    crate::widgets::section_label(ui, tl!(Section::Models.label()));
    crate::widgets::hairline(ui);
    for o in workflow::OPTIONS.iter().filter(|o| o.section == Section::Models) {
        option_row(ui, f, o);
    }
    ui.add_space(4.0);
    advanced_rows(ui, f);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> PhotocraftApp {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
        app.session.execute("file.new", json!({"width": 1000, "height": 500})).unwrap();
        app
    }

    fn fields(app: &PhotocraftApp, id: u64) -> &Map<String, Value> {
        &app.ui.dialogs.iter().find(|d| d.id == id).unwrap().fields
    }

    #[test]
    fn the_menu_item_opens_the_dialog_on_the_workflow_s_values() {
        let mut app = app();
        let ctx = egui::Context::default();
        let r = crate::menus::invoke(&mut app, &ctx, COMMAND, json!({})).unwrap();
        let id = r["dialog"].as_u64().unwrap();
        let f = fields(&app, id);
        assert_eq!(f[URL], photocraft_comfy::DEFAULT_URL);
        assert_eq!(f["megapixels"], 4);
        assert_eq!(f["steps"], 8);
        assert_eq!((f["__width"].as_u64(), f["__height"].as_u64()), (Some(1000), Some(500)));
        assert_eq!(crate::dialogs::title(app.ui.dialogs.iter().find(|d| d.id == id).unwrap()), "AI Upscale");
        crate::dialogs::cancel(&mut app, id).unwrap();
        assert!(app.ui.dialogs.is_empty());
        assert!(!app.session.prefs().dialogs.contains_key(COMMAND), "Cancel remembers nothing");
    }

    #[test]
    fn only_the_command_s_params_are_sent() {
        let mut f = workflow::defaults();
        f.insert(URL.into(), json!("x"));
        f.insert(RANDOMIZE.into(), json!(true));
        f.insert(MARK.into(), json!(true));
        f.insert("__choices".into(), json!({}));
        let p = params(&f);
        assert!(p.contains_key(URL) && p.contains_key("seed"));
        assert!(!p.keys().any(|k| k.starts_with("__") || k == RANDOMIZE));
    }

    #[test]
    fn a_failed_run_keeps_the_dialog_s_choice_of_nothing_and_says_why() {
        let mut app = app();
        // A port nothing listens on: the run fails, whether inline or as a job.
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let id = open(&mut app);
        app.ui.dialog_mut(id).unwrap().fields.insert(URL.into(), json!(format!("http://127.0.0.1:{port}")));
        let e = crate::dialogs::confirm(&mut app, id).unwrap_err();
        assert!(e.contains("Could not reach ComfyUI"), "{e}");
        assert!(!app.session.prefs().dialogs.contains_key(COMMAND));
    }

    #[test]
    fn a_new_seed_is_rolled_for_each_run_and_kept_for_the_next_open() {
        let mut app = app();
        let id = open(&mut app);
        let f = &mut app.ui.dialog_mut(id).unwrap().fields;
        f.insert(URL.into(), json!("not a url at all"));
        let fixed = fields(&app, id).clone();
        // Bad address: nothing runs, nothing is remembered, and the value isn't lost to a panic.
        assert!(confirm(&mut app, &fixed).is_err());

        let mut saved = workflow::defaults();
        saved.insert("seed".into(), json!(1234));
        saved.insert("steps".into(), json!(12));
        saved.insert(RANDOMIZE.into(), json!(false));
        saved.insert("fromAnOldVersion".into(), json!(1));
        app.session.prefs.edit(|p| p.dialogs.insert(COMMAND.into(), Value::Object(saved)));
        let id = open(&mut app);
        let f = fields(&app, id);
        assert_eq!((&f["seed"], &f["steps"], &f[RANDOMIZE]), (&json!(1234), &json!(12), &json!(false)));
        assert!(!f.contains_key("fromAnOldVersion"));
    }

    #[test]
    fn reset_restores_the_workflow_but_keeps_the_server() {
        let mut f = workflow::defaults();
        f.insert(URL.into(), json!("http://gpu:8188"));
        f.insert("steps".into(), json!(99));
        f.insert(MARK.into(), json!(true));
        f.insert("__choices".into(), json!({"unet": ["a"]}));
        reset(&mut f);
        assert_eq!(f["steps"], 8);
        assert_eq!(f[URL], "http://gpu:8188");
        assert!(owns(&f));
        assert_eq!(f["__choices"], json!({"unet": ["a"]}));
    }

    #[test]
    fn choices_keep_the_current_value_and_flag_what_the_server_lacks() {
        let mut f = workflow::defaults();
        f.insert("__choices".into(), json!({"unet": ["a.safetensors"], "vae": [f["vae"].clone()]}));
        assert_eq!(choices(&f, "unet", "a.safetensors"), ["a.safetensors"]);
        assert_eq!(choices(&f, "unet", "other"), ["other", "a.safetensors"]);
        assert_eq!(choices(&f, "lora", "only"), ["only"], "no list yet");
        assert_eq!(absent_choices(&f), [text(&f, "unet")]);
        f.insert("unet".into(), json!("a.safetensors"));
        assert!(absent_choices(&f).is_empty());
    }

    #[test]
    fn a_probe_s_answer_fills_the_lists_and_an_error_says_why() {
        let ctx = egui::Context::default();
        let answer = |value: Value, f: &mut Map<String, Value>| {
            let slot: Slot = Arc::new(Mutex::new(Some(value)));
            ctx.data_mut(|d| d.insert_temp(slot_id(), slot));
            f.insert("__probing".into(), json!(true));
            finish_probe(&ctx, f);
        };
        let mut f = Map::new();
        answer(json!({"version": "0.42.0", "choices": {"unet": ["a"]}, "missing": ["X"]}), &mut f);
        assert_eq!(f["__status"], "Connected to ComfyUI 0.42.0");
        assert_eq!(f["__ok"], true);
        assert_eq!(f["__choices"], json!({"unet": ["a"]}));
        assert_eq!(f["__missing"], json!(["X"]));
        assert!(!f.contains_key("__probing"));
        answer(json!({"error": "down"}), &mut f);
        assert_eq!(f["__status"], "down");
        assert_eq!(f["__ok"], false);
        assert!(!f.contains_key("__choices"));
    }

    #[test]
    fn the_dialog_is_gated_by_the_engine_without_a_document() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
        assert!((photocraft_engine::commands::find(COMMAND).unwrap().enabled)(&app.session).is_err());
        assert!(app.run(COMMAND, json!({"url": "http://127.0.0.1:9"})).is_err());
    }
}

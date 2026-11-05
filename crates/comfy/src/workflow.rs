//! The bundled AI Upscale workflow and the options it offers.
//!
//! `workflows/upscale.json` is a ComfyUI API-format graph. [`OPTIONS`] names what can be changed
//! in it and where, so the dialog, its defaults and the command's validation all follow one table.
//! Everything else in the graph stays as authored.

use serde_json::{Map, Value, json};

use crate::{ComfyError, Result};

const UPSCALE: &str = include_str!("../workflows/upscale.json");

/// The node that loads the source image.
pub const INPUT_NODE: &str = "343";
/// The Output Image preview node: what comes back from the server.
pub const OUTPUT_NODE: &str = "342";
const SEED_NODE: &str = "10";
const OUTPUT_SIZE_NODE: &str = "330";
const ENHANCER_NODE: &str = "129";

/// Most megapixels the output may ask for (the resize node's own limit).
pub const MAX_MEGAPIXELS: f64 = 1000.0;
/// Largest edge multiplier (the resize node's own limit).
pub const MAX_SCALE: f64 = 100.0;
/// Largest seed: the prompt enhancer's limit, the lowest of the nodes the seed reaches.
pub const MAX_SEED: u64 = u32::MAX as u64;

/// The always-visible Models group and the collapsed groups under Advanced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Models,
    Prompt,
    Enhancer,
    Sampling,
    Resize,
    Realign,
    Skin,
    Performance,
}

impl Section {
    /// The groups under Advanced, in dialog order.
    pub const ADVANCED: [Section; 7] =
        [Section::Prompt, Section::Enhancer, Section::Sampling, Section::Resize, Section::Realign, Section::Skin, Section::Performance];

    pub fn label(self) -> &'static str {
        match self {
            Section::Models => "Models",
            Section::Prompt => "Prompt",
            Section::Enhancer => "Prompt Enhancer",
            Section::Sampling => "Sampling",
            Section::Resize => "Resize",
            Section::Realign => "Realign to Source",
            Section::Skin => "Skin Texture Cleanup",
            Section::Performance => "Performance",
        }
    }
}

/// How an option is edited and checked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Bool,
    Int {
        min: i64,
        max: i64,
    },
    Float {
        min: f64,
        max: f64,
    },
    /// One line of text.
    Text,
    /// Several lines of text.
    Multiline,
    /// One of the values the server lists for this input (models, samplers, …).
    Choice,
}

/// One changeable input of the workflow.
#[derive(Clone, Copy, Debug)]
pub struct Opt {
    /// The command's parameter name.
    pub key: &'static str,
    pub label: &'static str,
    pub section: Section,
    /// The node and input it sets.
    pub node: &'static str,
    pub input: &'static str,
    pub kind: Kind,
    /// Shown on hover; empty when the label says it all.
    pub tip: &'static str,
}

const fn opt(key: &'static str, label: &'static str, section: Section, node: &'static str, input: &'static str, kind: Kind, tip: &'static str) -> Opt {
    Opt { key, label, section, node, input, kind, tip }
}

const fn int(min: i64, max: i64) -> Kind {
    Kind::Int { min, max }
}
const fn float(min: f64, max: f64) -> Kind {
    Kind::Float { min, max }
}

use Kind::{Bool, Choice, Multiline, Text};
use Section::{Enhancer, Models, Performance, Prompt, Realign, Resize, Sampling, Skin};

/// Everything the dialog offers, in display order. The source image, the seed and the output size
/// are set by [`build`]. The prompt enhancer's Prompt Count stays 1, since a batch has no single
/// result to load back, and the sparse-attention method stays as authored, since its settings
/// depend on the method.
#[rustfmt::skip]
pub const OPTIONS: &[Opt] = &[
    opt("unet", "Diffusion model", Models, "7:1", "unet_name", Choice, "The image model that redraws the picture."),
    opt("lora", "LoRA", Models, "127", "lora_name", Choice, "Loaded on top of the diffusion model. The bundled one is a speed-up, tuned for few steps."),
    opt("loraStrength", "LoRA strength", Models, "127", "strength_model", float(-100.0, 100.0), ""),
    opt("clip", "Text encoder", Models, "7:2", "clip_name", Choice, "Reads the prompt and the reference image."),
    opt("vae", "VAE", Models, "7:3", "vae_name", Choice, "Turns the model's result into pixels."),

    opt("prompt", "Prompt", Prompt, "130", "text", Multiline, "What to ask for. The prompt enhancer rewrites it from the image when it is on."),
    opt("negativePrompt", "Negative prompt", Prompt, "14", "negative_prompt", Multiline, ""),
    opt("dynamicPrompts", "Dynamic prompts", Prompt, "130", "dynamic_prompts", Bool, "{red|blue} in the prompt picks one option at random."),

    opt("promptEnhancement", "Enhance the prompt", Enhancer, "129", "Prompt Enhancement", Bool, "Off passes the prompt through as written and loads no enhancer model."),
    opt("peI2iModel", "Image model (I2I)", Enhancer, "128", "I2I GGUF", Choice, ""),
    opt("peT2iModel", "Text model (T2I)", Enhancer, "128", "T2I GGUF", Choice, ""),
    opt("peVisionModel", "Vision model", Enhancer, "128", "Vision Model", Choice, ""),
    opt("peTask", "Task", Enhancer, "129", "Task", Choice, ""),
    opt("peLanguage", "Output language", Enhancer, "129", "Output Language", Choice, ""),
    opt("peAspectRatio", "Aspect ratio", Enhancer, "129", "Aspect Ratio", Choice, ""),
    opt("peUseCache", "Reuse earlier results", Enhancer, "129", "Use Cache", Bool, "The same request and seed reuse the last enhanced prompt."),
    opt("peImageMegapixels", "Image size for the enhancer", Enhancer, "325", "total_megapixels", float(0.01, 1000.0), "Megapixels of the copy the enhancer looks at."),
    opt("peThinking", "Think first", Enhancer, "128", "Thinking", Bool, "Better prompts, slower."),
    opt("pePlanTokens", "Plan tokens", Enhancer, "128", "Plan Tokens", int(-1, 16384), "How long it may think. -1 has no limit."),
    opt("peSystemPromptFile", "System prompt file", Enhancer, "128", "System Prompt File", Text, "A path on the ComfyUI machine. Empty uses the built-in prompt."),
    opt("peContextLength", "Context length", Enhancer, "128", "Context Length", Choice, ""),
    opt("peGpuLayers", "GPU offload layers", Enhancer, "128", "GPU Offload Layers", int(-1, 256), "-1 offloads every layer."),
    opt("peUnload", "Unload after use", Enhancer, "128", "Unload Model After Generation", Bool, ""),
    opt("peSamplingPreset", "Sampling preset", Enhancer, "131", "Sampling Preset", Choice, "The values below apply with Custom."),
    opt("peTemperature", "Temperature", Enhancer, "131", "Temperature", float(0.0, 2.0), ""),
    opt("peTopP", "Top P", Enhancer, "131", "Top P", float(0.0, 1.0), ""),
    opt("peTopK", "Top K", Enhancer, "131", "Top K", int(0, 200), ""),
    opt("pePresencePenalty", "Presence penalty", Enhancer, "131", "Presence Penalty", float(-2.0, 2.0), ""),
    opt("peMaxTokens", "Max new tokens", Enhancer, "131", "Max New Tokens", int(0, 32768), "0 uses the official default."),

    opt("steps", "Steps", Sampling, "327", "steps", int(1, 10000), ""),
    opt("cfg", "CFG", Sampling, "12", "cfg", float(0.0, 100.0), ""),
    opt("denoise", "Denoise", Sampling, "327", "denoise", float(0.0, 1.0), ""),
    opt("sampler", "Sampler", Sampling, "11", "sampler_name", Choice, ""),
    opt("scheduler", "Scheduler", Sampling, "327", "scheduler", Choice, ""),

    opt("resizeMethod", "Resize method", Resize, "330", "upscale_method", Choice, "How the source is scaled before the model redraws it."),
    opt("keepProportion", "Fit", Resize, "330", "keep_proportion", Choice, "What happens to the edges when the size rounds to a multiple."),
    opt("cropPosition", "Crop position", Resize, "330", "crop_position", Choice, ""),
    opt("padColor", "Pad color", Resize, "330", "pad_color", Text, "Red, green and blue from 0 to 255, for the pad fits."),
    opt("divisibleBy", "Round size to a multiple of", Resize, "330", "divisible_by", int(0, 512), ""),
    opt("resizeDevice", "Resize on", Resize, "330", "device", Choice, ""),

    opt("realignFit", "Fit", Realign, "148", "fit", Choice, "Undoes the small drift the model adds. Affine also fixes a slight tilt."),
    opt("realignFill", "Fill empty edges", Realign, "148", "empty_fill", Choice, ""),
    opt("realignMaxZoom", "Largest zoom to undo (%)", Realign, "148", "max_zoom", float(1.0, 50.0), ""),

    opt("skinDetect", "Pattern size", Skin, "318", "detect", Choice, "Auto finds the honeycomb pattern's size. Manual uses Pattern cell."),
    opt("skinPeriod", "Pattern cell (px)", Skin, "318", "period_px", float(2.0, 256.0), ""),
    opt("skinBandWidth", "Band width", Skin, "318", "band_width", float(0.2, 3.0), "Lower keeps more real pores."),
    opt("skinStrength", "Strength", Skin, "318", "strength", float(0.0, 1.0), "0 leaves the pattern in place."),
    opt("skinGrain", "Grain", Skin, "318", "grain", float(0.0, 2.0), "Texture put back, as a share of what was removed."),
    opt("skinMask", "Apply to", Skin, "318", "mask_mode", Choice, ""),
    opt("skinThreshold", "Auto threshold", Skin, "318", "auto_threshold", float(1.0, 10.0), "Lower it if the cleanup keeps passing the image through."),
    opt("skinScreenDoor", "Screen door removal", Skin, "318", "screen_door", float(0.0, 1.0), "Removes the fine 2/4/8 px grid the VAE can leave."),
    opt("skinEdgeProtect", "Edge protection", Skin, "318", "edge_protect", float(0.0, 20.0), ""),
    opt("skinSeed", "Grain seed", Skin, "318", "seed", int(0, 9_007_199_254_740_991), ""),

    opt("unetDtype", "Model weight type", Performance, "7:1", "weight_dtype", Choice, ""),
    opt("clipDevice", "Text encoder device", Performance, "7:2", "device", Choice, ""),
    opt("cacheDevice", "Cache device", Performance, "9", "device", Choice, ""),
    opt("cacheDtype", "Cache precision", Performance, "9", "dtype", Choice, ""),
    opt("sageAttention", "Sage attention", Performance, "322", "sage_attention", Choice, ""),
    opt("sparseStart", "Sparse attention start", Performance, "329", "start_percent", float(0.0, 1.0), ""),
    opt("sparseEnd", "Sparse attention end", Performance, "329", "end_percent", float(0.0, 1.0), ""),
    opt("sparseDenseBlocks", "Dense blocks", Performance, "329", "dense_blocks", Text, "Blocks that always run dense, such as 0, 1, 47-49."),
    opt("sparseMinTokens", "Minimum tokens", Performance, "329", "min_tokens", int(0, 1_048_576), ""),
    opt("sparseExtraTokens", "Extra tokens", Performance, "329", "extra_tokens", int(0, 256), ""),
];

/// `key`'s entry in [`OPTIONS`].
pub fn option(key: &str) -> Option<&'static Opt> {
    OPTIONS.iter().find(|o| o.key == key)
}

/// The output size: a total pixel count, or a multiple of the source's edges.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    Megapixels(f64),
    Scale(f64),
}

impl Size {
    /// The requested size from the `sizeMode`, `megapixels` and `scale` parameters. Absent values
    /// take the workflow's 4 MP.
    pub fn from_params(params: &Map<String, Value>) -> Result<Size> {
        let number = |key: &str, fallback: f64| match params.get(key) {
            None | Some(Value::Null) => Ok(fallback),
            Some(v) => v.as_f64().filter(|n| n.is_finite()).ok_or_else(|| ComfyError::Param(format!("`{key}` must be a number"))),
        };
        match params.get("sizeMode").map_or(Some("megapixels"), Value::as_str) {
            Some("megapixels") => {
                let mp = number("megapixels", 4.0)?;
                if !(0.01..=MAX_MEGAPIXELS).contains(&mp) {
                    return Err(ComfyError::Param(format!("`megapixels` must be from 0.01 to {MAX_MEGAPIXELS}")));
                }
                Ok(Size::Megapixels(mp))
            }
            Some("scale") => {
                let scale = number("scale", 2.0)?;
                if !(0.01..=MAX_SCALE).contains(&scale) {
                    return Err(ComfyError::Param(format!("`scale` must be from 0.01 to {MAX_SCALE}")));
                }
                Ok(Size::Scale(scale))
            }
            _ => Err(ComfyError::Param("`sizeMode` must be \"megapixels\" or \"scale\"".into())),
        }
    }

    /// The pixels the resize node makes of a `width`×`height` source: nearest multiples of
    /// `multiple`, with the node's 1 MP = 1024×1024 px.
    pub fn output(self, width: u32, height: u32, multiple: u32) -> (u32, u32) {
        let (w, h) = (f64::from(width.max(1)), f64::from(height.max(1)));
        let (w, h) = match self {
            Size::Scale(s) => (w * s, h * s),
            Size::Megapixels(mp) => {
                let s = (mp * 1024.0 * 1024.0 / (w * h)).sqrt();
                (w * s, h * s)
            }
        };
        (round_to_multiple(w, multiple), round_to_multiple(h, multiple))
    }
}

fn round_to_multiple(value: f64, multiple: u32) -> u32 {
    if multiple > 1 {
        let m = f64::from(multiple);
        ((value / m).round() * m).clamp(m, f64::from(u32::MAX)) as u32
    } else {
        value.round().clamp(1.0, f64::from(u32::MAX)) as u32
    }
}

fn template() -> Result<Map<String, Value>> {
    match serde_json::from_str(UPSCALE) {
        Ok(Value::Object(graph)) => Ok(graph),
        _ => Err(ComfyError::Param("the bundled workflow is damaged".into())),
    }
}

fn input<'a>(graph: &'a Map<String, Value>, node: &str, input: &str) -> Option<&'a Value> {
    graph.get(node)?.get("inputs")?.get(input)
}

fn set_input(graph: &mut Map<String, Value>, node: &str, input: &str, value: Value) -> Result<()> {
    let inputs = graph.get_mut(node).and_then(|n| n.get_mut("inputs")).and_then(Value::as_object_mut);
    inputs.map(|i| i.insert(input.to_string(), value)).map(|_| ()).ok_or_else(|| ComfyError::Param(format!("the bundled workflow has no node {node}")))
}

/// The workflow's own value of every option, plus the seed and the output size.
pub fn defaults() -> Map<String, Value> {
    let Ok(graph) = template() else { return Map::new() };
    let mut d: Map<String, Value> = OPTIONS.iter().filter_map(|o| Some((o.key.to_string(), input(&graph, o.node, o.input)?.clone()))).collect();
    d.insert("seed".into(), input(&graph, SEED_NODE, "seed").cloned().unwrap_or(json!(0)));
    d.insert("sizeMode".into(), json!("megapixels"));
    d.insert("megapixels".into(), input(&graph, OUTPUT_SIZE_NODE, "total_megapixels").cloned().unwrap_or(json!(4.0)));
    d.insert("scale".into(), json!(2.0));
    d
}

/// The node classes the workflow needs, without repeats.
pub fn node_classes() -> Vec<String> {
    let Ok(graph) = template() else { return Vec::new() };
    let mut classes: Vec<String> = graph.values().filter_map(|n| n.get("class_type")?.as_str().map(str::to_string)).collect();
    classes.sort();
    classes.dedup();
    classes
}

/// The class of `node`.
pub fn node_class(node: &str) -> Option<String> {
    template().ok()?.get(node)?.get("class_type")?.as_str().map(str::to_string)
}

/// Check `value` against `o` and return what to store in the graph.
fn checked(o: &Opt, value: &Value) -> Result<Value> {
    let bad = |what: String| ComfyError::Param(format!("`{}` {what}", o.key));
    match o.kind {
        Kind::Bool => value.as_bool().map(Value::Bool).ok_or_else(|| bad("must be true or false".into())),
        Kind::Int { min, max } => {
            // A dialog's number field holds a float; a whole one is fine.
            let n = value.as_i64().or_else(|| value.as_f64().filter(|f| f.is_finite() && f.fract() == 0.0 && f.abs() < 9e15).map(|f| f as i64));
            n.filter(|n| (min..=max).contains(n)).map(|n| json!(n)).ok_or_else(|| bad(format!("must be a whole number from {min} to {max}")))
        }
        Kind::Float { min, max } => value
            .as_f64()
            .filter(|n| n.is_finite() && (min..=max).contains(n))
            .map(|_| value.clone())
            .ok_or_else(|| bad(format!("must be a number from {min} to {max}"))),
        Kind::Text | Kind::Multiline => value.as_str().filter(|s| s.len() <= 1 << 20).map(|s| json!(s)).ok_or_else(|| bad("must be text".into())),
        Kind::Choice => {
            value.as_str().filter(|s| !s.is_empty() && s.len() <= 4096).map(|s| json!(s)).ok_or_else(|| bad("must name one of the server's choices".into()))
        }
    }
}

/// The graph to queue: the bundled workflow with `params` applied and `image` (a name in the
/// server's input folder) as its source, a `width`×`height` image.
pub fn build(params: &Map<String, Value>, image: &str, width: u32, height: u32) -> Result<Map<String, Value>> {
    let mut graph = template()?;
    for o in OPTIONS {
        if let Some(value) = params.get(o.key).filter(|v| !v.is_null()) {
            set_input(&mut graph, o.node, o.input, checked(o, value)?)?;
        }
    }
    if let Some(seed) = params.get("seed").filter(|v| !v.is_null()) {
        let seed = seed.as_u64().filter(|s| *s <= MAX_SEED).ok_or_else(|| ComfyError::Param(format!("`seed` must be a whole number from 0 to {MAX_SEED}")))?;
        set_input(&mut graph, SEED_NODE, "seed", json!(seed))?;
    }
    let size = Size::from_params(params)?;
    match size {
        Size::Megapixels(mp) => {
            set_input(&mut graph, OUTPUT_SIZE_NODE, "upscale_mode", json!("total_megapixels"))?;
            set_input(&mut graph, OUTPUT_SIZE_NODE, "total_megapixels", json!(mp))?;
        }
        Size::Scale(s) => {
            set_input(&mut graph, OUTPUT_SIZE_NODE, "upscale_mode", json!("upscale_by"))?;
            set_input(&mut graph, OUTPUT_SIZE_NODE, "upscale_by", json!(s))?;
        }
    }
    // The enhancer plans for the same size, within what it accepts.
    let (w, h) = size.output(width, height, 1);
    let megapixels = (f64::from(w) * f64::from(h) / (1024.0 * 1024.0)).clamp(0.1, 16.0);
    set_input(&mut graph, ENHANCER_NODE, "Target Megapixels", json!(megapixels))?;
    set_input(&mut graph, INPUT_NODE, "image", json!(image))?;
    Ok(graph)
}

/// What the server is doing, for the job's progress: `node` is the running node, `done` of `total`
/// its own steps when it reports them.
pub fn stage(node: &str, done: f64, total: f64) -> (f32, String) {
    let part = if total > 0.0 { (done / total).clamp(0.0, 1.0) as f32 } else { 0.0 };
    match node {
        "128" | "129" => (0.10 + part * 0.15, "Enhancing the prompt".into()),
        "132" | "14" => (0.25, "Encoding the prompt".into()),
        "15" if total > 0.0 => (0.30 + part * 0.50, format!("Sampling: step {done:.0} of {total:.0}")),
        "15" => (0.30, "Sampling".into()),
        "124" => (0.82, "Decoding the image".into()),
        "318" => (0.90, "Cleaning skin texture".into()),
        "148" => (0.92, "Realigning to the source".into()),
        "342" => (0.94, "Saving the result".into()),
        "343" | "330" | "325" => (0.08, "Preparing the image".into()),
        _ => (0.09, "Loading models".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap()
    }

    #[test]
    fn the_bundled_graph_is_closed_and_ends_at_the_output_node() {
        let graph = template().unwrap();
        for (id, node) in &graph {
            assert!(node["class_type"].is_string(), "{id}");
            for (name, value) in node["inputs"].as_object().unwrap() {
                if let Some([Value::String(from), Value::Number(_)]) = value.as_array().map(Vec::as_slice) {
                    assert!(graph.contains_key(from), "{id}.{name} links to missing node {from}");
                }
            }
        }
        assert_eq!(graph[OUTPUT_NODE]["class_type"], "PreviewImage");
        // Everything is on the way to the output: no leftovers from authoring the workflow.
        let (mut reached, mut stack) = (std::collections::BTreeSet::new(), vec![OUTPUT_NODE.to_string()]);
        while let Some(id) = stack.pop() {
            if reached.insert(id.clone()) {
                let inputs = graph[&id]["inputs"].as_object().unwrap();
                stack.extend(inputs.values().filter_map(|v| v.as_array()?.first()?.as_str().map(str::to_string)));
            }
        }
        assert_eq!(reached.len(), graph.len(), "unreachable nodes in the bundled workflow");
    }

    #[test]
    fn no_text_of_an_earlier_run_is_bundled() {
        assert!(!UPSCALE.contains("text_0"), "a prompt an earlier run displayed");
        assert_eq!(template().unwrap()[INPUT_NODE]["inputs"]["image"], "input.png");
    }

    #[test]
    fn every_option_points_at_a_real_literal_input() {
        let graph = template().unwrap();
        let mut keys = std::collections::BTreeSet::new();
        for o in OPTIONS {
            assert!(keys.insert(o.key), "duplicate key {}", o.key);
            let value = input(&graph, o.node, o.input).unwrap_or_else(|| panic!("{}: no input {}.{}", o.key, o.node, o.input));
            assert!(!value.is_array(), "{} would replace a link", o.key);
            checked(o, value).unwrap_or_else(|e| panic!("the default of {} fails its own check: {e}", o.key));
        }
        for reserved in ["seed", "url", "sizeMode", "megapixels", "scale"] {
            assert!(!keys.contains(reserved));
        }
    }

    #[test]
    fn defaults_change_nothing_but_the_image_and_the_enhancer_size() {
        let built = build(&defaults(), "x.png", 1024, 1024).unwrap();
        let mut expected = template().unwrap();
        set_input(&mut expected, INPUT_NODE, "image", json!("x.png")).unwrap();
        set_input(&mut expected, ENHANCER_NODE, "Target Megapixels", json!(4.0)).unwrap();
        set_input(&mut expected, OUTPUT_SIZE_NODE, "total_megapixels", json!(4.0)).unwrap();
        assert_eq!(serde_json::to_value(&built).unwrap(), serde_json::to_value(&expected).unwrap());
    }

    #[test]
    fn the_bundled_workflow_carries_no_machine_s_paths() {
        assert_eq!(defaults()["peSystemPromptFile"], "");
        for marker in ["/home/", "/Users/", ":\\\\"] {
            assert!(!UPSCALE.contains(marker), "{marker}");
        }
    }

    #[test]
    fn params_land_on_their_nodes() {
        let p = params(json!({"seed": 77, "unet": "m.safetensors", "steps": 12, "cfg": 2.5, "prompt": "hello", "promptEnhancement": false}));
        let g = build(&p, "a.png", 100, 100).unwrap();
        assert_eq!(g["10"]["inputs"]["seed"], 77);
        assert_eq!(g["7:1"]["inputs"]["unet_name"], "m.safetensors");
        assert_eq!(g["327"]["inputs"]["steps"], 12);
        assert_eq!(g["12"]["inputs"]["cfg"], 2.5);
        assert_eq!(g["130"]["inputs"]["text"], "hello");
        assert_eq!(g["129"]["inputs"]["Prompt Enhancement"], false);
        assert_eq!(g["343"]["inputs"]["image"], "a.png");
        // Links between nodes stay links.
        assert_eq!(g["13"]["inputs"]["noise_seed"], json!(["10", 3]));
    }

    #[test]
    fn the_output_size_reaches_the_resize_node_and_the_enhancer() {
        let g = build(&params(json!({"megapixels": 8})), "a.png", 1000, 500).unwrap();
        assert_eq!(g["330"]["inputs"]["upscale_mode"], "total_megapixels");
        assert_eq!(g["330"]["inputs"]["total_megapixels"], 8.0);
        let target = g["129"]["inputs"]["Target Megapixels"].as_f64().unwrap();
        assert!((target - 8.0).abs() < 0.01, "{target}");

        let g = build(&params(json!({"sizeMode": "scale", "scale": 3})), "a.png", 1000, 500).unwrap();
        assert_eq!(g["330"]["inputs"]["upscale_mode"], "upscale_by");
        assert_eq!(g["330"]["inputs"]["upscale_by"], 3.0);
        let target = g["129"]["inputs"]["Target Megapixels"].as_f64().unwrap();
        assert!((target - 3000.0 * 1500.0 / 1048576.0).abs() < 0.01, "{target}");

        // A size past what the enhancer accepts is clamped for it, not for the output.
        let g = build(&params(json!({"megapixels": 40})), "a.png", 1000, 500).unwrap();
        assert_eq!(g["129"]["inputs"]["Target Megapixels"], 16.0);
        assert_eq!(g["330"]["inputs"]["total_megapixels"], 40.0);
    }

    #[test]
    fn the_estimated_size_matches_the_resize_node() {
        // resize_image_advanced.py: scale to the pixel count, round each edge to the multiple.
        assert_eq!(Size::Megapixels(4.0).output(1000, 1000, 32), (2048, 2048));
        assert_eq!(Size::Megapixels(4.0).output(3000, 2000, 32), (2496, 1664));
        assert_eq!(Size::Scale(2.0).output(1001, 777, 32), (2016, 1568));
        assert_eq!(Size::Scale(2.0).output(1001, 777, 0), (2002, 1554));
        assert_eq!(Size::Megapixels(0.01).output(4000, 10, 32), (2048, 32), "never below one multiple");
        assert_eq!(Size::Scale(1.0).output(0, 0, 0), (1, 1));
    }

    #[test]
    fn bad_params_are_errors() {
        for bad in [
            json!({"steps": 0}),
            json!({"steps": "many"}),
            json!({"steps": 1.5}),
            json!({"cfg": null, "denoise": 2}),
            json!({"peThinking": 1}),
            json!({"unet": ""}),
            json!({"unet": 3}),
            json!({"prompt": 3}),
            json!({"seed": -1}),
            json!({"seed": 5_000_000_000u64}),
            json!({"seed": "x"}),
            json!({"megapixels": 0}),
            json!({"megapixels": 1e308}),
            json!({"sizeMode": "huge"}),
            json!({"sizeMode": 3}),
            json!({"sizeMode": "scale", "scale": 0}),
            json!({"sizeMode": "scale", "scale": "x"}),
        ] {
            assert!(build(&params(bad.clone()), "a.png", 10, 10).is_err(), "{bad}");
        }
    }

    #[test]
    fn stages_never_run_backwards_through_a_normal_run() {
        let order = [
            ("343", 0.0, 0.0),
            ("7:1", 0.0, 0.0),
            ("129", 0.0, 0.0),
            ("14", 0.0, 0.0),
            ("15", 0.0, 8.0),
            ("15", 8.0, 8.0),
            ("124", 0.0, 0.0),
            ("318", 0.0, 0.0),
            ("148", 0.0, 0.0),
            ("342", 0.0, 0.0),
        ];
        let fractions: Vec<f32> = order.iter().map(|(n, d, t)| stage(n, *d, *t).0).collect();
        assert!(fractions.windows(2).all(|w| w[0] <= w[1]), "{fractions:?}");
        assert_eq!(stage("15", 3.0, 8.0).1, "Sampling: step 3 of 8");
        assert!(stage("15", f64::NAN, f64::NAN).0.is_finite());
    }
}

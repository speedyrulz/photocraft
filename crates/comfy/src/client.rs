//! A client for a ComfyUI server's HTTP API: upload an image, queue a graph, follow it, fetch what
//! a preview node produced.
//!
//! The endpoints: `POST /upload/image`, `POST /prompt`, `GET /history/{id}`, `GET /queue`,
//! `GET /view`, `POST /interrupt`, `GET /system_stats`, `GET /object_info/{class}`, and `/ws` for
//! live progress. `/ws` is a nicety: when it can't be reached the job still finishes, through
//! `/history`, with coarser progress. Redirects aren't followed, so an image only ever goes to
//! the address the user gave.

use std::collections::BTreeMap;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use tungstenite::{Message, WebSocket};

use crate::workflow::{self, Kind, OPTIONS};
use crate::{ComfyError, Result, url};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// A large image can take a while to upload or download; only a stalled one fails.
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(600);
/// How long `/ws` is waited on per turn of [`Client::wait`].
const TICK: Duration = Duration::from_millis(250);
/// How often the server is asked how the job is doing.
const POLL: Duration = Duration::from_secs(1);
/// Failed polls in a row before the server counts as gone.
const MAX_POLL_FAILURES: u32 = 30;
/// Polls in a row that find the job in neither the queue nor the history before it counts as lost.
const MAX_LOST_POLLS: u32 = 5;
/// The largest result fetched: far above any image the workflow can make.
const MAX_DOWNLOAD: u64 = 4 << 30;
/// The largest JSON reply read.
const MAX_JSON: u64 = 64 << 20;

/// What a server offers the bundled workflow.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    /// ComfyUI's version.
    pub version: String,
    /// The values the server lists for each [`Kind::Choice`] option, by option key.
    pub choices: BTreeMap<String, Vec<String>>,
    /// Node classes the workflow needs that the server doesn't have.
    pub missing: Vec<String>,
}

/// An image a node saved on the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageRef {
    pub filename: String,
    pub subfolder: String,
    pub kind: String,
}

/// A job's state as a person would describe it, for progress.
#[derive(Clone, Debug, PartialEq)]
pub enum Update {
    /// Waiting behind `ahead` other jobs.
    Queued { ahead: usize },
    /// Running node `node`, `done` of `total` steps when it reports them.
    Running { node: String, done: f64, total: f64 },
}

/// One ComfyUI server.
pub struct Client {
    base: String,
    id: String,
    agent: ureq::Agent,
}

fn server_error(e: impl std::fmt::Display, base: &str) -> ComfyError {
    ComfyError::Server(format!("Could not reach ComfyUI at {base}: {e}"))
}

impl Client {
    /// A client for the server at `raw` (see [`url::normalize_url`]). Connects to nothing yet.
    pub fn new(raw: &str) -> Result<Self> {
        let base = url::normalize_url(raw)?;
        let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_recv_response(Some(TRANSFER_TIMEOUT))
            .timeout_recv_body(Some(TRANSFER_TIMEOUT))
            .timeout_send_body(Some(TRANSFER_TIMEOUT))
            .http_status_as_error(false)
            .max_redirects(0)
            .proxy(None)
            .tls_config(tls)
            .user_agent("Photocraft-dev")
            .build()
            .into();
        Ok(Client { base, id: unique_id(), agent })
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    fn get_json(&self, path: &str) -> Result<Value> {
        let url = format!("{}{path}", self.base);
        let mut r = self.agent.get(&url).call().map_err(|e| server_error(e, &self.base))?;
        let status = r.status().as_u16();
        let text = r.body_mut().with_config().limit(MAX_JSON).read_to_string().map_err(|e| server_error(e, &self.base))?;
        if status != 200 {
            return Err(ComfyError::Server(format!("ComfyUI answered {path} with HTTP {status}: {}", snippet(&text))));
        }
        serde_json::from_str(&text).map_err(|_| ComfyError::Server(format!("{} doesn't look like a ComfyUI server: {path} didn't answer with JSON", self.base)))
    }

    fn post_json(&self, path: &str, body: &Value) -> Result<(u16, String)> {
        let url = format!("{}{path}", self.base);
        let bytes = serde_json::to_vec(body).map_err(|e| ComfyError::Param(e.to_string()))?;
        let mut r = self.agent.post(&url).header("Content-Type", "application/json").send(&bytes[..]).map_err(|e| server_error(e, &self.base))?;
        let status = r.status().as_u16();
        let text = r.body_mut().with_config().limit(MAX_JSON).read_to_string().map_err(|e| server_error(e, &self.base))?;
        Ok((status, text))
    }

    /// The server's version, and what it offers each option of the bundled workflow.
    pub fn probe(&self) -> Result<Info> {
        let stats = self.get_json("/system_stats")?;
        let version = stats.pointer("/system/comfyui_version").and_then(Value::as_str).unwrap_or("unknown version").to_string();
        let mut info = Info { version, ..Info::default() };
        let mut nodes: BTreeMap<String, Value> = BTreeMap::new();
        for class in workflow::node_classes() {
            let answer = self.get_json(&format!("/object_info/{}", path_encode(&class)))?;
            match answer.get(&class) {
                Some(node) => {
                    nodes.insert(class, node.clone());
                }
                None => info.missing.push(class),
            }
        }
        for o in OPTIONS.iter().filter(|o| o.kind == Kind::Choice) {
            let listed = workflow::node_class(o.node).and_then(|class| nodes.get(&class)).and_then(|node| choices_of(node, o.input));
            if let Some(list) = listed {
                info.choices.insert(o.key.to_string(), list);
            }
        }
        Ok(info)
    }

    /// Put `png` in the server's input folder and return the name a load node takes. The name
    /// comes from the contents, so running the same image again replaces its own upload instead
    /// of filling the folder.
    pub fn upload_png(&self, png: &[u8]) -> Result<String> {
        let name = format!("photocraft_{:016x}.png", fnv1a(png));
        let boundary = format!("----photocraft{}", self.id);
        let body = multipart(&boundary, &name, png);
        let url = format!("{}/upload/image", self.base);
        let mut r = self
            .agent
            .post(&url)
            .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
            .send(&body[..])
            .map_err(|e| server_error(e, &self.base))?;
        let status = r.status().as_u16();
        let text = r.body_mut().with_config().limit(MAX_JSON).read_to_string().map_err(|e| server_error(e, &self.base))?;
        if status == 413 {
            return Err(ComfyError::Server(format!(
                "The image ({} MB) is larger than ComfyUI accepts. Start ComfyUI with a larger --max-upload-size.",
                png.len() >> 20
            )));
        }
        if status != 200 {
            return Err(ComfyError::Server(format!("ComfyUI refused the image (HTTP {status}): {}", snippet(&text))));
        }
        let reply: Value = serde_json::from_str(&text).map_err(|_| ComfyError::Server("ComfyUI's upload reply wasn't JSON".into()))?;
        let stored = reply.get("name").and_then(Value::as_str).ok_or_else(|| ComfyError::Server("ComfyUI's upload reply named no file".into()))?;
        Ok(match reply.get("subfolder").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            Some(sub) => format!("{sub}/{stored}"),
            None => stored.to_string(),
        })
    }

    /// `/ws` for this client, when it can be opened: before [`Client::queue`], so no message is
    /// missed. Plain `http` servers only.
    pub fn open_progress(&self) -> Option<Progress> {
        let rest = self.base.strip_prefix("http://")?;
        let (host, port) = url::host_port(&self.base)?;
        let addr = (host.as_str(), port).to_socket_addrs().ok()?.next()?;
        let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).ok()?;
        stream.set_read_timeout(Some(CONNECT_TIMEOUT)).ok()?;
        stream.set_write_timeout(Some(CONNECT_TIMEOUT)).ok()?;
        let (socket, _) = tungstenite::client(format!("ws://{rest}/ws?clientId={}", self.id), stream).ok()?;
        socket.get_ref().set_read_timeout(Some(TICK)).ok()?;
        Some(Progress { socket })
    }

    /// Queue `graph` and return its prompt id.
    pub fn queue(&self, graph: &Map<String, Value>) -> Result<String> {
        let (status, text) = self.post_json("/prompt", &json!({"prompt": graph, "client_id": self.id}))?;
        let reply: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status != 200 {
            return Err(ComfyError::Server(queue_error(&reply).unwrap_or_else(|| format!("ComfyUI refused the workflow (HTTP {status}): {}", snippet(&text)))));
        }
        reply.get("prompt_id").and_then(Value::as_str).map(str::to_string).ok_or_else(|| ComfyError::Server("ComfyUI's reply named no prompt id".into()))
    }

    /// Follow prompt `id` until it finishes and return what node `output` saved. `cancelled` is
    /// asked every turn; when it says so the job is taken off the server. `update` hears how it's
    /// going.
    pub fn wait(
        &self,
        id: &str,
        output: &str,
        mut progress: Option<Progress>,
        cancelled: &dyn Fn() -> bool,
        update: &mut dyn FnMut(Update),
    ) -> Result<ImageRef> {
        let mut last_poll: Option<Instant> = None;
        let (mut failures, mut lost) = (0u32, 0u32);
        let mut running = false;
        loop {
            if cancelled() {
                self.stop(id);
                return Err(ComfyError::Cancelled);
            }
            match progress.as_mut().map(|p| p.next_update(id)) {
                Some(Ok(Some(u))) => {
                    running = true;
                    update(u);
                }
                Some(Ok(None)) => {}
                // The polls below carry on without it.
                Some(Err(())) => progress = None,
                None => std::thread::sleep(TICK),
            }
            if last_poll.is_some_and(|t| t.elapsed() < POLL) {
                continue;
            }
            last_poll = Some(Instant::now());
            match self.poll(id, output) {
                Ok(Poll::Done(image)) => return Ok(image),
                Ok(Poll::Failed(why)) => return Err(ComfyError::Server(why)),
                Ok(Poll::Waiting(ahead)) => {
                    failures = 0;
                    lost = 0;
                    if !running {
                        update(Update::Queued { ahead });
                    }
                }
                Ok(Poll::Started) => {
                    failures = 0;
                    lost = 0;
                }
                Ok(Poll::Lost) => {
                    failures = 0;
                    lost += 1;
                    if lost >= MAX_LOST_POLLS {
                        return Err(ComfyError::Server("ComfyUI dropped the job: it's in neither the queue nor the history. Was the server restarted?".into()));
                    }
                }
                Err(e) => {
                    failures += 1;
                    if failures >= MAX_POLL_FAILURES {
                        return Err(e);
                    }
                }
            }
        }
    }

    fn poll(&self, id: &str, output: &str) -> Result<Poll> {
        let history = self.get_json(&format!("/history/{}", path_encode(id)))?;
        if let Some(entry) = history.get(id) {
            return Ok(finished(entry, output));
        }
        let queue = self.get_json("/queue")?;
        Ok(queue_state(&queue, id))
    }

    /// The bytes of `image`, a file the server saved.
    pub fn download(&self, image: &ImageRef) -> Result<Vec<u8>> {
        let url = format!("{}/view", self.base);
        let mut r = self
            .agent
            .get(&url)
            .query("filename", &image.filename)
            .query("subfolder", &image.subfolder)
            .query("type", &image.kind)
            .call()
            .map_err(|e| server_error(e, &self.base))?;
        let status = r.status().as_u16();
        if status != 200 {
            return Err(ComfyError::Server(format!("ComfyUI couldn't send the result “{}” (HTTP {status})", image.filename)));
        }
        r.body_mut().with_config().limit(MAX_DOWNLOAD).read_to_vec().map_err(|e| server_error(e, &self.base))
    }

    /// Take prompt `id` off the server, queued or running. Best effort: the user has moved on
    /// whatever the server says.
    pub fn stop(&self, id: &str) {
        let _ = self.post_json("/queue", &json!({"delete": [id]}));
        let running =
            self.get_json("/queue").is_ok_and(|q| q.get("queue_running").and_then(Value::as_array).is_some_and(|r| r.iter().any(|e| entry_id(e) == Some(id))));
        if running {
            let _ = self.post_json("/interrupt", &json!({"prompt_id": id}));
        }
    }
}

/// `/ws`, read a little at a time.
pub struct Progress {
    socket: WebSocket<TcpStream>,
}

impl Progress {
    /// The next message about prompt `id` that arrives within a tick. `Err` when the socket has
    /// gone.
    fn next_update(&mut self, id: &str) -> std::result::Result<Option<Update>, ()> {
        match self.socket.read() {
            Ok(Message::Text(text)) => Ok(serde_json::from_str::<Value>(text.as_str()).ok().and_then(|m| update_of(&m, id))),
            Ok(_) => Ok(None),
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => Ok(None),
            Err(_) => Err(()),
        }
    }
}

/// What a `/ws` message says about prompt `id`.
fn update_of(message: &Value, id: &str) -> Option<Update> {
    let data = message.get("data")?;
    if data.get("prompt_id").and_then(Value::as_str) != Some(id) {
        return None;
    }
    let node = || data.get("node").and_then(Value::as_str).map(str::to_string);
    match message.get("type")?.as_str()? {
        "progress" => Some(Update::Running { node: node()?, done: data.get("value")?.as_f64()?, total: data.get("max")?.as_f64()? }),
        "executing" => Some(Update::Running { node: node()?, done: 0.0, total: 0.0 }),
        _ => None,
    }
}

#[derive(Debug, PartialEq)]
enum Poll {
    Done(ImageRef),
    Failed(String),
    /// In the queue, behind this many.
    Waiting(usize),
    Started,
    Lost,
}

/// A `/history/{id}` entry as an outcome.
fn finished(entry: &Value, output: &str) -> Poll {
    let status = entry.get("status");
    let failed = status.and_then(|s| s.get("status_str")).and_then(Value::as_str) == Some("error");
    if failed {
        return Poll::Failed(execution_error(status));
    }
    let image = entry.pointer(&format!("/outputs/{output}/images/0"));
    match image.and_then(|i| {
        Some(ImageRef {
            filename: i.get("filename")?.as_str()?.to_string(),
            subfolder: i.get("subfolder").and_then(Value::as_str).unwrap_or("").to_string(),
            kind: i.get("type").and_then(Value::as_str).unwrap_or("temp").to_string(),
        })
    }) {
        Some(image) => Poll::Done(image),
        None if status.and_then(|s| s.get("completed")).and_then(Value::as_bool) == Some(false) => Poll::Started,
        None => Poll::Failed("ComfyUI finished without an image from the Output Image node".into()),
    }
}

/// The failure a `/history` status reports.
fn execution_error(status: Option<&Value>) -> String {
    let messages = status.and_then(|s| s.get("messages")).and_then(Value::as_array);
    let failure = messages.and_then(|m| {
        m.iter().find_map(|entry| {
            let [kind, data] = entry.as_array()?.as_slice() else { return None };
            match kind.as_str()? {
                "execution_error" => {
                    let node = data.get("node_type").and_then(Value::as_str).unwrap_or("a node");
                    let why = data.get("exception_message").and_then(Value::as_str).unwrap_or("it failed").trim();
                    Some(format!("ComfyUI failed in {node}: {}", snippet(why)))
                }
                "execution_interrupted" => Some("The job was interrupted on the ComfyUI server".to_string()),
                _ => None,
            }
        })
    });
    failure.unwrap_or_else(|| "ComfyUI reported an error running the workflow".into())
}

/// The prompt id of a `/queue` entry, `[number, prompt_id, …]`.
fn entry_id(entry: &Value) -> Option<&str> {
    entry.get(1)?.as_str()
}

/// Where `/queue` has prompt `id`.
fn queue_state(queue: &Value, id: &str) -> Poll {
    let list = |key: &str| queue.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
    let (running, pending) = (list("queue_running"), list("queue_pending"));
    if running.iter().any(|e| entry_id(e) == Some(id)) {
        return Poll::Started;
    }
    let number = |e: &Value| e.get(0).and_then(Value::as_f64).unwrap_or(0.0);
    match pending.iter().find(|e| entry_id(e) == Some(id)) {
        Some(mine) => Poll::Waiting(running.len() + pending.iter().filter(|e| number(e) < number(mine)).count()),
        None => Poll::Lost,
    }
}

/// The reason in a refused `/prompt` reply: what's wrong with the workflow, per node.
fn queue_error(reply: &Value) -> Option<String> {
    let head = reply.pointer("/error/message").and_then(Value::as_str);
    let mut lines: Vec<String> = Vec::new();
    if let Some(nodes) = reply.get("node_errors").and_then(Value::as_object) {
        for (id, node) in nodes {
            let class = node.get("class_type").and_then(Value::as_str).unwrap_or(id);
            for e in node.get("errors").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]) {
                let message = e.get("message").and_then(Value::as_str).unwrap_or("invalid");
                let details = e.get("details").and_then(Value::as_str).unwrap_or("");
                lines.push(format!("{class}: {message} {details}").trim().to_string());
            }
        }
    }
    lines.truncate(4);
    match (head, lines.is_empty()) {
        (Some(head), true) => {
            let details = reply.pointer("/error/details").and_then(Value::as_str).filter(|d| !d.is_empty());
            Some(details.map_or_else(|| format!("ComfyUI refused the workflow: {head}"), |d| format!("ComfyUI refused the workflow: {head} {}", snippet(d))))
        }
        (_, false) => Some(format!("ComfyUI refused the workflow. {}", snippet(&lines.join("; ")))),
        (None, true) => None,
    }
}

/// The values `/object_info` lists for `input` of a node, in either form it uses: `[[a, b], {…}]`
/// or `["COMBO", {"options": [a, b]}]`.
fn choices_of(node: &Value, input: &str) -> Option<Vec<String>> {
    let spec = ["required", "optional"].iter().find_map(|section| node.get("input")?.get(section)?.get(input))?;
    let listed = match spec.get(0)? {
        Value::Array(list) => list,
        _ => spec.get(1)?.get("options")?.as_array()?,
    };
    Some(listed.iter().filter_map(Value::as_str).map(str::to_string).collect())
}

/// A long reply cut for a message.
fn snippet(text: &str) -> String {
    const MAX: usize = 300;
    let line = text.trim();
    if line.chars().count() <= MAX {
        return line.to_string();
    }
    format!("{}…", line.chars().take(MAX).collect::<String>())
}

/// `text` as one URL path segment.
fn path_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The `multipart/form-data` body of an upload.
fn multipart(boundary: &str, filename: &str, png: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(png.len() + 512);
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"{filename}\"\r\nContent-Type: image/png\r\n\r\n").as_bytes(),
    );
    body.extend_from_slice(png);
    for (name, value) in [("type", "input"), ("overwrite", "true")] {
        body.extend_from_slice(format!("\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}").as_bytes());
    }
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, b| (hash ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

/// An OS-random number: `RandomState` is seeded from the OS, and needs no dependency.
fn random_u64() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    std::collections::hash_map::RandomState::new().build_hasher().finish()
}

fn unique_id() -> String {
    format!("photocraft-{:016x}{:016x}", random_u64(), random_u64())
}

/// A seed for a new run, in the range every node takes.
pub fn random_seed() -> u64 {
    random_u64() % (workflow::MAX_SEED + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    #[test]
    fn choices_are_read_from_both_forms() {
        let node = json!({"input": {
            "required": {
                "old": [["a", "b"], {"default": "a"}],
                "new": ["COMBO", {"options": ["c", "d"]}],
                "number": ["INT", {"default": 1}],
            },
            "optional": {"device": [["cpu", "gpu"]]},
        }});
        assert_eq!(choices_of(&node, "old").unwrap(), ["a", "b"]);
        assert_eq!(choices_of(&node, "new").unwrap(), ["c", "d"]);
        assert_eq!(choices_of(&node, "device").unwrap(), ["cpu", "gpu"]);
        assert_eq!(choices_of(&node, "number"), None);
        assert_eq!(choices_of(&node, "absent"), None);
    }

    #[test]
    fn a_finished_entry_names_the_output_image() {
        let entry = json!({
            "status": {"status_str": "success", "completed": true, "messages": []},
            "outputs": {
                "132": {"text": ["hi"]},
                "342": {"images": [{"filename": "ComfyUI_temp_00001_.png", "subfolder": "", "type": "temp"}]},
            },
        });
        assert_eq!(finished(&entry, "342"), Poll::Done(ImageRef { filename: "ComfyUI_temp_00001_.png".into(), subfolder: "".into(), kind: "temp".into() }));
        assert!(matches!(finished(&entry, "999"), Poll::Failed(_)), "no image from that node");
    }

    #[test]
    fn a_failed_entry_says_why() {
        let entry = json!({"status": {"status_str": "error", "completed": false, "messages": [
            ["execution_start", {"prompt_id": "x"}],
            ["execution_error", {"node_type": "VAELoader", "exception_message": "file not found\n"}],
        ]}});
        assert_eq!(finished(&entry, "342"), Poll::Failed("ComfyUI failed in VAELoader: file not found".into()));
        let entry = json!({"status": {"status_str": "error", "messages": [["execution_interrupted", {}]]}});
        assert_eq!(finished(&entry, "342"), Poll::Failed("The job was interrupted on the ComfyUI server".into()));
        // Odd shapes still produce an answer.
        for odd in [json!({"status": {"status_str": "error", "messages": [1, [2], ["execution_error"]]}}), json!({"status": {"status_str": "error"}})] {
            assert!(matches!(finished(&odd, "342"), Poll::Failed(_)));
        }
    }

    #[test]
    fn the_queue_says_where_a_job_stands() {
        let q = json!({"queue_running": [[1, "a", {}]], "queue_pending": [[3, "c", {}], [2, "b", {}]]});
        assert_eq!(queue_state(&q, "a"), Poll::Started);
        assert_eq!(queue_state(&q, "b"), Poll::Waiting(1));
        assert_eq!(queue_state(&q, "c"), Poll::Waiting(2));
        assert_eq!(queue_state(&q, "z"), Poll::Lost);
        assert_eq!(queue_state(&json!({}), "z"), Poll::Lost);
    }

    #[test]
    fn progress_messages_are_read_for_this_job_only() {
        let m = json!({"type": "progress", "data": {"value": 3, "max": 8, "prompt_id": "a", "node": "15"}});
        assert_eq!(update_of(&m, "a"), Some(Update::Running { node: "15".into(), done: 3.0, total: 8.0 }));
        assert_eq!(update_of(&m, "b"), None);
        let m = json!({"type": "executing", "data": {"node": "124", "prompt_id": "a"}});
        assert_eq!(update_of(&m, "a"), Some(Update::Running { node: "124".into(), done: 0.0, total: 0.0 }));
        // The end of a run: `node` is null.
        assert_eq!(update_of(&json!({"type": "executing", "data": {"node": null, "prompt_id": "a"}}), "a"), None);
        assert_eq!(update_of(&json!({"type": "status", "data": {"status": {}}}), "a"), None);
        assert_eq!(update_of(&json!(3), "a"), None);
    }

    #[test]
    fn a_refused_workflow_names_the_nodes() {
        let reply = json!({
            "error": {"type": "prompt_outputs_failed_validation", "message": "Prompt outputs failed validation", "details": ""},
            "node_errors": {"7:1": {
                "class_type": "UNETLoader",
                "errors": [{"message": "Value not in list", "details": "unet_name: 'x' not in [...]"}],
            }},
        });
        assert_eq!(queue_error(&reply).unwrap(), "ComfyUI refused the workflow. UNETLoader: Value not in list unet_name: 'x' not in [...]");
        let reply = json!({"error": {"message": "Cannot execute because node X does not exist."}, "node_errors": {}});
        assert_eq!(queue_error(&reply).unwrap(), "ComfyUI refused the workflow: Cannot execute because node X does not exist.");
        assert_eq!(queue_error(&json!({})), None);
    }

    #[test]
    fn the_upload_body_is_well_formed() {
        let body = multipart("B", "a.png", b"\x89PNG");
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with("--B\r\nContent-Disposition: form-data; name=\"image\"; filename=\"a.png\"\r\nContent-Type: image/png\r\n\r\n"));
        assert!(text.contains("name=\"overwrite\"\r\n\r\ntrue\r\n"));
        assert!(text.ends_with("--B--\r\n"));
    }

    #[test]
    fn segments_are_encoded() {
        assert_eq!(path_encode("Text Multiline"), "Text%20Multiline");
        assert_eq!(path_encode("a/b?c"), "a%2Fb%3Fc");
        assert_eq!(path_encode("é"), "%C3%A9");
    }

    #[test]
    fn seeds_stay_in_range() {
        assert!((0..100).all(|_| random_seed() <= workflow::MAX_SEED));
    }

    #[test]
    fn long_text_is_cut_on_a_character_boundary() {
        assert_eq!(snippet(&"é".repeat(400)).chars().count(), 301);
    }

    /// One request as a tiny server saw it.
    #[derive(Debug)]
    struct Seen {
        line: String,
        body: Vec<u8>,
    }

    /// A one-thread server answering requests from `route` (path → status and body), recording
    /// what it was sent.
    fn server(route: impl Fn(&str) -> (u16, Vec<u8>) + Send + 'static) -> (String, Arc<Mutex<Vec<Seen>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream);
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    let mut length = 0usize;
                    loop {
                        let mut header = String::new();
                        if reader.read_line(&mut header).unwrap_or(0) == 0 || header.trim().is_empty() {
                            break;
                        }
                        if let Some(v) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                            length = v.trim().parse().unwrap_or(0);
                        }
                    }
                    let mut body = vec![0; length];
                    if reader.read_exact(&mut body).is_err() {
                        break;
                    }
                    let (status, reply) = route(line.split_whitespace().nth(1).unwrap_or(""));
                    log.lock().unwrap().push(Seen { line: line.trim().to_string(), body });
                    let head = format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n", reply.len());
                    let stream = reader.get_mut();
                    if stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(&reply)).is_err() {
                        break;
                    }
                }
            }
        });
        (url, seen)
    }

    #[test]
    fn a_whole_run_against_a_fake_server() {
        let (url, seen) = server(|path| {
            match path.split('?').next().unwrap_or("") {
            "/upload/image" => (200, br#"{"name": "stored.png", "subfolder": "", "type": "input"}"#.to_vec()),
            "/prompt" => (200, br#"{"prompt_id": "abc", "number": 1, "node_errors": {}}"#.to_vec()),
            "/history/abc" => (200, br#"{"abc": {"status": {"status_str": "success", "completed": true}, "outputs": {"342": {"images": [{"filename": "o.png", "subfolder": "", "type": "temp"}]}}}}"#.to_vec()),
            "/view" => (200, b"PNGBYTES".to_vec()),
            _ => (404, b"{}".to_vec()),
        }
        });
        let client = Client::new(&url).unwrap();
        assert_eq!(client.upload_png(b"source").unwrap(), "stored.png");
        let graph = workflow::build(&Map::new(), "stored.png", 100, 100).unwrap();
        let id = client.queue(&graph).unwrap();
        let image = client.wait(&id, workflow::OUTPUT_NODE, None, &|| false, &mut |_| {}).unwrap();
        assert_eq!(client.download(&image).unwrap(), b"PNGBYTES");

        let log = seen.lock().unwrap();
        let upload = log.iter().find(|s| s.line.starts_with("POST /upload/image")).unwrap();
        assert!(String::from_utf8_lossy(&upload.body).contains("source"));
        let queued = log.iter().find(|s| s.line.starts_with("POST /prompt")).unwrap();
        let queued: Value = serde_json::from_slice(&queued.body).unwrap();
        assert_eq!(queued["prompt"]["343"]["inputs"]["image"], "stored.png");
        assert!(queued["client_id"].as_str().unwrap().starts_with("photocraft-"));
        assert!(log.iter().any(|s| s.line.starts_with("GET /view?") && s.line.contains("filename=o.png") && s.line.contains("type=temp")));
    }

    #[test]
    fn cancelling_takes_the_job_off_the_server() {
        let (url, seen) = server(|path| match path {
            "/queue" => (200, br#"{"queue_running": [[1, "abc", {}]], "queue_pending": []}"#.to_vec()),
            _ => (200, b"{}".to_vec()),
        });
        let client = Client::new(&url).unwrap();
        let r = client.wait("abc", "342", None, &|| true, &mut |_| {});
        assert!(matches!(r, Err(ComfyError::Cancelled)));
        let log = seen.lock().unwrap();
        assert!(log.iter().any(|s| s.line.starts_with("POST /queue")), "{log:?}");
        assert!(log.iter().any(|s| s.line.starts_with("POST /interrupt")), "{log:?}");
    }

    #[test]
    fn a_job_the_server_forgot_is_reported() {
        let (url, _) = server(|path| match path {
            "/queue" => (200, br#"{"queue_running": [], "queue_pending": []}"#.to_vec()),
            _ => (200, b"{}".to_vec()),
        });
        let client = Client::new(&url).unwrap();
        let mut polls = 0;
        // Answer "not cancelled" fast enough for the test: each turn sleeps a tick.
        let r = client.wait("abc", "342", None, &|| false, &mut |_| polls += 1);
        assert!(matches!(r, Err(ComfyError::Server(m)) if m.contains("dropped")));
        assert_eq!(polls, 0);
    }

    #[test]
    fn a_refused_upload_and_a_server_that_is_not_comfy_are_errors() {
        let (url, _) = server(|path| match path {
            "/upload/image" => (413, b"too big".to_vec()),
            _ => (200, b"<html>".to_vec()),
        });
        let client = Client::new(&url).unwrap();
        assert!(matches!(client.upload_png(&[0; 8]), Err(ComfyError::Server(m)) if m.contains("--max-upload-size")));
        assert!(matches!(client.probe(), Err(ComfyError::Server(m)) if m.contains("doesn't look like a ComfyUI server")));
    }

    #[test]
    fn nothing_listening_is_a_clear_error() {
        // Bind and drop to find a port nothing listens on.
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let client = Client::new(&format!("127.0.0.1:{port}")).unwrap();
        assert!(matches!(client.probe(), Err(ComfyError::Server(m)) if m.contains("Could not reach ComfyUI")));
        assert!(client.open_progress().is_none());
    }

    #[test]
    fn a_probe_lists_choices_and_missing_nodes() {
        let (url, _) = server(|path| {
            if path == "/system_stats" {
                return (200, br#"{"system": {"comfyui_version": "0.42.0"}}"#.to_vec());
            }
            let class = path.trim_start_matches("/object_info/").replace("%20", " ");
            match class.as_str() {
                "UNETLoader" => (200, br#"{"UNETLoader": {"input": {"required": {"unet_name": [["a.safetensors", "b.safetensors"]]}}}}"#.to_vec()),
                "KSamplerSelect" => (200, br#"{"KSamplerSelect": {"input": {"required": {"sampler_name": ["COMBO", {"options": ["lcm"]}]}}}}"#.to_vec()),
                "PreviewImage" => (200, b"{}".to_vec()),
                other => (200, serde_json::to_vec(&json!({other: {"input": {}}})).unwrap()),
            }
        });
        let info = Client::new(&url).unwrap().probe().unwrap();
        assert_eq!(info.version, "0.42.0");
        assert_eq!(info.choices["unet"], ["a.safetensors", "b.safetensors"]);
        assert_eq!(info.choices["sampler"], ["lcm"]);
        assert_eq!(info.missing, ["PreviewImage"]);
    }
}

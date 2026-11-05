use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use photocraft_color::{ColorMode, SampleType};
use serde_json::json;

use crate::{EngineError, Session};

fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", json!({"width": 64, "height": 48})).unwrap();
    s
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut s = Session::new();
    s.execute("file.new", json!({"width": width, "height": height})).unwrap();
    photocraft_io::export_srgb_png(&s.active().unwrap().doc).unwrap()
}

/// A port nothing listens on.
fn closed_port() -> String {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    format!("http://127.0.0.1:{port}")
}

/// A ComfyUI that finishes every job with `result`, keeping what it was sent.
struct FakeServer {
    url: String,
    bodies: Arc<Mutex<Bodies>>,
}

/// Each request's path and body.
type Bodies = Vec<(String, Vec<u8>)>;

fn fake_server(result: Vec<u8>) -> FakeServer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let log = bodies.clone();
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
                let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                log.lock().unwrap().push((path.clone(), body));
                let reply: Vec<u8> = match path.split('?').next().unwrap_or("") {
                    "/upload/image" => br#"{"name": "stored.png", "subfolder": "", "type": "input"}"#.to_vec(),
                    "/prompt" => br#"{"prompt_id": "job", "number": 1, "node_errors": {}}"#.to_vec(),
                    "/history/job" => {
                        br#"{"job": {"status": {"status_str": "success", "completed": true}, "outputs": {"342": {"images": [{"filename": "out.png", "subfolder": "", "type": "temp"}]}}}}"#.to_vec()
                    }
                    "/view" => result.clone(),
                    _ => b"{}".to_vec(),
                };
                let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", reply.len());
                let stream = reader.get_mut();
                if stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(&reply)).is_err() {
                    break;
                }
            }
        }
    });
    FakeServer { url, bodies }
}

#[test]
fn the_result_opens_as_a_new_document_and_the_source_is_untouched() {
    let server = fake_server(png(128, 96));
    let mut s = session();
    let before = s.active().unwrap().doc.clone();
    let r = s.execute(super::UPSCALE, json!({"url": server.url, "seed": 5, "steps": 3})).unwrap();
    assert_eq!((r["width"].as_u64(), r["height"].as_u64()), (Some(128), Some(96)));
    assert_eq!(s.documents().len(), 2);
    let new = s.active().unwrap();
    assert_eq!((new.doc.size.width, new.doc.size.height), (128, 96));
    assert_eq!(new.doc.name, format!("{} upscaled", crate::file_cmds::stem(&before.name)));
    assert!(new.path.is_none(), "unsaved: File › Save asks where");

    let bodies = server.bodies.lock().unwrap();
    let (_, queued) = bodies.iter().find(|(path, _)| path == "/prompt").unwrap();
    let queued: serde_json::Value = serde_json::from_slice(queued).unwrap();
    assert_eq!(queued["prompt"]["343"]["inputs"]["image"], "stored.png");
    assert_eq!(queued["prompt"]["10"]["inputs"]["seed"], 5);
    assert_eq!(queued["prompt"]["327"]["inputs"]["steps"], 3);
    assert!(bodies.iter().any(|(path, _)| path.starts_with("/view?") && path.contains("filename=out.png")));
}

#[test]
fn a_background_job_finishes_through_poll_and_leaves_the_document_free() {
    let server = fake_server(png(32, 32));
    let mut s = session();
    let crate::jobs::Started::Job(id) = s.start(super::UPSCALE, json!({"url": server.url})).unwrap() else { panic!("not a job") };
    // The document stays editable meanwhile.
    s.execute("layer.new.layer", json!({})).unwrap();
    s.wait_job(id).unwrap();
    assert_eq!(s.documents().len(), 2);
}

#[test]
fn a_cancelled_job_opens_nothing() {
    let server = fake_server(png(32, 32));
    let mut s = session();
    let ctx = crate::jobs::JobCtx::new();
    ctx.cancel();
    s.set_inline_job_ctx(Some(ctx));
    let r = s.execute(super::UPSCALE, json!({"url": server.url}));
    assert!(matches!(r, Err(EngineError::Cancelled)), "{r:?}");
    assert_eq!(s.documents().len(), 1);
}

#[test]
fn what_is_sent_is_an_8_bit_srgb_image_with_no_transparency() {
    let mut s = Session::new();
    s.execute("file.new", json!({"width": 8, "height": 8, "depth": 16, "background": "transparent"})).unwrap();
    let doc = s.active().unwrap().doc.clone();
    assert_eq!(doc.pixel_format().sample, SampleType::U16, "the test needs a 16-bit document");
    let bytes = photocraft_io::export_srgb_png(&doc).unwrap();
    let back = photocraft_io::import("sent.png", &bytes).unwrap().document;
    assert_eq!(back.mode, ColorMode::Rgb);
    assert_eq!(back.pixel_format().sample, SampleType::U8);
    let flat = crate::file_cmds::flattened(&back, back.pixel_format());
    assert_eq!(flat.pixel(0, 0).get(..3), Some(&[1.0f32, 1.0, 1.0][..]), "transparency is matted over white");
}

#[test]
fn a_server_that_is_down_is_an_error_and_changes_nothing() {
    let mut s = session();
    let e = s.execute(super::UPSCALE, json!({"url": closed_port()})).unwrap_err().to_string();
    assert!(e.contains("Could not reach ComfyUI"), "{e}");
    assert_eq!(s.documents().len(), 1);
    let e = s.execute(super::INFO, json!({"url": closed_port()})).unwrap_err().to_string();
    assert!(e.contains("Could not reach ComfyUI"), "{e}");
}

#[test]
fn bad_params_fail_before_anything_is_sent() {
    let server = fake_server(Vec::new());
    let mut s = session();
    let url = server.url.clone();
    for bad in [
        json!({}),
        json!({"url": 3}),
        json!({"url": ""}),
        json!({"url": "ftp://x"}),
        json!({"url": url, "stpes": 3}),
        json!({"url": url, "steps": 0}),
        json!({"url": url, "seed": -1}),
        json!({"url": url, "sizeMode": "huge"}),
        json!({"url": url, "megapixels": "big"}),
    ] {
        assert!(matches!(s.execute(super::UPSCALE, bad.clone()), Err(EngineError::BadParams { .. })), "{bad}");
    }
    assert!(server.bodies.lock().unwrap().is_empty());
    assert!(s.execute(super::UPSCALE, json!(3)).is_err());
    assert!(s.execute(super::INFO, json!({})).is_err());
}

#[test]
fn it_needs_a_document() {
    let mut s = Session::new();
    assert!(s.execute(super::UPSCALE, json!({"url": "http://127.0.0.1:9"})).is_err());
}

#[test]
fn server_info_lists_every_option_for_agents() {
    let list = super::options_json();
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), photocraft_comfy::workflow::OPTIONS.len());
    assert!(list.iter().any(|o| o["key"] == "steps" && o["kind"] == "int" && o["default"] == 8));
}

#[test]
fn only_a_server_on_this_machine_counts_as_local() {
    assert!(super::is_local_server("http://127.0.0.1:8188"));
    assert!(!super::is_local_server("http://192.168.1.5:8188"));
}

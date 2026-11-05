//! ComfyUI: the bundled AI workflows and a small client for a ComfyUI server's HTTP API.
//!
//! [`workflow`] is pure data (the bundled API-format graph, the table of options a dialog shows,
//! how they map onto its nodes), so it builds everywhere. [`client`] talks to the server and exists
//! on the desktop only; the browser build's commands report that they need the desktop app.
//!
//! Standalone: no workspace dependencies, nothing here knows about documents. The engine hands it
//! a PNG and gets a PNG back.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(not(target_arch = "wasm32"))]
pub mod client;
pub mod url;
pub mod workflow;

pub use url::{DEFAULT_URL, is_loopback_url, normalize_url};

/// Why a ComfyUI request failed, in words a person can act on.
#[derive(Debug, thiserror::Error)]
pub enum ComfyError {
    /// A value this crate was given is unusable; nothing was sent.
    #[error("{0}")]
    Param(String),
    /// The server could not be reached or refused the request.
    #[error("{0}")]
    Server(String),
    /// The caller cancelled.
    #[error("cancelled")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, ComfyError>;

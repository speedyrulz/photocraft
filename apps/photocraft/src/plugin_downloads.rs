//! Plugins › Browse Plugins… downloads through `craft-update`'s HTTPS client, one background
//! thread per module. The engine checks what arrives against the catalog's size and SHA-256.

use photocraft_engine::plugin_store_cmds::{FetchFn, FetchResult};

pub fn service() -> FetchFn {
    Box::new(|url: &str, max: usize| {
        let url = url.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("plug-in download".into())
            .spawn(move || {
                let result = fetch(&url, max);
                if let Err(e) = &result {
                    log::warn!("Plug-in download {url}: {e}");
                }
                let _ = tx.send(result);
            })
            .map_err(|e| format!("Could not start the download: {e}"))?;
        Ok(rx)
    })
}

fn fetch(url: &str, max: usize) -> FetchResult {
    let http = craft_update::Http::new(&format!("PhotoCraft/{}", photocraft_engine::build_info::VERSION))?;
    http.get_bytes(url, max)?.ok_or_else(|| format!("{url} was not found"))
}

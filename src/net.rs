use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{bail, Context, Result};

/// Shared so connections (and their TLS sessions) are kept alive between
/// requests to the same host.
fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout_read(Duration::from_secs(60))
            .user_agent(concat!("hsbg-overlay/", env!("CARGO_PKG_VERSION")))
            .build()
    })
}

pub fn get_bytes(url: &str) -> Result<Vec<u8>> {
    let resp = agent().get(url).call().with_context(|| format!("GET {url}"))?;
    let mut body = Vec::new();
    resp.into_reader().take(256 << 20).read_to_end(&mut body)?;
    if body.is_empty() {
        bail!("GET {url}: empty body");
    }
    Ok(body)
}

/// Downloads to a temporary file next to `dest`, then renames it into place
/// so readers never see a partial file.
pub fn download(url: &str, dest: &Path) -> Result<()> {
    let body = get_bytes(url)?;
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Unique per call: two threads may fetch the same file at once.
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let tmp = dest.with_extension(format!("{}.{}.part", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(&tmp, &body)?;
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

//! Resumable, hash-verified downloads. No account or token is ever sent.
use overlaysubs_core::registry::ModelDef;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

/// reqwest's Display stops at "error sending request"; the cause (DNS, refused, TLS) is in the chain.
fn chain(e: reqwest::Error) -> String {
    let mut s = e.to_string();
    let mut src = std::error::Error::source(&e);
    while let Some(c) = src {
        s = format!("{s}: {c}");
        src = c.source();
    }
    s
}

pub fn fetch(m: &ModelDef, dir: &Path, mirror: &str, progress: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let dst = dir.join(&m.file);
    if dst.exists() {
        return Ok(());
    }
    let part = dir.join(format!("{}.part", m.file));
    let have = fs::metadata(&part).map(|x| x.len()).unwrap_or(0);
    let client = reqwest::blocking::Client::builder().user_agent("overlaysubs").timeout(None).build().map_err(chain)?;
    let mut req = client.get(m.url_via(mirror));
    if have > 0 {
        req = req.header("Range", format!("bytes={have}-"));
    }
    let mut resp = req.send().and_then(|r| r.error_for_status()).map_err(chain)?;
    let resumed = resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    let mut f = OpenOptions::new().create(true).write(true).append(resumed).truncate(!resumed).open(&part).map_err(|e| e.to_string())?;
    let mut done = if resumed { have } else { 0 };
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = resp.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        progress(done, m.size);
    }
    drop(f);
    // Hash the whole .part (not the stream) so a resumed download is verified end to end.
    let mut h = Sha256::new();
    let mut r = fs::File::open(&part).map_err(|e| e.to_string())?;
    loop {
        let n = r.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    let got: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if got != m.sha256 {
        let _ = fs::remove_file(&part);
        return Err(format!("{}: checksum mismatch, deleted partial file", m.file));
    }
    fs::rename(&part, &dst).map_err(|e| e.to_string())
}

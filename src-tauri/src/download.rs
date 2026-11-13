//! Resumable, hash-verified downloads. No account or token is ever sent.
use overlaysubs_core::registry::ModelDef;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

pub fn fetch(m: &ModelDef, dir: &Path, progress: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
    fetch_file(&m.url, &m.file, m.size, &m.sha256, dir, progress)
}

/// Downloads `url` to `dir/file`; a no-op when that file is already there.
pub fn fetch_file(url: &str, file: &str, size: u64, sha256: &str, dir: &Path, progress: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let dst = dir.join(file);
    if dst.exists() {
        return Ok(());
    }
    let part = dir.join(format!("{file}.part"));
    let have = fs::metadata(&part).map(|x| x.len()).unwrap_or(0);
    let client = reqwest::blocking::Client::builder().user_agent("overlaysubs").timeout(None).build().map_err(|e| e.to_string())?;
    let mut req = client.get(url);
    if have > 0 {
        req = req.header("Range", format!("bytes={have}-"));
    }
    let mut resp = req.send().and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
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
        progress(done, size);
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
    if got != sha256 {
        let _ = fs::remove_file(&part);
        return Err(format!("{file}: checksum mismatch, deleted partial file"));
    }
    fs::rename(&part, &dst).map_err(|e| e.to_string())
}

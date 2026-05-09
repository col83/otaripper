use anyhow::{bail, Context};
use indicatif::ProgressBar;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicUsize, Ordering};

const MAX_RETRIES: usize = 10;
const CHUNK_SIZE: usize = 64 * 1024; // 64KB network reads
const CACHE_SIZE: u64 = 256 * 1024;  // 256KB head/tail ZIP pre fetch

// global bandwidth tracker (shows exactly how much data we saved the user)
pub static NETWORK_BYTES_READ: AtomicUsize = AtomicUsize::new(0);

/// pulls a specific partition chunk over HTTP with aggressive retries
/// Fakes the progress bar bytes so it matches the uncompressed disk writes (keeps UI smooth)
pub fn fetch_http_chunk(
    client: &reqwest::blocking::Client,
    url: &str,
    start: u64,
    size: usize,
    pb: &ProgressBar,
    total_dst_size: usize,
) -> anyhow::Result<Vec<u8>> {
    // prevent u64 underflow panic on 0 byte operations
    if size == 0 {
        return Ok(Vec::new());
    }
    let end = start + size as u64 - 1;
    let mut buf = vec![0u8; size];
    
    let mut total_read = 0;
    let mut ui_reported = 0u64;

    // how much bigger is the uncompressed data vs the network chunk?
    let ratio = if size > 0 { total_dst_size as f64 / size as f64 } else { 1.0 };

    let mut attempts = 0;
    while total_read < size {
        attempts += 1;
        let req_start = start + total_read as u64;

        match client.get(url).header("Range", format!("bytes={}-{}", req_start, end)).send() {
            Ok(mut resp) => {
                // check If the server returns 200 OK, it ignored our Range request
                // and is trying to send the entire ROM, Bail gracefully!
                if resp.status() == reqwest::StatusCode::OK {
                    bail!("The remote server does not support HTTP Range requests, Streaming is impossible!");
                } else if !resp.status().is_success() {
                    if attempts >= MAX_RETRIES { bail!("Server rejected range request: {}", resp.status()); }
                } else {
                    let mut chunk = [0u8; CHUNK_SIZE];
                    
                    loop {
                        let n = match resp.read(&mut chunk) {
                            Ok(0) => break, // EOF
                            Ok(n) => n,
                            Err(e) => {
                                if attempts >= MAX_RETRIES { bail!("Connection died and ran out of retries: {}", e); }
                                break;
                            }
                        };

                        // prevent buffer overflows if a rogue server sends too much data
                        if total_read + n > size {
                            bail!("Rogue server sent more data than requested, Aborting to prevent memory corruption!");
                        }

                        buf[total_read..total_read + n].copy_from_slice(&chunk[..n]);
                        total_read += n;
                        NETWORK_BYTES_READ.fetch_add(n, Ordering::Relaxed);

                        // Sync UI in real time
                        let expected = (total_read as f64 * ratio) as u64;
                        let diff = expected.saturating_sub(ui_reported);
                        if diff > 0 {
                            pb.inc(diff);
                            ui_reported += diff;
                        }
                    }
                }
            }
            Err(e) if attempts >= MAX_RETRIES => bail!("Network timeout/error: {}", e),
            _ => {}
        }
        
        // give the connection a breather
        if total_read < size { std::thread::sleep(std::time::Duration::from_millis(500)); }
    }

    // Snap the UI to the exact destination size to avoid 99% hanging
    let diff = (total_dst_size as u64).saturating_sub(ui_reported);
    if diff > 0 { pb.inc(diff); }

    Ok(buf)
}

/// instant start HTTP reader, that pre-fetches the head and tail of the remote ZIP
pub struct CachingHttpReader {
    client: reqwest::blocking::Client,
    url: String,
    length: u64,
    pos: u64,
    head_buf: Vec<u8>,
    tail_buf: Vec<u8>,
    tail_start: u64,
}

impl CachingHttpReader {
    pub fn new(client: reqwest::blocking::Client, url: &str) -> anyhow::Result<Self> {
        let resp = client.head(url).send()?;
        if !resp.status().is_success() {
            bail!("Failed to access URL: {}", resp.status());
        }
        
        let length = resp.headers()
            .get("Content-Length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .context("Server didn't return Content-Length, can't seek this remote file.")?;

        // first, Snag the Payload headers (Head)
        let head_size = CACHE_SIZE.min(length);
        let mut head_buf = Vec::new();
        if head_size > 0 {
            let mut req = client.get(url).header("Range", format!("bytes=0-{}", head_size - 1)).send()?;
            if req.status() == reqwest::StatusCode::OK {
                bail!("The remote server does not support HTTP Range requests, Streaming is impossible!");
            } else if req.status().is_success() {
                if let Ok(n) = req.read_to_end(&mut head_buf) {
                    NETWORK_BYTES_READ.fetch_add(n, Ordering::Relaxed);
                }
            }
        }

        // second, Snag the Zip Central Directory (Tail)
        let tail_size = CACHE_SIZE.min(length);
        let tail_start = length.saturating_sub(tail_size).max(head_buf.len() as u64);
        let mut tail_buf = Vec::new();
        if tail_start < length {
            let mut req = client.get(url).header("Range", format!("bytes={}-{}", tail_start, length - 1)).send()?;
            if req.status() == reqwest::StatusCode::OK {
                bail!("The remote server does not support HTTP Range requests, Streaming is impossible!");
            } else if req.status().is_success() {
                if let Ok(n) = req.read_to_end(&mut tail_buf) {
                    NETWORK_BYTES_READ.fetch_add(n, Ordering::Relaxed);
                }
            }
        }

        Ok(Self { client, url: url.to_string(), length, pos: 0, head_buf, tail_buf, tail_start })
    }
}

impl Read for CachingHttpReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.length || buf.is_empty() { return Ok(0); }
        
        let end = (self.pos + buf.len() as u64 - 1).min(self.length - 1);
        let len_to_copy = (end - self.pos + 1) as usize;

        // Head cache hit...
        if self.pos < self.head_buf.len() as u64 && end < self.head_buf.len() as u64 {
            let p = self.pos as usize;
            buf[..len_to_copy].copy_from_slice(&self.head_buf[p..p + len_to_copy]);
            self.pos += len_to_copy as u64;
            return Ok(len_to_copy);
        }

        // Tail cache hit...
        if self.pos >= self.tail_start && end < self.length {
            let offset = (self.pos - self.tail_start) as usize;
            buf[..len_to_copy].copy_from_slice(&self.tail_buf[offset..offset + len_to_copy]);
            self.pos += len_to_copy as u64;
            return Ok(len_to_copy);
        }

        // cache miss --> Network roundtrip
        let mut resp = self.client.get(&self.url)
            .header("Range", format!("bytes={}-{}", self.pos, end))
            .send()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        let n = resp.read(buf)?;
        NETWORK_BYTES_READ.fetch_add(n, Ordering::Relaxed);
        
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for CachingHttpReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.pos = match pos {
            SeekFrom::Start(p) => p,
            SeekFrom::End(p) => (self.length as i64 + p) as u64,
            SeekFrom::Current(p) => (self.pos as i64 + p) as u64,
        };
        Ok(self.pos)
    }
}
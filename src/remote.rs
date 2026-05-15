use anyhow::{bail, Context};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicUsize, Ordering};

const MAX_RETRIES: usize = 10;
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
    let mut buf = vec![0u8; size];
    let ratio = if size > 0 { total_dst_size as f64 / size as f64 } else { 1.0 };

    // Parallel chunking For heavy payloads, we split the single HTTP request into
    // multiple 8MB parallel Range requests to saturate the bandwidth!
    let part_size = 8 * 1024 * 1024;

    if size > part_size {
        let results: Vec<anyhow::Result<()>> = buf
            .par_chunks_mut(part_size)
            .enumerate()
            .map(|(i, chunk_slice)| {
                let chunk_start = start + (i *part_size) as u64;
                let chunk_end = chunk_start + chunk_slice.len() as u64 - 1;
                fetch_range_with_retries(client, url, chunk_start, chunk_end, chunk_slice, pb, ratio)
            })
            .collect();
        
        for res in results {
            res?;
        }
    } else {
        // normal fast path for smaller operations like where the overhead of parallelism isn't worth it
        fetch_range_with_retries(client, url, start, start+ size as u64 - 1, &mut buf, pb, ratio)?;
    }

    Ok(buf)
}

fn fetch_range_with_retries(
    client: &reqwest::blocking::Client,
    url: &str,
    start: u64,
    end: u64,
    buf: &mut [u8],
    pb: &ProgressBar,
    ratio: f64,
) -> anyhow::Result<()> {
    let size = buf.len();
    let mut total_read = 0;
    let mut ui_reported = 0u64;
    let mut attempts = 0;
    
    // massive fr 256KB read buffer to prevent loop/syscall overhead
    let mut chunk = vec![0u8; 256 * 1024];

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
    Ok(())
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
    pub fn new(client: reqwest::blocking::Client, url: &str, pb: &ProgressBar) -> anyhow::Result<Self> {
        pb.set_message("Connecting to remote server...");
        let resp = client.head(url).send()?;
        if !resp.status().is_success() {
            bail!("Failed to access URL: {}", resp.status());
        }
        
        let length = resp.headers()
            .get("Content-Length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .context("Server didn't return Content-Length, can't seek this remote file.")?;

        let head_size = CACHE_SIZE.min(length);
        let tail_size = CACHE_SIZE.min(length);
        let tail_start = length.saturating_sub(tail_size).max(head_size);

        // upgrade the spinner to show realtime byte progress (makes it feel more responsive)
        pb.set_length(head_size + tail_size);
        pb.set_style(
            ProgressStyle::with_template("{spinner:.cyan.bold} {msg} [{bar:30.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec})")
                .unwrap()
                .progress_chars("=> ")
        );

        // first, Snag the Payload headers (Head)
        pb.set_message("Fetching headers...");
        let mut head_buf = Vec::with_capacity(head_size as usize);
        if head_size > 0 {
            let mut req = client.get(url).header("Range", format!("bytes=0-{}", head_size - 1)).send()?;
            if req.status() == reqwest::StatusCode::OK {
                bail!("The remote server does not support HTTP Range requests, Streaming is impossible!");
            } else if req.status().is_success() {
                let mut chunk = vec![0u8; 128 * 1024]; // 128kb burst read
                loop {
                    let n = match req.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(_) => break,
                    };
                    head_buf.extend_from_slice(&chunk[..n]);
                    NETWORK_BYTES_READ.fetch_add(n, Ordering::Relaxed);
                    pb.inc(n as u64);
                }
            }
        }

        // second, Snag the Zip Central Directory (Tail)
        pb.set_message("Fetching ZIP directory...");
        let mut tail_buf = Vec::with_capacity(tail_size as usize);
        if tail_start < length {
            let mut req = client.get(url).header("Range", format!("bytes={}-{}", tail_start, length - 1)).send()?;
            if req.status() == reqwest::StatusCode::OK {
                bail!("The remote server does not support HTTP Range requests, Streaming is impossible!");
            } else if req.status().is_success() {
                let mut chunk = vec![0u8; 128 * 1024];
                loop {
                    let n = match req.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(_) => break,
                    };
                    tail_buf.extend_from_slice(&chunk[..n]);
                    NETWORK_BYTES_READ.fetch_add(n, Ordering::Relaxed);
                    pb.inc(n as u64);
                }
            }
        }

        // Snap tail_start down if head buffer was actually smaller than we think
        let actual_tail_start = length.saturating_sub(tail_size).max(head_buf.len() as u64);

        Ok(Self { client, url: url.to_string(), length, pos: 0, head_buf, tail_buf, tail_start: actual_tail_start })
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
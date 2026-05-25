use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::Path;
use zip::ZipArchive;

pub fn fetch_metadata(path_str: &str) -> Option<HashMap<String, String>> {
    if path_str.starts_with("http://") || path_str.starts_with("https://") {
        #[cfg(feature = "remote")]
        {
            return fetch_remote(path_str);
        }
        #[cfg(not(feature = "remote"))]
        {
            return None;
        }
    }

    let path = Path::new(path_str);
    let mut file = File::open(path).ok()?;

    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).ok()?;
    file.seek(std::io::SeekFrom::Start(0)).ok()?;

    if &magic == b"PK\x03\x04" {
        let mut archive = ZipArchive::new(&file).ok()?;
        return extract_metadata_from_zip(&mut archive);
    }

    None
}

#[cfg(feature = "remote")]
fn fetch_remote(url: &str) -> Option<HashMap<String, String>> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()
        .ok()?;

    // Suppress the spinner to keep it clean and inline with normal extraction
    let spinner = indicatif::ProgressBar::hidden();

    let mut reader = crate::remote::CachingHttpReader::new(client, url, &spinner).ok()?;

    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic).ok()?;
    reader.seek(std::io::SeekFrom::Start(0)).ok()?;

    if &magic == b"PK\x03\x04" {
        let mut archive = ZipArchive::new(&mut reader).ok()?;
        return extract_metadata_from_zip(&mut archive);
    }

    None
}

fn extract_metadata_from_zip<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
) -> Option<HashMap<String, String>> {
    let mut metadata = HashMap::new();

    // Try to read META-INF/com/android/metadata
    if let Ok(mut file) = archive.by_name("META-INF/com/android/metadata") {
        let mut content = String::new();
        if file.read_to_string(&mut content).is_ok() {
            parse_properties(&content, &mut metadata);
        }
    }

    // Try to read payload_properties.txt
    if let Ok(mut file) = archive.by_name("payload_properties.txt") {
        let mut content = String::new();
        if file.read_to_string(&mut content).is_ok() {
            parse_properties(&content, &mut metadata);
        }
    }

    if metadata.is_empty() {
        None
    } else {
        Some(metadata)
    }
}

fn parse_properties(content: &str, map: &mut HashMap<String, String>) {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(idx) = line.find('=') {
            let key = line[..idx].trim().to_string();
            let value = line[idx + 1..].trim().to_string();
            map.insert(key, value);
        }
    }
}

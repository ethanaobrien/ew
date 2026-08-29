use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Read, Seek, Write};
use zip::write::SimpleFileOptions;

use super::{song_path, LEVEL_COUNT};

// Export packages carry the ORIGINAL upload artifacts. SIF1 is the canonical
// interchange format - the transcoded NoteData charts are derived data and are
// never exported. Importing a package on another ew server replays the exact
// same upload pipeline the multipart form uses. Layout of the zip:
//   manifest.json        upload metadata, same schema as the multipart fields
//   jacket               original jacket image bytes (png/jpg)
//   audio                original audio bytes (ogg/mp3/wav)
//   chart_{level}.json   SIF1-schema charts, level 1..4 (only uploaded levels)
// visibility/shared_with/downloads_disabled are per-server settings and are
// deliberately NOT part of the package.

pub fn build(music_id: i64) -> Result<Vec<u8>, String> {
    // Songs uploaded before export support have no original artifacts on disk
    let manifest = fs::read(song_path(music_id, "original/manifest.json"))
        .map_err(|_| String::from("This song was uploaded before export support and can't be downloaded"))?;

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default();
    let mut add = |name: &str, bytes: &[u8]| -> Result<(), String> {
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        zip.write_all(bytes).map_err(|e| e.to_string())
    };

    add("manifest.json", &manifest)?;
    for name in ["jacket", "audio"] {
        let bytes = fs::read(song_path(music_id, &format!("original/{}", name))).map_err(|e| e.to_string())?;
        add(name, &bytes)?;
    }
    for level in 1..=LEVEL_COUNT {
        let Ok(bytes) = fs::read(song_path(music_id, &format!("original/chart_{}.json", level))) else { continue; };
        add(&format!("chart_{}.json", level), &bytes)?;
    }

    Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
}

// Reads one entry, capped. Deflate's ceiling is about 1032:1, so an uncapped
// read_to_end here is a zip bomb: a one-megabyte entry inflates to a gigabyte and
// grows the Vec until the allocator or the OOM killer stops it. Every entry is
// bounded by the upload form's own per-file cap, and by what is left of the
// per-request budget across all entries.
//
// The central directory's declared size rejects the obvious case without
// inflating anything; take(cap + 1) makes that declaration untrusted - a lying
// header runs out of budget one byte past the cap and stops there.
//
// Ok(None) is "the package does not carry this entry", which is a normal outcome
// for the optional ones
fn read_entry<R: Read + Seek>(archive: &mut zip::ZipArchive<R>, name: &str, remaining: &mut usize) -> Result<Option<Vec<u8>>, String> {
    let Ok(mut file) = archive.by_name(name) else {
        return Ok(None);
    };
    let cap = std::cmp::min(super::MAX_FILE_BYTES, *remaining);
    // Which limit the entry actually ran into, so the message names the right one
    let too_big = if cap >= super::MAX_FILE_BYTES {
        super::over_file_limit(name)
    } else {
        super::over_request_limit()
    };
    if file.size() > cap as u64 {
        return Err(too_big);
    }
    let mut bytes = Vec::new();
    file.by_ref().take(cap as u64 + 1).read_to_end(&mut bytes)
        .map_err(|_| format!("Package entry '{}' could not be read", name))?;
    if bytes.len() > cap {
        return Err(too_big);
    }
    *remaining -= bytes.len();
    Ok(Some(bytes))
}

// Expands a package into the same field map the upload form produces - the zip
// contents map 1:1 onto the multipart fields, so an import is just an upload
// with its fields sourced from the package. Fields already present in the map
// that the package also carries are overwritten (the package's metadata wins);
// visibility/shared_with/downloads_disabled aren't packaged and stay untouched
pub fn expand(package: &[u8], fields: &mut HashMap<String, Vec<u8>>) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(package)).map_err(|_| String::from("Package is not a valid zip file"))?;
    // The decompressed budget for the whole package, shared by every entry
    let mut remaining = super::MAX_REQUEST_BYTES;

    let manifest = read_entry(&mut archive, "manifest.json", &mut remaining)?.ok_or(String::from("Package has no manifest.json"))?;
    let manifest = jzon::parse(&String::from_utf8_lossy(&manifest)).map_err(|_| String::from("Package manifest is not valid JSON"))?;
    if manifest["format"].as_i64() != Some(1) {
        return Err(String::from("Unsupported package format"));
    }

    for key in ["name", "name_en", "short_name", "kana", "artist", "artist_en", "band_category", "attribute", "bpm", "preview_start_sec", "preview_length_sec"] {
        if !manifest[key].is_null() {
            fields.insert(key.to_string(), manifest[key].to_string().into_bytes());
        }
    }
    for data in manifest["levels"].members() {
        let Some(level) = data["level"].as_i64() else { continue; };
        if !data["level_number"].is_null() {
            fields.insert(format!("level_number_{}", level), data["level_number"].to_string().into_bytes());
        }
    }

    fields.insert(String::from("jacket"), read_entry(&mut archive, "jacket", &mut remaining)?.ok_or(String::from("Package has no jacket"))?);
    fields.insert(String::from("audio"), read_entry(&mut archive, "audio", &mut remaining)?.ok_or(String::from("Package has no audio"))?);
    let mut has_chart = false;
    for level in 1..=LEVEL_COUNT {
        if let Some(chart) = read_entry(&mut archive, &format!("chart_{}.json", level), &mut remaining)? {
            fields.insert(format!("chart_{}", level), chart);
            has_chart = true;
        }
    }
    if !has_chart {
        return Err(String::from("Package has no charts"));
    }
    Ok(())
}

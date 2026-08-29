use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Read, Seek, Write};
use zip::write::SimpleFileOptions;

use super::{blob_path, field_key};
use crate::database::custom_3dmv as database;

// Export packages carry the stored blobs byte-for-byte (they ARE the original
// uploads - nothing is transcoded) plus the upload metadata, so an MV can be
// re-uploaded on any ew server. Layout of the zip:
//   manifest.json   {format, name, name_en, music_id, member_count}
//   model_{slot} / motion_{slot} / facial_{slot} / camera / config / stage
// published is a per-server setting and deliberately not part of the package.

pub fn build(mv_id: i64) -> Result<Vec<u8>, String> {
    let mv = database::get_mv(mv_id).ok_or(String::from("MV not found"))?;

    let manifest = jzon::object!{
        "format": 1,
        "name": mv["name"].clone(),
        "name_en": mv["name_en"].clone(),
        "music_id": mv["music_id"].clone(),
        "member_count": mv["member_count"].clone()
    };

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default();
    let mut add = |name: &str, bytes: &[u8]| -> Result<(), String> {
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        zip.write_all(bytes).map_err(|e| e.to_string())
    };

    add("manifest.json", jzon::stringify(manifest).as_bytes())?;
    for file in mv["files"].members() {
        let Some(name) = field_key(file) else { continue; };
        let md5 = file["md5"].as_str().unwrap_or("");
        let bytes = fs::read(blob_path(md5)).map_err(|e| e.to_string())?;
        add(&name, &bytes)?;
    }

    Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
}

// Reads one entry, capped. Deflate's ceiling is about 1032:1, so an uncapped
// read_to_end here is a zip bomb: a one-megabyte entry inflates to a gigabyte and
// grows the Vec until the allocator or the OOM killer stops it, and a package has
// 39 addressable entries. Every entry is bounded by the upload form's own
// per-file cap, and by what is left of the per-request budget across all entries.
//
// The central directory's declared size rejects the obvious case without
// inflating anything; take(cap + 1) makes that declaration untrusted - a lying
// header runs out of budget one byte past the cap and stops there.
//
// Ok(None) is "the package does not carry this entry", a normal outcome for every
// optional role
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

// Expands a package into the same field map the upload form produces. The
// package's metadata wins over form fields - except music_id, which is a
// server-local id: a form-supplied song wins, and the manifest's only fills
// in when the form left it blank (same-server re-upload)
pub fn expand(package: &[u8], fields: &mut HashMap<String, Vec<u8>>) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(package)).map_err(|_| String::from("Package is not a valid zip file"))?;
    // The decompressed budget for the whole package, shared by every entry
    let mut remaining = super::MAX_REQUEST_BYTES;

    let manifest = read_entry(&mut archive, "manifest.json", &mut remaining)?.ok_or(String::from("Package has no manifest.json"))?;
    let manifest = jzon::parse(&String::from_utf8_lossy(&manifest)).map_err(|_| String::from("Package manifest is not valid JSON"))?;
    if manifest["format"].as_i64() != Some(1) {
        return Err(String::from("Unsupported package format"));
    }

    for key in ["name", "name_en", "member_count"] {
        if !manifest[key].is_null() {
            fields.insert(key.to_string(), manifest[key].to_string().into_bytes());
        }
    }
    if !fields.get("music_id").is_some_and(|v| !v.is_empty()) && !manifest["music_id"].is_null() {
        fields.insert(String::from("music_id"), manifest["music_id"].to_string().into_bytes());
    }

    let member_count = manifest["member_count"].as_i64().unwrap_or(0);
    for slot in 1..=member_count.clamp(0, super::MAX_MEMBER_COUNT) {
        for role in ["model", "motion", "facial"] {
            if let Some(bytes) = read_entry(&mut archive, &format!("{}_{}", role, slot), &mut remaining)? {
                fields.insert(format!("{}_{}", role, slot), bytes);
            }
        }
    }
    for name in ["camera", "config", "stage"] {
        if let Some(bytes) = read_entry(&mut archive, name, &mut remaining)? {
            fields.insert(String::from(name), bytes);
        }
    }
    Ok(())
}

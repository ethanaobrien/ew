use jzon::{array, object, JsonValue};
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::Mutex;

use include_dir::{include_dir, Dir};

use crate::include_file;

static MASTERDATA_JP: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/src/router/databases/csv");
static MASTERDATA_EN: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/src/router/databases/csv-en");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Region {
    Jp,
    En,
}

lazy_static! {
    static ref TABLE_CACHE: Mutex<HashMap<(Region, String), JsonValue>> =
        Mutex::new(HashMap::new());

    // This also needs to be packed into the client - this never changes
    static ref SCHEMAS: JsonValue = jzon::parse(
        &include_file!("src/router/databases/schemas.json")
    ).expect("schemas.json is malformed");
}

fn dir_for(region: Region) -> &'static Dir<'static> {
    match region {
        Region::Jp => &MASTERDATA_JP,
        Region::En => &MASTERDATA_EN,
    }
}

pub fn csv_bytes(region: Region, name: &str) -> Option<&'static [u8]> {
    dir_for(region)
        .get_file(format!("{name}.csv"))
        .map(|f| f.contents())
}

pub fn table(region: Region, name: &str) -> JsonValue {
    let key = (region, name.to_owned());
    if let Some(cached) = TABLE_CACHE.lock().unwrap().get(&key) {
        return cached.clone();
    }

    let bytes = csv_bytes(region, name).unwrap_or_else(|| {
        panic!("masterdata CSV not bundled: {name}.csv ({region:?})")
    });
    let parsed = parse_csv(name, bytes);

    TABLE_CACHE.lock().unwrap().insert(key, parsed.clone());
    parsed
}

fn field_types(table_name: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let table = &SCHEMAS["tables"][table_name];
    for f in table["fields"].members() {
        out.insert(f["name"].to_string(), f["type"].to_string());
    }
    out
}

fn parse_csv(table_name: &str, bytes: &[u8]) -> JsonValue {
    let types = field_types(table_name);

    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(bytes);

    let raw_headers: Vec<String> = rdr
        .headers()
        .expect("malformed CSV header")
        .iter()
        .enumerate()
        .map(|(i, h)| {
            if i == 0 {
                h.trim_start_matches('\u{feff}').to_owned()
            } else {
                h.to_owned()
            }
        })
        .collect();

    let json_keys: Vec<String> = raw_headers
        .iter()
        .map(|h| h.strip_prefix('_').unwrap_or(h).to_owned())
        .collect();

    // Default missing schema columns to "string" — safer than guessing.
    let column_types: Vec<&str> = raw_headers
        .iter()
        .map(|h| types.get(h.as_str()).map(String::as_str).unwrap_or("string"))
        .collect();

    let mut out = array![];
    for record in rdr.records() {
        let record = record.expect("malformed CSV row");
        let mut row = object! {};
        for (i, raw) in record.iter().enumerate() {
            let key = match json_keys.get(i) {
                Some(k) => k.as_str(),
                None => continue, // extra trailing columns — ignore
            };
            row[key] = coerce(raw, column_types[i]);
        }
        out.push(row).expect("array push");
    }
    out
}

fn coerce(raw: &str, type_token: &str) -> JsonValue {
    if let Some(elem) = type_token.strip_suffix("[]") {
        if raw.is_empty() {
            return array![];
        }
        let mut out = array![];
        for part in raw.split(',') {
            out.push(coerce(part, elem)).expect("array push");
        }
        return out;
    }

    match type_token {
        "string" => JsonValue::String(raw.to_owned()),
        "bool" => JsonValue::Boolean(matches!(raw, "1" | "true" | "True" | "TRUE")),
        "float" | "double" => {
            if raw.is_empty() {
                JsonValue::from(0.0)
            } else {
                raw.parse::<f64>()
                    .map(JsonValue::from)
                    .unwrap_or_else(|_| JsonValue::String(raw.to_owned()))
            }
        }
        _ => {
            if raw.is_empty() {
                JsonValue::from(0)
            } else {
                raw.parse::<i64>()
                    .map(JsonValue::from)
                    .unwrap_or_else(|_| JsonValue::String(raw.to_owned()))
            }
        }
    }
}

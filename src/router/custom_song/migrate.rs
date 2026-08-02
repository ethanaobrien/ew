use std::fs;

use super::{chart, database, song_path, asset_meta, LEVEL_COUNT};
use crate::runtime::get_data_path;

// One-time, idempotent startup migration for charts transcoded before the spawn-group pairing
// rule (chart.rs header): the old transcoder gave every note of an equal-time cluster ONE
// shared num, and the client creates no head markers for a group of 3+ notes
// (LiveMarkerControl.CreateMarkerUI plain-returns on count > 2), so 3+ simultaneous notes
// were judged but never rendered. Stored transcoded charts are derived data with everything
// the regroup needs (time + line per note), so they are rewritten in place — no original
// upload required, which also covers songs from before export support.
//
// The same pass also corrects the stored catalog values that were fabricated rather than
// derived from official masterdata (see below): the looping PLAY cue, the per-song score-rank
// thresholds and the combo-mission targets.
//
// For each song directory with a catalog row, every chart with an over-shared num is
// regrouped (chart::regroup), rewritten to disk, and its level's md5/size in the catalog
// blob updated — the changed md5 re-keys the client's content-addressed cache, so clients
// re-download the fixed chart on their next catalog sync. The revision is bumped ONCE if
// anything changed. Charts the current transcoder produced (and official-shaped encodings)
// are left byte-identical, so re-running every boot is free.
pub fn run() {
    if super::disabled() {
        return;
    }
    let Ok(entries) = fs::read_dir(get_data_path("custom_songs")) else {
        // No custom_songs directory yet - nothing was ever uploaded
        return;
    };

    let mut music_ids: Vec<i64> = entries.flatten()
        .filter_map(|entry| entry.file_name().to_string_lossy().parse::<i64>().ok())
        .collect();
    music_ids.sort();

    let mut songs_changed = 0;
    let mut charts_changed = 0;
    for music_id in music_ids {
        // A song directory without a catalog row is never served; leave it alone
        let Some(mut song) = database::get_song(music_id) else { continue; };

        let mut changed = false;

        // Catalogs written before cue_json took an is_loop argument marked BOTH cues as loop
        // cues. A looping PLAY cue never reports playback-end to the client, and the live's end
        // trigger (LiveTimeController: isMusicEnded -> InLiveDelay -> EndWait) hangs off exactly
        // that, so those lives never finish. Only the metadata changes - the ogg bytes and their
        // md5 are untouched, so no re-download is needed, just a catalog resync.
        if song["sound"]["play"]["is_loop"] == true {
            song["sound"]["play"]["is_loop"] = false.into();
            song["sound"]["play"]["loop_end_sec"] = 0.0.into();
            changed = true;
            println!("Custom song {}: play cue un-looped (pre-fix catalog)", music_id);
        }

        // Score-rank thresholds and combo-mission targets used to be invented per song. Both are
        // objective masterdata: the official live rows all carry ONE score tuple, and every
        // live_mission_combo row is round(hardest full combo * 0.2/0.4/0.6/0.8) (see
        // default_scores / mission_combo). Recompute them the way an edit would, so songs
        // uploaded before the fix stop handing out rank S for free and stop asking for a literal
        // full combo on the fourth combo mission.
        let (score, multi_score) = super::default_scores();
        if song["score"] != score || song["multi_score"] != multi_score {
            song["score"] = score;
            song["multi_score"] = multi_score;
            changed = true;
            println!("Custom song {}: score rank thresholds reset to the official values", music_id);
        }
        // Same "hardest difficulty" rule as upload/edit: the last level entry, which both write
        // in ascending level order
        if let Some(hardest) = song["levels"].members().last().and_then(|l| l["full_combo"].as_i64()) {
            let missions = super::mission_combo(hardest);
            if song["mission_combo"] != missions {
                song["mission_combo"] = missions;
                changed = true;
                println!("Custom song {}: combo missions rescaled to the official 20/40/60/80%", music_id);
            }
        }

        for level in 1..=LEVEL_COUNT {
            let path = song_path(music_id, &format!("chart_{}.json", level));
            let Ok(bytes) = fs::read(&path) else { continue; };
            let Ok(mut chart_data) = jzon::parse(&String::from_utf8_lossy(&bytes)) else {
                println!("Custom song {} chart {}: not valid JSON, migration skipped", music_id, level);
                continue;
            };
            if !chart::regroup(&mut chart_data) {
                continue;
            }
            let new_bytes = jzon::stringify(chart_data);
            if let Err(e) = fs::write(&path, &new_bytes) {
                println!("Custom song {} chart {}: rewrite failed ({}), migration skipped", music_id, level, e);
                continue;
            }
            // The catalog md5/size must follow the served bytes or the client's
            // download-and-verify loop would never accept the asset
            let (md5, size) = asset_meta(new_bytes.as_bytes());
            for entry in song["levels"].members_mut() {
                if entry["level"] == level {
                    entry["md5"] = md5.clone().into();
                    entry["size"] = size.into();
                }
            }
            changed = true;
            charts_changed += 1;
            println!("Custom song {}: regrouped chart level {} (pre-pairing spawn groups)", music_id, level);
        }

        if changed {
            database::update_song(music_id, &song);
            songs_changed += 1;
        }
    }

    if songs_changed > 0 {
        database::bump_revision();
        println!("Custom song spawn-group migration: rewrote {} chart(s) in {} song(s), catalog revision bumped", charts_changed, songs_changed);
    }
}

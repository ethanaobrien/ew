use jzon::{object, JsonValue};

// Transcodes a SIF1/NPPS4 beatmap (array of {timing_sec, effect, effect_value, position})
// into the SIF2 chart JSON the client deserializes into NoteData.
//
// Mapping rules:
// - line = position - 1 (both are right-to-left)
// - effect 1 (and 2, the "parallel" marker) -> type 1 (tap)
// - effect 3 (hold) -> head note (type 1) at timing_sec plus a SYNTHESIZED tail note
//   (type 1, same line) at timing_sec + effect_value, linked through parent/child ids
// - effect 4 (star/token) -> type 3
// - effect 11/12/13 (swing) and anything unknown -> plain type 1 tap for v1.
//   Slider chains are a later feature.
// - notes_attribute / notes_level are dropped (SIF2 has no per-note attribute)
// - ids are sequential from 1 in time order. num is the spawn group: the dummy
//   header occupies 100, real groups count up from 101, and notes that hit
//   simultaneously (equal timing_sec, which covers SIF1 effect 2 pairs) share one num.
// - notes[0] is ALWAYS the dummy header (id 0, num 100, type 0) - the client
//   deserializes it verbatim.
// - max_combo_count = all real notes EXCEPT hold heads whose tail is on the same
//   line (the game counts a same-lane hold as one combo for the chain)

struct WorkNote {
    time: f64,
    line: i64,
    kind: i64,
    // Index into the work list of the hold head this tail belongs to
    head: Option<usize>
}

fn parse_sif_note(data: &JsonValue, index: usize) -> Result<(f64, i64, f64, i64), String> {
    let timing = data["timing_sec"].as_f64().ok_or(format!("Note {}: missing timing_sec", index))?;
    let effect = data["effect"].as_i64().ok_or(format!("Note {}: missing effect", index))?;
    let effect_value = data["effect_value"].as_f64().unwrap_or(0.0);
    let position = data["position"].as_i64().ok_or(format!("Note {}: missing position", index))?;

    if !(1..=9).contains(&position) {
        return Err(format!("Note {}: position {} is outside 1-9", index, position));
    }
    if timing < 0.0 {
        return Err(format!("Note {}: negative timing_sec {}", index, timing));
    }
    if effect == 3 && effect_value <= 0.0 {
        return Err(format!("Note {}: hold with effect_value {} (must be > 0)", index, effect_value));
    }

    Ok((timing, effect, effect_value, position))
}

// Returns the chart JSON and its max_combo_count (== the difficulty's full_combo)
pub fn transcode(beatmap: &JsonValue) -> Result<(JsonValue, i64), String> {
    if !beatmap.is_array() || beatmap.is_empty() {
        return Err(String::from("Chart is not a JSON array of notes"));
    }

    let mut work: Vec<WorkNote> = Vec::new();
    for (i, data) in beatmap.members().enumerate() {
        let (timing, effect, effect_value, position) = parse_sif_note(data, i)?;

        for other in beatmap.members().take(i) {
            if other["timing_sec"].as_f64() == Some(timing) && other["position"].as_i64() == Some(position) && other["effect"].as_i64() != Some(effect) {
                return Err(format!("Note {}: duplicate timing {} on position {} with a different effect", i, timing, position));
            }
        }

        let head = work.len();
        work.push(WorkNote {
            time: timing,
            line: position - 1,
            kind: if effect == 4 { 3 } else { 1 },
            head: None
        });
        if effect == 3 {
            work.push(WorkNote {
                time: timing + effect_value,
                line: position - 1,
                kind: 1,
                head: Some(head)
            });
        }
    }

    // Sequential ids in time order. Stable sort keeps input order on ties
    let mut order: Vec<usize> = (0..work.len()).collect();
    order.sort_by(|a, b| work[*a].time.partial_cmp(&work[*b].time).unwrap());

    let mut ids = vec![0i64; work.len()];
    let mut nums = vec![0i64; work.len()];
    let mut num = 100;
    let mut last_time = f64::NEG_INFINITY;
    for (i, index) in order.iter().enumerate() {
        ids[*index] = (i + 1) as i64;
        // Simultaneous notes share a spawn group
        if work[*index].time != last_time {
            num += 1;
            last_time = work[*index].time;
        }
        nums[*index] = num;
    }

    let mut tail_of = vec![0usize; work.len()];
    for (i, note) in work.iter().enumerate() {
        if let Some(head) = note.head {
            tail_of[head] = i;
        }
    }

    let mut notes = jzon::array![{
        "id": 0, "num": 100, "line": 0, "time": 0.0, "type": 0,
        "parent_id": 0, "child_id": 0, "child_num": 0, "child_line": 0,
        "force_sync_group_id": 0
    }];
    let mut max_combo_count = 0;
    for index in order.iter() {
        let note = &work[*index];
        let tail = tail_of[*index];
        let is_head = tail != 0;

        // Same-lane hold heads don't count toward the combo, their tail does
        if !(is_head && work[tail].line == note.line) {
            max_combo_count += 1;
        }

        notes.push(object!{
            "id": ids[*index],
            "num": nums[*index],
            "line": note.line,
            "time": note.time,
            "type": note.kind,
            "parent_id": if let Some(head) = note.head { ids[head] } else { 0 },
            "child_id": if is_head { ids[tail] } else { 0 },
            "child_num": if is_head { nums[tail] } else { 0 },
            "child_line": if is_head { work[tail].line } else { 0 },
            "force_sync_group_id": 0
        }).unwrap();
    }

    Ok((object!{
        "max_lane": 9,
        "sound_name": "",
        "max_combo_count": max_combo_count,
        "notes": notes
    }, max_combo_count))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sif_note(timing_sec: f64, position: i64, effect: i64, effect_value: f64) -> JsonValue {
        object!{
            "timing_sec": timing_sec,
            "notes_attribute": 1,
            "notes_level": 1,
            "effect": effect,
            "effect_value": effect_value,
            "position": position
        }
    }

    #[test]
    fn plain_taps() {
        let beatmap = jzon::array![
            sif_note(1.0, 1, 1, 2.0),
            sif_note(2.0, 5, 1, 2.0),
            sif_note(3.0, 9, 1, 2.0)
        ];
        let (chart, combo) = transcode(&beatmap).unwrap();

        assert_eq!(combo, 3);
        assert_eq!(chart["max_combo_count"], 3);
        assert_eq!(chart["max_lane"], 9);
        assert_eq!(chart["notes"].len(), 4);
        // Dummy header is verbatim
        assert_eq!(chart["notes"][0]["id"], 0);
        assert_eq!(chart["notes"][0]["num"], 100);
        assert_eq!(chart["notes"][0]["type"], 0);
        // Real notes: sequential ids, monotonic nums, right-to-left lines
        assert_eq!(chart["notes"][1]["id"], 1);
        assert_eq!(chart["notes"][1]["num"], 101);
        assert_eq!(chart["notes"][1]["line"], 0);
        assert_eq!(chart["notes"][1]["type"], 1);
        assert_eq!(chart["notes"][2]["num"], 102);
        assert_eq!(chart["notes"][2]["line"], 4);
        assert_eq!(chart["notes"][3]["id"], 3);
        assert_eq!(chart["notes"][3]["num"], 103);
        assert_eq!(chart["notes"][3]["line"], 8);
    }

    #[test]
    fn hold_head_and_tail() {
        let beatmap = jzon::array![
            sif_note(1.0, 3, 3, 2.5)
        ];
        let (chart, combo) = transcode(&beatmap).unwrap();

        // The synthesized same-lane tail counts, the head does not
        assert_eq!(combo, 1);
        assert_eq!(chart["notes"].len(), 3);
        let head = &chart["notes"][1];
        let tail = &chart["notes"][2];
        assert_eq!(head["id"], 1);
        assert_eq!(head["child_id"], 2);
        assert_eq!(head["child_num"], tail["num"].clone());
        assert_eq!(head["child_line"], 2);
        assert_eq!(head["parent_id"], 0);
        assert_eq!(tail["id"], 2);
        assert_eq!(tail["parent_id"], 1);
        assert_eq!(tail["child_id"], 0);
        assert_eq!(tail["line"], 2);
        assert_eq!(tail["time"].as_f64().unwrap(), 3.5);
    }

    #[test]
    fn parallel_pair() {
        let beatmap = jzon::array![
            sif_note(1.0, 2, 2, 2.0),
            sif_note(1.0, 8, 2, 2.0)
        ];
        let (chart, combo) = transcode(&beatmap).unwrap();

        // Simultaneous notes share a spawn group and both count
        assert_eq!(combo, 2);
        assert_eq!(chart["notes"][1]["num"], chart["notes"][2]["num"].clone());
        assert_eq!(chart["notes"][1]["type"], 1);
        assert_eq!(chart["notes"][2]["type"], 1);
    }

    #[test]
    fn mixed() {
        let beatmap = jzon::array![
            sif_note(1.0, 5, 1, 2.0),  // tap
            sif_note(2.0, 3, 3, 1.5),  // hold: head at 2.0, tail at 3.5
            sif_note(2.5, 7, 4, 0.0),  // star
            sif_note(3.5, 1, 2, 2.0),  // parallel with the hold tail
            sif_note(4.0, 9, 11, 0.0)  // swing -> plain tap for v1
        ];
        let (chart, combo) = transcode(&beatmap).unwrap();

        // 6 real notes, minus the same-lane hold head
        assert_eq!(combo, 5);
        assert_eq!(chart["notes"].len(), 7);
        // Time order: tap(1.0), head(2.0), star(2.5), tail(3.5), parallel(3.5), swing(4.0)
        assert_eq!(chart["notes"][2]["child_id"], 4);
        assert_eq!(chart["notes"][3]["type"], 3);
        assert_eq!(chart["notes"][4]["parent_id"], 2);
        // The tail and the parallel tap at 3.5 share a spawn group
        assert_eq!(chart["notes"][4]["num"], chart["notes"][5]["num"].clone());
        assert_eq!(chart["notes"][6]["type"], 1);
        // Ids stay sequential in time order
        for (i, data) in chart["notes"].members().enumerate() {
            assert_eq!(data["id"], i);
        }
    }

    #[test]
    fn rejects_bad_charts() {
        assert!(transcode(&jzon::array![sif_note(1.0, 0, 1, 2.0)]).is_err());
        assert!(transcode(&jzon::array![sif_note(1.0, 10, 1, 2.0)]).is_err());
        assert!(transcode(&jzon::array![sif_note(-1.0, 5, 1, 2.0)]).is_err());
        assert!(transcode(&jzon::array![sif_note(1.0, 5, 3, 0.0)]).is_err());
        assert!(transcode(&jzon::array![sif_note(1.0, 5, 1, 2.0), sif_note(1.0, 5, 3, 2.0)]).is_err());
        assert!(transcode(&jzon::object!{}).is_err());
    }

}

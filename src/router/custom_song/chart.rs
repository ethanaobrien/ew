use jzon::{object, JsonValue};

// Transcodes a SIF1/NPPS4 beatmap (array of {timing_sec, effect, effect_value, position})
// into the SIF2 chart JSON the client deserializes into NoteData.
//
// SIF1 note effects, from the game's own LiveModel.NoteEffect
// (m_live/model/note_effect.lua): note_normal 1, note_event 2, note_hold 3, note_bomb_1 4,
// note_bomb_3 5, note_bomb_5 6, note_bomb_9 7, note_slide 11, note_slide_event 12,
// note_slide_hold 13, with isHold(e) = e == 3 and isSlide(e) = e >= 11.
//
// SIF2 note types, from Aoharu.LiveTimeController.ToMarkerType: 1 tap, 2 flick, 3 skill.
// Anything outside 1..3 becomes MarkerType.None, so those three are the whole vocabulary.
//
// Mapping rules:
// - line = position - 1 (both are right-to-left)
// - effect 1 (and 2, the "parallel" marker) -> type 1 (tap)
// - effect 3 (hold) -> head note (type 1) at timing_sec plus a SYNTHESIZED tail note
//   (type 1, same line) at timing_sec + effect_value, linked through parent/child ids
// - effect 4 (bomb_1, drawn as SIF1's star note) -> type 3
// - effect 11/12 (slide) -> type 2, SIF2's flick. These are SIF1's swipe notes; sending
//   them as taps made a swipe chart playable as a tap chart.
// - effect 13 (slide hold) -> a flick HEAD (type 2) plus the same synthesized tail as
//   effect 3, tail as type 1. SIF1 wants the swipe on entry and a plain release, and SIF2
//   agrees on both counts: LiveInputControl judges a Flick root through InputType.Flick
//   (so the head demands a swipe) while InputType.Released rejects a Flick outright
//   (so a flick tail could never be released). Previously effect 13 lost BOTH halves —
//   no swipe and no hold, just a lone tap.
// - effect 5/6/7 (bomb_3/5/9) and anything unknown -> plain type 1 tap. SIF2 has no
//   multi-lane bomb, so there is nothing better to send.
// - notes_attribute is dropped (SIF2 has no per-note attribute). notes_level is dropped
//   too: in SIF1 a notes_level > 1 marks a simultaneous-hit group (notes.lua groups on it
//   regardless of effect, so it is NOT the slide chain). SIF2's force_sync_group_id is the
//   equivalent and is left at 0 for now.
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

// LiveModel.NoteEffect.isHold, widened to note_slide_hold: both carry a duration in
// effect_value (notes.lua isTimeOver adds effect_value for note_hold and note_slide_hold alike).
fn is_hold(effect: i64) -> bool {
    effect == 3 || effect == 13
}

// LiveModel.NoteEffect.isSlide
fn is_slide(effect: i64) -> bool {
    effect >= 11
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
    if is_hold(effect) && effect_value <= 0.0 {
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
            kind: if is_slide(effect) { 2 } else if effect == 4 { 3 } else { 1 },
            head: None
        });
        if is_hold(effect) {
            // Tail is always a tap: a flick tail is unreleasable (InputType.Released rejects
            // MarkerType.Flick), so the swipe stays on the head where SIF1 puts it.
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
            sif_note(4.0, 9, 11, 0.0)  // slide -> flick
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
        assert_eq!(chart["notes"][6]["type"], 2);
        // Ids stay sequential in time order
        for (i, data) in chart["notes"].members().enumerate() {
            assert_eq!(data["id"], i);
        }
    }

    #[test]
    fn slides_become_flicks() {
        // note_slide and note_slide_event are both plain swipes
        let beatmap = jzon::array![
            sif_note(1.0, 1, 11, 0.0),
            sif_note(2.0, 9, 12, 0.0)
        ];
        let (chart, combo) = transcode(&beatmap).unwrap();

        assert_eq!(combo, 2);
        assert_eq!(chart["notes"].len(), 3);
        assert_eq!(chart["notes"][1]["type"], 2);
        assert_eq!(chart["notes"][2]["type"], 2);
        // A plain slide is not a hold, so neither gets a tail
        assert_eq!(chart["notes"][1]["child_id"], 0);
        assert_eq!(chart["notes"][2]["child_id"], 0);
    }

    #[test]
    fn slide_hold_keeps_swipe_and_hold() {
        // Regression: effect 13 used to lose both halves and arrive as one plain tap.
        let beatmap = jzon::array![
            sif_note(1.0, 3, 13, 2.5)
        ];
        let (chart, combo) = transcode(&beatmap).unwrap();

        assert_eq!(combo, 1);
        assert_eq!(chart["notes"].len(), 3);
        let head = &chart["notes"][1];
        let tail = &chart["notes"][2];
        // Swipe on entry, plain release: flick head, tap tail
        assert_eq!(head["type"], 2);
        assert_eq!(tail["type"], 1);
        // ... linked as a hold, exactly like effect 3
        assert_eq!(head["child_id"], 2);
        assert_eq!(head["child_line"], 2);
        assert_eq!(tail["parent_id"], 1);
        assert_eq!(tail["time"].as_f64().unwrap(), 3.5);
    }

    #[test]
    fn slide_hold_needs_a_duration() {
        // The effect 3 duration check has to cover note_slide_hold too
        assert!(transcode(&jzon::array![sif_note(1.0, 5, 13, 0.0)]).is_err());
        assert!(transcode(&jzon::array![sif_note(1.0, 5, 13, -1.0)]).is_err());
    }

    #[test]
    fn unknown_effects_stay_taps() {
        // bomb_3/5/9 have no SIF2 equivalent; they must not become flicks
        for effect in [5, 6, 7] {
            let (chart, _) = transcode(&jzon::array![sif_note(1.0, 5, effect, 0.0)]).unwrap();
            assert_eq!(chart["notes"][1]["type"], 1, "effect {}", effect);
        }
    }

    // Lifted verbatim from a chart uploaded to the live server (custom song 10008, "Edelied",
    // exported via /custom_song/download/10008) — a swipe run of note_slide into note_slide_hold,
    // which is the shape that used to arrive as undifferentiated taps. Note the editor writes a
    // large arbitrary notes_level (38615 here) rather than SIF1's small group index, which is why
    // notes_level is not used as a sync group.
    #[test]
    fn real_uploaded_swipe_run() {
        let beatmap = jzon::array![
            object!{ "timing_sec": 22.0, "effect": 11, "effect_value": 2.0, "notes_attribute": 2, "notes_level": 38615, "position": 9 },
            object!{ "timing_sec": 22.166666666666668, "effect": 11, "effect_value": 2.0, "notes_attribute": 2, "notes_level": 38615, "position": 8 },
            object!{ "timing_sec": 22.5, "effect": 13, "effect_value": 0.33333333333333215, "notes_attribute": 2, "notes_level": 38615, "position": 6 }
        ];
        let (chart, combo) = transcode(&beatmap).unwrap();

        // 3 source notes + the slide-hold's tail; the same-lane head does not count for combo
        assert_eq!(chart["notes"].len(), 5);
        assert_eq!(combo, 3);
        // Both plain slides are flicks with no tail
        assert_eq!(chart["notes"][1]["type"], 2);
        assert_eq!(chart["notes"][1]["line"], 8);
        assert_eq!(chart["notes"][1]["child_id"], 0);
        assert_eq!(chart["notes"][2]["type"], 2);
        assert_eq!(chart["notes"][2]["line"], 7);
        // The slide-hold is a flick head linked to a tap tail
        assert_eq!(chart["notes"][3]["type"], 2);
        assert_eq!(chart["notes"][3]["child_id"], 4);
        assert_eq!(chart["notes"][4]["type"], 1);
        assert_eq!(chart["notes"][4]["parent_id"], 3);
        // Every type must be one ToMarkerType resolves; 0 would silently become MarkerType.None
        for data in chart["notes"].members().skip(1) {
            let kind = data["type"].as_i64().unwrap();
            assert!((1..=3).contains(&kind), "unresolvable type {}", kind);
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

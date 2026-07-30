use jzon::{object, JsonValue};

// Transcodes a SIF1/NPPS4 beatmap (array of {timing_sec, effect, effect_value, position})
// into the SIF2 chart JSON the client deserializes into NoteData.
//
// SIF1 note effects, from the game's own LiveModel.NoteEffect
// (m_live/model/note_effect.lua): note_normal 1, note_event 2, note_hold 3, note_bomb_1 4,
// note_bomb_3 5, note_bomb_5 6, note_bomb_9 7, note_slide 11, note_slide_event 12,
// note_slide_hold 13, with isHold(e) = e == 3 and isSlide(e) = e >= 11.
//
// SIF2 side: `type` only distinguishes an ordinary note (1) from a star/bomb note (3).
// LiveTimeController.ToMarkerType accepts 1..3 and maps anything else to None; type 2 exists in
// the enum but appears in ZERO of the 2146 shipped charts, so nothing emits it here.
//
// A SLIDE is structural, not a type. MarkerData derives it from the parent/child chain:
//   IsSliderMarker      chained and the child is on a DIFFERENT line  -> a slide segment
//   IsSliderLongMarker  chained, child SAME line, parent different    -> a slide ending in a hold
//   IsDistanceMarker    has both a parent and a child                 -> a middle segment
// So a hold is a chain that stays in its lane and a slide is a chain that moves across lanes;
// both are type 1. This is also how the client counts combo (NoteData.CalcMaxCombo: a note whose
// child shares its line does not count, its tail does).
//
// Mapping rules:
// - line = position - 1 (both are right-to-left)
// - effect 1 (note_normal) and 2 (note_event) -> type 1 (tap). note_event is an ordinary
//   note that also fed SIF1's event scoring; it plays identically and SIF2 has no
//   equivalent, so the distinction is dropped. Simultaneous hits are expressed by sharing
//   a spawn num, not by the effect.
// - effect 3 (hold) -> head note (type 1) at timing_sec plus a SYNTHESIZED tail note
//   (type 1, same line) at timing_sec + effect_value, linked through parent/child ids
// - effect 4/5/6/7 (bomb_1/3/5/9, SIF1's star notes) -> type 3, SIF2's bomb note. SIF1
//   varies the blast width per effect; SIF2 has one bomb with one damage value, so the
//   four collapse into type 3 and only the radius is lost. Previously only bomb_1 mapped
//   here and the three wider ones arrived as ordinary taps.
// - effect 11/12/13 (slide) -> CHAINED across lanes, all type 1. Slides sharing a notes_level
//   form one run: sorted by time and linked parent -> child, so each link crosses lanes and
//   the client sees a slider. A run ends on effect 13 (slide hold), whose synthesized
//   same-line tail then makes it IsSliderLongMarker — the slide settling into a hold the
//   player releases. Verified against a real upload: every notes_level shared by more than
//   one note held exactly the slide-effect notes, each a monotonic sweep like
//   pos 9->8->7->6 with effects 11,11,11,13.
//   A lone slide with no chain partner stays a plain tap: a slider needs a cross-lane child.
// - effect 0 (random) and anything else unknown -> plain type 1. Every effect the game
//   actually defines is covered above, so this is only a floor for hand-authored charts.
// - notes_attribute is dropped (SIF2 has no per-note attribute). notes_level is consumed as
//   the chain id above and not emitted; force_sync_group_id stays 0.
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
    // Chain links, as indices into the work list. A hold is parent -> child on the SAME line;
    // a slide is parent -> child across DIFFERENT lines (see MarkerData.IsSliderMarker).
    parent: Option<usize>,
    child: Option<usize>
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

// SIF1's star notes: note_bomb_1/3/5/9. The suffix is the blast width — star_icon.lua maps
// them to 0/1/2/4 extra lanes either side, damaging that spread when the note is missed.
// SIF2 has a single bomb note with one damage value (LiveInputResultMst._bombLifeDamage,
// applied by LiveLifeControl.CheckLife for type 3, and drawn with the star mark by
// MarkerUI), so all four map to type 3 and only the radius is lost.
fn is_bomb(effect: i64) -> bool {
    (4..=7).contains(&effect)
}

fn parse_sif_note(data: &JsonValue, index: usize) -> Result<(f64, i64, f64, i64, i64), String> {
    let timing = data["timing_sec"].as_f64().ok_or(format!("Note {}: missing timing_sec", index))?;
    let effect = data["effect"].as_i64().ok_or(format!("Note {}: missing effect", index))?;
    let effect_value = data["effect_value"].as_f64().unwrap_or(0.0);
    let position = data["position"].as_i64().ok_or(format!("Note {}: missing position", index))?;
    // Slide chain id. SIF1 keeps it at 1 for unchained notes; editors emit an arbitrary
    // per-chain number, so it is only meaningful as "these slides belong together".
    let group = data["notes_level"].as_i64().unwrap_or(1);

    if !(1..=9).contains(&position) {
        return Err(format!("Note {}: position {} is outside 1-9", index, position));
    }
    if timing < 0.0 {
        return Err(format!("Note {}: negative timing_sec {}", index, timing));
    }
    if is_hold(effect) && effect_value <= 0.0 {
        return Err(format!("Note {}: hold with effect_value {} (must be > 0)", index, effect_value));
    }

    Ok((timing, effect, effect_value, position, group))
}

// Returns the chart JSON and its max_combo_count (== the difficulty's full_combo)
pub fn transcode(beatmap: &JsonValue) -> Result<(JsonValue, i64), String> {
    if !beatmap.is_array() || beatmap.is_empty() {
        return Err(String::from("Chart is not a JSON array of notes"));
    }

    let mut work: Vec<WorkNote> = Vec::new();
    // Slide chain id -> the work indices in that chain, in input order
    let mut chains: Vec<(i64, Vec<usize>)> = Vec::new();
    for (i, data) in beatmap.members().enumerate() {
        let (timing, effect, effect_value, position, group) = parse_sif_note(data, i)?;

        for other in beatmap.members().take(i) {
            if other["timing_sec"].as_f64() == Some(timing) && other["position"].as_i64() == Some(position) && other["effect"].as_i64() != Some(effect) {
                return Err(format!("Note {}: duplicate timing {} on position {} with a different effect", i, timing, position));
            }
        }

        let head = work.len();
        work.push(WorkNote {
            time: timing,
            line: position - 1,
            kind: if is_bomb(effect) { 3 } else { 1 },
            parent: None,
            child: None
        });
        // notes_level > 1 identifies the chain; 1 is SIF1's "not chained" default and must NOT be
        // treated as a group, or every unchained slide in the song would link into one run
        // (notes.lua guards its own grouping the same way: `if 1 < notes_level`).
        if is_slide(effect) && group > 1 {
            match chains.iter_mut().find(|(id, _)| *id == group) {
                Some((_, members)) => members.push(head),
                None => chains.push((group, vec![head]))
            }
        }
        if is_hold(effect) {
            let tail = work.len();
            work.push(WorkNote {
                time: timing + effect_value,
                line: position - 1,
                kind: 1,
                parent: Some(head),
                child: None
            });
            work[head].child = Some(tail);
        }
    }

    // Link each slide chain in time order. Consecutive members sit on different lines, which is
    // exactly what makes SIF2 treat the run as a slider rather than a hold. A member that already
    // has a child is a slide-hold, i.e. the end of the run, so the chain stops there — its tail
    // stays its child and the cross-lane parent link makes it IsSliderLongMarker.
    for (_, members) in chains.iter() {
        if members.len() < 2 {
            // A lone slide cannot be a slider: SIF2 needs a cross-lane child. Leave it a tap.
            continue;
        }
        let mut ordered = members.clone();
        ordered.sort_by(|a, b| work[*a].time.partial_cmp(&work[*b].time).unwrap());
        for pair in ordered.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if work[a].child.is_some() || work[b].parent.is_some() {
                break;
            }
            if work[a].line == work[b].line {
                // Same lane would read as a hold, not a slide; skip the link rather than lie
                continue;
            }
            work[a].child = Some(b);
            work[b].parent = Some(a);
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

    let mut notes = jzon::array![{
        "id": 0, "num": 100, "line": 0, "time": 0.0, "type": 0,
        "parent_id": 0, "child_id": 0, "child_num": 0, "child_line": 0,
        "force_sync_group_id": 0
    }];
    let mut max_combo_count = 0;
    for index in order.iter() {
        let note = &work[*index];

        // NoteData.CalcMaxCombo: a note whose child is on the SAME line (a hold) does not count,
        // its tail does. A cross-lane child (a slide segment) counts normally.
        match note.child {
            Some(child) if work[child].line == note.line => {},
            _ => max_combo_count += 1
        }

        notes.push(object!{
            "id": ids[*index],
            "num": nums[*index],
            "line": note.line,
            "time": note.time,
            "type": note.kind,
            "parent_id": if let Some(parent) = note.parent { ids[parent] } else { 0 },
            "child_id": if let Some(child) = note.child { ids[child] } else { 0 },
            "child_num": if let Some(child) = note.child { nums[child] } else { 0 },
            "child_line": if let Some(child) = note.child { work[child].line } else { 0 },
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

    // A slide carrying its chain id; slides sharing one belong to the same run
    fn sif_slide(timing_sec: f64, position: i64, effect: i64, effect_value: f64, group: i64) -> JsonValue {
        object!{
            "timing_sec": timing_sec,
            "notes_attribute": 1,
            "notes_level": group,
            "effect": effect,
            "effect_value": effect_value,
            "position": position
        }
    }

    // (line, type, parent_id, child_id, child_line) for each note after the dummy header
    fn shape(chart: &JsonValue) -> Vec<(i64, i64, i64, i64, i64)> {
        chart["notes"].members().skip(1).map(|d| (
            d["line"].as_i64().unwrap(),
            d["type"].as_i64().unwrap(),
            d["parent_id"].as_i64().unwrap(),
            d["child_id"].as_i64().unwrap(),
            d["child_line"].as_i64().unwrap()
        )).collect()
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
            sif_note(4.0, 9, 11, 0.0)  // lone slide, no chain partner -> stays a tap
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
    fn slide_chain_links_across_lanes() {
        // A three-note sweep right to left, one chain. Every link must cross lanes, which is
        // what MarkerData.IsSliderMarker keys on.
        let beatmap = jzon::array![
            sif_slide(1.0, 9, 11, 0.0, 500),
            sif_slide(1.2, 8, 11, 0.0, 500),
            sif_slide(1.4, 7, 12, 0.0, 500)
        ];
        let (chart, combo) = transcode(&beatmap).unwrap();

        // No tails: nothing here is a hold
        assert_eq!(chart["notes"].len(), 4);
        // Slides are ordinary notes; the chain carries the meaning
        assert_eq!(shape(&chart), vec![
            (8, 1, 0, 2, 7),   // root, child on line 7
            (7, 1, 1, 3, 6),   // middle: has parent AND child -> IsDistanceMarker
            (6, 1, 2, 0, 0)    // last of the run
        ]);
        // Every link crosses lanes, so all three count for combo
        assert_eq!(combo, 3);
    }

    #[test]
    fn separate_chains_do_not_link() {
        let beatmap = jzon::array![
            sif_slide(1.0, 9, 11, 0.0, 500),
            sif_slide(1.2, 8, 11, 0.0, 500),
            sif_slide(2.0, 4, 11, 0.0, 501),
            sif_slide(2.2, 3, 11, 0.0, 501)
        ];
        let (chart, _) = transcode(&beatmap).unwrap();
        assert_eq!(shape(&chart), vec![
            (8, 1, 0, 2, 7),
            (7, 1, 1, 0, 0),   // chain 500 ends here, does not reach chain 501
            (3, 1, 0, 4, 2),
            (2, 1, 3, 0, 0)
        ]);
    }

    #[test]
    fn default_notes_level_does_not_chain() {
        // notes_level 1 is SIF1's "unchained" default. Treating it as a group id would link every
        // slide in the song into one run spanning the whole track.
        let beatmap = jzon::array![
            sif_note(1.0, 9, 11, 0.0),
            sif_note(1.2, 8, 11, 0.0),
            sif_note(40.0, 2, 11, 0.0)
        ];
        let (chart, combo) = transcode(&beatmap).unwrap();
        assert_eq!(shape(&chart), vec![
            (8, 1, 0, 0, 0),
            (7, 1, 0, 0, 0),
            (1, 1, 0, 0, 0)
        ]);
        assert_eq!(combo, 3);
    }

    #[test]
    fn lone_slide_stays_a_tap() {
        // Nothing to chain to, and a slider needs a cross-lane child
        let (chart, combo) = transcode(&jzon::array![sif_slide(1.0, 5, 11, 0.0, 500)]).unwrap();
        assert_eq!(chart["notes"].len(), 2);
        assert_eq!(shape(&chart), vec![(4, 1, 0, 0, 0)]);
        assert_eq!(combo, 1);
    }

    #[test]
    fn slide_chain_ends_in_a_hold() {
        // A sweep terminating on effect 13: the run settles into a hold on the last lane.
        // Regression: effect 13 used to lose its hold entirely and arrive as a lone note.
        let beatmap = jzon::array![
            sif_slide(1.0, 9, 11, 0.0, 500),
            sif_slide(1.2, 8, 11, 0.0, 500),
            sif_slide(1.4, 7, 13, 0.5, 500)
        ];
        let (chart, combo) = transcode(&beatmap).unwrap();

        assert_eq!(chart["notes"].len(), 5);
        assert_eq!(shape(&chart), vec![
            (8, 1, 0, 2, 7),   // root of the slide
            (7, 1, 1, 3, 6),   // middle
            (6, 1, 2, 4, 6),   // parent on line 7, child on line 6 -> IsSliderLongMarker
            (6, 1, 3, 0, 0)    // the hold tail, released normally
        ]);
        // The tail sits at the slide-hold's time plus its duration
        assert_eq!(chart["notes"][4]["time"].as_f64().unwrap(), 1.9);
        // The same-lane hold head does not count; its tail does
        assert_eq!(combo, 3);
    }

    #[test]
    fn slide_hold_needs_a_duration() {
        // The effect 3 duration check has to cover note_slide_hold too
        assert!(transcode(&jzon::array![sif_note(1.0, 5, 13, 0.0)]).is_err());
        assert!(transcode(&jzon::array![sif_note(1.0, 5, 13, -1.0)]).is_err());
    }

    #[test]
    fn every_bomb_width_is_a_star_note() {
        // note_bomb_1/3/5/9. SIF2 has one bomb note, so all four land on type 3; before,
        // only bomb_1 did and the wider three arrived as ordinary taps.
        for effect in [4, 5, 6, 7] {
            let (chart, combo) = transcode(&jzon::array![sif_note(1.0, 5, effect, 0.0)]).unwrap();
            assert_eq!(chart["notes"][1]["type"], 3, "effect {}", effect);
            // Bombs are instantaneous — no tail, and they count for combo
            assert_eq!(chart["notes"].len(), 2, "effect {}", effect);
            assert_eq!(chart["notes"][1]["child_id"], 0, "effect {}", effect);
            assert_eq!(combo, 1, "effect {}", effect);
        }
    }

    #[test]
    fn every_defined_effect_maps_to_a_real_note_type() {
        // The whole LiveModel.NoteEffect vocabulary, and what each must become
        for (effect, kind) in [(1, 1), (2, 1), (3, 1), (4, 3), (5, 3), (6, 3), (7, 3), (11, 1), (12, 1), (13, 1)] {
            let (chart, _) = transcode(&jzon::array![sif_note(1.0, 5, effect, 1.0)]).unwrap();
            assert_eq!(chart["notes"][1]["type"], kind, "effect {}", effect);
            // Whatever it is, the client must be able to resolve it
            for data in chart["notes"].members().skip(1) {
                let t = data["type"].as_i64().unwrap();
                assert!((1..=3).contains(&t), "effect {} produced unresolvable type {}", effect, t);
            }
        }
    }

    #[test]
    fn undefined_effects_fall_back_to_taps() {
        // Not part of NoteEffect; a hand-authored chart must still transcode to something valid
        for effect in [0, 8, 9, 10] {
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
            object!{ "timing_sec": 22.333333333333332, "effect": 11, "effect_value": 2.0, "notes_attribute": 2, "notes_level": 38615, "position": 7 },
            object!{ "timing_sec": 22.5, "effect": 13, "effect_value": 0.33333333333333215, "notes_attribute": 2, "notes_level": 38615, "position": 6 }
        ];
        let (chart, combo) = transcode(&beatmap).unwrap();

        // 4 source notes + the slide-hold's tail, plus the dummy header
        assert_eq!(chart["notes"].len(), 6);
        // One slider running 9 -> 8 -> 7 -> 6 (lines 8..5), the last settling into a hold
        assert_eq!(shape(&chart), vec![
            (8, 1, 0, 2, 7),
            (7, 1, 1, 3, 6),
            (6, 1, 2, 4, 5),
            (5, 1, 3, 5, 5),   // slide into hold
            (5, 1, 4, 0, 0)    // tail
        ]);
        // The same-lane hold head is the only note that does not count
        assert_eq!(combo, 4);
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

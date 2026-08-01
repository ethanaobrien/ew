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
//   the chain id above and not emitted.
// - ids are sequential from 1 in time order. num is the spawn group: the dummy
//   header occupies 100 and real groups count up from 101. The client spawns markers one
//   num-group at a time, and LiveMarkerControl.CreateMarkerUI (list overload) plain-RETURNS
//   when the group holds more than 2 markers — so a num may be shared by AT MOST two notes,
//   or the whole group's head markers never render (hold bands are created by the separate,
//   unguarded CreateLongMarkerBandUI call, which is why 4 simultaneous holds showed trails
//   with no heads). Simultaneous notes (equal final time, which covers SIF1 effect 2 pairs
//   AND synthesized hold tails) are therefore sorted by lane and chunked into pairs: each
//   chunk gets its own num, and every chunk after the first carries the PREVIOUS chunk's num
//   in force_sync_group_id. That is exactly the official encoding — all 4292 shipped NoteData
//   assets have num groups of only 1 or 2, and the 15 charts with 3-4 simultaneous notes
//   (SIFAC ports, e.g. 1132_5_Sn, 1136_5_An) pair the lowest lanes under the first num and
//   point the later chunk's m_ForceSyncGroupID at it. The client turns that into the extra
//   connector line (LiveTimeController.CreateMarkerTimeData force-group pass matches
//   ForceGroupId against the other chunk's GroupId and links the lane-closest pair).
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
    let mut force_sync = vec![0i64; work.len()];
    let mut num = 100;
    for (i, index) in order.iter().enumerate() {
        ids[*index] = (i + 1) as i64;
    }
    // Spawn groups: at most TWO notes per num (see the header comment — a bigger group's head
    // markers never render). A cluster of simultaneous notes is sorted by lane and chunked into
    // pairs; the leftmost pair takes the first num, and each later chunk points its
    // force_sync_group_id at the previous chunk's num, matching the official SIFAC-port encoding.
    let mut start = 0;
    while start < order.len() {
        let mut end = start + 1;
        while end < order.len() && work[order[end]].time == work[order[start]].time {
            end += 1;
        }
        let mut cluster: Vec<usize> = order[start..end].to_vec();
        cluster.sort_by_key(|index| work[*index].line);
        let mut prev_num = 0;
        for chunk in cluster.chunks(2) {
            num += 1;
            for index in chunk {
                nums[*index] = num;
                force_sync[*index] = prev_num;
            }
            prev_num = num;
        }
        start = end;
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
            "force_sync_group_id": force_sync[*index]
        }).unwrap();
    }

    Ok((object!{
        "max_lane": 9,
        "sound_name": "",
        "max_combo_count": max_combo_count,
        "notes": notes
    }, max_combo_count))
}

// Regroups a STORED transcoded chart whose spawn groups predate the pairing rule above: the
// old transcoder gave every note of an equal-time cluster one shared num (force_sync_group_id
// always 0), and the client renders no head markers for a group of 3+ (see the header
// comment). This rebuilds num / force_sync_group_id in place with the same clustering the
// transcoder now uses — equal final time, lane-sorted, chunks of two, chained
// force_sync_group_id — and re-points child_num at each child's renumbered spawn group.
// Everything else (ids, times, lines, types, parent/child links, max_combo_count — combo
// counting never depended on grouping) is untouched, so on a chart the current transcoder
// produced this reproduces the stored bytes exactly.
//
// Returns false (chart untouched) unless some num is shared by MORE than two notes. That
// makes it a safe no-op on current uploads AND on official-style encodings (whose num values
// differ from ours — e.g. gaps of 3 — but whose groups never exceed two).
pub fn regroup(chart: &mut JsonValue) -> bool {
    // (id, time, line) per real note; the dummy header at [0] stays untouched
    let notes: Vec<(i64, f64, i64)> = chart["notes"].members().skip(1).map(|n| (
        n["id"].as_i64().unwrap_or(0),
        n["time"].as_f64().unwrap_or(0.0),
        n["line"].as_i64().unwrap_or(0)
    )).collect();

    // Only a pre-pairing chart (some num shared 3+ ways) is rewritten
    let mut group_sizes: Vec<(i64, i64)> = Vec::new();
    for data in chart["notes"].members().skip(1) {
        let num = data["num"].as_i64().unwrap_or(0);
        match group_sizes.iter_mut().find(|(n, _)| *n == num) {
            Some((_, count)) => *count += 1,
            None => group_sizes.push((num, 1))
        }
    }
    if group_sizes.iter().all(|(_, count)| *count <= 2) {
        return false;
    }

    // Time order; ids break ties (transcode issues them in time order, so this reproduces
    // the emission order the grouping pass originally saw)
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by(|a, b| notes[*a].1.total_cmp(&notes[*b].1).then(notes[*a].0.cmp(&notes[*b].0)));

    // id -> (new num, new force_sync_group_id)
    let mut assigned: Vec<(i64, i64, i64)> = Vec::with_capacity(notes.len());
    let mut num = 100;
    let mut start = 0;
    while start < order.len() {
        let mut end = start + 1;
        while end < order.len() && notes[order[end]].1 == notes[order[start]].1 {
            end += 1;
        }
        let mut cluster: Vec<usize> = order[start..end].to_vec();
        cluster.sort_by_key(|index| notes[*index].2);
        let mut prev_num = 0;
        for chunk in cluster.chunks(2) {
            num += 1;
            for index in chunk {
                assigned.push((notes[*index].0, num, prev_num));
            }
            prev_num = num;
        }
        start = end;
    }
    let lookup = |id: i64| assigned.iter().find(|(i, _, _)| *i == id).map(|(_, n, f)| (*n, *f));

    for data in chart["notes"].members_mut().skip(1) {
        let Some((new_num, force)) = lookup(data["id"].as_i64().unwrap_or(0)) else { continue; };
        data["num"] = new_num.into();
        data["force_sync_group_id"] = force.into();
        let child = data["child_id"].as_i64().unwrap_or(0);
        if child != 0 {
            // child_num names the child's spawn group and must follow its new num
            data["child_num"] = lookup(child).map(|(n, _)| n).unwrap_or(0).into();
        }
    }
    true
}

// Test helper: fabricates what pre-pairing servers stored, by squashing a current chart back
// to the OLD encoding — one shared num per equal-time cluster, force_sync_group_id 0, and
// child_num following. Lives outside the tests module so the migration tests in
// router/custom_song.rs can build realistic pre-fix fixtures from transcode output.
#[cfg(test)]
pub fn squash_to_pre_fix(chart: &mut JsonValue) {
    let notes: Vec<(i64, f64)> = chart["notes"].members().skip(1)
        .map(|n| (n["id"].as_i64().unwrap(), n["time"].as_f64().unwrap()))
        .collect();
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by(|a, b| notes[*a].1.total_cmp(&notes[*b].1).then(notes[*a].0.cmp(&notes[*b].0)));
    let mut nums: Vec<(i64, i64)> = Vec::new();
    let mut num = 100;
    let mut last_time = f64::NEG_INFINITY;
    for index in order {
        if notes[index].1 != last_time {
            num += 1;
            last_time = notes[index].1;
        }
        nums.push((notes[index].0, num));
    }
    let lookup = |id: i64| nums.iter().find(|(i, _)| *i == id).map(|(_, n)| *n).unwrap_or(0);
    for data in chart["notes"].members_mut().skip(1) {
        data["num"] = lookup(data["id"].as_i64().unwrap()).into();
        data["force_sync_group_id"] = 0.into();
        let child = data["child_id"].as_i64().unwrap_or(0);
        if child != 0 {
            data["child_num"] = lookup(child).into();
        }
    }
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

    // The client spawns markers one num-group at a time and CreateMarkerUI refuses lists of
    // more than 2, so no num may ever be shared by 3+ notes (official charts never do)
    fn assert_spawn_groups_hold_at_most_two(chart: &JsonValue) {
        let mut counts: Vec<(i64, i64)> = Vec::new();
        for data in chart["notes"].members().skip(1) {
            let num = data["num"].as_i64().unwrap();
            match counts.iter_mut().find(|(n, _)| *n == num) {
                Some((_, c)) => *c += 1,
                None => counts.push((num, 1))
            }
        }
        for (num, count) in counts {
            assert!(count <= 2, "num {} is shared by {} notes; the client renders no heads for such a group", num, count);
        }
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
        // A plain pair is the GroupSync path; the force-group field stays clear
        assert_eq!(chart["notes"][1]["force_sync_group_id"], 0);
        assert_eq!(chart["notes"][2]["force_sync_group_id"], 0);
    }

    #[test]
    fn three_simultaneous_notes_split_into_pair_plus_force_synced_single() {
        // Official encoding (e.g. 1132_5_Sn, 1136_5_An: the only shipped charts with 3-4
        // simultaneous notes): the cluster is sorted by lane, the lowest two lanes share the
        // first num, and the leftover note takes the NEXT num with force_sync_group_id pointing
        // back at the pair's num. Input arrives lane-scrambled to prove the chunking sorts.
        let beatmap = jzon::array![
            sif_note(1.0, 8, 1, 0.0),
            sif_note(1.0, 2, 1, 0.0),
            sif_note(1.0, 5, 1, 0.0)
        ];
        let (chart, combo) = transcode(&beatmap).unwrap();

        assert_eq!(combo, 3);
        assert_eq!(chart["notes"].len(), 4);
        assert_spawn_groups_hold_at_most_two(&chart);
        // Emission keeps input order on time ties; grouping is by lane
        let (right, left, mid) = (&chart["notes"][1], &chart["notes"][2], &chart["notes"][3]);
        assert_eq!(left["line"], 1);
        assert_eq!(mid["line"], 4);
        assert_eq!(right["line"], 7);
        // Lanes 1 and 4 pair under the first num, force-clear
        assert_eq!(left["num"], 101);
        assert_eq!(mid["num"], 101);
        assert_eq!(left["force_sync_group_id"], 0);
        assert_eq!(mid["force_sync_group_id"], 0);
        // Lane 7 rides the next num and force-syncs against the pair's num
        assert_eq!(right["num"], 102);
        assert_eq!(right["force_sync_group_id"], 101);
    }

    #[test]
    fn four_simultaneous_holds_pair_heads_and_tails() {
        // Second field report: 4 holds hitting together rendered their bands but no head
        // markers — all four heads shared one num, and the client's CreateMarkerUI refuses
        // groups over 2 while CreateLongMarkerBandUI (a separate, unguarded call) still drew
        // the bands. Officially both the head cluster AND the tail cluster split 2+2 with the
        // second chunk force-synced to the first (1132_5_Sn time 12.208 does this to tails).
        let beatmap = jzon::array![
            sif_note(1.0, 3, 3, 2.0),
            sif_note(1.0, 4, 3, 2.0),
            sif_note(1.0, 6, 3, 2.0),
            sif_note(1.0, 7, 3, 2.0)
        ];
        let (chart, combo) = transcode(&beatmap).unwrap();

        // Same-lane hold heads don't count; the four tails do
        assert_eq!(combo, 4);
        assert_eq!(chart["notes"].len(), 9);
        assert_spawn_groups_hold_at_most_two(&chart);

        // Heads at 1.0: lanes 2,3 share num 101; lanes 5,6 share num 102 force-synced to 101
        for (index, line, num, fs) in [(1, 2, 101, 0), (2, 3, 101, 0), (3, 5, 102, 101), (4, 6, 102, 101)] {
            let head = &chart["notes"][index];
            assert_eq!(head["line"], line, "head {}", index);
            assert_eq!(head["num"], num, "head {}", index);
            assert_eq!(head["force_sync_group_id"], fs, "head {}", index);
            assert_eq!(head["parent_id"], 0, "head {}", index);
        }
        // Tails at 3.0: the SAME pairing applies to the synthesized cluster
        for (index, line, num, fs) in [(5, 2, 103, 0), (6, 3, 103, 0), (7, 5, 104, 103), (8, 6, 104, 103)] {
            let tail = &chart["notes"][index];
            assert_eq!(tail["line"], line, "tail {}", index);
            assert_eq!(tail["num"], num, "tail {}", index);
            assert_eq!(tail["force_sync_group_id"], fs, "tail {}", index);
            assert_eq!(tail["child_id"], 0, "tail {}", index);
        }
        // The chains still line up: each head's child_num names the tail's spawn group
        assert_eq!(chart["notes"][1]["child_id"], 5);
        assert_eq!(chart["notes"][1]["child_num"], 103);
        assert_eq!(chart["notes"][3]["child_id"], 7);
        assert_eq!(chart["notes"][3]["child_num"], 104);
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

    // The 10011 field shape: 4 parallel holds into a full 9-lane wall into a triple. Squashing
    // transcode output reproduces the old encoding exactly (one num per cluster, no force
    // links); regroup must restore the current encoding BYTE-IDENTICALLY, child_num included.
    #[test]
    fn regroup_restores_pre_fix_wall_and_parallel_holds() {
        let beatmap = jzon::array![
            sif_note(1.0, 2, 3, 1.0), sif_note(1.0, 4, 3, 1.0),
            sif_note(1.0, 6, 3, 1.0), sif_note(1.0, 8, 3, 1.0),
            sif_note(2.75, 1, 1, 0.0), sif_note(2.75, 2, 1, 0.0), sif_note(2.75, 3, 1, 0.0),
            sif_note(2.75, 4, 1, 0.0), sif_note(2.75, 5, 1, 0.0), sif_note(2.75, 6, 1, 0.0),
            sif_note(2.75, 7, 1, 0.0), sif_note(2.75, 8, 1, 0.0), sif_note(2.75, 9, 1, 0.0),
            sif_note(3.625, 3, 1, 0.0), sif_note(3.625, 5, 1, 0.0), sif_note(3.625, 7, 1, 0.0)
        ];
        let (expected, _) = transcode(&beatmap).unwrap();

        let mut chart = expected.clone();
        squash_to_pre_fix(&mut chart);
        // Sanity: the squash really is the old encoding — whole clusters share one num
        assert_eq!(chart["notes"][1]["num"], 101);
        assert_eq!(chart["notes"][4]["num"], 101);   // all 4 hold heads
        assert_eq!(chart["notes"][9]["num"], 103);
        assert_eq!(chart["notes"][17]["num"], 103);  // all 9 wall notes
        assert_eq!(chart["notes"][1]["child_num"], 102, "squashed child_num must follow");
        for data in chart["notes"].members() {
            assert_eq!(data["force_sync_group_id"], 0);
        }

        assert!(regroup(&mut chart), "a squashed chart must be rewritten");
        assert_eq!(jzon::stringify(chart.clone()), jzon::stringify(expected.clone()),
            "regroup must reproduce the current transcoder's output exactly");

        // Spell the wall out: adjacent-lane pairs, each later chunk force-synced to the
        // previous one (heads 101/102, tails 103/104, wall 105..109, triple 110/111)
        assert_spawn_groups_hold_at_most_two(&chart);
        let wall: Vec<(i64, i64, i64)> = chart["notes"].members()
            .filter(|d| d["time"].as_f64() == Some(2.75))
            .map(|d| (d["line"].as_i64().unwrap(), d["num"].as_i64().unwrap(), d["force_sync_group_id"].as_i64().unwrap()))
            .collect();
        let mut wall_sorted = wall.clone();
        wall_sorted.sort();
        assert_eq!(wall_sorted, vec![
            (0, 105, 0), (1, 105, 0),
            (2, 106, 105), (3, 106, 105),
            (4, 107, 106), (5, 107, 106),
            (6, 108, 107), (7, 108, 107),
            (8, 109, 108)
        ]);
        // child_num follows the child's NEW num: each head's child_num names a tail group
        for head in chart["notes"].members().filter(|d| d["time"].as_f64() == Some(1.0)) {
            let child_id = head["child_id"].as_i64().unwrap();
            let tail = chart["notes"].members().find(|d| d["id"].as_i64() == Some(child_id)).unwrap();
            assert_eq!(head["child_num"], tail["num"].clone());
            assert!([103, 104].contains(&tail["num"].as_i64().unwrap()));
        }
    }

    #[test]
    fn regroup_is_a_no_op_on_current_encoding() {
        let beatmap = jzon::array![
            sif_note(1.0, 2, 1, 0.0), sif_note(1.0, 5, 1, 0.0), sif_note(1.0, 8, 1, 0.0),
            sif_note(2.0, 4, 3, 1.5)
        ];
        let (chart, _) = transcode(&beatmap).unwrap();
        let before = jzon::stringify(chart.clone());
        let mut chart = chart;
        assert!(!regroup(&mut chart));
        assert_eq!(jzon::stringify(chart), before);
    }

    // Official-shaped encodings (1132_5_Sn t=9.781: num gaps of 3, force_sync naming the other
    // pair) have groups of at most two and must never be "normalized" to our num sequence
    #[test]
    fn regroup_is_a_no_op_on_official_shaped_charts() {
        let mut chart = object!{
            "max_lane": 9, "sound_name": "", "max_combo_count": 4,
            "notes": [
                {"id": 0, "num": 100, "line": 0, "time": 0.0, "type": 0, "parent_id": 0, "child_id": 0, "child_num": 0, "child_line": 0, "force_sync_group_id": 0},
                {"id": 45, "num": 145, "line": 0, "time": 9.781, "type": 1, "parent_id": 0, "child_id": 0, "child_num": 0, "child_line": 0, "force_sync_group_id": 0},
                {"id": 46, "num": 145, "line": 1, "time": 9.781, "type": 1, "parent_id": 0, "child_id": 0, "child_num": 0, "child_line": 0, "force_sync_group_id": 0},
                {"id": 47, "num": 148, "line": 7, "time": 9.781, "type": 1, "parent_id": 0, "child_id": 0, "child_num": 0, "child_line": 0, "force_sync_group_id": 145},
                {"id": 48, "num": 148, "line": 8, "time": 9.781, "type": 1, "parent_id": 0, "child_id": 0, "child_num": 0, "child_line": 0, "force_sync_group_id": 145}
            ]
        };
        let before = jzon::stringify(chart.clone());
        assert!(!regroup(&mut chart));
        assert_eq!(jzon::stringify(chart), before);
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

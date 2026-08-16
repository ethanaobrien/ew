// Structural validation of a VMD (Vocaloid Motion Data) upload: the header
// magic plus a full section walk with bounds checks, so a truncated or
// corrupt file is rejected cheaply at upload time instead of crashing a
// client mid-live. Nothing here decodes the animation - the stored bytes are
// served verbatim and the client owns the semantics.
//
// Layout (little-endian): 30-byte magic "Vocaloid Motion Data 0002"
// (NUL-padded), 20-byte model name, then up to 6 sections, each a uint32
// count followed by fixed-size records - bone (111), morph (23), camera (61),
// light (28), self-shadow (9) - and the property/IK section whose records are
// variable-sized (9 bytes + 21 per IK entry). Camera-only and motion-only
// files legitimately end early: EOF on a section boundary reads as count 0.

const MAGIC_V2: &[u8] = b"Vocaloid Motion Data 0002";
const MAGIC_V1: &[u8] = b"Vocaloid Motion Data file";
const HEADER_LEN: usize = 30 + 20;

// (record size, section name) for the fixed-size sections, in file order
const FIXED_SECTIONS: &[(usize, &str)] = &[
    (111, "bone"),
    (23, "morph"),
    (61, "camera"),
    (28, "light"),
    (9, "self-shadow")
];

fn read_count(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_le_bytes(bytes.get(offset..end)?.try_into().unwrap()))
}

pub fn validate(label: &str, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < HEADER_LEN {
        return Err(format!("'{}' is too short to be a VMD file", label));
    }
    if bytes.starts_with(MAGIC_V1) {
        return Err(format!("'{}' is a version 1 VMD (\"Vocaloid Motion Data file\") - re-save it as version 2 in MMD", label));
    }
    if !bytes.starts_with(MAGIC_V2) {
        return Err(format!("'{}' is not a VMD file (missing the \"Vocaloid Motion Data 0002\" header)", label));
    }

    let mut offset = HEADER_LEN;
    for (record_size, section) in FIXED_SECTIONS {
        // EOF exactly on a section boundary: the remaining sections are absent
        if offset == bytes.len() {
            return Ok(());
        }
        let Some(count) = read_count(bytes, offset) else {
            return Err(format!("'{}' is truncated in the {} section header", label, section));
        };
        offset += 4;
        let section_len = (count as usize).checked_mul(*record_size)
            .filter(|len| offset.checked_add(*len).is_some_and(|end| end <= bytes.len()))
            .ok_or(format!("'{}' is truncated: the {} section claims {} records past the end of the file", label, section, count))?;
        offset += section_len;
    }

    // Property/IK section: uint32 frame, byte visible, uint32 ikCount, then
    // ikCount x (20-byte bone name + 1-byte enabled)
    if offset == bytes.len() {
        return Ok(());
    }
    let Some(count) = read_count(bytes, offset) else {
        return Err(format!("'{}' is truncated in the property section header", label));
    };
    offset += 4;
    for _ in 0..count {
        let Some(ik_count) = read_count(bytes, offset + 5) else {
            return Err(format!("'{}' is truncated in the property section", label));
        };
        let record_len = (ik_count as usize).checked_mul(21)
            .and_then(|len| len.checked_add(9))
            .filter(|len| offset.checked_add(*len).is_some_and(|end| end <= bytes.len()))
            .ok_or(format!("'{}' is truncated in the property section", label))?;
        offset += record_len;
    }
    // Trailing bytes after the last section are tolerated, like MMD does
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structure_walk_accepts_real_shapes_and_rejects_corruption() {
        let vmd = crate::router::custom_3dmv::tests::test_vmd(1);
        assert!(validate("motion_1", &vmd).is_ok());

        // Camera-only file: sections end after camera
        let cam = crate::router::custom_3dmv::tests::test_camera_vmd(2);
        assert!(validate("camera", &cam).is_ok());

        // A bare header with every section absent is structurally fine
        let mut bare = Vec::new();
        bare.extend(MAGIC_V2);
        bare.resize(HEADER_LEN, 0);
        assert!(validate("motion_1", &bare).is_ok());

        // Truncations and lies
        assert!(validate("motion_1", &vmd[..vmd.len() - 1]).unwrap_err().contains("truncated"));
        assert!(validate("motion_1", &vmd[..HEADER_LEN + 2]).unwrap_err().contains("truncated"));
        let mut liar = vmd.clone();
        liar[HEADER_LEN] = 200; // bone count far past EOF
        assert!(validate("motion_1", &liar).unwrap_err().contains("claims 200 records"));
        // A count that would overflow the length math
        let mut overflow = bare.clone();
        overflow.extend(u32::MAX.to_le_bytes());
        assert!(validate("motion_1", &overflow).unwrap_err().contains("truncated"));

        // Wrong or old magic
        assert!(validate("motion_1", b"garbage").unwrap_err().contains("too short"));
        let mut wrong = vmd.clone();
        wrong[0] = b'X';
        assert!(validate("motion_1", &wrong).unwrap_err().contains("not a VMD"));
        let mut v1 = vmd.clone();
        v1[..MAGIC_V1.len()].copy_from_slice(MAGIC_V1);
        assert!(validate("motion_1", &v1).unwrap_err().contains("version 1"));
    }
}

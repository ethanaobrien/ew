// TextMeshPro rich-text guarding for user-supplied strings.
//
// Every name/description the custom-song and custom-card uploads accept is rendered by the game
// through TextMeshUI -> TMP with rich text ENABLED (the shipped labels carry m_isRichText: 1),
// and the client does NOT escape it: it hands user text straight to SetText, exactly as it does
// for other players' account names. So a song called "<size=400%>x", a card whose skill
// description carries <sprite=...>, or a character named "<font=nonexistent>" mangles or breaks
// every screen that shows it - for EVERY player, since public songs and published cards are
// visible to all. This is the in-game equivalent of stored XSS.
//
// The fix belongs here rather than at the render seam, because that is where official data draws
// the line: the shipped masterdata carries NO markup in any name or artist column (1102 of the
// 1103 tags in the whole EN music table are <br>, all of them inside detailInfo), <br> in the
// descriptive columns, and <size=NN> only in character nameRichtextGacha - a column whose name
// says it is meant to be rich text. Uploads are held to exactly that shape.
//
// A '<' that TMP would not read as a tag is left alone, so titles like "<3" still upload.

// The tags found in `text`, lowercased and without a leading '/'. Mirrors TMP's own scan: a tag
// opens at '<' followed by an optional '/' then a letter or '#' (a colour tag), and must close
// with '>' before the next '<'. Anything else is literal text.
fn tags(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut rv = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '<' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        if j < chars.len() && chars[j] == '/' {
            j += 1;
        }
        // Not tag-like: a bare "<3", "< 3", "<<"
        if j >= chars.len() || !(chars[j].is_ascii_alphabetic() || chars[j] == '#') {
            i += 1;
            continue;
        }
        let name_start = j;
        while j < chars.len() && chars[j] != '>' && chars[j] != '<' && chars[j] != '=' && chars[j] != ' ' {
            j += 1;
        }
        let name: String = chars[name_start..j].iter().collect();
        // The tag has to actually close before another one opens, or TMP prints it verbatim
        while j < chars.len() && chars[j] != '>' && chars[j] != '<' {
            j += 1;
        }
        if j < chars.len() && chars[j] == '>' {
            rv.push(name.to_lowercase());
            i = j + 1;
        } else {
            i += 1;
        }
    }
    rv
}

// Reject text carrying a rich-text tag the field is not supposed to have. `allowed` holds
// lowercase tag names without the slash, so listing "size" permits both <size=80> and </size>.
pub fn reject_tags(label: &str, text: &str, allowed: &[&str]) -> Result<(), String> {
    for tag in tags(text) {
        if !allowed.contains(&tag.as_str()) {
            return Err(format!(
                "{} may not contain the rich text tag <{}> - the game renders it as formatting and it would break the screens it appears on",
                label, tag
            ));
        }
    }
    Ok(())
}

// Drop every rich-text tag, keeping the text between them. For strings that are ALREADY stored
// and cannot be rejected at the point of use - an uploader's account name, which ew accepts
// verbatim on the profile route.
pub fn strip_tags(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut rv = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            let mut j = i + 1;
            if j < chars.len() && chars[j] == '/' {
                j += 1;
            }
            if j < chars.len() && (chars[j].is_ascii_alphabetic() || chars[j] == '#') {
                while j < chars.len() && chars[j] != '>' && chars[j] != '<' {
                    j += 1;
                }
                if j < chars.len() && chars[j] == '>' {
                    i = j + 1;
                    continue;
                }
            }
        }
        rv.push(chars[i]);
        i += 1;
    }
    rv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_real_tags_are_tags() {
        // Literal text TMP never reads as formatting
        for text in ["I <3 you", "a < b", "3 <", "<<", "1<2", "<b unclosed", "<b never closes"] {
            assert!(reject_tags("Name", text, &[]).is_ok(), "{}", text);
        }
        // Everything TMP would format with
        for text in ["<b>x", "x</b>", "<size=400%>x", "<sprite=1>", "<#ff0000>x", "<color=red>x",
                     "<font=\"none\">x", "<rotate=45>x", "<noparse>x", "<voffset=5em>x",
                     // the unclosed opener is literal, but the tag after it is not
                     "<b <i>x"] {
            assert!(reject_tags("Name", text, &[]).is_err(), "{}", text);
        }
    }

    #[test]
    fn allowed_tags_pass_and_others_still_dont() {
        assert!(reject_tags("Description", "line one<br>line two", &["br"]).is_ok());
        assert!(reject_tags("Description", "line one<br>line two", &[]).is_err());
        // The slash form of an allowed tag is allowed too
        assert!(reject_tags("Gacha name", "<size=80>Mari</size>", &["size"]).is_ok());
        assert!(reject_tags("Gacha name", "<size=80><sprite=3>", &["size"]).is_err());
    }

    #[test]
    fn the_error_names_the_field_and_the_tag() {
        let error = reject_tags("Song name", "<size=400%>boom", &[]).unwrap_err();
        assert!(error.contains("Song name"), "{}", error);
        assert!(error.contains("<size>"), "{}", error);
    }

    #[test]
    fn stripping_keeps_the_words_and_the_harmless_angle_brackets() {
        assert_eq!(strip_tags("<size=400%>Nozomi</size>"), "Nozomi");
        assert_eq!(strip_tags("I <3 <b>you</b>"), "I <3 you");
        assert_eq!(strip_tags("plain name"), "plain name");
        assert_eq!(strip_tags("<b unclosed"), "<b unclosed");
    }
}

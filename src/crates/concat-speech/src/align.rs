// SPDX-License-Identifier: AGPL-3.0-or-later
//! Captions in a script's words, timed by what the transcriber heard.
//!
//! A transcriber mishears: a homophone, a name, a number. The person who
//! wrote the script knows the words; the transcription knows when they
//! were said. [`to_script`] keeps the one and takes the other: both are
//! cut down to their letters, digits and CJK characters (punctuation and
//! spacing never say anything about timing), lined up character by
//! character by edit distance, and every script character takes the time
//! of the heard character it lines up with. A script character nothing was
//! heard for is spread over the gap between its neighbours; heard words
//! the script does not have (a slip, an aside) are left out.
//!
//! The script's own punctuation and line breaks decide where one caption
//! ends and the next begins, so the person writing it controls the cut:
//! a comma, full stop, question or exclamation mark, semicolon, colon or a
//! line break ends a caption, and the commas and full stops are not shown.
//!
//! Whisper times whole segments, not characters, so a heard character's
//! time is its share of its segment's span, weighted by how long it takes
//! to say. When the transcriber gives character times, they go in here
//! unchanged and the captions get sharper without anything else changing.

use crate::transcribe::Segment;

/// The most cells the alignment table may have: script characters times
/// heard characters, a byte each. About 40 MB, or two scripts of some
/// 6,000 characters each - twenty-odd minutes of speech.
pub const MAX_CELLS: usize = 40_000_000;

/// The shortest a caption stays on screen, in seconds.
const MIN_SECONDS: f64 = 0.3;

/// A gap between two captions shorter than this is closed by holding the
/// first until the second, so the line does not blink off for a breath.
const BRIDGE_SECONDS: f64 = 0.4;

/// A script laid over a transcription.
#[derive(Debug, Clone, PartialEq)]
pub struct Aligned {
    /// One caption per script phrase, in the script's words, timed by the
    /// transcription; in the same seconds as the segments given.
    pub segments: Vec<Segment>,
    /// The share of the script's characters heard exactly as written,
    /// from 0 to 1. Low means the script and the voice say different
    /// things - the wrong script, or the wrong clip.
    pub matched: f32,
    /// Script phrases nothing at all was heard for, in order. They are
    /// not among [`Self::segments`].
    pub unheard: Vec<String>,
}

/// When one character was said.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Span {
    start: f64,
    end: f64,
}

/// The script's phrases, in its words, timed by `heard`.
///
/// Fails when the script has no words, nothing was heard, or the two are
/// too long to line up (see [`MAX_CELLS`]).
pub fn to_script(heard: &[Segment], script: &str) -> Result<Aligned, String> {
    let phrases = phrases(script);
    let mut keys = Vec::new();
    let mut ranges = Vec::with_capacity(phrases.len());
    for phrase in &phrases {
        let from = keys.len();
        keys.extend(phrase.chars().filter_map(key));
        ranges.push(from..keys.len());
    }
    if keys.is_empty() {
        return Err("the script has no words to caption".to_owned());
    }
    let (spoken, times) = heard_keys(heard);
    if spoken.is_empty() {
        return Err("nothing was said in the clip".to_owned());
    }
    if keys.len().saturating_mul(spoken.len()) > MAX_CELLS {
        return Err(format!(
            "the script ({} characters) and the clip ({} characters heard) are too long to line up at once; caption it in parts",
            keys.len(),
            spoken.len()
        ));
    }

    let pairs = align(&keys, &spoken);
    let exact = pairs
        .iter()
        .zip(&keys)
        .filter(|(pair, key)| pair.is_some_and(|j| spoken[j] == **key))
        .count();
    let spans = key_spans(&pairs, &times);

    let mut segments: Vec<Segment> = Vec::with_capacity(phrases.len());
    let mut unheard = Vec::new();
    for (phrase, range) in phrases.into_iter().zip(ranges) {
        // Every phrase has a key: `phrases` keeps no other.
        if pairs[range.clone()].iter().all(Option::is_none) {
            unheard.push(phrase);
            continue;
        }
        segments.push(Segment {
            start: spans[range.start].start,
            end: spans[range.end - 1].end,
            text: phrase,
        });
    }
    settle(&mut segments);

    Ok(Aligned {
        segments,
        matched: exact as f32 / keys.len() as f32,
        unheard,
    })
}

/// A script cut into the phrases that become captions, as they are
/// shown: a line break, or a comma, full stop, question or exclamation
/// mark, semicolon, colon or ellipsis ends one. The ASCII marks end one
/// only before a space or the line's end, so "3.5", "1,000" and "10:30"
/// hold together. Closing quotes and brackets stay with the phrase they
/// close; trailing commas, full stops, semicolons, colons and ellipses are
/// dropped, question and exclamation marks kept. A phrase with no letter,
/// digit or CJK character in it is not one.
pub fn phrases(script: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in script.lines() {
        let mut current = String::new();
        let mut chars = line.chars().peekable();
        while let Some(ch) = chars.next() {
            current.push(ch);
            let ends = match ch {
                '。' | '！' | '？' | '，' | '；' | '：' | '…' => true,
                '.' | '!' | '?' | ',' | ';' | ':' => {
                    chars.peek().is_none_or(|next| next.is_whitespace())
                }
                _ => false,
            };
            if ends {
                while let Some(&next) = chars.peek() {
                    if closes(next) || matches!(next, '。' | '！' | '？' | '…' | '!' | '?') {
                        current.push(next);
                        chars.next();
                    } else {
                        break;
                    }
                }
                keep_phrase(&mut out, &mut current);
            }
        }
        keep_phrase(&mut out, &mut current);
    }
    out
}

/// `current`, tidied, onto `out` if it says anything; `current` emptied.
fn keep_phrase(out: &mut Vec<String>, current: &mut String) {
    let mut shown = current.trim().to_owned();
    while shown.ends_with(['，', '。', '；', '：', '、', '…', ',', '.', ';', ':']) {
        shown.pop();
    }
    let shown = shown.trim_end();
    if shown.chars().any(|ch| key(ch).is_some()) {
        out.push(shown.to_owned());
    }
    current.clear();
}

/// A closing quote or bracket.
fn closes(ch: char) -> bool {
    matches!(
        ch,
        '”' | '’' | '」' | '』' | '）' | '》' | '】' | ')' | ']' | '"' | '\''
    )
}

/// The character as the alignment compares it, or `None` for one it
/// skips: full-width forms folded to ASCII, letters to lower case, and
/// anything that is not a letter, digit or CJK character dropped.
fn key(ch: char) -> Option<char> {
    let ch = match ch {
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(ch as u32 - 0xFEE0).unwrap_or(ch),
        _ => ch,
    };
    ch.is_alphanumeric()
        .then(|| ch.to_lowercase().next().unwrap_or(ch))
}

/// How long a character takes to say, relative to a CJK syllable: a
/// Latin letter is a fraction of a syllable, a digit about one.
fn weight(ch: char) -> f64 {
    if ch.is_ascii_alphabetic() { 0.35 } else { 1.0 }
}

/// What was heard as keys, each with the share of its segment's span its
/// weight gives it.
fn heard_keys(heard: &[Segment]) -> (Vec<char>, Vec<Span>) {
    let mut keys = Vec::new();
    let mut spans = Vec::new();
    for segment in heard {
        let chars: Vec<char> = segment.text.chars().filter_map(key).collect();
        let total: f64 = chars.iter().copied().map(weight).sum();
        if total <= 0.0 {
            continue;
        }
        let length = (segment.end - segment.start).max(0.0);
        let mut at = 0.0;
        for ch in chars {
            let next = at + weight(ch);
            spans.push(Span {
                start: segment.start + length * at / total,
                end: segment.start + length * next / total,
            });
            keys.push(ch);
            at = next;
        }
    }
    (keys, spans)
}

/// What a substitution costs: a misheard character.
const SUBSTITUTE: u32 = 2;
/// What starting a run of unheard script, or of heard words not in the
/// script, costs on top of [`GAP_EACH`] per character. Skipped sentences
/// and asides come whole, so one long run is cheaper than the same
/// characters scattered - which keeps a skipped sentence from borrowing
/// the matching characters of the phrase beside it.
const GAP_OPEN: u32 = 2;
/// What each character of such a run costs.
const GAP_EACH: u32 = 1;

/// The alignment of `script` to `heard`, by edit distance with affine
/// gaps (Gotoh): for each script character, the heard character it is
/// matched with or stands in for, or `None` when nothing was heard for it.
/// Matches are free. A substitution costs less than an unheard character
/// and an extra one together, so a misheard character keeps its time.
fn align(script: &[char], heard: &[char]) -> Vec<Option<usize>> {
    // The three ways a cell is reached: from its diagonal (a match or a
    // substitution), from above (a script character unheard) or from the
    // left (a heard character not in the script). Each cell keeps, per
    // way, which way the cell it came from was reached: two bits each.
    const PAIR: u8 = 0;
    const UNHEARD: u8 = 1;
    const EXTRA: u8 = 2;
    const NEVER: u32 = u32::MAX / 4;
    let (n, m) = (script.len(), heard.len());
    let width = m + 1;
    let pack = |pair: u8, unheard: u8, extra: u8| pair | unheard << 2 | extra << 4;
    let mut way = vec![0u8; (n + 1) * width];

    // Row 0: only heard characters, one run from the start.
    let mut above_pair = vec![NEVER; width];
    let mut above_unheard = vec![NEVER; width];
    let mut above_extra = vec![NEVER; width];
    above_pair[0] = 0;
    for j in 1..=m {
        above_extra[j] = GAP_OPEN + GAP_EACH * j as u32;
        way[j] = pack(PAIR, PAIR, if j == 1 { PAIR } else { EXTRA });
    }
    let mut pair = vec![NEVER; width];
    let mut unheard = vec![NEVER; width];
    let mut extra = vec![NEVER; width];

    let best = |p: u32, u: u32, e: u32| -> (u32, u8) {
        if p <= u && p <= e {
            (p, PAIR)
        } else if u <= e {
            (u, UNHEARD)
        } else {
            (e, EXTRA)
        }
    };

    for i in 1..=n {
        // Column 0: only script characters, one run from the start.
        pair[0] = NEVER;
        extra[0] = NEVER;
        unheard[0] = GAP_OPEN + GAP_EACH * i as u32;
        way[i * width] = pack(PAIR, if i == 1 { PAIR } else { UNHEARD }, PAIR);
        for j in 1..=m {
            let cost = if script[i - 1] == heard[j - 1] {
                0
            } else {
                SUBSTITUTE
            };
            let (p, p_from) = best(above_pair[j - 1], above_unheard[j - 1], above_extra[j - 1]);
            let (u, u_from) = best(
                above_pair[j] + GAP_OPEN,
                above_unheard[j],
                above_extra[j] + GAP_OPEN,
            );
            let (e, e_from) = best(
                pair[j - 1] + GAP_OPEN,
                unheard[j - 1] + GAP_OPEN,
                extra[j - 1],
            );
            pair[j] = p + cost;
            unheard[j] = u + GAP_EACH;
            extra[j] = e + GAP_EACH;
            way[i * width + j] = pack(p_from, u_from, e_from);
        }
        std::mem::swap(&mut above_pair, &mut pair);
        std::mem::swap(&mut above_unheard, &mut unheard);
        std::mem::swap(&mut above_extra, &mut extra);
    }

    let mut pairs = vec![None; n];
    let (mut i, mut j) = (n, m);
    let mut layer = best(above_pair[m], above_unheard[m], above_extra[m]).1;
    while i > 0 || j > 0 {
        let cell = way[i * width + j];
        match layer {
            PAIR => {
                layer = cell & 3;
                pairs[i - 1] = Some(j - 1);
                i -= 1;
                j -= 1;
            }
            UNHEARD => {
                layer = cell >> 2 & 3;
                i -= 1;
            }
            _ => {
                layer = cell >> 4 & 3;
                j -= 1;
            }
        }
    }
    pairs
}

/// Each script character's time: its heard character's, or for a run
/// nothing was heard for, an even share of the gap between the characters
/// either side of it.
fn key_spans(pairs: &[Option<usize>], times: &[Span]) -> Vec<Span> {
    let n = pairs.len();
    let mut spans = vec![
        Span {
            start: 0.0,
            end: 0.0
        };
        n
    ];
    let mut i = 0;
    while i < n {
        if let Some(j) = pairs[i] {
            spans[i] = times[j];
            i += 1;
            continue;
        }
        let first = i;
        while i < n && pairs[i].is_none() {
            i += 1;
        }
        let next = pairs.get(i).copied().flatten().map(|j| times[j].start);
        let from = if first > 0 {
            spans[first - 1].end
        } else {
            next.unwrap_or(times[0].start)
        };
        let to = next.unwrap_or(from).max(from);
        let count = (i - first) as f64;
        for (k, span) in spans[first..i].iter_mut().enumerate() {
            *span = Span {
                start: from + (to - from) * k as f64 / count,
                end: from + (to - from) * (k + 1) as f64 / count,
            };
        }
    }
    spans
}

/// Captions made watchable: none starts before the one before it ends,
/// each stays up at least [`MIN_SECONDS`], and a gap shorter than
/// [`BRIDGE_SECONDS`] is closed.
fn settle(segments: &mut [Segment]) {
    for k in 0..segments.len() {
        if k > 0 {
            let before = segments[k - 1].end;
            if segments[k].start < before {
                segments[k].start = before;
            }
        }
        let shortest = segments[k].start + MIN_SECONDS;
        if segments[k].end < shortest {
            segments[k].end = shortest;
        }
    }
    for k in 1..segments.len() {
        let gap = segments[k].start - segments[k - 1].end;
        if gap > 0.0 && gap < BRIDGE_SECONDS {
            segments[k - 1].end = segments[k].start;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heard(start: f64, end: f64, text: &str) -> Segment {
        Segment {
            start,
            end,
            text: text.to_owned(),
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn the_scripts_words_replace_what_was_misheard_and_keep_its_timing() {
        let said = [
            heard(0.0, 2.0, "今天我们来讲重庆的高考分素线"),
            heard(2.5, 4.0, "第一档是本科保低线"),
        ];
        let script = "今天我们来讲重庆的高考分数线。\n第一档：本科保底线。";
        let aligned = to_script(&said, script).unwrap();
        let texts: Vec<&str> = aligned.segments.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(
            texts,
            ["今天我们来讲重庆的高考分数线", "第一档", "本科保底线"]
        );
        assert!(close(aligned.segments[0].start, 0.0));
        assert!(close(aligned.segments[0].end, 2.0));
        assert!(close(aligned.segments[1].start, 2.5));
        assert!(close(aligned.segments[2].end, 4.0));
        // 分素线 → 分数线, 保低线 → 保底线: two of 22 script characters differ.
        assert!((aligned.matched - 20.0 / 22.0).abs() < 1e-6);
        assert!(aligned.unheard.is_empty());
    }

    #[test]
    fn words_said_but_not_in_the_script_are_left_out() {
        let said = [heard(0.0, 4.0, "嗯那个今天好")];
        let aligned = to_script(&said, "今天好").unwrap();
        assert_eq!(aligned.segments.len(), 1);
        assert_eq!(aligned.segments[0].text, "今天好");
        // 嗯那个 takes the first half of the span.
        assert!(close(aligned.segments[0].start, 2.0));
        assert!(close(aligned.segments[0].end, 4.0));
        assert!(close(f64::from(aligned.matched), 1.0));
    }

    #[test]
    fn a_phrase_nothing_was_heard_for_is_reported_not_placed() {
        let said = [heard(0.0, 1.0, "第一句"), heard(5.0, 6.0, "第三句")];
        let aligned = to_script(&said, "第一句。这一句老师没有说出来。第三句。").unwrap();
        let texts: Vec<&str> = aligned.segments.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["第一句", "第三句"]);
        assert_eq!(aligned.unheard, ["这一句老师没有说出来"]);
        assert!(close(aligned.segments[1].start, 5.0));
    }

    #[test]
    fn a_short_gap_is_bridged_and_captions_never_overlap() {
        let said = [heard(0.0, 1.0, "一二"), heard(1.2, 2.0, "三四")];
        let aligned = to_script(&said, "一二，三四").unwrap();
        let [a, b] = &aligned.segments[..] else {
            panic!("two captions")
        };
        assert!(close(a.end, b.start));
        assert!(close(b.start, 1.2));
    }

    #[test]
    fn the_script_punctuation_cuts_the_phrases() {
        assert_eq!(
            phrases("分数线是1,000分，比去年高3.5倍！真的吗？“好”。\n\n下一段：10:30开始…"),
            [
                "分数线是1,000分",
                "比去年高3.5倍！",
                "真的吗？",
                "“好”",
                "下一段",
                "10:30开始"
            ]
        );
        assert_eq!(phrases("Hello, world. Fine"), ["Hello", "world", "Fine"]);
        assert!(phrases("。，……\n  \n").is_empty());
    }

    #[test]
    fn full_width_and_case_do_not_count_as_differences() {
        let said = [heard(0.0, 1.0, "ABC１２３")];
        let aligned = to_script(&said, "abc123").unwrap();
        assert!(close(f64::from(aligned.matched), 1.0));
    }

    #[test]
    fn an_empty_script_or_silence_is_an_error() {
        assert!(to_script(&[heard(0.0, 1.0, "你好")], "。。").is_err());
        assert!(to_script(&[], "你好").is_err());
    }
}

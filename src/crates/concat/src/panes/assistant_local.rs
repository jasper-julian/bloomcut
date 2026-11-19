// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat contributors

//! The AI assistant's own tools and the person's rules.
//!
//! The tools `concat --mcp` offers are one edit a call, and a title's
//! words are styled by character offsets the caller counts. Marking the
//! key words of a whole video's captions that way is dozens of round trips
//! to the model and a count of Chinese characters it gets wrong. So the
//! assistant has two tools of its own: [`CAPTIONS_LIST`], every title on
//! the timeline in one short list, and [`HIGHLIGHT_WORDS`], which takes
//! the words themselves, finds them in each title here, and applies every
//! look as one batch - one round trip, and one Undo.
//!
//! The rules are the person's: Markdown files in a folder of the app's
//! config (`assistant-rules`), every one of them handed to the model after
//! its own instructions, so how this person wants captions marked - which
//! words, how many, in which colours - is changed by editing a file, not
//! by building Concat again.

use std::path::{Path, PathBuf};

use concat_project::commands::Command;
use concat_project::model::{ClipKind, RunStyle};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::studio::Studio;

/// Lists the titles.
pub const CAPTIONS_LIST: &str = "captions_list";
/// Styles words of titles, found by their text.
pub const HIGHLIGHT_WORDS: &str = "highlight_words";

/// The most rules handed to the model, in characters: a folder that has
/// gathered more is cut, rather than every round paying for all of it.
const RULES_CHARS: usize = 12_000;

/// The folder the rules are kept in.
pub fn rules_folder(config: &Path) -> PathBuf {
    config.join("assistant-rules")
}

/// The rule files, by name, in name order.
pub fn rule_files(config: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(rules_folder(config))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| {
                            extension.eq_ignore_ascii_case("md")
                                || extension.eq_ignore_ascii_case("txt")
                        })
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

/// Every rule file's words, each under its name, for the model; empty when
/// there are none.
pub fn rules(config: &Path) -> String {
    let mut text = String::new();
    for file in rule_files(config) {
        let Ok(words) = std::fs::read_to_string(&file) else {
            log::warn!("assistant: could not read the rules in {}", file.display());
            continue;
        };
        let name = file
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        text.push_str(&format!("=== {name} ===\n{}\n\n", words.trim()));
    }
    if text.chars().count() > RULES_CHARS {
        log::warn!(
            "assistant: the rules are longer than {RULES_CHARS} characters; the rest is left out"
        );
        text = text.chars().take(RULES_CHARS).collect();
    }
    text
}

/// The two tools in the chat-completions form.
pub fn specs() -> Vec<Value> {
    let style = json!({
        "type": "object",
        "description": "The look of the words. Leave out what stays as the caption's own.",
        "properties": {
            "color": { "type": "string", "description": "Fill colour, \"#rrggbb\"." },
            "fontWeight": { "type": "number", "minimum": 100, "maximum": 900, "description": "400 regular, 700 bold, 900 black." },
            "fontScale": { "type": "number", "minimum": 0.1, "maximum": 10, "description": "Size as a multiple of the caption's own: 1.3 is 30% larger." },
            "strokeColor": { "type": "string", "description": "Outline colour, \"#rrggbb\"." },
            "italic": { "type": "boolean" },
            "underline": { "type": "boolean" }
        }
    });
    vec![
        json!({
            "type": "function",
            "function": {
                "name": CAPTIONS_LIST,
                "description": "Every caption and title on the timeline in time order: its clip id, when it starts and ends (seconds), its words, and the words already styled. Use this rather than timeline_read when working on captions' words.",
                "parameters": { "type": "object", "properties": {} }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": HIGHLIGHT_WORDS,
                "description": "Marks key words in captions: each item names a caption by its clip id and a word or phrase exactly as it is written in that caption, and gives it a look. The word is found in the caption's text here - never count characters yourself. Everything in one call is one edit, undone with one Undo, so put all the captions in a single call. Items whose word is not in the caption are reported back and nothing else is held up.",
                "parameters": {
                    "type": "object",
                    "required": ["items"],
                    "properties": {
                        "clearFirst": { "type": "boolean", "description": "Take every earlier word style off the captions named before marking them: true when marking again from scratch." },
                        "items": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "required": ["clipId", "word", "style"],
                                "properties": {
                                    "clipId": { "type": "string" },
                                    "word": { "type": "string", "description": "The words to mark, copied exactly from the caption; the whole caption's text to mark all of it." },
                                    "occurrence": { "type": "integer", "minimum": 1, "description": "Which time the word appears in the caption, from 1. Default 1." },
                                    "style": style
                                }
                            }
                        }
                    }
                }
            }
        }),
    ]
}

/// Whether `name` is one of these tools.
pub fn is_local(name: &str) -> bool {
    name == CAPTIONS_LIST || name == HIGHLIGHT_WORDS
}

/// [`CAPTIONS_LIST`] on the window's project.
pub fn captions_list(studio: &Studio) -> Result<Value, String> {
    let mut titles: Vec<Value> = studio
        .timeline()
        .clips
        .iter()
        .filter(|clip| clip.kind == ClipKind::Text)
        .filter_map(|clip| {
            let text = clip.text.as_ref()?;
            let styled: Vec<String> = text
                .runs
                .iter()
                .map(|run| {
                    text.content
                        .chars()
                        .skip(run.start)
                        .take(run.end.saturating_sub(run.start))
                        .collect()
                })
                .collect();
            let mut entry = json!({
                "clipId": clip.id,
                "start": round(clip.start),
                "end": round(clip.start + clip.duration),
                "text": text.content,
            });
            if !styled.is_empty() {
                entry["styled"] = json!(styled);
            }
            Some(entry)
        })
        .collect();
    titles.sort_by(|a, b| {
        a["start"]
            .as_f64()
            .unwrap_or(0.0)
            .total_cmp(&b["start"].as_f64().unwrap_or(0.0))
    });
    Ok(json!({ "count": titles.len(), "captions": titles }))
}

fn round(seconds: f64) -> f64 {
    (seconds * 100.0).round() / 100.0
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Highlight {
    #[serde(default)]
    clear_first: bool,
    items: Vec<Item>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Item {
    clip_id: String,
    word: String,
    #[serde(default)]
    occurrence: Option<usize>,
    style: RunStyle,
}

/// [`HIGHLIGHT_WORDS`] on the window's project: one batch, one undo step.
pub fn highlight_words(studio: &mut Studio, arguments: Value) -> Result<Value, String> {
    let asked: Highlight = serde_json::from_value(arguments)
        .map_err(|error| format!("the arguments do not fit the tool: {error}"))?;
    let mut commands = Vec::new();
    let mut cleared: Vec<String> = Vec::new();
    let mut missed = Vec::new();
    let mut marked = 0;
    for item in asked.items {
        let content = studio
            .timeline()
            .clips
            .iter()
            .find(|clip| clip.id == item.clip_id)
            .and_then(|clip| clip.text.as_ref())
            .map(|text| text.content.clone());
        let Some(content) = content else {
            missed.push(json!({ "clipId": item.clip_id, "word": item.word, "why": "no caption has this clip id" }));
            continue;
        };
        let Some((start, end)) = find(&content, &item.word, item.occurrence.unwrap_or(1)) else {
            missed.push(json!({ "clipId": item.clip_id, "word": item.word, "why": format!("not in its text {content:?}") }));
            continue;
        };
        if asked.clear_first && !cleared.contains(&item.clip_id) {
            commands.push(Command::ClearTextStyles {
                clip_id: item.clip_id.clone(),
                start: None,
                end: None,
            });
            cleared.push(item.clip_id.clone());
        }
        commands.push(Command::StyleTextRange {
            clip_id: item.clip_id,
            start,
            end,
            patch: item.style,
        });
        marked += 1;
    }
    if !commands.is_empty() {
        studio.apply_reporting(Command::Batch { commands })?;
    }
    Ok(json!({ "marked": marked, "notFound": missed }))
}

/// Where the `occurrence`th `word` is in `text`, in characters: start, and
/// one past the end.
fn find(text: &str, word: &str, occurrence: usize) -> Option<(usize, usize)> {
    let word = word.trim();
    if word.is_empty() {
        return None;
    }
    let byte = text
        .match_indices(word)
        .nth(occurrence.max(1) - 1)
        .map(|(byte, _)| byte)?;
    let start = text[..byte].chars().count();
    Some((start, start + word.chars().count()))
}

#[cfg(test)]
mod tests {
    use super::{find, specs};

    /// Words are found by characters, not bytes, and by which time they
    /// come.
    #[test]
    fn a_word_is_found_in_characters() {
        assert_eq!(find("目标稳拿", "稳拿", 1), Some((2, 4)));
        assert_eq!(find("我们看张雪 他呢", "张雪", 1), Some((3, 5)));
        assert_eq!(find("冲武大、武大", "武大", 2), Some((4, 6)));
        assert_eq!(find("目标稳拿", "稳妥", 1), None);
        assert_eq!(find("目标稳拿", " ", 1), None);
    }

    #[test]
    fn the_tools_are_objects_with_names() {
        for spec in specs() {
            assert_eq!(spec["function"]["parameters"]["type"], "object");
        }
    }
}

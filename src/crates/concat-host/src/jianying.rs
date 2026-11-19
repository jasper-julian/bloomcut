// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 BloomCut contributors

//! Writing the project as a 剪映 (JianyingPro) draft: the file half of
//! `concat_project::jianying`.
//!
//! Where 剪映 keeps its drafts on this machine - the folder its settings
//! name (`currentCustomDraftPath` in `User Data\Config\globalSetting`), or
//! its own default - and a new folder there for the draft, named for the
//! project and never over one that is there. 剪映 lists every folder of
//! its drafts folder that holds a draft, so the next time it shows its
//! list, the draft is in it.

use std::path::{Path, PathBuf};

use concat_project::jianying::{self, Target};
use concat_project::model::Project;

/// 剪映's own folder under `%LOCALAPPDATA%`.
fn jianying_home() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|base| PathBuf::from(base).join("JianyingPro"))
}

/// 剪映's drafts folder on this machine, where there is one: the folder its
/// settings name, else where it keeps drafts when none is named.
pub fn drafts_folder() -> Option<PathBuf> {
    let home = jianying_home()?;
    let user = home.join("User Data");
    let named = std::fs::read_to_string(user.join("Config").join("globalSetting"))
        .ok()
        .and_then(|settings| setting(&settings, "currentCustomDraftPath"))
        .map(PathBuf::from)
        .filter(|folder| folder.is_dir());
    named.or_else(|| {
        Some(user.join("Projects").join("com.lveditor.draft")).filter(|folder| folder.is_dir())
    })
}

/// Whether 剪映 is installed here at all: its folder is.
pub fn installed() -> bool {
    jianying_home().is_some_and(|home| home.is_dir())
}

/// The value of `key` in a Qt INI file, unescaped: `\\` is a backslash and
/// `\xNNNN` a character, as Qt writes a path with Chinese in it.
fn setting(ini: &str, key: &str) -> Option<String> {
    let raw = ini.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim().to_owned())
    })?;
    let raw = raw.trim_matches('"');
    let mut value = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '\\' {
            value.push(character);
            continue;
        }
        match chars.next() {
            Some('x') => {
                let hex: String =
                    std::iter::from_fn(|| chars.next_if(char::is_ascii_hexdigit)).collect();
                if let Some(decoded) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    value.push(decoded);
                }
            }
            Some(other) => value.push(other),
            None => value.push('\\'),
        }
    }
    (!value.is_empty()).then_some(value)
}

/// What an export came to.
pub struct Written {
    /// The draft's name as 剪映 lists it.
    pub name: String,
    /// Its folder.
    pub folder: PathBuf,
    /// What did not come across.
    pub notes: Vec<String>,
}

/// Writes `project`'s active timeline as a new draft named `name` in
/// `drafts`. A draft of that name already there is left alone, and this one
/// is named `name (2)`, `name (3)`, ...
pub fn export(project: &Project, name: &str, drafts: &Path) -> Result<Written, String> {
    let base: String = name
        .chars()
        .map(|character| {
            if r#"\/:*?"<>|"#.contains(character) {
                '_'
            } else {
                character
            }
        })
        .collect();
    let base = base.trim().trim_end_matches('.');
    let base = if base.is_empty() { "BloomCut" } else { base };
    let (name, folder) = (1..1000)
        .map(|n| {
            if n == 1 {
                base.to_owned()
            } else {
                format!("{base} ({n})")
            }
        })
        .map(|name| {
            let folder = drafts.join(&name);
            (name, folder)
        })
        .find(|(_, folder)| !folder.exists())
        .ok_or_else(|| {
            format!(
                "{} already has a thousand drafts called {base}",
                drafts.display()
            )
        })?;
    let now_micros = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_micros() as i64);
    let exported = jianying::export(
        project,
        &Target {
            name: &name,
            folder: &folder.to_string_lossy(),
            root: &drafts.to_string_lossy(),
            now_micros,
        },
    );
    let write = |file: &str, value: &serde_json::Value| {
        std::fs::write(folder.join(file), value.to_string())
            .map_err(|error| format!("could not write {}: {error}", folder.join(file).display()))
    };
    std::fs::create_dir_all(&folder)
        .map_err(|error| format!("could not make {}: {error}", folder.display()))?;
    let written = write("draft_content.json", &exported.content)
        .and_then(|()| write("draft_meta_info.json", &exported.meta));
    if let Err(error) = written {
        let _ = std::fs::remove_dir_all(&folder);
        return Err(error);
    }
    log::info!(
        "jianying: wrote the draft {name} to {} ({} kinds not carried)",
        folder.display(),
        exported.notes.len()
    );
    Ok(Written {
        name,
        folder,
        notes: exported.notes,
    })
}

#[cfg(test)]
mod tests {
    use super::{export, setting};

    #[test]
    fn a_qt_setting_is_read_unescaped() {
        let ini = "[General]\r\nother=1\r\ncurrentCustomDraftPath=G:\\\\JianyingPro Drafts\r\n";
        assert_eq!(
            setting(ini, "currentCustomDraftPath").as_deref(),
            Some(r"G:\JianyingPro Drafts")
        );
        let chinese = "currentCustomDraftPath=D:\\\\\\x526a\\x6620\\x8349\\x7a3f\n";
        assert_eq!(
            setting(chinese, "currentCustomDraftPath").as_deref(),
            Some(r"D:\剪映草稿")
        );
        assert_eq!(setting(ini, "missing"), None);
    }

    /// A second export of the same name is a draft of its own beside the
    /// first, and both hold the two files 剪映 reads.
    #[test]
    fn a_draft_is_never_written_over() {
        let drafts = std::env::temp_dir().join(format!("bloomcut-jianying-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&drafts);
        std::fs::create_dir_all(&drafts).unwrap();
        let project = concat_project::model::Project::default();
        let first = export(&project, "口播:第1条", &drafts).unwrap();
        let second = export(&project, "口播:第1条", &drafts).unwrap();
        assert_eq!(first.name, "口播_第1条");
        assert_eq!(second.name, "口播_第1条 (2)");
        for written in [&first, &second] {
            assert!(written.folder.join("draft_content.json").is_file());
            assert!(written.folder.join("draft_meta_info.json").is_file());
        }
        let _ = std::fs::remove_dir_all(&drafts);
    }

    /// Writes a sample draft - a video and three captions with marked words
    /// in the three looks of the team's rules - into the drafts folder
    /// `BLOOMCUT_SAMPLE_DRAFTS` names, from the video `BLOOMCUT_SAMPLE_VIDEO`,
    /// to open in 剪映 by hand. `cargo test -- --ignored write_a_sample_draft`.
    #[test]
    #[ignore = "writes into a real drafts folder"]
    fn write_a_sample_draft() {
        use concat_project::model::{
            Clip, ClipKind, MediaItem, MediaKind, Project, RunStyle, TextRun, TextStyle, Timeline,
            Track, VideoSettings,
        };
        use std::sync::Arc;
        let drafts = std::env::var("BLOOMCUT_SAMPLE_DRAFTS").expect("BLOOMCUT_SAMPLE_DRAFTS");
        let video = std::env::var("BLOOMCUT_SAMPLE_VIDEO").expect("BLOOMCUT_SAMPLE_VIDEO");
        let mut clips = Vec::new();
        let mut cut = Clip::blank("c1", "t3", ClipKind::Video, "sample", 0.0, 10.0);
        cut.media_id = "m1".into();
        clips.push(Arc::new(cut));
        let captions: [(&str, f64, (usize, usize), RunStyle); 3] = [
            (
                "目标稳拿不是问题",
                0.5,
                (2, 4),
                RunStyle {
                    color: Some("#ffe400".into()),
                    ..RunStyle::default()
                },
            ),
            (
                "普通家庭没有渠道",
                3.5,
                (4, 8),
                RunStyle {
                    color: Some("#ffe400".into()),
                    font_weight: Some(800.0),
                    font_scale: Some(1.3),
                    stroke_color: Some("#e84820".into()),
                    ..RunStyle::default()
                },
            ),
            (
                "就失业了",
                6.5,
                (0, 4),
                RunStyle {
                    color: Some("#ffe400".into()),
                    font_weight: Some(900.0),
                    font_scale: Some(1.8),
                    ..RunStyle::default()
                },
            ),
        ];
        for (n, (words, start, (from, to), look)) in captions.into_iter().enumerate() {
            let mut title = Clip::blank(format!("t{n}"), "t1", ClipKind::Text, words, start, 2.8);
            title.offset_y = 0.32;
            title.text = Some(TextStyle {
                content: words.into(),
                font_size: 0.05,
                stroke_width: 0.004,
                runs: vec![TextRun {
                    start: from,
                    end: to,
                    style: look,
                }],
                ..TextStyle::default()
            });
            clips.push(Arc::new(title));
        }
        let timeline = Timeline {
            id: "tl".into(),
            video: VideoSettings {
                width: 1920,
                height: 1080,
                rate_num: 30,
                rate_den: 1,
                ..VideoSettings::default()
            },
            tracks: ["t1", "t3"]
                .map(|id| Track {
                    id: id.into(),
                    ..Track::default()
                })
                .to_vec(),
            clips,
            ..Timeline::default()
        };
        let project = Project {
            media: vec![MediaItem {
                id: "m1".into(),
                path: video.clone(),
                name: std::path::Path::new(&video)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                duration: Some(10.01),
                kind: MediaKind::Video,
                width: Some(1920),
                height: Some(1080),
                has_audio: true,
                ..MediaItem::default()
            }],
            active_timeline_id: "tl".into(),
            timelines: vec![Arc::new(timeline)],
            ..Project::default()
        };
        let written = export(&project, "BloomCut导出测试", std::path::Path::new(&drafts)).unwrap();
        println!("wrote {}", written.folder.display());
    }
}

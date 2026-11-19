// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 BloomCut contributors

//! A timeline written out as a 剪映 (JianyingPro) draft.
//!
//! The other way from [`crate::capcut`]: a project in, the two JSON files
//! of a draft folder out - `draft_content.json`, the edit, and
//! `draft_meta_info.json`, what the drafts list shows - nothing read or
//! written. The host finds 剪映's drafts folder and writes them there.
//!
//! 剪映 encrypts the drafts it saves, from 6.0 on, but still opens a draft
//! written in the plain JSON its older builds saved, which is what this
//! writes: the fields 剪映 itself writes for a cut, a sound and a title, in
//! its units - microseconds; a position as the half-frame it moves the
//! centre by, up positive; a title's size in its points. Every number is
//! the inverse of what [`crate::capcut`] reads, so a draft written here and
//! read back is the project it came from, as near as the two editors'
//! notions of a look allow.
//!
//! What comes across: the picture, sound and title clips of a timeline, on
//! a track of their kind for each of its lanes, with their timing, in-point,
//! speed, volume, fades, place, size, turn, opacity and flips; a title's
//! words in its colour, size, weight, outline and alignment, and each run
//! of words styled on its own (the key words a caption marks) as a style
//! of their own over those characters. What does not - effects, filters,
//! transitions, masks, keyframes, a speed curve, a title's box - is said,
//! once per kind, in [`Exported::notes`].

use serde_json::{Map, Value, json};

use crate::model::{Clip, ClipKind, MediaKind, Project, RunStyle, TextAlign, TextStyle, Timeline};

/// 剪映's times are microseconds.
const MICROS: f64 = 1_000_000.0;

/// Concat's title size over 剪映's: the inverse of
/// `capcut::TEXT_SIZE`, 剪映's 15 as nine hundredths of the frame.
const TEXT_SIZE: f64 = 0.09 / 15.0;

/// How long 剪映 says a still lasts: three hours, as it writes for a photo.
const STILL_MICROS: i64 = 10_800_000_000;

/// Where 剪映 starts titles' render order, above every picture.
const TEXT_RENDER_INDEX: i64 = 14_000;

/// The draft format this writes: 剪映 10.x's plain JSON.
const VERSION: i64 = 360_000;
const NEW_VERSION: &str = "164.0.0";
const APP_VERSION: &str = "10.7.0";

/// A draft ready to write.
pub struct Exported {
    /// `draft_content.json`.
    pub content: Value,
    /// `draft_meta_info.json`.
    pub meta: Value,
    /// What did not come across, one line a kind.
    pub notes: Vec<String>,
}

/// Where the draft goes and what it is called.
pub struct Target<'a> {
    /// The name the drafts list shows, and the folder's.
    pub name: &'a str,
    /// The draft's own folder.
    pub folder: &'a str,
    /// The folder of drafts it is in.
    pub root: &'a str,
    /// Now, in microseconds since 1970, for the draft's dates.
    pub now_micros: i64,
}

/// The active timeline of `project` as a 剪映 draft.
pub fn export(project: &Project, target: &Target<'_>) -> Exported {
    let timeline = project
        .timelines
        .iter()
        .find(|timeline| timeline.id == project.active_timeline_id)
        .or_else(|| project.timelines.first());
    let mut writer = Writer {
        project,
        ids: Ids(target.now_micros as u64 ^ 0x9e37_79b9_7f4a_7c15),
        materials: Map::new(),
        notes: Vec::new(),
    };
    let draft_id = writer.ids.next();
    let (tracks, duration) = match timeline {
        Some(timeline) => writer.tracks(timeline),
        None => (Vec::new(), 0),
    };
    let (width, height, fps) = timeline.map_or((1920, 1080, 30.0), |timeline| {
        let video = &timeline.video;
        let fps = if video.rate_den > 0 {
            video.rate_num as f64 / video.rate_den as f64
        } else {
            30.0
        };
        (video.width, video.height, fps)
    });
    let platform = json!({
        "os": "windows",
        "app_id": 3704,
        "app_source": "lv",
        "app_version": APP_VERSION,
    });
    let content = json!({
        "id": draft_id,
        "version": VERSION,
        "new_version": NEW_VERSION,
        "duration": duration,
        "fps": fps,
        "color_space": 0,
        "canvas_config": { "ratio": "original", "width": width, "height": height },
        "config": { "original_sound_last_index": 1 },
        "keyframes": {},
        "materials": Value::Object(writer.materials),
        "tracks": tracks,
        "platform": platform,
        "last_modified_platform": platform,
        "render_index_track_mode_on": true,
        "path": "",
    });
    let draft_materials: Vec<Value> = [0, 1, 2, 3, 6, 7, 8]
        .iter()
        .map(|kind| json!({ "type": kind, "value": [] }))
        .collect();
    let meta = json!({
        "draft_id": draft_id,
        "draft_name": target.name,
        "draft_fold_path": target.folder.replace('\\', "/"),
        "draft_root_path": target.root,
        "draft_json_file": format!("{}/draft_content.json", target.folder.replace('\\', "/")),
        "draft_cover": "",
        "draft_removable_storage_device": "",
        "draft_type": "",
        "draft_materials": draft_materials,
        "tm_draft_create": target.now_micros,
        "tm_draft_modified": target.now_micros,
        "tm_draft_removed": 0,
        "tm_duration": duration,
        "draft_is_invisible": false,
    });
    let mut notes = writer.notes;
    notes.dedup();
    Exported {
        content,
        meta,
        notes,
    }
}

/// Ids in the shape 剪映 writes them, uppercase hex in 8-4-4-4-12, from a
/// seed: a draft's ids differ from every other draft's and repeat for a
/// test's seed.
struct Ids(u64);

impl Ids {
    fn word(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn next(&mut self) -> String {
        let (a, b) = (self.word(), self.word());
        format!(
            "{:08X}-{:04X}-4{:03X}-{:04X}-{:012X}",
            a >> 32,
            (a >> 16) & 0xffff,
            a & 0x0fff,
            0x8000 | ((b >> 48) & 0x3fff),
            b & 0xffff_ffff_ffff
        )
    }
}

struct Writer<'p> {
    project: &'p Project,
    ids: Ids,
    materials: Map<String, Value>,
    notes: Vec<String>,
}

/// The tracks a lane becomes, one a kind of 剪映's, in the order a
/// draft lists them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Video,
    Audio,
    Text,
}

impl Writer<'_> {
    fn note(&mut self, what: &str) {
        if !self.notes.iter().any(|note| note == what) {
            self.notes.push(what.to_owned());
        }
    }

    /// Adds `material` to the list `list` and returns its id.
    fn material(&mut self, list: &str, mut material: Value) -> String {
        let id = self.ids.next();
        material["id"] = json!(id);
        self.materials
            .entry(list.to_owned())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .expect("a list")
            .push(material);
        id
    }

    /// The timeline's tracks, bottom lane first as 剪映 lists them, and
    /// the draft's length.
    fn tracks(&mut self, timeline: &Timeline) -> (Vec<Value>, i64) {
        let mut tracks = Vec::new();
        let mut end = 0.0_f64;
        let mut text_layer = 0;
        for kind in [Kind::Video, Kind::Text, Kind::Audio] {
            // Concat lists lanes top to bottom; 剪映 draws a later track
            // over an earlier one.
            for (layer, lane) in timeline.tracks.iter().rev().enumerate() {
                let mut clips: Vec<&Clip> = timeline
                    .clips
                    .iter()
                    .map(|clip| clip.as_ref())
                    .filter(|clip| clip.track_id == lane.id && kind_of(clip) == Some(kind))
                    .collect();
                if clips.is_empty() {
                    continue;
                }
                clips.sort_by(|a, b| a.start.total_cmp(&b.start));
                let mut segments = Vec::new();
                for clip in clips {
                    end = end.max(clip.start + clip.duration);
                    let segment = match kind {
                        Kind::Video => self.picture(clip, layer),
                        Kind::Audio => self.sound(clip),
                        Kind::Text => self.title(clip, text_layer),
                    };
                    if let Some(segment) = segment {
                        segments.push(segment);
                    }
                }
                if kind == Kind::Text {
                    text_layer += 1;
                }
                if segments.is_empty() {
                    continue;
                }
                let mut track = json!({
                    "id": self.ids.next(),
                    "type": match kind { Kind::Video => "video", Kind::Audio => "audio", Kind::Text => "text" },
                    "attribute": 0,
                    "flag": 0,
                    "is_default_name": true,
                    "segments": segments,
                });
                if !lane.visible && kind != Kind::Audio {
                    track["attribute"] = json!(1);
                }
                tracks.push(track);
            }
        }
        for clip in &timeline.clips {
            if kind_of(clip).is_none() {
                self.note(match clip.kind {
                    ClipKind::Layer | ClipKind::SoundLayer => "效果层（剪映里请重新添加特效）",
                    ClipKind::Shape => "形状",
                    _ => "转场",
                });
            }
        }
        (tracks, micros(end))
    }

    /// What every segment says about where it sits.
    fn segment(&mut self, clip: &Clip, material_id: &str, extras: Vec<String>) -> Value {
        let source_length = (clip.duration * clip.speed.max(0.01) * MICROS).round() as i64;
        let segment = json!({
            "id": self.ids.next(),
            "material_id": material_id,
            "extra_material_refs": extras,
            "source_timerange": { "start": micros(clip.source_start), "duration": source_length },
            "target_timerange": { "start": micros(clip.start), "duration": micros(clip.duration) },
            "render_timerange": {},
            "speed": clip.speed,
            "volume": if clip.muted == Some(true) { 0.0 } else { clip.volume },
            "last_nonzero_volume": clip.volume,
            "visible": true,
            "reverse": false,
            "clip": placement(clip),
            "uniform_scale": { "on": true, "value": 1.0 },
            "source": "segmentsourcenormal",
        });
        if !clip.keys.is_empty() {
            self.note("关键帧动画");
        }
        if clip.speed_curve.is_some() {
            self.note("曲线变速（按匀速导出）");
        }
        if !clip.video_effects.is_empty() || !clip.filters.is_empty() {
            self.note("特效和滤镜");
        }
        if !clip.masks.is_empty() || clip.cutout.is_some() {
            self.note("蒙版和抠像");
        }
        if clip.transition_in.is_some() {
            self.note("转场");
        }
        segment
    }

    fn speed_material(&mut self, clip: &Clip) -> String {
        self.material(
            "speeds",
            json!({ "type": "speed", "mode": 0, "speed": clip.speed, "curve_speed": null }),
        )
    }

    /// A video or a still on a video track.
    fn picture(&mut self, clip: &Clip, layer: usize) -> Option<Value> {
        let media = self
            .project
            .media
            .iter()
            .find(|item| item.id == clip.media_id)?;
        let still = clip.kind == ClipKind::Image || media.kind == MediaKind::Image;
        let width = media.width.unwrap_or(1920);
        let height = media.height.unwrap_or(1080);
        let material = self.material(
            "videos",
            json!({
                "type": if still { "photo" } else { "video" },
                "path": media.path.replace('\\', "/"),
                "material_name": media.name,
                "duration": if still { STILL_MICROS } else { media.duration.map_or(micros(clip.source_start + clip.duration * clip.speed), micros) },
                "width": width,
                "height": height,
                "crop": crop(clip),
                "crop_ratio": "free",
                "crop_scale": 1.0,
                "category_name": "local",
                "check_flag": 63487,
            }),
        );
        let speed = self.speed_material(clip);
        let canvas = self.material(
            "canvases",
            json!({ "type": "canvas_color", "color": "", "blur": 0.0, "image": "" }),
        );
        let mut extras = vec![speed, canvas];
        if let Some(fade) = self.fade(clip) {
            extras.push(fade);
        }
        let mut segment = self.segment(clip, &material, extras);
        segment["render_index"] = json!(layer);
        segment["track_render_index"] = json!(layer);
        Some(segment)
    }

    /// A sound on an audio track.
    fn sound(&mut self, clip: &Clip) -> Option<Value> {
        let media = self
            .project
            .media
            .iter()
            .find(|item| item.id == clip.media_id)?;
        let material = self.material(
            "audios",
            json!({
                "type": "extract_music",
                "name": media.name,
                "path": media.path.replace('\\', "/"),
                "duration": media.duration.map_or(micros(clip.source_start + clip.duration * clip.speed), micros),
                "category_name": "local",
                "check_flag": 1,
            }),
        );
        let speed = self.speed_material(clip);
        let mut extras = vec![speed];
        if let Some(fade) = self.fade(clip) {
            extras.push(fade);
        }
        let mut segment = self.segment(clip, &material, extras);
        segment
            .as_object_mut()
            .map(|segment| segment.remove("clip"));
        Some(segment)
    }

    fn fade(&mut self, clip: &Clip) -> Option<String> {
        (clip.fade_in > 0.0 || clip.fade_out > 0.0).then(|| {
            self.material(
                "audio_fades",
                json!({
                    "type": "audio_fade",
                    "fade_type": 0,
                    "fade_in_duration": micros(clip.fade_in),
                    "fade_out_duration": micros(clip.fade_out),
                }),
            )
        })
    }

    /// A title on a text track: its words, in its look, each styled run
    /// of them as a style of its own.
    fn title(&mut self, clip: &Clip, layer: usize) -> Option<Value> {
        let style = clip.text.as_ref()?;
        if style.content.trim().is_empty() {
            return None;
        }
        if !style.background.is_empty() && style.background != "transparent" {
            self.note("字幕底框");
        }
        let words = style.display_content();
        let size = jianying_size(style.font_size);
        let content = json!({ "text": words, "styles": styles(style, &words) });
        let material = self.material(
            "texts",
            json!({
                "type": "text",
                "content": content.to_string(),
                "font_size": size,
                "text_color": style.color.to_uppercase(),
                "text_alpha": style.opacity,
                "global_alpha": style.opacity,
                "alignment": match style.align { TextAlign::Left => 0, TextAlign::Right => 2, _ => 1 },
                "line_spacing": 0.02,
                "border_color": if style.stroke_width > 0.0 { style.stroke_color.to_uppercase() } else { String::new() },
                "border_width": stroke(style),
                "has_shadow": style.shadow,
                "shadow_color": "#000000",
                "shadow_alpha": 0.6,
                "bold_width": 0.0,
                "check_flag": 7,
                "initial_scale": 1.0,
                "layer_weight": 1,
                "typesetting": 0,
            }),
        );
        let mut segment = self.segment(clip, &material, Vec::new());
        for key in [
            "source_timerange",
            "speed",
            "volume",
            "last_nonzero_volume",
            "reverse",
        ] {
            segment.as_object_mut().map(|segment| segment.remove(key));
        }
        segment["render_index"] = json!(TEXT_RENDER_INDEX + layer as i64);
        segment["track_render_index"] = json!(layer);
        Some(segment)
    }
}

/// Which of 剪映's tracks a clip goes on; None for what has no place there.
fn kind_of(clip: &Clip) -> Option<Kind> {
    match clip.kind {
        ClipKind::Video | ClipKind::Image => Some(Kind::Video),
        ClipKind::Audio => Some(Kind::Audio),
        ClipKind::Text => clip.text.as_ref().map(|_| Kind::Text),
        _ => None,
    }
}

fn micros(seconds: f64) -> i64 {
    (seconds.max(0.0) * MICROS).round() as i64
}

/// The segment's `clip`: place, size, turn, opacity and flips, the inverse
/// of `capcut`'s reading.
fn placement(clip: &Clip) -> Value {
    json!({
        "alpha": clip.opacity.clamp(0.0, 1.0),
        "rotation": clip.rotation,
        "scale": { "x": clip.scale * clip.stretch_x, "y": clip.scale * clip.stretch_y },
        "transform": { "x": clip.offset_x * 2.0, "y": -clip.offset_y * 2.0 },
        "flip": { "horizontal": clip.flip_h, "vertical": clip.flip_v },
    })
}

/// The crop as 剪映's four corners, each in `0..1` of the frame.
fn crop(clip: &Clip) -> Value {
    let (left, top, right, bottom) = clip.crop.as_ref().map_or((0.0, 0.0, 1.0, 1.0), |crop| {
        (crop.left, crop.top, 1.0 - crop.right, 1.0 - crop.bottom)
    });
    json!({
        "upper_left_x": left, "upper_left_y": top,
        "upper_right_x": right, "upper_right_y": top,
        "lower_left_x": left, "lower_left_y": bottom,
        "lower_right_x": right, "lower_right_y": bottom,
    })
}

/// A Concat title size in 剪映's points, one decimal.
fn jianying_size(font_size: f64) -> f64 {
    ((font_size / TEXT_SIZE) * 10.0).round() / 10.0
}

/// The outline's width as 剪映 measures it, against the letters' size:
/// Concat measures it against the frame.
fn stroke(style: &TextStyle) -> f64 {
    if style.stroke_width <= 0.0 || style.font_size <= 0.0 {
        0.0
    } else {
        ((style.stroke_width / style.font_size) * 1000.0).round() / 1000.0
    }
}

/// `#rrggbb` as 剪映's `[r, g, b]` in `0..1`.
fn rgb(hex: &str) -> Value {
    let hex = hex.trim_start_matches('#');
    let channel = |at: usize| {
        u8::from_str_radix(hex.get(at..at + 2).unwrap_or("ff"), 16).map_or(1.0, |level| {
            (f64::from(level) / 255.0 * 10_000.0).round() / 10_000.0
        })
    };
    json!([channel(0), channel(2), channel(4)])
}

/// The look of `chars` characters of a title, the title's own with `run`
/// over it.
fn style_for(style: &TextStyle, run: Option<&RunStyle>, range: (usize, usize)) -> Value {
    let pick = |own: Option<&String>, base: &str| own.cloned().unwrap_or_else(|| base.to_owned());
    let color = pick(run.and_then(|run| run.color.as_ref()), &style.color);
    let weight = run
        .and_then(|run| run.font_weight)
        .unwrap_or(style.font_weight);
    let scale = run.and_then(|run| run.font_scale).unwrap_or(1.0);
    let stroke_color = pick(
        run.and_then(|run| run.stroke_color.as_ref()),
        &style.stroke_color,
    );
    let mut look = json!({
        "fill": { "content": { "render_type": "solid", "solid": { "color": rgb(&color) } } },
        "size": jianying_size(style.font_size * scale),
        "bold": weight >= 600.0,
        "italic": run.and_then(|run| run.italic).unwrap_or(style.italic),
        "underline": run.and_then(|run| run.underline).unwrap_or(style.underline),
        "range": [range.0, range.1],
    });
    let width = stroke(style);
    if width > 0.0 {
        look["strokes"] = json!([{
            "content": { "render_type": "solid", "solid": { "color": rgb(&stroke_color) } },
            "width": width,
        }]);
    }
    look
}

/// The title's words cut where its runs begin and end, each piece in its
/// own look: the whole title in one style when nothing is styled.
fn styles(style: &TextStyle, words: &str) -> Vec<Value> {
    let length = words.chars().count();
    let mut cuts = vec![0, length];
    for run in &style.runs {
        cuts.push(run.start.min(length));
        cuts.push(run.end.min(length));
    }
    cuts.sort_unstable();
    cuts.dedup();
    cuts.windows(2)
        .filter(|pair| pair[0] < pair[1])
        .map(|pair| {
            let run = style
                .runs
                .iter()
                .rev()
                .find(|run| run.start <= pair[0] && pair[1] <= run.end)
                .map(|run| &run.style);
            style_for(style, run, (pair[0], pair[1]))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Clip, MediaItem, TextRun, Track, VideoSettings};
    use std::sync::Arc;

    fn project() -> Project {
        let mut video = Clip::blank("c1", "t2", ClipKind::Video, "C2786.MP4", 0.0, 10.0);
        video.media_id = "m1".into();
        video.offset_y = 0.1;
        let mut title = Clip::blank("c2", "t1", ClipKind::Text, "目标稳拿", 1.0, 2.0);
        title.offset_y = 0.35;
        title.text = Some(TextStyle {
            content: "目标稳拿".into(),
            stroke_width: 0.006,
            runs: vec![TextRun {
                start: 2,
                end: 4,
                style: RunStyle {
                    color: Some("#ffe400".into()),
                    font_weight: Some(800.0),
                    font_scale: Some(1.3),
                    ..RunStyle::default()
                },
            }],
            ..TextStyle::default()
        });
        let timeline = Timeline {
            id: "tl".into(),
            video: VideoSettings {
                width: 1080,
                height: 1920,
                rate_num: 30,
                rate_den: 1,
                ..VideoSettings::default()
            },
            tracks: ["t1", "t2"]
                .map(|id| Track {
                    id: id.into(),
                    ..Track::default()
                })
                .to_vec(),
            clips: vec![Arc::new(video), Arc::new(title)],
            ..Timeline::default()
        };
        Project {
            media: vec![MediaItem {
                id: "m1".into(),
                path: r"F:\clips\C2786.MP4".into(),
                name: "C2786.MP4".into(),
                duration: Some(10.01),
                kind: MediaKind::Video,
                width: Some(1920),
                height: Some(1080),
                ..MediaItem::default()
            }],
            active_timeline_id: "tl".into(),
            timelines: vec![Arc::new(timeline)],
            ..Project::default()
        }
    }

    fn exported() -> Exported {
        export(
            &project(),
            &Target {
                name: "测试",
                folder: r"G:\Drafts\测试",
                root: r"G:\Drafts",
                now_micros: 42,
            },
        )
    }

    /// The picture goes on a video track below the title's text track,
    /// in microseconds, placed the way 剪映 measures it.
    #[test]
    fn the_cut_comes_out_in_jianyings_units() {
        let draft = exported().content;
        let tracks = draft["tracks"].as_array().unwrap();
        assert_eq!(tracks[0]["type"], "video");
        assert_eq!(tracks[1]["type"], "text");
        let cut = &tracks[0]["segments"][0];
        assert_eq!(cut["target_timerange"]["duration"], 10_000_000);
        assert!((cut["clip"]["transform"]["y"].as_f64().unwrap() + 0.2).abs() < 1e-9);
        let video = &draft["materials"]["videos"][0];
        assert_eq!(video["path"], "F:/clips/C2786.MP4");
        assert_eq!(draft["duration"], 10_000_000);
        assert_eq!(draft["canvas_config"]["width"], 1080);
    }

    /// A caption's marked words are a style of their own over their
    /// characters, larger and bold in their colour; the rest is the title's.
    #[test]
    fn marked_words_are_a_style_of_their_own() {
        let draft = exported().content;
        let content: Value =
            serde_json::from_str(draft["materials"]["texts"][0]["content"].as_str().unwrap())
                .unwrap();
        assert_eq!(content["text"], "目标稳拿");
        let styles = content["styles"].as_array().unwrap();
        assert_eq!(styles.len(), 2);
        assert_eq!(styles[0]["range"], json!([0, 2]));
        assert_eq!(styles[1]["range"], json!([2, 4]));
        assert_eq!(styles[1]["fill"]["content"]["solid"]["color"][0], 1.0);
        assert_eq!(styles[1]["bold"], true);
        assert!(styles[1]["size"].as_f64().unwrap() > styles[0]["size"].as_f64().unwrap());
        assert_eq!(styles[0]["size"], 15.0);
        assert!(styles[0]["strokes"][0]["width"].as_f64().unwrap() > 0.0);
    }

    /// The drafts list's file names the draft and its folder.
    #[test]
    fn the_meta_names_the_draft() {
        let meta = exported().meta;
        assert_eq!(meta["draft_name"], "测试");
        assert_eq!(meta["draft_fold_path"], "G:/Drafts/测试");
        assert_eq!(meta["draft_id"], exported().content["id"]);
    }

    #[test]
    fn ids_have_jianyings_shape() {
        let mut ids = Ids(7);
        let id = ids.next();
        assert_eq!(id.len(), 36);
        assert_eq!(id.matches('-').count(), 4);
        assert_ne!(id, ids.next());
    }
}

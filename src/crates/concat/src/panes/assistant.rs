// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat contributors

//! The AI assistant: a chat card that edits the project on screen.
//!
//! The person says what they want in words; a language model of their
//! choosing - any service speaking the OpenAI chat-completions protocol,
//! set up on the card itself - answers, and does it with the same tools
//! `concat --mcp` offers an outside agent ([`concat_mcp::tools`]). The
//! difference is where the tools act. An outside agent edits projects of
//! its own, never the one in the window; here each tool's request is
//! carried to the event-loop thread and run on the window's own session,
//! so an edit appears as it is made, lands in the undo history like one
//! made by hand, and is refused with the sentence the window would show.
//!
//! The conversation runs on a thread of its own: a round trip to the
//! model takes seconds, and the window must not wait on it. Each tool
//! call the model makes blocks that thread, never the window's, until the
//! window has run it.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use concat_api::{ApiError, ErrorCode, Request};
use concat_mcp::Backend;
use concat_mcp::tools::{Action, Context, Toolbox};
use concat_project::commands::Command;
use serde_json::{Value, json};
use slint::SharedString;

use super::assistant_local as local;
use crate::host::Shell;
use crate::prefs::AssistantPrefs;
use crate::studio::Studio;
use crate::ui::{AssistantData, AssistantLine};

/// The most model round trips one message may take: a request that keeps
/// calling tools past this is stopped and said to have been.
const MAX_ROUNDS: usize = 24;
/// How long the model may take to answer one round.
const MODEL_PATIENCE: Duration = Duration::from_secs(180);
/// How long a tool may wait for the window to run it.
const WINDOW_PATIENCE: Duration = Duration::from_secs(60);
/// The longest tool result handed back to the model, in characters: a
/// long timeline read is cut rather than filling its context.
const RESULT_CHARS: usize = 16_000;

/// The API methods whose tools make sense on the project on screen. Not
/// `edit.apply`, whose schema is every command at once (two thirds of
/// all the tools' size, sent with every round) and which the command
/// tools cover one by one; not `project.get`, the whole document, which
/// `timeline_read` gives summarised. The
/// rest - opening, creating, closing projects, templates, exports, preview
/// pictures - belong to the window's own controls, or need a session of
/// the API's own.
const METHODS: [&str; 4] = ["media.probe", "media.import", "edit.undo", "edit.redo"];

/// Settings' AI assistant page, by its index there.
const SETTINGS_PAGE: i32 = 8;

/// What the model is told before the conversation.
const SYSTEM: &str = "\
You are the assistant built into BloomCut, a video editor. The person is editing a project in the \
window and asks you, in their own words, to change it. Do it with the tools: they act on the \
project on screen, at once, and each change can be undone with Undo.

- The project is chosen for you: never ask for or pass a project path.
- Read before you edit: call timeline_read at the start of every request to learn the clips, \
their ids, lanes and times. The person also edits by hand between messages, so what you read \
earlier may be out of date; never take a change you did not make for one of yours.
- Times are in seconds from the start of the timeline unless a tool says otherwise.
- Prefer one well-formed call over several guesses; if a tool refuses, read its message and correct.
- Do not invent clip or media ids; take them from what the tools return.
- When done, say in one or two short sentences what you changed. Answer in the language the \
person writes in (usually Chinese).
- To mark key words in captions: call captions_list, choose the words by the person's rules, \
then call highlight_words once with every caption's items. Copy each word exactly as the caption \
writes it. If some come back as not found, fix only those.
- Write plain text: no Markdown, no asterisks, no backticks, no headings or tables. Short lines \
starting with \"· \" are fine for a list.";

/// Who said a line on the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    User,
    Assistant,
    /// A tool the model called, as a step under its answer.
    Step,
    Error,
}

impl Role {
    fn index(self) -> i32 {
        match self {
            Role::User => 0,
            Role::Assistant => 1,
            Role::Step => 2,
            Role::Error => 3,
        }
    }
}

/// The conversation as the model reads it. Its own type so a message
/// carrying it says how long it is in the log, not the whole of it.
#[derive(Clone, Default)]
pub struct History(Vec<Value>);

impl fmt::Debug for History {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "{} messages", self.0.len())
    }
}

/// What the conversation's thread reports.
#[derive(Debug)]
pub enum Event {
    /// A tool is being called: its title.
    Step(String),
    /// Words from the model.
    Reply(String),
    /// The turn is over; the conversation as it now stands.
    Finished(History),
    /// The turn failed; the conversation up to the failure.
    Failed(String, History),
}

#[derive(Debug)]
pub enum AssistantMsg {
    Toggle,
    Close,
    /// Settings, at the AI assistant's page.
    OpenSettings,
    /// Settings' Test connection.
    Test,
    /// The folder of rules, in the file manager; made first if it is not
    /// there yet.
    OpenRules,
    /// What the test came to: how long the model took, or why it failed.
    Tested(Result<u128, String>),
    BaseUrlEdited(String),
    ApiKeyEdited(String),
    ModelEdited(String),
    DraftEdited(String),
    Send,
    Stop,
    Clear,
    /// From the conversation's thread; `turn` is which turn it belongs to.
    Event {
        turn: u64,
        event: Event,
    },
}

/// The card, the conversation and the turn running.
#[derive(Default)]
pub struct AssistantPane {
    open: bool,
    /// Settings' Test connection: 0 not tried, 1 trying, 2 answered,
    /// 3 failed; and what it came to, in words.
    test_state: i32,
    test_status: String,
    draft: String,
    lines: Vec<(Role, String)>,
    history: History,
    /// The turn under way: its number and its stop flag.
    running: Option<(u64, Arc<AtomicBool>)>,
    /// Counts turns, so an event from one that was stopped is let go.
    turns: u64,
}

impl AssistantPane {
    pub fn update(&mut self, msg: AssistantMsg, studio: &mut Studio) {
        match msg {
            AssistantMsg::Toggle => {
                self.open = !self.open;
            }
            AssistantMsg::Close => self.open = false,
            AssistantMsg::OpenSettings => {
                studio.handle(crate::panes::Msg::Settings(
                    crate::panes::settings::SettingsMsg::Open,
                ));
                studio.handle(crate::panes::Msg::Settings(
                    crate::panes::settings::SettingsMsg::PageChanged(SETTINGS_PAGE),
                ));
            }
            AssistantMsg::OpenRules => {
                let folder = local::rules_folder(&studio.host.dirs.config);
                let opened = std::fs::create_dir_all(&folder)
                    .map_err(|error| error.to_string())
                    .and_then(|()| crate::platform::open_link(&folder.to_string_lossy()));
                if let Err(error) = opened {
                    studio.notify(&error, true);
                }
            }
            AssistantMsg::Test => {
                let config = studio.prefs.assistant.clone();
                if !config.is_set() || self.test_state == 1 {
                    return;
                }
                self.test_state = 1;
                self.test_status.clear();
                log::info!("assistant: testing {} ({})", config.model, config.base_url);
                std::thread::Builder::new()
                    .name("concat assistant test".to_owned())
                    .spawn(move || {
                        let began = std::time::Instant::now();
                        let outcome = chat(
                            &config,
                            &[json!({ "role": "user", "content": "ping" })],
                            &[],
                        )
                        .map(|_| began.elapsed().as_millis());
                        let _ = slint::invoke_from_event_loop(move || {
                            Shell::with(|shell, app| {
                                shell
                                    .studio
                                    .borrow_mut()
                                    .handle(crate::panes::Msg::Assistant(AssistantMsg::Tested(
                                        outcome,
                                    )));
                                shell.studio.borrow().publish(&app, &shell.models);
                            });
                        });
                    })
                    .map_err(|error| log::error!("assistant: no thread for the test: {error}"))
                    .ok();
            }
            AssistantMsg::Tested(outcome) => match outcome {
                Ok(millis) => {
                    self.test_state = 2;
                    self.test_status = crate::i18n::tf(
                        "assistant.testPassed",
                        &[&studio.prefs.assistant.model.trim(), &millis],
                    );
                }
                Err(error) => {
                    log::warn!("assistant: the test failed: {error}");
                    self.test_state = 3;
                    self.test_status = error;
                }
            },
            AssistantMsg::BaseUrlEdited(text) => {
                studio.prefs.assistant.base_url = text;
                self.untested();
                studio.prefs.save(&studio.host.dirs);
            }
            AssistantMsg::ApiKeyEdited(text) => {
                studio.prefs.assistant.api_key = text;
                self.untested();
                studio.prefs.save(&studio.host.dirs);
            }
            AssistantMsg::ModelEdited(text) => {
                studio.prefs.assistant.model = text;
                self.untested();
                studio.prefs.save(&studio.host.dirs);
            }
            AssistantMsg::DraftEdited(text) => self.draft = text,
            AssistantMsg::Send => self.send(studio),
            AssistantMsg::Stop => {
                if let Some((_, stop)) = self.running.take() {
                    stop.store(true, Ordering::Relaxed);
                    self.lines
                        .push((Role::Error, crate::i18n::t("assistant.stopped")));
                }
            }
            AssistantMsg::Clear => {
                if let Some((_, stop)) = self.running.take() {
                    stop.store(true, Ordering::Relaxed);
                }
                self.lines.clear();
                self.history = History::default();
            }
            AssistantMsg::Event { turn, event } => {
                if self.running.as_ref().map(|(running, _)| *running) != Some(turn) {
                    return;
                }
                match event {
                    Event::Step(title) => self.lines.push((Role::Step, title)),
                    Event::Reply(text) => self.lines.push((Role::Assistant, plain(&text))),
                    Event::Finished(history) => {
                        self.history = history;
                        self.running = None;
                    }
                    Event::Failed(error, history) => {
                        log::warn!("assistant: the turn failed: {error}");
                        self.history = history;
                        self.lines.push((Role::Error, error));
                        self.running = None;
                    }
                }
            }
        }
    }

    /// Hands what is typed to the model.
    fn send(&mut self, studio: &mut Studio) {
        let text = self.draft.trim().to_owned();
        if text.is_empty() || self.running.is_some() {
            return;
        }
        let config = studio.prefs.assistant.clone();
        if !config.is_set() {
            self.lines
                .push((Role::Error, crate::i18n::t("assistant.notSet")));
            return;
        }
        let Some(path) = studio
            .session
            .as_ref()
            .map(|session| session.path().to_owned())
        else {
            self.lines
                .push((Role::Error, crate::i18n::t("assistant.noProject")));
            return;
        };
        self.draft.clear();
        self.lines.push((Role::User, text.clone()));
        let mut history = self.history.clone();
        history.0.push(json!({ "role": "user", "content": text }));
        self.turns += 1;
        let turn = self.turns;
        let stop = Arc::new(AtomicBool::new(false));
        self.running = Some((turn, Arc::clone(&stop)));
        let rules = local::rules(&studio.host.dirs.config);
        log::info!(
            "assistant: asked {} ({}), with {} characters of rules",
            config.model,
            config.base_url,
            rules.chars().count()
        );
        std::thread::Builder::new()
            .name("concat assistant".to_owned())
            .spawn(move || {
                let tell = |event| post(turn, event);
                match converse(&config, &mut history, &path, &rules, &stop, &tell) {
                    Ok(()) => tell(Event::Finished(history)),
                    Err(error) => tell(Event::Failed(error, history)),
                }
            })
            .map_err(|error| log::error!("assistant: no thread for the conversation: {error}"))
            .ok();
    }

    pub fn data(&self, studio: &Studio) -> AssistantData {
        let prefs = &studio.prefs.assistant;
        AssistantData {
            open: self.open,
            rules: {
                let names: Vec<String> = local::rule_files(&studio.host.dirs.config)
                    .iter()
                    .filter_map(|file| {
                        file.file_stem()
                            .map(|stem| stem.to_string_lossy().into_owned())
                    })
                    .collect();
                if names.is_empty() {
                    crate::i18n::t("assistant.noRules").into()
                } else {
                    crate::i18n::tf("assistant.rulesLoaded", &[&names.len(), &names.join("、")])
                        .into()
                }
            },
            test_state: self.test_state,
            test_status: self.test_status.clone().into(),
            base_url: prefs.base_url.clone().into(),
            api_key: prefs.api_key.clone().into(),
            model: prefs.model.clone().into(),
            configured: prefs.is_set(),
            draft: self.draft.clone().into(),
            running: self.running.is_some(),
        }
    }

    /// A test run on the fields as they were says nothing about them now.
    fn untested(&mut self) {
        if self.test_state != 1 {
            self.test_state = 0;
            self.test_status.clear();
        }
    }

    pub fn line_rows(&self) -> Vec<AssistantLine> {
        self.lines
            .iter()
            .map(|(role, text)| AssistantLine {
                role: role.index(),
                text: SharedString::from(text.as_str()),
            })
            .collect()
    }
}

/// A reply as the card shows it: a model that writes Markdown anyway has
/// its emphasis and code marks taken out, since the card draws plain text,
/// and its headings' and list items' marks made plain.
fn plain(text: &str) -> String {
    text.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            let indent = &line[..line.len() - trimmed.len()];
            let body = if trimmed.starts_with('#') {
                trimmed.trim_start_matches('#').trim_start()
            } else {
                trimmed
            };
            let body = match body.strip_prefix("- ").or_else(|| body.strip_prefix("* ")) {
                Some(rest) => format!("· {rest}"),
                None => body.to_owned(),
            };
            let body = body.replace("**", "").replace("__", "").replace('`', "");
            format!("{indent}{body}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Carries an event from the conversation's thread to the card.
fn post(turn: u64, event: Event) {
    let _ = slint::invoke_from_event_loop(move || {
        Shell::with(|shell, app| {
            shell
                .studio
                .borrow_mut()
                .handle(crate::panes::Msg::Assistant(AssistantMsg::Event {
                    turn,
                    event,
                }));
            shell.studio.borrow().publish(&app, &shell.models);
        });
    });
}

/// The tools, made once: building them generates the API's schemas.
fn toolbox() -> &'static Toolbox {
    static TOOLBOX: OnceLock<Toolbox> = OnceLock::new();
    TOOLBOX.get_or_init(Toolbox::new)
}

/// Whether the tool may be offered on the window's project.
fn offered(action: Action) -> bool {
    match action {
        Action::Method { method, .. } => METHODS.contains(&method),
        Action::Command { .. } | Action::TimelineRead | Action::Captions | Action::Mask => true,
        Action::Version | Action::ExportWait | Action::PreviewFrame => false,
    }
}

/// The tools in the chat-completions form, without the project path: the
/// window fills that in.
fn tool_specs(toolbox: &Toolbox) -> Vec<Value> {
    toolbox
        .list()
        .into_iter()
        .filter(|tool| {
            tool["name"]
                .as_str()
                .and_then(|name| toolbox.find(name))
                .is_some_and(|(tool, _)| offered(tool.action))
        })
        .map(|tool| {
            let mut schema = tool["inputSchema"].clone();
            if let Some(object) = schema.as_object_mut() {
                object.remove("$schema");
                object.remove("title");
                if let Some(properties) =
                    object.get_mut("properties").and_then(Value::as_object_mut)
                {
                    properties.remove("path");
                }
                if let Some(required) = object.get_mut("required").and_then(Value::as_array_mut) {
                    required.retain(|name| name != "path");
                }
            }
            json!({
                "type": "function",
                "function": {
                    "name": tool["name"],
                    "description": tool["description"],
                    "parameters": schema,
                },
            })
        })
        .collect()
}

/// One turn: the model asked, its tool calls run, until it answers in
/// words alone. `history` is left as far as the turn got.
fn converse(
    config: &AssistantPrefs,
    history: &mut History,
    path: &str,
    rules: &str,
    stop: &AtomicBool,
    tell: &dyn Fn(Event),
) -> Result<(), String> {
    let toolbox = toolbox();
    let mut tools = tool_specs(toolbox);
    tools.extend(local::specs());
    let system = if rules.is_empty() {
        concat_host::brand::BRAND.named(SYSTEM)
    } else {
        let named = concat_host::brand::BRAND.named(SYSTEM);
        format!("{named}\n\nThe person's own rules, which come before your habits:\n\n{rules}")
    };
    let mut backend = WindowBackend {
        path: path.to_owned(),
    };
    for _ in 0..MAX_ROUNDS {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let mut messages = vec![json!({ "role": "system", "content": system })];
        messages.extend(history.0.iter().cloned());
        let message = chat(config, &messages, &tools)?;
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let text = message["content"].as_str().unwrap_or("").trim().to_owned();
        let calls: Vec<Value> = message["tool_calls"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let mut kept = json!({ "role": "assistant", "content": text });
        if !calls.is_empty() {
            kept["tool_calls"] = Value::Array(calls.clone());
        }
        // Some models think aloud in a field of their own and want it back.
        if let Some(reasoning) = message
            .get("reasoning_content")
            .filter(|value| value.is_string())
        {
            kept["reasoning_content"] = reasoning.clone();
        }
        history.0.push(kept);
        if !text.is_empty() {
            tell(Event::Reply(text));
        }
        if calls.is_empty() {
            return Ok(());
        }
        for call in calls {
            let id = call["id"].as_str().unwrap_or_default().to_owned();
            let name = call["function"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let raw = call["function"]["arguments"].as_str().unwrap_or("{}");
            let result = run_tool(toolbox, &mut backend, &name, raw, stop, tell);
            history
                .0
                .push(json!({ "role": "tool", "tool_call_id": id, "content": result }));
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
        }
    }
    Err(crate::i18n::tf("assistant.tooManySteps", &[&MAX_ROUNDS]))
}

/// Runs one tool call; what the model is told came of it.
fn run_tool(
    toolbox: &Toolbox,
    backend: &mut WindowBackend,
    name: &str,
    raw: &str,
    stop: &AtomicBool,
    tell: &dyn Fn(Event),
) -> String {
    if local::is_local(name) {
        return run_local(backend, name, raw, tell);
    }
    let Some((tool, _)) = toolbox.find(name).filter(|(tool, _)| offered(tool.action)) else {
        return format!("There is no tool called {name:?} here.");
    };
    let mut arguments: Value =
        match serde_json::from_str(if raw.trim().is_empty() { "{}" } else { raw }) {
            Ok(arguments) => arguments,
            Err(error) => {
                return format!("The arguments were not JSON ({error}); call it again with JSON.");
            }
        };
    if let Some(object) = arguments.as_object_mut() {
        object.insert("path".to_owned(), Value::String(backend.path.clone()));
    }
    tell(Event::Step(tool.title.to_owned()));
    log::info!("assistant: calls {name}");
    let Some(outcome) = toolbox.call(
        name,
        arguments,
        Context {
            backend,
            cancelled: stop,
            progress: None,
        },
    ) else {
        return format!("There is no tool called {name:?} here.");
    };
    let mut text: String = outcome
        .content
        .iter()
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if outcome.is_error {
        log::info!("assistant: {name} was refused: {text}");
        text = format!("Error: {text}");
    }
    if text.chars().count() > RESULT_CHARS {
        text = text.chars().take(RESULT_CHARS).collect::<String>() + "\n… (cut)";
    }
    text
}

/// Runs one of the assistant's own tools on the window; what the model is
/// told came of it.
fn run_local(backend: &WindowBackend, name: &str, raw: &str, tell: &dyn Fn(Event)) -> String {
    let arguments: Value =
        match serde_json::from_str(if raw.trim().is_empty() { "{}" } else { raw }) {
            Ok(arguments) => arguments,
            Err(error) => {
                return format!("The arguments were not JSON ({error}); call it again with JSON.");
            }
        };
    let key = if name == local::CAPTIONS_LIST {
        "assistant.stepCaptions"
    } else {
        "assistant.stepHighlight"
    };
    tell(Event::Step(crate::i18n::t(key)));
    log::info!("assistant: calls {name}");
    let name = name.to_owned();
    let outcome = on_window(backend.path.clone(), move |studio| {
        if name == local::CAPTIONS_LIST {
            local::captions_list(studio)
        } else {
            local::highlight_words(studio, arguments)
        }
    });
    match outcome {
        Ok(value) => {
            let text = value.to_string();
            if text.chars().count() > RESULT_CHARS {
                text.chars().take(RESULT_CHARS).collect::<String>() + "\n… (cut)"
            } else {
                text
            }
        }
        Err(error) => {
            log::info!(
                "assistant: a tool of its own was refused: {}",
                error.message
            );
            format!("Error: {}", error.message)
        }
    }
}

/// One round with the model: its message, or what went wrong in words.
fn chat(config: &AssistantPrefs, messages: &[Value], tools: &[Value]) -> Result<Value, String> {
    let url = format!(
        "{}/chat/completions",
        config.base_url.trim().trim_end_matches('/')
    );
    let body = json!({
        "model": config.model.trim(),
        "messages": messages,
        "stream": false,
    });
    let mut body = body;
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools.to_vec());
        body["tool_choice"] = json!("auto");
    } else {
        // A test: one word back is all it needs to say it is there.
        body["max_tokens"] = json!(8);
    }
    let body = body.to_string();
    let key = config.api_key.trim().to_owned();
    let response = concat_host::net::post_json(
        &url,
        Duration::from_secs(15),
        MODEL_PATIENCE,
        concat_host::net::Trust::System,
        &|request| {
            if key.is_empty() {
                request
            } else {
                request.set("Authorization", &format!("Bearer {key}"))
            }
        },
        &body,
    );
    let response = match response {
        Ok(response) => response,
        Err(failure) => {
            return Err(match failure.error {
                ureq::Error::Status(code, response) => {
                    let said = response.into_string().unwrap_or_default();
                    let said = serde_json::from_str::<Value>(&said)
                        .ok()
                        .and_then(|value| value["error"]["message"].as_str().map(str::to_owned))
                        .unwrap_or(said);
                    crate::i18n::tf(
                        "assistant.modelRefused",
                        &[&code, &said.chars().take(400).collect::<String>()],
                    )
                }
                _ => crate::i18n::tf("assistant.unreachable", &[&failure]),
            });
        }
    };
    let reply: Value = response
        .into_string()
        .map_err(|error| error.to_string())
        .and_then(|text| serde_json::from_str(&text).map_err(|error| error.to_string()))
        .map_err(|error| crate::i18n::tf("assistant.unreadable", &[&error]))?;
    let message = reply["choices"][0]["message"].clone();
    if message.is_null() {
        return Err(crate::i18n::tf(
            "assistant.unreadable",
            &[&reply.to_string().chars().take(400).collect::<String>()],
        ));
    }
    Ok(message)
}

/// Where the assistant's tools send their requests: the window's session,
/// on the event-loop thread.
struct WindowBackend {
    /// The project the turn began on. A request is refused if the window
    /// has moved to another since.
    path: String,
}

impl Backend for WindowBackend {
    fn call(&mut self, request: Request) -> Result<Value, ApiError> {
        let path = self.path.clone();
        match request {
            Request::MediaProbe { path: file } => {
                encode(&concat_host::media::probe(&file).map_err(ApiError::failed)?)
            }
            Request::MediaImport { file, .. } => {
                let item = concat_host::media::probe(&file)
                    .map_err(ApiError::failed)?
                    .to_new_media();
                on_window(path, move |studio| {
                    studio.apply_reporting(Command::AddMedia { item })
                })
            }
            Request::ProjectGet { .. } => on_window(path, |studio| {
                studio
                    .session
                    .as_ref()
                    .map(|session| session.view())
                    .ok_or_else(|| "no project is open".to_owned())
            }),
            Request::EditApply { command, .. } => {
                on_window(path, move |studio| studio.apply_reporting(*command))
            }
            Request::EditUndo { .. } => on_window(path, |studio| {
                studio.undo();
                studio
                    .session
                    .as_ref()
                    .map(|session| session.view())
                    .ok_or_else(|| "no project is open".to_owned())
            }),
            Request::EditRedo { .. } => on_window(path, |studio| {
                studio.redo();
                studio
                    .session
                    .as_ref()
                    .map(|session| session.view())
                    .ok_or_else(|| "no project is open".to_owned())
            }),
            _ => Err(ApiError::new(
                ErrorCode::Invalid,
                "that is not something the assistant can do in this window",
            )),
        }
    }

    fn describe(&self) -> Value {
        json!({ "mode": "window", "project": self.path })
    }
}

fn encode(value: &impl serde::Serialize) -> Result<Value, ApiError> {
    serde_json::to_value(value)
        .map_err(|error| ApiError::failed(format!("the reply did not encode: {error}")))
}

/// Runs `body` on the window's state on the event-loop thread, then
/// republishes the window, and waits for what it came to.
fn on_window<T: serde::Serialize + Send + 'static>(
    path: String,
    body: impl FnOnce(&mut Studio) -> Result<T, String> + Send + 'static,
) -> Result<Value, ApiError> {
    let (sender, receiver) = mpsc::channel();
    slint::invoke_from_event_loop(move || {
        Shell::with(|shell, app| {
            let outcome = {
                let mut studio = shell.studio.borrow_mut();
                let here = studio
                    .session
                    .as_ref()
                    .map(|session| session.path() == path);
                match here {
                    Some(true) => body(&mut studio)
                        .map_err(|message| ApiError::new(ErrorCode::Refused, message)),
                    Some(false) => Err(ApiError::new(
                        ErrorCode::NotOpen,
                        "the window has moved to another project since this began",
                    )),
                    None => Err(ApiError::new(
                        ErrorCode::NotOpen,
                        "no project is open in the window",
                    )),
                }
            };
            shell.studio.borrow_mut().refresh_art();
            shell.studio.borrow().publish(&app, &shell.models);
            let _ = sender.send(outcome.and_then(|value| encode(&value)));
        });
    })
    .map_err(|error| ApiError::failed(format!("the window is gone: {error}")))?;
    receiver
        .recv_timeout(WINDOW_PATIENCE)
        .map_err(|_| ApiError::failed("the window did not answer"))?
}

#[cfg(test)]
mod tests {
    use super::{plain, tool_specs, toolbox};

    /// Markdown a model writes anyway reads as plain text on the card.
    #[test]
    fn markdown_marks_are_taken_out() {
        assert_eq!(
            plain("## 结果\n- **项目**：`test`\n  * 片段 `c5`\n普通一行"),
            "结果\n· 项目：test\n  · 片段 c5\n普通一行"
        );
    }

    /// Every tool offered has a name, a description and an object schema
    /// with no project path in it; the project-level ones are left out.
    #[test]
    fn the_tools_offered_act_on_the_window_and_never_name_a_project() {
        let specs = tool_specs(toolbox());
        let names: Vec<&str> = specs
            .iter()
            .filter_map(|spec| spec["function"]["name"].as_str())
            .collect();
        assert!(names.contains(&"timeline_read"));
        assert!(names.contains(&"captions_add"));
        assert!(names.contains(&"clips_split"));
        assert!(!names.contains(&"project_open"));
        assert!(!names.contains(&"export_start"));
        assert!(!names.contains(&"edit_apply"));
        // Sent with every round: kept to what a model's context bears.
        let size = serde_json::to_string(&specs).map_or(0, |text| text.len());
        assert!(size < 100_000, "the tools are {size} bytes");
        for spec in &specs {
            let parameters = &spec["function"]["parameters"];
            assert_eq!(parameters["type"], "object", "{spec}");
            assert!(parameters["properties"].get("path").is_none(), "{spec}");
            assert!(
                spec["function"]["description"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
            );
        }
    }
}

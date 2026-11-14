// The Notetaker: records a call (your microphone, and what the computer plays
// for the other people), shows a live transcript, then writes the whole
// transcript person by person with Parakeet and the diarizer, and a summary
// with the to-dos. Port of NoteTaker.swift for Windows and Linux.
//
// Everything is stored in the app's folder (Notes/<id>/note.json); the audio is
// deleted once the transcript is written. No bot joins the call: the other
// people should be told it is recorded.

pub mod builder;
#[cfg(windows)]
mod export;
pub mod personas;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::flow::{pill, store::SummaryEngine};
use crate::i18n::{t, tf};
use crate::speech::{self, audio};
use builder::{Line, Speaker, TimedWord, Todo};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Recording,
    Processing,
    Ready,
    Failed,
}

/// A call noted by the Notetaker.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub title: String,
    /// Set once you rename the note: the summary then keeps your title.
    #[serde(default)]
    pub title_edited: bool,
    /// Milliseconds since 1970.
    pub start: u64,
    #[serde(default)]
    pub end: Option<u64>,
    /// The app in front when the recording started ("Teams", "Firefox").
    #[serde(default)]
    pub app: Option<String>,
    pub status: Status,
    #[serde(default)]
    pub speakers: Vec<Speaker>,
    #[serde(default)]
    pub lines: Vec<Line>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub key_points: Vec<String>,
    #[serde(default)]
    pub todos: Vec<Todo>,
    #[serde(default)]
    pub error: Option<String>,
    /// The start written out in local time ("2026-10-14 22:31"), for the Markdown.
    #[serde(default)]
    pub start_local: String,
    #[serde(default)]
    pub comments: Vec<personas::NoteComment>,
    #[serde(default)]
    pub comment_hash: Option<String>,
}

/// One side of the call being written to disk, with what the live transcript
/// has not taken yet.
struct Channel {
    writer: Option<audio::WavWriter>,
    /// Samples since the last live chunk.
    live: Vec<f32>,
    /// Where `live` starts, in samples from the start of the recording.
    live_start: usize,
    total: usize,
}

impl Channel {
    fn new(path: &std::path::Path) -> std::io::Result<Channel> {
        Ok(Channel { writer: Some(audio::WavWriter::create(path)?), live: Vec::new(), live_start: 0, total: 0 })
    }

    fn push(&mut self, samples: &[f32]) {
        if let Some(w) = self.writer.as_mut() {
            let _ = w.write(samples);
        }
        self.live.extend_from_slice(samples);
        self.total += samples.len();
    }

    /// A stretch ending on a pause (or a long one) for the live transcript.
    fn take_chunk(&mut self) -> Option<(f64, Vec<f32>)> {
        let rate = speech::SAMPLE_RATE as usize;
        if self.live.len() < rate * 3 / 2 {
            return None;
        }
        let tail = &self.live[self.live.len().saturating_sub(rate / 2)..];
        let quiet = tail.iter().fold(0.0f32, |m, &s| m.max(s.abs())) < 0.01;
        if !quiet && self.live.len() < rate * 12 {
            return None;
        }
        let start = self.live_start as f64 / rate as f64;
        let chunk = std::mem::take(&mut self.live);
        self.live_start = self.total;
        let peak = chunk.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        if peak < 0.01 {
            return None;
        }
        Some((start, chunk))
    }

    fn close(&mut self) {
        if let Some(w) = self.writer.take() {
            let _ = w.finish();
        }
    }
}

struct Recording {
    id: String,
    mic: audio::Capture,
    system: Option<audio::Capture>,
    mic_channel: Arc<Mutex<Channel>>,
    system_channel: Option<Arc<Mutex<Channel>>>,
    stop_live: Arc<AtomicBool>,
}

pub struct NoteTaker {
    notes: Mutex<Vec<Note>>,
    recording: Mutex<Option<Recording>>,
    /// Notes whose transcript or summary is being written.
    working: Mutex<Vec<String>>,
    /// Notas em geração de comentários de personas (evita corrida/duplicação).
    commenting: Mutex<HashSet<String>>,
    starting: AtomicBool,
}

pub fn folder() -> PathBuf {
    crate::settings::local_dir().join("Notes")
}

fn note_dir(id: &str) -> PathBuf {
    folder().join(id)
}

fn get(app: &AppHandle) -> Option<Arc<NoteTaker>> {
    app.try_state::<Arc<NoteTaker>>().map(|s| s.inner().clone())
}

pub fn is_recording(app: &AppHandle) -> bool {
    get(app).is_some_and(|n| n.recording.lock().unwrap().is_some())
}

fn now_ms() -> u64 {
    crate::flow::store::now_ms()
}

fn local_stamp() -> String {
    let t = crate::platform::local_time();
    format!("{:04}-{:02}-{:02} {:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute)
}

fn person_name(n: usize) -> String {
    tf("Person {0}", &[("0", &n.to_string())])
}

impl NoteTaker {
    fn save(&self, note: &Note) {
        let dir = note_dir(&note.id);
        if crate::platform::ensure_private_dir(&dir).is_err() {
            return;
        }
        if let Ok(text) = serde_json::to_string_pretty(note) {
            let tmp = dir.join("note.json.tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(tmp, dir.join("note.json"));
            }
        }
    }

    fn note(&self, id: &str) -> Option<Note> {
        self.notes.lock().unwrap().iter().find(|n| n.id == id).cloned()
    }

    /// Writes a change onto the note as it is now (it may have been renamed meanwhile).
    fn change(&self, app: &AppHandle, id: &str, f: impl FnOnce(&mut Note)) -> Option<Note> {
        let updated = {
            let mut notes = self.notes.lock().unwrap();
            let note = notes.iter_mut().find(|n| n.id == id)?;
            f(note);
            note.clone()
        };
        self.save(&updated);
        let _ = app.emit_to(crate::HUB_LABEL, "notes-changed", &updated.id);
        Some(updated)
    }
}

pub fn setup(app: &AppHandle) {
    let mut notes: Vec<Note> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(folder()) {
        for entry in entries.flatten() {
            if let Ok(text) = std::fs::read_to_string(entry.path().join("note.json")) {
                if let Ok(note) = serde_json::from_str::<Note>(&text) {
                    notes.push(note);
                }
            }
        }
    }
    notes.sort_by(|a, b| b.start.cmp(&a.start));
    // A call left recording or being written when Coucou last quit: written now.
    let unfinished: Vec<String> = notes
        .iter()
        .filter(|n| matches!(n.status, Status::Recording | Status::Processing))
        .map(|n| n.id.clone())
        .collect();
    for n in notes.iter_mut().filter(|n| n.status == Status::Recording) {
        n.status = Status::Processing;
        n.end.get_or_insert(n.start);
    }
    let taker = Arc::new(NoteTaker {
        notes: Mutex::new(notes),
        recording: Mutex::new(None),
        working: Mutex::new(Vec::new()),
        commenting: Mutex::new(HashSet::new()),
        starting: AtomicBool::new(false),
    });
    app.manage(taker.clone());
    for id in unfinished {
        let app = app.clone();
        std::thread::spawn(move || process(&app, &id));
    }
}

/// Captura do Anotador: grava apenas o microfone em uma voz, sem áudio de reunião.
pub fn toggle(app: &AppHandle) {
    toggle_as(app, true);
}

/// The shortcut for an idea: your voice alone, no call, saved as a note with its date and time.
pub fn toggle_idea(app: &AppHandle) {
    toggle_as(app, true);
}

fn toggle_as(app: &AppHandle, idea: bool) {
    if is_recording(app) {
        stop(app);
    } else if let Err(err) = start_as(app, idea) {
        let (text, action) = if err == "models" {
            (t("Download the voice model first, in Coucou → Settings → Recognition."), Some((t("Open Settings"), "openSettings")))
        } else {
            (err, None)
        };
        pill::announce(app, "error", "#FF6B6B", text, action, 8);
    }
}

pub fn start(app: &AppHandle) -> Result<String, String> {
    start_as(app, true)
}

fn start_as(app: &AppHandle, idea: bool) -> Result<String, String> {
    let taker = get(app).ok_or("not ready")?;
    if taker.recording.lock().unwrap().is_some() || taker.starting.swap(true, Ordering::Relaxed) {
        return Err("already recording".into());
    }
    let result = start_inner(app, &taker, idea);
    taker.starting.store(false, Ordering::Relaxed);
    result
}

fn start_inner(app: &AppHandle, taker: &Arc<NoteTaker>, idea: bool) -> Result<String, String> {
    if !speech::is_ready() {
        return Err("models".into());
    }
    let now = now_ms();
    let stamp = local_stamp();
    let id = crate::flow::text::new_id();
    let note = Note {
        id: id.clone(),
        title: {
            let when = [("0", stamp.get(5..).unwrap_or(&stamp))];
            if idea { tf("Idea · {0}", &when) } else { tf("Call · {0}", &when) }
        },
        title_edited: false,
        start: now,
        end: None,
        app: crate::flow::os::foreground_app().filter(|(id, _)| !crate::flow::is_shell(id)).map(|(_, name)| name),
        status: Status::Recording,
        speakers: Vec::new(),
        lines: Vec::new(),
        summary: None,
        key_points: Vec::new(),
        todos: Vec::new(),
        error: None,
        start_local: stamp,
        comments: Vec::new(),
        comment_hash: None,
    };
    let dir = note_dir(&id);
    crate::platform::ensure_private_dir(&dir).map_err(|e| e.to_string())?;
    let mic_channel = Arc::new(Mutex::new(Channel::new(&dir.join("mic.wav")).map_err(|e| e.to_string())?));
    let mic_pref = crate::flow::get(app).map(|f| f.store.lock().unwrap().prefs.mic.clone()).unwrap_or_default();
    let sink = mic_channel.clone();
    let mic = audio::Capture::start(audio::Source::Mic(if mic_pref.is_empty() { None } else { Some(mic_pref) }), move |s| {
        sink.lock().unwrap().push(s)
    })
    .map_err(|err| {
        crate::log::line(format!("notes: microphone: {err}"));
        let _ = std::fs::remove_dir_all(&dir);
        t("Coucou can't hear you: the microphone is not allowed or not available.")
    })?;
    // Without the computer's sound, the call is noted from the microphone alone.
    let (system, system_channel) = match Channel::new(&dir.join("system.wav")) {
        // An idea is your voice alone: the computer's sound is not recorded.
        _ if idea => (None, None),
        Ok(channel) => {
            let channel = Arc::new(Mutex::new(channel));
            let sink = channel.clone();
            match audio::Capture::start(audio::Source::System, move |s| sink.lock().unwrap().push(s)) {
                Ok(capture) => (Some(capture), Some(channel)),
                Err(err) => {
                    crate::log::line(format!("notes: system sound unavailable: {err}"));
                    let _ = std::fs::remove_file(dir.join("system.wav"));
                    (None, None)
                }
            }
        }
        Err(_) => (None, None),
    };
    let stop_live = Arc::new(AtomicBool::new(false));
    taker.notes.lock().unwrap().insert(0, note.clone());
    taker.save(&note);
    *taker.recording.lock().unwrap() = Some(Recording {
        id: id.clone(),
        mic,
        system,
        mic_channel: mic_channel.clone(),
        system_channel: system_channel.clone(),
        stop_live: stop_live.clone(),
    });
    speech::preload();
    spawn_live(app.clone(), taker.clone(), id.clone(), mic_channel, system_channel, stop_live);
    pill::update(app, |s| s.note_started_at = Some(note.start));
    let _ = app.emit_to(crate::island::WINDOW_LABEL, "meeting", true);
    let _ = app.emit_to(crate::HUB_LABEL, "notes-changed", &id);
    crate::log::line("notes: recording");
    Ok(id)
}

/// Every second while recording: the stretches ending on a pause go to
/// Parakeet for the live transcript (the final one is written again from the
/// whole recording).
fn spawn_live(
    app: AppHandle,
    taker: Arc<NoteTaker>,
    id: String,
    mic: Arc<Mutex<Channel>>,
    system: Option<Arc<Mutex<Channel>>>,
    stop: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        let started = std::time::Instant::now();
        let mut last_words = std::time::Instant::now();
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(1));
            // A call left recording by mistake keeps the microphone and the
            // voice model busy for hours, then takes as long again to write:
            // it stops by itself after a long silence, or a very long call.
            if let Some(why) = auto_stop(started.elapsed(), last_words.elapsed()) {
                crate::log::line(format!("notes: stopped by itself ({why:?})"));
                // `stop` here is the live loop's flag: the module's function.
                crate::notes::stop(&app);
                let text = match why {
                    AutoStop::Silence => t("Nobody has spoken for 15 minutes: Coucou stopped recording the call."),
                    AutoStop::Length => t("The call has been recorded for 4 hours: Coucou stopped recording it."),
                };
                pill::announce(&app, "mic", "#F7B32B", text, None, 30);
                break;
            }
            let mut channels = vec![(mic.clone(), builder::ME)];
            if let Some(s) = &system {
                channels.push((s.clone(), "others"));
            }
            for (channel, speaker) in channels {
                let chunk = channel.lock().unwrap().take_chunk();
                let Some((start, samples)) = chunk else { continue };
                let Some(transcript) = speech::transcribe(&samples) else { continue };
                let text = transcript.text.trim().to_string();
                if text.is_empty() || stop.load(Ordering::Relaxed) {
                    continue;
                }
                last_words = std::time::Instant::now();
                let end = start + samples.len() as f64 / speech::SAMPLE_RATE as f64;
                taker.change(&app, &id, |n| {
                    n.lines.push(Line { id: crate::flow::text::new_id(), speaker: speaker.into(), start, end, text });
                    n.lines.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
                });
            }
        }
    });
}

/// Why a call stopped recording by itself.
#[derive(Debug, Clone, Copy, PartialEq)]
enum AutoStop {
    Silence,
    Length,
}

/// Longest stretch with nobody speaking, and longest call.
const SILENCE_LIMIT: Duration = Duration::from_secs(15 * 60);
const LENGTH_LIMIT: Duration = Duration::from_secs(4 * 3600);

fn auto_stop(recording: Duration, since_words: Duration) -> Option<AutoStop> {
    if recording >= LENGTH_LIMIT {
        Some(AutoStop::Length)
    } else if since_words >= SILENCE_LIMIT {
        Some(AutoStop::Silence)
    } else {
        None
    }
}

pub fn stop(app: &AppHandle) {
    let Some(taker) = get(app) else { return };
    let Some(mut rec) = taker.recording.lock().unwrap().take() else { return };
    rec.stop_live.store(true, Ordering::Relaxed);
    rec.mic.stop();
    if let Some(s) = rec.system.as_mut() {
        s.stop();
    }
    rec.mic_channel.lock().unwrap().close();
    if let Some(c) = &rec.system_channel {
        c.lock().unwrap().close();
    }
    taker.change(app, &rec.id, |n| {
        n.end = Some(now_ms());
        n.status = Status::Processing;
    });
    pill::update(app, |s| s.note_started_at = None);
    let _ = app.emit_to(crate::island::WINDOW_LABEL, "meeting", false);
    let app = app.clone();
    let id = rec.id.clone();
    std::thread::spawn(move || process(&app, &id));
}

/// Coucou quits during a call: the recordings are closed properly, and the
/// transcript is written at the next launch.
pub fn shutdown(app: &AppHandle) {
    let Some(taker) = get(app) else { return };
    let Some(mut rec) = taker.recording.lock().unwrap().take() else { return };
    rec.stop_live.store(true, Ordering::Relaxed);
    rec.mic.stop();
    if let Some(s) = rec.system.as_mut() {
        s.stop();
    }
    rec.mic_channel.lock().unwrap().close();
    if let Some(c) = &rec.system_channel {
        c.lock().unwrap().close();
    }
    taker.change(app, &rec.id, |n| {
        n.end = Some(now_ms());
        n.status = Status::Processing;
    });
}

fn words_in(path: &std::path::Path) -> Result<(Vec<TimedWord>, Vec<f32>), String> {
    if !path.exists() {
        return Ok((Vec::new(), Vec::new()));
    }
    let samples = audio::read_wav(path).map_err(|e| e.to_string())?;
    let transcript = speech::transcribe(&samples).ok_or_else(|| t("Download the voice model first, in Coucou → Settings → Recognition."))?;
    Ok((transcript.words, samples))
}

/// Writes the transcript from the recordings, then the summary, then deletes the audio.
fn process(app: &AppHandle, id: &str) {
    let Some(taker) = get(app) else { return };
    {
        let mut working = taker.working.lock().unwrap();
        if working.iter().any(|w| w == id) {
            return;
        }
        working.push(id.to_string());
    }
    let _ = app.emit_to(crate::HUB_LABEL, "notes-changed", id);
    let result = transcript(app, &taker, id);
    if let Err(err) = &result {
        crate::log::line(format!("notes: transcript failed: {err}"));
        taker.change(app, id, |n| {
            n.status = Status::Failed;
            n.error = Some(err.clone());
        });
    }
    taker.working.lock().unwrap().retain(|w| w != id);
    if result.is_ok() {
        let _ = app.emit_to(crate::island::WINDOW_LABEL, "note-created", id);
        let engine = crate::flow::get(app).map(|f| f.store.lock().unwrap().prefs.summary_engine).unwrap_or_default();
        if engine == SummaryEngine::Claude {
            summarize(app, id);
        }
        maybe_generate_auto_comments(app, id, false);
        let title = taker.note(id).map(|n| n.title).unwrap_or_default();
        pill::announce(
            app,
            "check",
            "#34D399",
            tf("Notes ready: {0}", &[("0", &title)]),
            Some((t("See"), "openNotes")),
            12,
        );
    }
    let _ = app.emit_to(crate::HUB_LABEL, "notes-changed", id);
}

fn transcript(app: &AppHandle, taker: &Arc<NoteTaker>, id: &str) -> Result<(), String> {
    let dir = note_dir(id);
    let mic_path = dir.join("mic.wav");
    let system_path = dir.join("system.wav");
    if !mic_path.exists() {
        return Err(t("The recording of this call is gone."));
    }
    let me = t("Me");
    let (mine, mic_samples) = words_in(&mic_path)?;
    let (theirs, system_samples) = words_in(&system_path)?;
    let built = if !system_path.exists() {
        // Anotador por voz: apenas a fala do usuário no microfone.
        builder::build(&mine, &[], &[], &me, person_name)
    } else if theirs.len() >= 5 {
        // A call: you on the microphone, the others on the computer's sound.
        let turns = speech::diarize(&system_samples);
        builder::build(&mine, &theirs, &turns, &me, person_name)
    } else if mine.len() >= 20 {
        // People around the computer: one recording, the voices told apart.
        let turns = speech::diarize(&mic_samples);
        let voices: std::collections::HashSet<&str> = turns.iter().map(|t| t.speaker.as_str()).collect();
        if voices.len() > 1 {
            builder::build_room(&mine, &turns, person_name)
        } else {
            builder::build(&mine, &[], &[], &me, person_name)
        }
    } else {
        builder::build(&mine, &theirs, &[], &me, person_name)
    };
    let (speakers, lines) = built;
    let count = lines.len();
    taker.change(app, id, |n| {
        n.speakers = speakers;
        n.lines = lines;
        n.status = Status::Ready;
        n.error = None;
    });
    let _ = std::fs::remove_file(&mic_path);
    let _ = std::fs::remove_file(&system_path);
    crate::log::line(format!("notes: transcript of {count} lines"));
    Ok(())
}

fn plain_transcript(note: &Note) -> String {
    note.lines
        .iter()
        .map(|l| {
            let name = note.speakers.iter().find(|s| s.id == l.speaker).map(|s| s.name.clone()).unwrap_or_else(|| l.speaker.clone());
            format!("{name}: {}", l.text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Writes (or writes again) the summary, key points and to-dos of a note.
fn summarize(app: &AppHandle, id: &str) {
    let Some(taker) = get(app) else { return };
    let Some(note) = taker.note(id) else { return };
    if note.lines.is_empty() {
        return;
    }
    let Some(settings) = app.try_state::<crate::Shared>().map(|s| s.settings.lock().unwrap().clone()) else {
        return;
    };
    taker.working.lock().unwrap().push(id.to_string());
    let _ = app.emit_to(crate::HUB_LABEL, "notes-changed", id);
    let mut transcript = plain_transcript(&note);
    if transcript.len() > 400_000 {
        let mut cut = 400_000;
        while !transcript.is_char_boundary(cut) {
            cut -= 1;
        }
        transcript.truncate(cut);
    }
    let user = format!("<transcript>\n{transcript}\n</transcript>");
    let reply = tauri::async_runtime::block_on(async {
        crate::chat::complete_oneshot(&settings, builder::SUMMARY_INSTRUCTIONS, &user, 2048, Duration::from_secs(120))
            .await
            .map_err(|err| {
                crate::log::line(format!("notes: summarize: {err}"));
                err
            })
            .ok()
    });
    taker.working.lock().unwrap().retain(|w| w != id);
    let Some(reply) = reply else {
        let _ = app.emit_to(crate::HUB_LABEL, "notes-changed", id);
        return;
    };
    taker.change(app, id, |n| match builder::parse_summary(&reply) {
        Some(parsed) => {
            if let Some(title) = parsed.title.filter(|_| !n.title_edited) {
                n.title = title;
            }
            n.summary = Some(parsed.summary);
            n.key_points = parsed.key_points;
            n.todos = parsed.todos;
        }
        None => n.summary = Some(reply.trim().to_string()),
    });
}

/// The note as Markdown: title, date, summary, key points, to-dos, then the transcript.
pub fn markdown(note: &Note) -> String {
    let mut out = format!("# {}\n\n", note.title);
    let minutes = (((note.end.unwrap_or(note.start).saturating_sub(note.start)) as f64 / 60_000.0).round() as u64).max(1);
    out += &format!("_{} · {} min_", note.start_local, minutes);
    if !note.speakers.is_empty() {
        out += &format!("  \n_{}_", note.speakers.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", "));
    }
    out += "\n\n";
    if let Some(summary) = note.summary.as_ref().filter(|s| !s.is_empty()) {
        out += &format!("## {}\n\n{summary}\n\n", t("Summary"));
    }
    if !note.key_points.is_empty() {
        out += &format!("## {}\n\n", t("Key points"));
        out += &note.key_points.iter().map(|p| format!("- {p}")).collect::<Vec<_>>().join("\n");
        out += "\n\n";
    }
    if !note.todos.is_empty() {
        out += &format!("## {}\n\n", t("To do"));
        let lines: Vec<String> = note
            .todos
            .iter()
            .map(|todo| {
                let mut line = format!("- [{}] {}", if todo.done { "x" } else { " " }, todo.text);
                let extra: Vec<&str> = [todo.owner.as_deref(), todo.due.as_deref()].into_iter().flatten().filter(|s| !s.is_empty()).collect();
                if !extra.is_empty() {
                    line += &format!(" ({})", extra.join(", "));
                }
                line
            })
            .collect();
        out += &lines.join("\n");
        out += "\n\n";
    }
    if !note.lines.is_empty() {
        out += &format!("## {}\n\n", t("Transcript"));
        let lines: Vec<String> = note
            .lines
            .iter()
            .map(|l| {
                let name = note.speakers.iter().find(|s| s.id == l.speaker).map(|s| s.name.as_str()).unwrap_or(&l.speaker);
                format!("**{name}** ({}) : {}", builder::clock(l.start), l.text)
            })
            .collect();
        out += &lines.join("\n\n");
        out += "\n";
    }
    if !note.comments.is_empty() {
        out += &format!("\n## Comentários das personas\n\n");
        for comment in &note.comments {
            out += &format!("- **{}**: {}\n", comment.character_name, comment.content.trim());
        }
    }
    out
}

struct CommentRunGuard<'a> {
    set: &'a Mutex<HashSet<String>>,
    id: String,
}

impl<'a> CommentRunGuard<'a> {
    fn acquire(set: &'a Mutex<HashSet<String>>, id: &str) -> Option<Self> {
        let mut guard = set.lock().unwrap();
        if !guard.insert(id.to_string()) {
            return None;
        }
        Some(Self { set, id: id.to_string() })
    }
}

impl Drop for CommentRunGuard<'_> {
    fn drop(&mut self) {
        self.set.lock().unwrap().remove(&self.id);
    }
}

fn derive_note_title(explicit: Option<&str>, text: &str, stamp: &str) -> (String, bool) {
    if let Some(t_str) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return (t_str.to_string(), true);
    }
    let first_line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let chars: Vec<char> = first_line.chars().collect();
    if !chars.is_empty() {
        let snippet: String = if chars.len() > 56 {
            format!("{}…", chars[..55].iter().collect::<String>().trim_end())
        } else {
            chars.into_iter().collect()
        };
        return (snippet, false);
    }
    let when = [("0", stamp.get(5..).unwrap_or(stamp))];
    (tf("Idea · {0}", &when), false)
}

fn create_text_note_record(
    id: String,
    title: Option<&str>,
    text: &str,
    now: u64,
    stamp: String,
    me_name: String,
    app_name: Option<String>,
) -> Note {
    let (resolved_title, title_edited) = derive_note_title(title, text, &stamp);
    Note {
        id,
        title: resolved_title,
        title_edited,
        start: now,
        end: Some(now),
        app: app_name,
        status: Status::Ready,
        speakers: vec![Speaker {
            id: builder::ME.to_string(),
            name: me_name,
            color: 0,
            is_me: true,
        }],
        lines: vec![Line {
            id: "l1".to_string(),
            speaker: builder::ME.to_string(),
            start: 0.0,
            end: 0.0,
            text: text.trim().to_string(),
        }],
        summary: None,
        key_points: Vec::new(),
        todos: Vec::new(),
        error: None,
        start_local: stamp,
        comments: Vec::new(),
        comment_hash: None,
    }
}

/// Garante idempotência de comentários automáticos por nota e hash de conteúdo.
fn apply_auto_comments_dedup(
    note: &mut Note,
    hash: &str,
    new_comments: Vec<personas::NoteComment>,
) -> usize {
    if note.comment_hash.as_deref() == Some(hash) && new_comments.is_empty() {
        return 0;
    }
    let mut added = 0usize;
    for comment in new_comments {
        let already_top_level = note.comments.iter().any(|existing| {
            existing.reply_to_id.is_none()
                && existing.author_kind != "user"
                && existing.character_id == comment.character_id
        });
        if !already_top_level {
            note.comments.push(comment);
            added += 1;
        }
    }
    note.comment_hash = Some(hash.to_string());
    added
}

fn maybe_generate_auto_comments(app: &AppHandle, id: &str, force_refresh: bool) {
    let Some(taker) = get(app) else { return };
    let Some(_guard) = CommentRunGuard::acquire(&taker.commenting, id) else {
        return;
    };
    let Some(note) = taker.note(id) else { return };
    let body = plain_transcript(&note);
    if body.trim().is_empty() {
        return;
    }
    let hash = personas::content_hash(&note.title, &body);
    if !force_refresh && note.comment_hash.as_deref() == Some(hash.as_str()) {
        return;
    }
    let state = personas::load_state();
    let active: Vec<&personas::PersonaProfile> = state.personas.iter().filter(|p| p.enabled).collect();
    if active.is_empty() {
        return;
    }
    let Some(settings) = app.try_state::<crate::Shared>().map(|s| s.settings.lock().unwrap().clone()) else {
        return;
    };

    taker.working.lock().unwrap().push(id.to_string());
    let _ = app.emit_to(crate::HUB_LABEL, "notes-changed", id);

    let summary_text = note.summary.clone();
    let mut existing = note.comments.clone();
    let title = note.title.clone();
    let comment_settings = if force_refresh {
        personas::CommentSettings {
            enable_character_comment: true,
            ..state.settings.clone()
        }
    } else {
        state.settings.clone()
    };

    let generated = tauri::async_runtime::block_on(async {
        let selected = personas::select_characters_with(
            &active,
            &body,
            &body,
            &existing,
            None,
            &comment_settings,
            |sys, usr| {
                let cfg = settings.clone();
                async move {
                    crate::chat::complete_oneshot(&cfg, &sys, &usr, 256, Duration::from_secs(45)).await
                }
            },
        )
        .await;

        let mut produced = Vec::new();
        for item in selected {
            let already_commented = existing.iter().any(|c| {
                c.reply_to_id.is_none()
                    && c.author_kind != "user"
                    && c.character_id == item.persona.id
            });
            if already_commented {
                continue;
            }
            let (sys, usr) = personas::build_comment_prompt(
                item.persona,
                &title,
                &body,
                summary_text.as_deref(),
                None,
                &existing,
                item.force_reply,
            );
            let Ok(raw) = crate::chat::complete_oneshot(&settings, &sys, &usr, 512, Duration::from_secs(60)).await else {
                continue;
            };
            let Some(text) = personas::parse_comment_output(&raw, item.force_reply) else {
                continue;
            };
            let comment = personas::NoteComment {
                id: crate::flow::text::new_id(),
                character_id: item.persona.id.clone(),
                character_name: item.persona.name.clone(),
                author_kind: "persona".to_string(),
                content: text,
                reply_to_id: None,
                created_at: now_ms(),
            };
            existing.push(comment.clone());
            produced.push(comment);
        }
        produced
    });

    taker.working.lock().unwrap().retain(|w| w != id);
    taker.change(app, id, |n| {
        apply_auto_comments_dedup(n, &hash, generated);
    });
}

fn handle_user_comment_replies(
    app: &AppHandle,
    note_id: &str,
    user_comment_id: &str,
    user_text: &str,
    parent_reply_to: Option<&str>,
) {
    let Some(taker) = get(app) else { return };
    let Some(_guard) = CommentRunGuard::acquire(&taker.commenting, note_id) else {
        return;
    };
    let Some(note) = taker.note(note_id) else { return };
    let state = personas::load_state();
    let active: Vec<&personas::PersonaProfile> = state.personas.iter().filter(|p| p.enabled).collect();
    if active.is_empty() {
        return;
    }
    let has_direct_target = personas::extract_mention(user_text, &active).is_some()
        || personas::resolve_reply_target_character(&note.comments, parent_reply_to, &active).is_some();
    if !has_direct_target {
        return;
    }
    let remaining = personas::remaining_persona_turns(&note.comments, personas::MAX_PERSONA_TURNS_PER_USER_MESSAGE);
    if remaining == 0 {
        return;
    }
    let Some(settings) = app.try_state::<crate::Shared>().map(|s| s.settings.lock().unwrap().clone()) else {
        return;
    };
    let body = plain_transcript(&note);
    let summary_text = note.summary.clone();
    let title = note.title.clone();
    let existing = note.comments.clone();

    taker.working.lock().unwrap().push(note_id.to_string());
    let _ = app.emit_to(crate::HUB_LABEL, "notes-changed", note_id);

    let reply = tauri::async_runtime::block_on(async {
        let selected = personas::select_characters_with(
            &active,
            &body,
            user_text,
            &existing,
            parent_reply_to,
            &state.settings,
            |sys, usr| {
                let cfg = settings.clone();
                async move {
                    crate::chat::complete_oneshot(&cfg, &sys, &usr, 256, Duration::from_secs(45)).await
                }
            },
        )
        .await;
        let Some(first) = selected.into_iter().next() else {
            return None;
        };
        let (sys, usr) = personas::build_comment_prompt(
            first.persona,
            &title,
            &body,
            summary_text.as_deref(),
            Some(user_text),
            &existing,
            true,
        );
        let raw = crate::chat::complete_oneshot(&settings, &sys, &usr, 512, Duration::from_secs(60)).await.ok()?;
        let text = personas::parse_comment_output(&raw, true)?;
        Some(personas::NoteComment {
            id: crate::flow::text::new_id(),
            character_id: first.persona.id.clone(),
            character_name: first.persona.name.clone(),
            author_kind: "persona".to_string(),
            content: text,
            reply_to_id: Some(user_comment_id.to_string()),
            created_at: now_ms(),
        })
    });

    taker.working.lock().unwrap().retain(|w| w != note_id);
    if let Some(comment) = reply {
        taker.change(app, note_id, |n| {
            n.comments.push(comment);
        });
    } else {
        let _ = app.emit_to(crate::HUB_LABEL, "notes-changed", note_id);
    }
}

// ── Commands ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesState {
    notes: Vec<Note>,
    recording: Option<String>,
    working: Vec<String>,
}

#[tauri::command]
pub fn notes_state(taker: State<Arc<NoteTaker>>) -> NotesState {
    NotesState {
        notes: taker.notes.lock().unwrap().clone(),
        recording: taker.recording.lock().unwrap().as_ref().map(|r| r.id.clone()),
        working: taker.working.lock().unwrap().clone(),
    }
}

/// The "Note an idea" shortcut: start the one-voice note, or stop it.
#[tauri::command]
pub async fn notes_idea(app: AppHandle) {
    let _ = tauri::async_runtime::spawn_blocking(move || toggle_idea(&app)).await;
}

#[tauri::command]
pub async fn notes_start(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        start(&app).map_err(|e| if e == "models" { t("Download the voice model first, in Coucou → Settings → Recognition.") } else { e })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn notes_stop(app: AppHandle) {
    stop(&app);
}

#[tauri::command]
pub fn notes_rename(app: AppHandle, taker: State<Arc<NoteTaker>>, id: String, title: String) {
    let title = title.trim().to_string();
    if title.is_empty() {
        return;
    }
    taker.change(&app, &id, |n| {
        n.title = title;
        n.title_edited = true;
    });
}

#[tauri::command]
pub fn notes_rename_speaker(app: AppHandle, taker: State<Arc<NoteTaker>>, id: String, speaker: String, name: String) {
    let name = name.trim().to_string();
    if name.is_empty() {
        return;
    }
    taker.change(&app, &id, |n| {
        if let Some(s) = n.speakers.iter_mut().find(|s| s.id == speaker) {
            s.name = name;
        }
    });
}

#[tauri::command]
pub fn notes_toggle_todo(app: AppHandle, taker: State<Arc<NoteTaker>>, id: String, todo: String) {
    taker.change(&app, &id, |n| {
        if let Some(t) = n.todos.iter_mut().find(|t| t.id == todo) {
            t.done = !t.done;
        }
    });
}

#[tauri::command]
pub fn notes_delete(app: AppHandle, taker: State<Arc<NoteTaker>>, id: String) {
    if taker.recording.lock().unwrap().as_ref().is_some_and(|r| r.id == id) {
        return;
    }
    taker.notes.lock().unwrap().retain(|n| n.id != id);
    let _ = std::fs::remove_dir_all(note_dir(&id));
    let _ = app.emit_to(crate::HUB_LABEL, "notes-changed", &id);
}

#[tauri::command]
pub fn notes_summarize(app: AppHandle, id: String) {
    std::thread::spawn(move || summarize(&app, &id));
}

#[tauri::command]
pub fn notes_retry(app: AppHandle, id: String) {
    std::thread::spawn(move || process(&app, &id));
}

#[tauri::command]
pub fn notes_create_text(
    app: AppHandle,
    taker: State<Arc<NoteTaker>>,
    title: Option<String>,
    text: String,
) -> Result<Note, String> {
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return Err("Escreva uma ideia ou pensamento antes de salvar.".into());
    }
    let id = crate::flow::text::new_id();
    let now = now_ms();
    let stamp = local_stamp();
    let app_name = crate::flow::os::foreground_app()
        .filter(|(pid, _)| !crate::flow::is_shell(pid))
        .map(|(_, name)| name);
    let note = create_text_note_record(
        id.clone(),
        title.as_deref(),
        &trimmed,
        now,
        stamp,
        t("Me"),
        app_name,
    );
    taker.notes.lock().unwrap().insert(0, note.clone());
    taker.save(&note);
    let _ = app.emit_to(crate::HUB_LABEL, "notes-changed", &id);
    let _ = app.emit_to(crate::island::WINDOW_LABEL, "note-created", &id);

    let bg_app = app.clone();
    let bg_id = id.clone();
    std::thread::spawn(move || {
        let engine = crate::flow::get(&bg_app)
            .map(|f| f.store.lock().unwrap().prefs.summary_engine)
            .unwrap_or_default();
        if engine == SummaryEngine::Claude && trimmed.chars().count() >= 40 {
            summarize(&bg_app, &bg_id);
        }
        maybe_generate_auto_comments(&bg_app, &bg_id, false);
    });
    Ok(note)
}

#[tauri::command]
pub fn notes_add_comment(
    app: AppHandle,
    taker: State<Arc<NoteTaker>>,
    id: String,
    content: String,
    reply_to_id: Option<String>,
) -> Result<Note, String> {
    let trimmed = content.trim().to_string();
    if trimmed.is_empty() {
        return Err("Escreva um comentário antes de enviar.".into());
    }
    let user_comment_id = crate::flow::text::new_id();
    let comment = personas::NoteComment {
        id: user_comment_id.clone(),
        character_id: "user".to_string(),
        character_name: "Você".to_string(),
        author_kind: "user".to_string(),
        content: trimmed.clone(),
        reply_to_id: reply_to_id.clone(),
        created_at: now_ms(),
    };
    let updated = taker
        .change(&app, &id, |n| {
            n.comments.push(comment);
        })
        .ok_or_else(|| "Nota não encontrada.".to_string())?;

    let bg_app = app.clone();
    std::thread::spawn(move || {
        handle_user_comment_replies(
            &bg_app,
            &id,
            &user_comment_id,
            &trimmed,
            reply_to_id.as_deref(),
        );
    });
    Ok(updated)
}

#[tauri::command]
pub fn notes_generate_comments(app: AppHandle, id: String) -> Result<(), String> {
    if personas::load_personas().into_iter().all(|p| !p.enabled) {
        return Err("Importe ou ative ao menos uma persona para comentar.".into());
    }
    std::thread::spawn(move || maybe_generate_auto_comments(&app, &id, true));
    Ok(())
}

#[tauri::command]
pub fn personas_state() -> personas::PersonasState {
    personas::load_state()
}

#[tauri::command]
pub fn personas_import(app: AppHandle, raw: String) -> Result<personas::PersonasState, String> {
    let imported = personas::parse_import_json(&raw)?;
    let merged = personas::merge_imported(personas::load_personas(), imported);
    personas::save_personas(&merged)?;
    let _ = app.emit_to(crate::HUB_LABEL, "personas-changed", ());
    Ok(personas::load_state())
}

#[tauri::command]
pub fn personas_toggle(app: AppHandle, id: String, enabled: bool) -> Result<personas::PersonasState, String> {
    let mut all = personas::load_personas();
    let target = all.iter_mut().find(|p| p.id == id).ok_or_else(|| "Persona não encontrada.".to_string())?;
    target.enabled = enabled;
    personas::save_personas(&all)?;
    let _ = app.emit_to(crate::HUB_LABEL, "personas-changed", ());
    Ok(personas::load_state())
}

#[tauri::command]
pub fn personas_delete(app: AppHandle, id: String) -> Result<personas::PersonasState, String> {
    let mut all = personas::load_personas();
    all.retain(|p| p.id != id);
    personas::save_personas(&all)?;
    let _ = app.emit_to(crate::HUB_LABEL, "personas-changed", ());
    Ok(personas::load_state())
}

#[tauri::command]
pub fn personas_set_primary(app: AppHandle, id: String) -> Result<personas::PersonasState, String> {
    let mut all = personas::load_personas();
    if !all.iter().any(|p| p.id == id) {
        return Err("Persona não encontrada.".into());
    }
    for p in &mut all {
        p.is_primary_companion = p.id == id;
    }
    personas::save_personas(&all)?;
    let _ = app.emit_to(crate::HUB_LABEL, "personas-changed", ());
    Ok(personas::load_state())
}

#[tauri::command]
pub fn personas_set_comment_settings(
    app: AppHandle,
    enable_character_comment: bool,
    max_comment_characters: usize,
) -> Result<personas::PersonasState, String> {
    personas::save_comment_settings(personas::CommentSettings {
        enable_character_comment,
        max_comment_characters,
    })?;
    let _ = app.emit_to(crate::HUB_LABEL, "personas-changed", ());
    Ok(personas::load_state())
}

#[tauri::command]
pub fn notes_markdown(taker: State<Arc<NoteTaker>>, id: String) -> Option<String> {
    taker.note(&id).map(|n| markdown(&n))
}

/// Windows asks where to save; cancelling the native dialog writes nothing.
#[tauri::command]
pub async fn notes_export(window: tauri::WebviewWindow, taker: State<'_, Arc<NoteTaker>>, id: String) -> Result<Option<String>, String> {
    if window.label() != crate::HUB_LABEL { return Err("Open the note in Coucou to export it.".into()); }
    let note = taker.note(&id).ok_or("no such note")?;
    let safe: String = note
        .title
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let name = if safe.is_empty() { "Coucou notes" } else { &safe };
    #[cfg(windows)]
    {
        let owner = window.hwnd().map_err(|error| error.to_string())?.0 as isize;
        let selected = export::choose_path(owner, format!("{name}.md")).await?;
        let content = markdown(&note);
        tauri::async_runtime::spawn_blocking(move || export::write_selected(selected, &content))
            .await.map_err(|error| error.to_string())?
    }
    #[cfg(not(windows))]
    {
        let home = crate::platform::home_dir();
        let dir = [home.join("Documents"), home.clone()].into_iter().find(|d| d.is_dir()).unwrap_or(home);
        let mut path = dir.join(format!("{name}.md"));
        let mut n = 2;
        while path.exists() {
            path = dir.join(format!("{name} ({n}).md"));
            n += 1;
        }
        std::fs::write(&path, markdown(&note)).map_err(|e| e.to_string())?;
        crate::platform::reveal_folder(&dir.to_string_lossy());
        Ok(Some(path.to_string_lossy().to_string()))
    }
}

#[cfg(test)]
mod auto_stop_tests {
    use super::*;

    #[test]
    fn a_forgotten_call_stops_by_itself() {
        let min = |m: u64| Duration::from_secs(m * 60);
        assert_eq!(auto_stop(min(60), min(2)), None);
        assert_eq!(auto_stop(min(60), min(15)), Some(AutoStop::Silence));
        assert_eq!(auto_stop(min(240), min(1)), Some(AutoStop::Length));
        assert_eq!(auto_stop(min(14), min(14)), None);
    }

    #[test]
    fn text_note_creation_backward_compat_and_comment_dedup() {
        let legacy_json = r#"{
            "id": "legacy-1",
            "title": "Nota antiga",
            "start": 1000,
            "status": "ready",
            "speakers": [],
            "lines": [{ "id": "l1", "speaker": "me", "start": 0.0, "end": 1.0, "text": "Conteúdo legado." }]
        }"#;
        let legacy: Note = serde_json::from_str(legacy_json).unwrap();
        assert!(legacy.comments.is_empty());
        assert_eq!(legacy.comment_hash, None);

        let mut note = create_text_note_record(
            "n1".into(),
            None,
            "Ideia sobre arquitetura offline-first para desktop.\nSegunda linha.",
            2000,
            "2026-11-14 10:00".into(),
            "Eu".into(),
            None,
        );
        assert_eq!(note.title, "Ideia sobre arquitetura offline-first para desktop.");
        assert!(!note.title_edited);
        assert_eq!(note.status, Status::Ready);

        let hash = personas::content_hash(&note.title, &plain_transcript(&note));
        let first = personas::NoteComment {
            id: "c1".into(),
            character_id: "clara".into(),
            character_name: "Clara".into(),
            author_kind: "persona".into(),
            content: "Vale definir os limites de cache local.".into(),
            reply_to_id: None,
            created_at: 2100,
        };
        assert_eq!(apply_auto_comments_dedup(&mut note, &hash, vec![first.clone()]), 1);
        assert_eq!(note.comments.len(), 1);
        assert_eq!(note.comment_hash.as_deref(), Some(hash.as_str()));

        // Reexecução automática ou duplicada para a mesma persona não duplica comentário.
        assert_eq!(apply_auto_comments_dedup(&mut note, &hash, vec![first]), 0);
        assert_eq!(note.comments.len(), 1);
        assert!(markdown(&note).contains("## Comentários das personas"));
    }
}

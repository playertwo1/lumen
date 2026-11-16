//! Registro seguro dos servidores MCP locais do Lumen nas CLIs externas via `config_file`.
//!
//! Suporta os 4 alvos aprovados na M0.2:
//! - Claude Code (`~/.claude.json`)
//! - Codex (`~/.codex/config.toml`)
//! - Gemini CLI (`~/.gemini/settings.json`)
//! - Antigravity (`~/.gemini/antigravity/mcp_config.json`)
//!
//! Toda prévia (`preview`), backup datado (`.bak-YYYYMMDD-HHMMSS`), verificação de
//! `fingerprint` e gravação atômica passa obrigatoriamente por `crate::config_file`.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::agents::{object_at, unexpected};
use crate::config_file::{self, FileEdit, Plan};
use crate::{platform, settings};

const SERVER_KEY: &str = "lumen";
const MCP_ARG: &str = "--mcp";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum McpAgent {
    Claude,
    Codex,
    Gemini,
    Antigravity,
}

impl McpAgent {
    pub const ALL: &'static [McpAgent] = &[
        McpAgent::Claude,
        McpAgent::Codex,
        McpAgent::Gemini,
        McpAgent::Antigravity,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Antigravity => "antigravity",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|agent| agent.id() == id)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::Gemini => "Gemini CLI",
            Self::Antigravity => "Antigravity",
        }
    }

    fn binaries(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["claude"],
            Self::Codex => &["codex"],
            Self::Gemini => &["gemini"],
            Self::Antigravity => &["agy"],
        }
    }

    pub fn config_path(self, home: &Path) -> PathBuf {
        match self {
            Self::Claude => home.join(".claude.json"),
            Self::Codex => home.join(".codex").join("config.toml"),
            Self::Gemini => home.join(".gemini").join("settings.json"),
            Self::Antigravity => home
                .join(".gemini")
                .join("antigravity")
                .join("mcp_config.json"),
        }
    }

    fn has_home_footprint(self, home: &Path) -> bool {
        match self {
            Self::Claude => home.join(".claude.json").exists() || home.join(".claude").is_dir(),
            Self::Codex => home.join(".codex").is_dir(),
            Self::Gemini => {
                home.join(".gemini").join("settings.json").exists() || home.join(".gemini").is_dir()
            }
            Self::Antigravity => home.join(".gemini").join("antigravity").is_dir(),
        }
    }

    fn cli_found(self, home: &Path) -> bool {
        self.binaries()
            .iter()
            .any(|bin| !platform::cli_candidates(bin).is_empty())
            || self.has_home_footprint(home)
    }

    fn uses_stdio_type(self) -> bool {
        matches!(self, Self::Claude)
    }

    fn edit(self, home: &Path, hook_exe: &Path, install: bool) -> FileEdit<'static> {
        let path = self.config_path(home);
        let label = path.display().to_string();
        let command = hook_exe.to_string_lossy().to_string();
        let include_type = self.uses_stdio_type();

        let edit = match self {
            Self::Codex => config_file::text_edit(label.clone(), move |current| {
                edit_codex_toml(current, &command, install, &label)
            }),
            Self::Claude | Self::Gemini | Self::Antigravity => {
                config_file::json_edit(label, move |root| {
                    if install {
                        json_install(root, &command, include_type)
                    } else {
                        json_uninstall(root)
                    }
                })
            }
        };

        FileEdit { path, edit }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpAgentStatus {
    pub id: &'static str,
    pub name: &'static str,
    pub path: String,
    pub cli_found: bool,
    pub registered: bool,
    pub command_matches: bool,
}

fn find(id: &str) -> Result<McpAgent, String> {
    McpAgent::from_id(id).ok_or_else(|| crate::i18n::tf("Unknown agent \"{id}\".", &[("id", id)]))
}

pub fn status() -> Vec<McpAgentStatus> {
    status_at(&platform::home_dir(), &settings::hook_exe_path())
}

pub fn status_at(home: &Path, hook_exe: &Path) -> Vec<McpAgentStatus> {
    let expected_command = hook_exe.to_string_lossy();
    McpAgent::ALL
        .iter()
        .copied()
        .map(|agent| inspect_agent_status(agent, home, &expected_command, agent.cli_found(home)))
        .collect()
}

fn inspect_agent_status(
    agent: McpAgent,
    home: &Path,
    expected_command: &str,
    cli_found: bool,
) -> McpAgentStatus {
    let path = agent.config_path(home);
    let (registered, command_matches) = match agent {
        McpAgent::Codex => inspect_codex_status(&path, expected_command),
        McpAgent::Claude | McpAgent::Gemini | McpAgent::Antigravity => {
            inspect_json_status(&path, expected_command, agent.uses_stdio_type())
        }
    };

    McpAgentStatus {
        id: agent.id(),
        name: agent.name(),
        path: path.display().to_string(),
        cli_found,
        registered,
        command_matches,
    }
}

fn inspect_json_status(path: &Path, expected_command: &str, expect_stdio_type: bool) -> (bool, bool) {
    let Ok(Some(bytes)) = config_file::read(path) else {
        return (false, false);
    };
    let Ok(root) = config_file::parse_json(Some(&bytes), "") else {
        return (false, false);
    };
    let Some(entry) = root
        .get("mcpServers")
        .and_then(Value::as_object)
        .and_then(|servers| servers.get(SERVER_KEY))
    else {
        return (false, false);
    };

    let Some(obj) = entry.as_object() else {
        return (true, false);
    };
    let type_ok = !expect_stdio_type
        || obj.get("type").and_then(Value::as_str) == Some("stdio");
    let cmd_ok = obj.get("command").and_then(Value::as_str) == Some(expected_command);
    let args_ok = obj
        .get("args")
        .and_then(Value::as_array)
        .is_some_and(|args| {
            args.len() == 1 && args[0].as_str() == Some(MCP_ARG)
        });
    (true, type_ok && cmd_ok && args_ok)
}

fn inspect_codex_status(path: &Path, expected_command: &str) -> (bool, bool) {
    let Ok(Some(bytes)) = config_file::read(path) else {
        return (false, false);
    };
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return (false, false);
    };
    let Ok(doc) = parse_toml_doc(text, "") else {
        return (false, false);
    };
    let Some(section) = doc.lumen_section() else {
        return (false, false);
    };
    let mut found_command: Option<String> = None;
    let mut found_args: Option<Vec<String>> = None;
    for entry in &section.entries {
        if entry.key_path.as_slice() == ["command"] {
            found_command = parse_toml_string(&entry.raw_value);
        } else if entry.key_path.as_slice() == ["args"] {
            found_args = parse_toml_string_array(&entry.raw_value);
        }
    }
    let matches = found_command.as_deref() == Some(expected_command)
        && found_args
            .as_ref()
            .is_some_and(|args| args.len() == 1 && args[0] == MCP_ARG);
    (true, matches)
}

pub fn preview(agent_id: &str, install: bool) -> Result<Plan, String> {
    preview_at(
        agent_id,
        install,
        &platform::home_dir(),
        &settings::hook_exe_path(),
    )
}

pub fn preview_at(
    agent_id: &str,
    install: bool,
    home: &Path,
    hook_exe: &Path,
) -> Result<Plan, String> {
    let agent = find(agent_id)?;
    config_file::preview(&[agent.edit(home, hook_exe, install)])
}

pub fn apply(agent_id: &str, install: bool, fingerprint: &str) -> Result<String, String> {
    apply_at(
        agent_id,
        install,
        fingerprint,
        &platform::home_dir(),
        &settings::hook_exe_path(),
    )
}

pub fn apply_at(
    agent_id: &str,
    install: bool,
    fingerprint: &str,
    home: &Path,
    hook_exe: &Path,
) -> Result<String, String> {
    let agent = find(agent_id)?;
    let backups = config_file::apply(&[agent.edit(home, hook_exe, install)], fingerprint)?;
    Ok(backups
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join("\n"))
}

// ── JSON editors ──────────────────────────────────────────────────────────────

fn json_install(root: &Value, command: &str, include_type: bool) -> Result<Option<Value>, String> {
    let mut root = root.as_object().cloned().unwrap_or_default();
    let mut mcp_servers = object_at(&root, "mcpServers")?;
    if let Some(existing) = mcp_servers.get(SERVER_KEY) {
        if !existing.is_object() {
            return Err(unexpected("\"mcpServers\".\"lumen\""));
        }
    }

    let mut entry = Map::new();
    if include_type {
        entry.insert("type".to_string(), json!("stdio"));
    }
    entry.insert("command".to_string(), json!(command));
    entry.insert("args".to_string(), json!([MCP_ARG]));

    mcp_servers.insert(SERVER_KEY.to_string(), Value::Object(entry));
    root.insert("mcpServers".to_string(), Value::Object(mcp_servers));
    Ok(Some(Value::Object(root)))
}

fn json_uninstall(root: &Value) -> Result<Option<Value>, String> {
    let mut root = root.as_object().cloned().unwrap_or_default();
    if !root.contains_key("mcpServers") {
        return Ok(if root.is_empty() {
            None
        } else {
            Some(Value::Object(root))
        });
    }

    let mut mcp_servers = object_at(&root, "mcpServers")?;
    let removed = mcp_servers.remove(SERVER_KEY).is_some();
    if !removed {
        return Ok(Some(Value::Object(root)));
    }
    if mcp_servers.is_empty() {
        root.remove("mcpServers");
    } else {
        root.insert("mcpServers".to_string(), Value::Object(mcp_servers));
    }

    if root.is_empty() {
        Ok(None)
    } else {
        Ok(Some(Value::Object(root)))
    }
}

// ── Codex config.toml parser & surgical editor ────────────────────────────────

fn invalid_toml(label: &str, reason: &str) -> String {
    crate::i18n::tf(
        "{file} isn't valid TOML ({error}). Fix or move it, then try again — Coucou won't overwrite it.",
        &[("file", label), ("error", reason)],
    )
}

#[derive(Debug)]
struct TomlDoc<'a> {
    bom: &'a str,
    body: &'a str,
    eol: &'a str,
    sections: Vec<TomlSection>,
}

#[derive(Debug)]
struct TomlSection {
    path: Vec<String>,
    header_start: usize,
    header_end: usize,
    entries: Vec<TomlEntry>,
}

#[derive(Debug)]
struct TomlEntry {
    key_path: Vec<String>,
    span_start: usize,
    span_end: usize,
    raw_value: String,
}

impl<'a> TomlDoc<'a> {
    fn lumen_section_index(&self) -> Option<usize> {
        self.sections
            .iter()
            .position(|sec| sec.path.as_slice() == ["mcp_servers", SERVER_KEY])
    }

    fn lumen_section(&self) -> Option<&TomlSection> {
        self.lumen_section_index().map(|idx| &self.sections[idx])
    }
}

fn edit_codex_toml(
    current: Option<&str>,
    command: &str,
    install: bool,
    label: &str,
) -> Result<Option<String>, String> {
    let Some(raw) = current else {
        if !install {
            return Ok(None);
        }
        return Ok(Some(format!(
            "[mcp_servers.{SERVER_KEY}]\ncommand = {}\nargs = [\"{MCP_ARG}\"]\n",
            toml_basic_string(command)
        )));
    };

    let doc = parse_toml_doc(raw, label)?;
    let eol = doc.eol;
    let body = doc.body;

    if install {
        let cmd_line = format!("command = {}{eol}", toml_basic_string(command));
        let args_line = format!("args = [\"{MCP_ARG}\"]{eol}");

        if let Some(sec_idx) = doc.lumen_section_index() {
            let section = &doc.sections[sec_idx];
            let sec_end = doc
                .sections
                .get(sec_idx + 1)
                .map_or(body.len(), |next| next.header_start);

            let cmd_entry = section
                .entries
                .iter()
                .find(|entry| entry.key_path.as_slice() == ["command"]);
            let args_entry = section
                .entries
                .iter()
                .find(|entry| entry.key_path.as_slice() == ["args"]);

            // Preserva comentários e chaves extras dentro de `[mcp_servers.lumen]`.
            let mut replacements: Vec<(usize, usize, String)> = Vec::new();
            if let Some(entry) = cmd_entry {
                replacements.push((entry.span_start, entry.span_end, cmd_line.clone()));
            }
            if let Some(entry) = args_entry {
                replacements.push((entry.span_start, entry.span_end, args_line.clone()));
            }

            let mut missing = String::new();
            if cmd_entry.is_none() {
                missing.push_str(&cmd_line);
            }
            if args_entry.is_none() {
                missing.push_str(&args_line);
            }
            if !missing.is_empty() {
                let insert_at = section
                    .entries
                    .last()
                    .map_or(section.header_end, |entry| entry.span_end);
                let mut need_leading_eol = false;
                if insert_at == body.len() && !body[..insert_at].ends_with('\n') {
                    need_leading_eol = true;
                }
                let mut payload = String::new();
                if need_leading_eol {
                    payload.push_str(eol);
                }
                payload.push_str(&missing);
                replacements.push((insert_at, insert_at, payload));
            }

            replacements.sort_by_key(|(start, end, _)| (*start, *end));
            let mut out_body = String::with_capacity(body.len() + 128);
            let mut cursor = 0usize;
            for (start, end, text) in replacements {
                debug_assert!(start >= cursor && end <= sec_end);
                out_body.push_str(&body[cursor..start]);
                out_body.push_str(&text);
                cursor = end;
            }
            out_body.push_str(&body[cursor..]);
            return Ok(Some(format!("{}{}", doc.bom, out_body)));
        }

        let block = format!("[mcp_servers.{SERVER_KEY}]{eol}{cmd_line}{args_line}");
        if body.trim().is_empty() {
            let trimmed_prefix = body.trim_end_matches([' ', '\t', '\r', '\n']);
            if trimmed_prefix.is_empty() {
                return Ok(Some(format!("{}{}", doc.bom, block)));
            }
        }

        let mut out_body = body.to_string();
        if !out_body.ends_with('\n') {
            out_body.push_str(eol);
        }
        if !ends_with_blank_line(&out_body) {
            out_body.push_str(eol);
        }
        out_body.push_str(&block);
        return Ok(Some(format!("{}{}", doc.bom, out_body)));
    }

    // install == false
    let Some(sec_idx) = doc.lumen_section_index() else {
        return Ok(Some(raw.to_string()));
    };

    let sec_start = doc.sections[sec_idx].header_start;
    let next_start = doc
        .sections
        .get(sec_idx + 1)
        .map_or(body.len(), |next| next.header_start);

    let before = &body[..sec_start];
    let after = &body[next_start..];

    let mut combined = String::with_capacity(body.len());
    if after.trim().is_empty() {
        let trimmed_before = before.trim_end_matches(['\r', '\n']);
        if !trimmed_before.is_empty() {
            combined.push_str(trimmed_before);
            combined.push_str(eol);
        }
    } else if before.trim().is_empty() {
        let trimmed_after = after.trim_start_matches([' ', '\t', '\r', '\n']);
        combined.push_str(trimmed_after);
    } else {
        let trimmed_before = before.trim_end_matches(['\r', '\n']);
        let trimmed_after = after.trim_start_matches(['\r', '\n']);
        combined.push_str(trimmed_before);
        combined.push_str(eol);
        combined.push_str(eol);
        combined.push_str(trimmed_after);
    }

    if combined.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(format!("{}{}", doc.bom, combined)))
    }
}

fn ends_with_blank_line(text: &str) -> bool {
    let without_last = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'));
    let Some(prev) = without_last else {
        return false;
    };
    prev.ends_with('\n') || prev.trim_matches([' ', '\t']).is_empty()
}

fn toml_basic_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || c == '\u{007F}' => {
                use std::fmt::Write as _;
                let _ = write!(&mut out, "\\u{:04X}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn parse_toml_doc<'a>(raw: &'a str, label: &str) -> Result<TomlDoc<'a>, String> {
    let (bom, body) = if let Some(rest) = raw.strip_prefix('\u{FEFF}') {
        ("\u{FEFF}", rest)
    } else {
        ("", raw)
    };
    let eol = if body.contains("\r\n") { "\r\n" } else { "\n" };

    let mut sections: Vec<TomlSection> = Vec::new();
    let mut current_path: Option<Vec<String>> = None;
    let mut seen_lumen = false;

    let bytes = body.as_bytes();
    let mut pos = 0usize;

    while pos < bytes.len() {
        let line_start = pos;
        let line_end = find_line_end(bytes, pos);
        let content_end = strip_line_eol(bytes, line_start, line_end);
        let after_ws = skip_hspace(bytes, line_start, content_end);

        if after_ws >= content_end || bytes[after_ws] == b'#' {
            pos = line_end;
            continue;
        }

        if bytes[after_ws] == b'[' {
            let (is_array_table, path) = parse_table_header(body, after_ws, content_end)
                .map_err(|reason| invalid_toml(label, &reason))?;

            if is_array_table
                && (path.as_slice() == ["mcp_servers"]
                    || path.as_slice() == ["mcp_servers", SERVER_KEY])
            {
                return Err(unexpected(&quoted_path(&path)));
            }

            if !is_array_table && path.as_slice() == ["mcp_servers", SERVER_KEY] {
                if seen_lumen {
                    return Err(unexpected("\"mcp_servers\".\"lumen\""));
                }
                seen_lumen = true;
            }

            current_path = Some(path.clone());
            sections.push(TomlSection {
                path,
                header_start: line_start,
                header_end: line_end,
                entries: Vec::new(),
            });
            pos = line_end;
            continue;
        }

        let (key_path, after_eq) = parse_key_eq(body, after_ws, content_end)
            .map_err(|reason| invalid_toml(label, &reason))?;

        if current_path.is_none()
            && key_path.first().map(String::as_str) == Some("mcp_servers")
        {
            return Err(unexpected("\"mcp_servers\""));
        }
        if current_path
            .as_deref()
            .is_some_and(|sec| sec == ["mcp_servers"])
            && key_path.first().map(String::as_str) == Some(SERVER_KEY)
        {
            return Err(unexpected("\"mcp_servers\".\"lumen\""));
        }

        let scanned = scan_toml_value(body, after_eq)
            .map_err(|reason| invalid_toml(label, &reason))?;

        if let Some(section) = sections.last_mut() {
            section.entries.push(TomlEntry {
                key_path,
                span_start: line_start,
                span_end: scanned.line_end,
                raw_value: scanned.raw_value,
            });
        }

        pos = scanned.line_end;
    }

    Ok(TomlDoc {
        bom,
        body,
        eol,
        sections,
    })
}

fn quoted_path(path: &[String]) -> String {
    path.iter()
        .map(|part| format!("\"{part}\""))
        .collect::<Vec<_>>()
        .join(".")
}

fn find_line_end(bytes: &[u8], start: usize) -> usize {
    let mut i = start;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            return i + 1;
        }
        i += 1;
    }
    i
}

fn strip_line_eol(bytes: &[u8], start: usize, end: usize) -> usize {
    let mut e = end;
    if e > start && bytes[e - 1] == b'\n' {
        e -= 1;
    }
    if e > start && bytes[e - 1] == b'\r' {
        e -= 1;
    }
    e
}

fn skip_hspace(bytes: &[u8], mut i: usize, limit: usize) -> usize {
    while i < limit && matches!(bytes[i], b' ' | b'\t') {
        i += 1;
    }
    i
}

fn validate_trailing_comment_on_line(
    bytes: &[u8],
    pos: usize,
    line_content_end: usize,
) -> Result<(), String> {
    let i = skip_hspace(bytes, pos, line_content_end);
    if i >= line_content_end || bytes[i] == b'#' {
        Ok(())
    } else {
        Err("unexpected trailing characters".to_string())
    }
}

fn parse_table_header(
    src: &str,
    start: usize,
    content_end: usize,
) -> Result<(bool, Vec<String>), String> {
    let bytes = src.as_bytes();
    let is_array = start + 1 < content_end && bytes[start + 1] == b'[';
    let mut i = start + if is_array { 2 } else { 1 };
    let (path, next_i) = parse_dotted_key(src, i, content_end)?;
    i = skip_hspace(bytes, next_i, content_end);

    if is_array {
        if i + 1 >= content_end || bytes[i] != b']' || bytes[i + 1] != b']' {
            return Err("expected closing ']]' in table header".to_string());
        }
        i += 2;
    } else {
        if i >= content_end || bytes[i] != b']' {
            return Err("expected closing ']' in table header".to_string());
        }
        i += 1;
    }

    validate_trailing_comment_on_line(bytes, i, content_end)?;
    Ok((is_array, path))
}

fn parse_key_eq(src: &str, start: usize, content_end: usize) -> Result<(Vec<String>, usize), String> {
    let bytes = src.as_bytes();
    let (path, next_i) = parse_dotted_key(src, start, content_end)?;
    let i = skip_hspace(bytes, next_i, content_end);
    if i >= content_end || bytes[i] != b'=' {
        return Err("expected '=' after key".to_string());
    }
    Ok((path, i + 1))
}

fn parse_dotted_key(
    src: &str,
    mut i: usize,
    limit: usize,
) -> Result<(Vec<String>, usize), String> {
    let bytes = src.as_bytes();
    let mut parts = Vec::new();
    loop {
        i = skip_hspace(bytes, i, limit);
        if i >= limit {
            return Err("unexpected end of key".to_string());
        }
        let (part, next_i) = parse_key_segment(src, i, limit)?;
        parts.push(part);
        i = skip_hspace(bytes, next_i, limit);
        if i < limit && bytes[i] == b'.' {
            i += 1;
            continue;
        }
        break;
    }
    if parts.is_empty() {
        return Err("empty key".to_string());
    }
    Ok((parts, i))
}

fn parse_key_segment(src: &str, i: usize, limit: usize) -> Result<(String, usize), String> {
    let bytes = src.as_bytes();
    match bytes[i] {
        b'"' => {
            if src[i..limit].starts_with("\"\"\"") {
                return Err("multiline basic string cannot be used as key".to_string());
            }
            let end = scan_single_line_basic_string(src, i, limit)?;
            let parsed = unescape_basic_string(&src[i + 1..end - 1])
                .ok_or_else(|| "invalid escape in quoted key".to_string())?;
            Ok((parsed, end))
        }
        b'\'' => {
            if src[i..limit].starts_with("'''") {
                return Err("multiline literal string cannot be used as key".to_string());
            }
            let end = scan_single_line_literal_string(src, i, limit)?;
            Ok((src[i + 1..end - 1].to_string(), end))
        }
        _ => {
            let mut end = i;
            while end < limit && is_bare_key_byte(bytes[end]) {
                end += 1;
            }
            if end == i {
                return Err("expected TOML key".to_string());
            }
            Ok((src[i..end].to_string(), end))
        }
    }
}

fn is_bare_key_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

struct ScannedValue {
    line_end: usize,
    raw_value: String,
}

fn scan_toml_value(src: &str, start: usize) -> Result<ScannedValue, String> {
    let bytes = src.as_bytes();
    let line_end = find_line_end(bytes, start);
    let content_end = strip_line_eol(bytes, start, line_end);
    let i = skip_hspace(bytes, start, content_end);

    if i >= content_end || bytes[i] == b'#' {
        return Err("missing value after '='".to_string());
    }

    let (val_end, end_line_end) = match bytes[i] {
        b'"' if src[i..].starts_with("\"\"\"") => {
            let end = scan_multiline_basic_string(src, i)?;
            let tail_end = finish_value_line(src, end)?;
            (end, tail_end)
        }
        b'"' => {
            let end = scan_single_line_basic_string(src, i, content_end)?;
            validate_trailing_comment_on_line(bytes, end, content_end)?;
            (end, line_end)
        }
        b'\'' if src[i..].starts_with("'''") => {
            let end = scan_multiline_literal_string(src, i)?;
            let tail_end = finish_value_line(src, end)?;
            (end, tail_end)
        }
        b'\'' => {
            let end = scan_single_line_literal_string(src, i, content_end)?;
            validate_trailing_comment_on_line(bytes, end, content_end)?;
            (end, line_end)
        }
        b'[' | b'{' => {
            let end = scan_bracketed_value(src, i)?;
            let tail_end = finish_value_line(src, end)?;
            (end, tail_end)
        }
        _ => {
            let mut end = i;
            while end < content_end
                && !matches!(
                    bytes[end],
                    b' ' | b'\t' | b'#' | b'"' | b'\'' | b'[' | b']' | b'{' | b'}' | b',' | b'='
                )
            {
                end += 1;
            }
            if end == i {
                return Err("invalid TOML value".to_string());
            }
            validate_trailing_comment_on_line(bytes, end, content_end)?;
            (end, line_end)
        }
    };

    Ok(ScannedValue {
        line_end: end_line_end,
        raw_value: src[i..val_end].to_string(),
    })
}

fn finish_value_line(src: &str, pos: usize) -> Result<usize, String> {
    let bytes = src.as_bytes();
    let line_end = find_line_end(bytes, pos);
    let content_end = strip_line_eol(bytes, pos, line_end);
    validate_trailing_comment_on_line(bytes, pos, content_end)?;
    Ok(line_end)
}

fn scan_single_line_basic_string(src: &str, start: usize, limit: usize) -> Result<usize, String> {
    let bytes = src.as_bytes();
    let mut i = start + 1;
    while i < limit {
        if bytes[i] == b'\\' {
            i += 1;
            if i >= limit {
                return Err("unterminated escape in string".to_string());
            }
            let ch = src[i..limit]
                .chars()
                .next()
                .ok_or_else(|| "unterminated escape in string".to_string())?;
            i += ch.len_utf8();
            continue;
        }
        if bytes[i] == b'"' {
            return Ok(i + 1);
        }
        let ch = src[i..limit]
            .chars()
            .next()
            .ok_or_else(|| "unterminated string".to_string())?;
        i += ch.len_utf8();
    }
    Err("unterminated basic string".to_string())
}

fn scan_single_line_literal_string(src: &str, start: usize, limit: usize) -> Result<usize, String> {
    let bytes = src.as_bytes();
    let mut i = start + 1;
    while i < limit {
        if bytes[i] == b'\'' {
            return Ok(i + 1);
        }
        let ch = src[i..limit]
            .chars()
            .next()
            .ok_or_else(|| "unterminated literal string".to_string())?;
        i += ch.len_utf8();
    }
    Err("unterminated literal string".to_string())
}

fn scan_multiline_basic_string(src: &str, start: usize) -> Result<usize, String> {
    let bytes = src.as_bytes();
    let mut i = start + 3;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 1;
            if i >= bytes.len() {
                return Err("unterminated escape in multiline string".to_string());
            }
            let ch = src[i..]
                .chars()
                .next()
                .ok_or_else(|| "unterminated escape in multiline string".to_string())?;
            i += ch.len_utf8();
            continue;
        }
        if bytes[i..].starts_with(b"\"\"\"") {
            let mut count = 3;
            while i + count < bytes.len() && bytes[i + count] == b'"' {
                count += 1;
            }
            if count > 5 {
                return Err("invalid quote sequence in multiline string".to_string());
            }
            return Ok(i + count);
        }
        let ch = src[i..]
            .chars()
            .next()
            .ok_or_else(|| "unterminated multiline basic string".to_string())?;
        i += ch.len_utf8();
    }
    Err("unterminated multiline basic string".to_string())
}

fn scan_multiline_literal_string(src: &str, start: usize) -> Result<usize, String> {
    let bytes = src.as_bytes();
    let mut i = start + 3;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"'''") {
            let mut count = 3;
            while i + count < bytes.len() && bytes[i + count] == b'\'' {
                count += 1;
            }
            if count > 5 {
                return Err("invalid apostrophe sequence in multiline literal string".to_string());
            }
            return Ok(i + count);
        }
        let ch = src[i..]
            .chars()
            .next()
            .ok_or_else(|| "unterminated multiline literal string".to_string())?;
        i += ch.len_utf8();
    }
    Err("unterminated multiline literal string".to_string())
}

fn scan_bracketed_value(src: &str, start: usize) -> Result<usize, String> {
    let bytes = src.as_bytes();
    let mut stack = Vec::new();
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'[' => {
                stack.push(b']');
                i += 1;
            }
            b'{' => {
                stack.push(b'}');
                i += 1;
            }
            b']' | b'}' => {
                let expected = stack
                    .pop()
                    .ok_or_else(|| "unexpected closing delimiter".to_string())?;
                if bytes[i] != expected {
                    return Err("mismatched closing delimiter".to_string());
                }
                i += 1;
                if stack.is_empty() {
                    return Ok(i);
                }
            }
            b'#' => {
                i = find_line_end(bytes, i);
            }
            b'"' if src[i..].starts_with("\"\"\"") => {
                i = scan_multiline_basic_string(src, i)?;
            }
            b'"' => {
                let line_end = find_line_end(bytes, i);
                let content_end = strip_line_eol(bytes, i, line_end);
                i = scan_single_line_basic_string(src, i, content_end)?;
            }
            b'\'' if src[i..].starts_with("'''") => {
                i = scan_multiline_literal_string(src, i)?;
            }
            b'\'' => {
                let line_end = find_line_end(bytes, i);
                let content_end = strip_line_eol(bytes, i, line_end);
                i = scan_single_line_literal_string(src, i, content_end)?;
            }
            _ => i += 1,
        }
    }
    Err("unclosed '[' or '{' in TOML value".to_string())
}

fn parse_toml_string(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if let Some(inner) = trimmed
        .strip_prefix("\"\"\"")
        .and_then(|s| s.strip_suffix("\"\"\""))
    {
        let stripped = inner
            .strip_prefix("\r\n")
            .or_else(|| inner.strip_prefix('\n'))
            .unwrap_or(inner);
        return unescape_basic_string(stripped);
    }
    if let Some(inner) = trimmed
        .strip_prefix("'''")
        .and_then(|s| s.strip_suffix("'''"))
    {
        let stripped = inner
            .strip_prefix("\r\n")
            .or_else(|| inner.strip_prefix('\n'))
            .unwrap_or(inner);
        return Some(stripped.to_string());
    }
    if let Some(inner) = trimmed.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return unescape_basic_string(inner);
    }
    if let Some(inner) = trimmed
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
    {
        return Some(inner.to_string());
    }
    None
}

fn unescape_basic_string(inner: &str) -> Option<String> {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let esc = chars.next()?;
        match esc {
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'b' => out.push('\u{08}'),
            'f' => out.push('\u{0C}'),
            'u' => {
                let code = take_hex(&mut chars, 4)?;
                out.push(char::from_u32(code)?);
            }
            'U' => {
                let code = take_hex(&mut chars, 8)?;
                out.push(char::from_u32(code)?);
            }
            ' ' | '\t' | '\r' | '\n' => {
                // Line-ending backslash trim em multiline basic strings.
                let mut saw_newline = matches!(esc, '\r' | '\n');
                loop {
                    let clone = chars.clone();
                    match chars.next() {
                        Some(' ' | '\t') => {}
                        Some('\r' | '\n') => saw_newline = true,
                        Some(_) => {
                            chars = clone;
                            break;
                        }
                        None => break,
                    }
                }
                if !saw_newline {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

fn take_hex(chars: &mut std::str::Chars<'_>, count: usize) -> Option<u32> {
    let mut buf = String::with_capacity(count);
    for _ in 0..count {
        buf.push(chars.next()?);
    }
    u32::from_str_radix(&buf, 16).ok()
}

fn parse_toml_string_array(raw: &str) -> Option<Vec<String>> {
    let trimmed = raw.trim();
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?;
    let bytes = inner.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;

    loop {
        while i < bytes.len() {
            match bytes[i] {
                b' ' | b'\t' | b'\r' | b'\n' => i += 1,
                b'#' => {
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                }
                _ => break,
            }
        }
        if i >= bytes.len() {
            break;
        }
        let end = if inner[i..].starts_with("\"\"\"") {
            scan_multiline_basic_string(inner, i).ok()?
        } else if bytes[i] == b'"' {
            scan_single_line_basic_string(inner, i, bytes.len()).ok()?
        } else if inner[i..].starts_with("'''") {
            scan_multiline_literal_string(inner, i).ok()?
        } else if bytes[i] == b'\'' {
            scan_single_line_literal_string(inner, i, bytes.len()).ok()?
        } else {
            return None;
        };
        let s = parse_toml_string(&inner[i..end])?;
        out.push(s);
        i = end;

        while i < bytes.len() {
            match bytes[i] {
                b' ' | b'\t' | b'\r' | b'\n' => i += 1,
                b'#' => {
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                }
                _ => break,
            }
        }
        if i >= bytes.len() {
            break;
        }
        if bytes[i] == b',' {
            i += 1;
            continue;
        }
        return None;
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TestHome {
        path: PathBuf,
    }

    impl TestHome {
        fn new(prefix: &str) -> Self {
            static SEQ: AtomicUsize = AtomicUsize::new(0);
            let seq = SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "lumen-mcp-config-{prefix}-{}-{seq}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self { path: dir }
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn find_backups(dir: &Path, file_prefix: &str) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(&format!("{file_prefix}.bak-")) {
                out.push(entry.path());
            }
        }
        out.sort();
        out
    }

    #[test]
    fn json_clis_preserve_other_servers_write_backups_and_restore_cleanly() {
        let home = TestHome::new("json");
        let hook_exe = PathBuf::from(r"C:\Users\tester\AppData\Local\Coucou\bin\coucou-hook.exe");

        // Claude Code: ~/.claude.json com servidor pré-existente e outra chave raiz.
        let claude_path = home.path.join(".claude.json");
        let original_claude = serde_json::to_string_pretty(&json!({
            "numStartups": 4,
            "mcpServers": {
                "github": {
                    "type": "stdio",
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-github"]
                }
            }
        }))
        .unwrap()
            + "\n";
        std::fs::write(&claude_path, original_claude.as_bytes()).unwrap();

        let initial_status = status_at(&home.path, &hook_exe);
        let claude_status = initial_status.iter().find(|s| s.id == "claude").unwrap();
        assert!(!claude_status.registered);
        assert!(!claude_status.command_matches);

        let plan = preview_at("claude", true, &home.path, &hook_exe).unwrap();
        assert!(plan.diff.contains("lumen"));
        assert!(!plan.backup.is_empty());

        // Fingerprint obsoleto deve falhar antes de tocar no arquivo.
        assert!(apply_at("claude", true, "deadbeef00000000", &home.path, &hook_exe).is_err());

        let backups_str =
            apply_at("claude", true, &plan.fingerprint, &home.path, &hook_exe).unwrap();
        assert!(!backups_str.is_empty());

        let backups = find_backups(&home.path, ".claude.json");
        assert_eq!(backups.len(), 1);
        assert_eq!(std::fs::read(&backups[0]).unwrap(), original_claude.as_bytes());

        let after_install: Value =
            serde_json::from_slice(&std::fs::read(&claude_path).unwrap()).unwrap();
        assert_eq!(after_install["numStartups"], 4);
        assert_eq!(after_install["mcpServers"]["github"]["command"], "npx");
        assert_eq!(after_install["mcpServers"]["lumen"]["type"], "stdio");
        assert_eq!(
            after_install["mcpServers"]["lumen"]["command"],
            hook_exe.to_string_lossy().as_ref()
        );
        assert_eq!(after_install["mcpServers"]["lumen"]["args"], json!(["--mcp"]));

        let after_status = status_at(&home.path, &hook_exe);
        let claude_after = after_status.iter().find(|s| s.id == "claude").unwrap();
        assert!(claude_after.registered);
        assert!(claude_after.command_matches);

        // Quando muda o caminho do executável, continua registrado porém `commandMatches` vira false.
        let moved_status = status_at(&home.path, Path::new(r"C:\other\coucou-hook.exe"));
        let moved = moved_status.iter().find(|s| s.id == "claude").unwrap();
        assert!(moved.registered);
        assert!(!moved.command_matches);

        // Remoção tira apenas `lumen` e mantém `github` + backup datado.
        let remove_plan = preview_at("claude", false, &home.path, &hook_exe).unwrap();
        apply_at("claude", false, &remove_plan.fingerprint, &home.path, &hook_exe).unwrap();
        let after_remove: Value =
            serde_json::from_slice(&std::fs::read(&claude_path).unwrap()).unwrap();
        assert_eq!(after_remove["numStartups"], 4);
        assert_eq!(after_remove["mcpServers"]["github"]["command"], "npx");
        assert!(after_remove["mcpServers"].get("lumen").is_none());
        assert_eq!(find_backups(&home.path, ".claude.json").len(), 2);

        // Gemini CLI + Antigravity criados do zero e removidos limpos após backup.
        for (agent_id, rel) in [
            ("gemini", PathBuf::from(".gemini").join("settings.json")),
            (
                "antigravity",
                PathBuf::from(".gemini")
                    .join("antigravity")
                    .join("mcp_config.json"),
            ),
        ] {
            let p = preview_at(agent_id, true, &home.path, &hook_exe).unwrap();
            apply_at(agent_id, true, &p.fingerprint, &home.path, &hook_exe).unwrap();
            let target = home.path.join(&rel);
            let parsed: Value = serde_json::from_slice(&std::fs::read(&target).unwrap()).unwrap();
            assert!(parsed["mcpServers"]["lumen"].get("type").is_none());
            assert_eq!(
                parsed["mcpServers"]["lumen"]["command"],
                hook_exe.to_string_lossy().as_ref()
            );
            assert_eq!(parsed["mcpServers"]["lumen"]["args"], json!(["--mcp"]));

            let rm_p = preview_at(agent_id, false, &home.path, &hook_exe).unwrap();
            apply_at(agent_id, false, &rm_p.fingerprint, &home.path, &hook_exe).unwrap();
            assert!(!target.exists());
            let dir = target.parent().unwrap();
            let file_name = target.file_name().unwrap().to_str().unwrap();
            assert_eq!(find_backups(dir, file_name).len(), 1);
        }
    }

    #[test]
    fn codex_toml_preserves_comments_other_tables_and_escapes_windows_paths() {
        let home = TestHome::new("codex");
        let codex_dir = home.path.join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        let codex_path = codex_dir.join("config.toml");

        let original = concat!(
            "# Configuração pessoal do Codex\n",
            "model = \"gpt-5\"\n",
            "\n",
            "[features]\n",
            "shell_snapshot = true\n",
            "\n",
            "[mcp_servers.other]\n",
            "command = \"npx\"\n",
            "args = [\n",
            "  \"-y\",\n",
            "  \"[not_a_table]\"\n",
            "]\n"
        );
        std::fs::write(&codex_path, original.as_bytes()).unwrap();

        let hook_exe = PathBuf::from(r"C:\Users\alice\AppData\Local\Coucou\bin\coucou-hook.exe");
        let plan = preview_at("codex", true, &home.path, &hook_exe).unwrap();
        assert!(plan.diff.contains("[mcp_servers.lumen]"));
        apply_at("codex", true, &plan.fingerprint, &home.path, &hook_exe).unwrap();

        let backups = find_backups(&codex_dir, "config.toml");
        assert_eq!(backups.len(), 1);
        assert_eq!(std::fs::read(&backups[0]).unwrap(), original.as_bytes());

        let after_install = std::fs::read_to_string(&codex_path).unwrap();
        assert!(after_install.contains("# Configuração pessoal do Codex"));
        assert!(after_install.contains("[features]\nshell_snapshot = true"));
        assert!(after_install.contains("[mcp_servers.other]"));
        assert!(after_install.contains(
            r#"command = "C:\\Users\\alice\\AppData\\Local\\Coucou\\bin\\coucou-hook.exe""#
        ));
        assert!(after_install.contains("args = [\"--mcp\"]"));

        let st = status_at(&home.path, &hook_exe);
        let codex_st = st.iter().find(|s| s.id == "codex").unwrap();
        assert!(codex_st.registered);
        assert!(codex_st.command_matches);

        let moved_st = status_at(&home.path, Path::new(r"C:\moved\coucou-hook.exe"));
        let moved_codex = moved_st.iter().find(|s| s.id == "codex").unwrap();
        assert!(moved_codex.registered);
        assert!(!moved_codex.command_matches);

        // Atualização preserva chave extra dentro de `[mcp_servers.lumen]`.
        let with_extra = format!("{after_install}startup_timeout_sec = 20\n");
        std::fs::write(&codex_path, with_extra.as_bytes()).unwrap();
        let new_hook = PathBuf::from(r"C:\Program Files\Lumen\coucou-hook.exe");
        let upd_plan = preview_at("codex", true, &home.path, &new_hook).unwrap();
        apply_at("codex", true, &upd_plan.fingerprint, &home.path, &new_hook).unwrap();
        let after_update = std::fs::read_to_string(&codex_path).unwrap();
        assert!(after_update.contains("startup_timeout_sec = 20"));
        assert!(after_update.contains(r#"command = "C:\\Program Files\\Lumen\\coucou-hook.exe""#));

        // Remoção restaura o conteúdo original sem `[mcp_servers.lumen]`.
        let rm_plan = preview_at("codex", false, &home.path, &new_hook).unwrap();
        apply_at("codex", false, &rm_plan.fingerprint, &home.path, &new_hook).unwrap();
        let after_remove = std::fs::read_to_string(&codex_path).unwrap();
        assert_eq!(after_remove, original);
    }

    #[test]
    fn refuses_corrupted_json_and_incompatible_toml_without_overwriting() {
        let home = TestHome::new("invalid");
        let hook_exe = PathBuf::from(r"C:\Coucou\coucou-hook.exe");

        // JSON inválido e tipo incompatível de `mcpServers`.
        std::fs::write(home.path.join(".claude.json"), b"{not valid json").unwrap();
        assert!(preview_at("claude", true, &home.path, &hook_exe).is_err());

        std::fs::create_dir_all(home.path.join(".gemini")).unwrap();
        std::fs::write(
            home.path.join(".gemini").join("settings.json"),
            br#"{"mcpServers": ["bad"]}"#,
        )
        .unwrap();
        assert!(preview_at("gemini", true, &home.path, &hook_exe).is_err());

        // Codex com `mcp_servers` inline incompatível ou TOML corrompido.
        let codex_dir = home.path.join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        let codex_path = codex_dir.join("config.toml");

        std::fs::write(&codex_path, b"mcp_servers = { lumen = { command = \"x\" } }\n").unwrap();
        assert!(preview_at("codex", true, &home.path, &hook_exe).is_err());

        std::fs::write(&codex_path, b"[mcp_servers]\nlumen = { command = \"x\" }\n").unwrap();
        assert!(preview_at("codex", true, &home.path, &hook_exe).is_err());

        std::fs::write(&codex_path, b"[[mcp_servers.lumen]]\ncommand = \"x\"\n").unwrap();
        assert!(preview_at("codex", true, &home.path, &hook_exe).is_err());

        std::fs::write(&codex_path, b"[broken\nargs = [1, 2\n").unwrap();
        assert!(preview_at("codex", true, &home.path, &hook_exe).is_err());
    }

    #[test]
    fn codex_toml_handles_utf8_accents_and_emojis_in_multiline_strings() {
        let home = TestHome::new("utf8-multiline");
        let codex_dir = home.path.join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        let codex_path = codex_dir.join("config.toml");

        let original = concat!(
            "developer_instructions = \"\"\"\n",
            "Atenção: configuração em português com ç, ã, é, ô 🚀🔒\n",
            "\"\"\"\n\n",
            "[mcp_servers.outro]\n",
            "nota = '''\n",
            "Texto literal com acentuação: coração, ação, café ✨\n",
            "'''\n\n",
            "[mcp_servers.lumen]\n",
            "command = \"C:\\\\antigo\\\\coucou-hook.exe\"\n",
            "args = [\"--mcp\"]\n\n",
            "[perfil]\n",
            "resumo = \"\"\"Mais emojis 🎉 e acentos áéíóú\"\"\"\n",
            "literal = '''Ainda mais: 🧭 ñ ü'''\n",
        );
        std::fs::write(&codex_path, original.as_bytes()).unwrap();

        let hook_exe = PathBuf::from(r"C:\Lumen\coucou-hook.exe");
        let plan = preview_at("codex", true, &home.path, &hook_exe).unwrap();
        apply_at("codex", true, &plan.fingerprint, &home.path, &hook_exe).unwrap();

        let updated = std::fs::read_to_string(&codex_path).unwrap();
        assert!(updated.contains("Atenção: configuração em português com ç, ã, é, ô 🚀🔒"));
        assert!(updated.contains("Texto literal com acentuação: coração, ação, café ✨"));
        assert!(updated.contains("Mais emojis 🎉 e acentos áéíóú"));
        assert!(updated.contains("Ainda mais: 🧭 ñ ü"));

        let st = status_at(&home.path, &hook_exe);
        let codex = st.iter().find(|s| s.id == "codex").unwrap();
        assert!(codex.registered);
        assert!(codex.command_matches);

        let rm_plan = preview_at("codex", false, &home.path, &hook_exe).unwrap();
        apply_at("codex", false, &rm_plan.fingerprint, &home.path, &hook_exe).unwrap();
        let after_remove = std::fs::read_to_string(&codex_path).unwrap();
        assert!(after_remove.contains("Atenção: configuração em português com ç, ã, é, ô 🚀🔒"));
        assert!(after_remove.contains("Ainda mais: 🧭 ñ ü"));
        assert!(!after_remove.contains("[mcp_servers.lumen]"));
    }
}

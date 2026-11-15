// Personas locais (formato 3 partes inspiradas no QwenPaw + compatível com Segunda Mente)
// e regras de seleção/comentários em notas e ideias do Anotador.

use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::default_personas::{default_color_for_id, default_personas, CURRENT_SEED_VERSION};

pub const MAX_COMMENT_CHARACTERS_LIMIT: usize = 15;
pub const DEFAULT_MAX_COMMENT_CHARACTERS: usize = 15;
pub const MAX_PERSONA_TURNS_PER_USER_MESSAGE: usize = 15;
pub const MAX_CROSS_TALK_REPLIES_PER_WAVE: usize = 5;
const SEED_VERSION_FILE: &str = ".characters_seed_version";
const DEFAULT_COLORS: &[&str] = &["#6C5CE7", "#00B894", "#E17055", "#0984E3", "#E84393", "#FDCB6E"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PersonaProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub role: Option<String>,
    /// Identidade, tom e voz da persona (QwenPaw SOUL).
    #[serde(default)]
    pub identity: String,
    /// Fatos sobre quem escreve as notas (QwenPaw PROFILE).
    #[serde(default)]
    pub user_profile: String,
    /// Limites, foco e regras de comentário (QwenPaw AGENTS).
    #[serde(default)]
    pub rules: String,
    /// Texto livre legado (`CharacterModel.persona`) quando não veio dividido nas 3 seções.
    #[serde(default)]
    pub persona: String,
    #[serde(default)]
    pub system_prompt_override: Option<String>,
    #[serde(default)]
    pub post_history_instructions: Option<String>,
    #[serde(default)]
    pub mes_example: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub interest_filter: Option<String>,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub first_message: Option<String>,
    #[serde(default)]
    pub is_primary_companion: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_color")]
    pub color: String,
}

fn default_true() -> bool {
    true
}

fn default_color() -> String {
    DEFAULT_COLORS[0].to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommentSettings {
    #[serde(default = "default_true")]
    pub enable_character_comment: bool,
    #[serde(default = "default_max_chars")]
    pub max_comment_characters: usize,
}

fn default_max_chars() -> usize {
    DEFAULT_MAX_COMMENT_CHARACTERS
}

impl Default for CommentSettings {
    fn default() -> Self {
        Self {
            enable_character_comment: true,
            max_comment_characters: DEFAULT_MAX_COMMENT_CHARACTERS,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NoteComment {
    pub id: String,
    pub character_id: String,
    pub character_name: String,
    #[serde(default = "default_author_kind")]
    pub author_kind: String,
    pub content: String,
    #[serde(default)]
    pub reply_to_id: Option<String>,
    pub created_at: u64,
}

fn default_author_kind() -> String {
    "persona".to_string()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonasState {
    pub personas: Vec<PersonaProfile>,
    pub settings: CommentSettings,
}

pub fn store_dir() -> PathBuf {
    crate::settings::local_dir().join("Personas")
}

pub fn load_state() -> PersonasState {
    load_state_in(&store_dir())
}

pub fn load_state_in(dir: &Path) -> PersonasState {
    PersonasState {
        personas: load_personas_in(dir),
        settings: load_comment_settings_in(dir),
    }
}

fn read_seed_version(dir: &Path) -> Option<u32> {
    let raw = std::fs::read_to_string(dir.join(SEED_VERSION_FILE)).ok()?;
    raw.trim().parse::<u32>().ok()
}

fn write_seed_version(dir: &Path, version: u32) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(SEED_VERSION_FILE), version.to_string()).map_err(|e| e.to_string())
}

fn write_personas_file(dir: &Path, personas: &[PersonaProfile]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(personas).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("personas.json"), json).map_err(|e| e.to_string())
}

fn normalize_legacy_id_match(existing_id: &str, default_id: &str) -> bool {
    existing_id.eq_ignore_ascii_case(default_id)
        || existing_id.replace('-', "_").eq_ignore_ascii_case(default_id)
}

fn merge_defaults_non_destructive(existing: Vec<PersonaProfile>) -> Vec<PersonaProfile> {
    let defaults = default_personas();
    let existing_has_primary = existing.iter().any(|p| p.is_primary_companion);
    let mut matched = vec![false; existing.len()];
    let mut out = Vec::with_capacity(defaults.len() + existing.len());

    for mut def in defaults {
        if let Some((idx, old)) = existing
            .iter()
            .enumerate()
            .find(|(idx, p)| !matched[*idx] && normalize_legacy_id_match(&p.id, &def.id))
        {
            matched[idx] = true;
            def.enabled = old.enabled;
            def.is_primary_companion = old.is_primary_companion;
            if !old.user_profile.trim().is_empty() {
                def.user_profile = old.user_profile.clone();
            }
            if old.system_prompt_override.is_some() {
                def.system_prompt_override = old.system_prompt_override.clone();
            }
            if old.role.is_some() {
                def.role = old.role.clone();
            }
            // Se existirem duplicatas legadas hifenizadas (ex: socio-tecnico), marca todas como migradas.
            for (j, extra) in existing.iter().enumerate() {
                if !matched[j] && normalize_legacy_id_match(&extra.id, &def.id) {
                    matched[j] = true;
                }
            }
        } else if existing_has_primary {
            def.is_primary_companion = false;
        }
        out.push(def);
    }

    for (idx, custom) in existing.into_iter().enumerate() {
        if !matched[idx] {
            out.push(custom);
        }
    }

    ensure_single_primary(&mut out);
    out
}

fn ensure_single_primary(personas: &mut [PersonaProfile]) {
    let mut seen_primary = false;
    for p in personas.iter_mut() {
        if p.is_primary_companion {
            if seen_primary {
                p.is_primary_companion = false;
            } else {
                seen_primary = true;
            }
        }
    }
    if !seen_primary && !personas.is_empty() {
        if let Some(socio) = personas.iter_mut().find(|p| p.id == "socio_tecnico") {
            socio.is_primary_companion = true;
        } else if let Some(first_enabled) = personas.iter_mut().find(|p| p.enabled) {
            first_enabled.is_primary_companion = true;
        } else {
            personas[0].is_primary_companion = true;
        }
    }
}

pub fn ensure_default_personas_seeded(dir: &Path) -> Vec<PersonaProfile> {
    let personas_path = dir.join("personas.json");
    let seed_version = read_seed_version(dir);

    let raw_opt = std::fs::read(&personas_path).ok();
    let is_missing_or_blank = raw_opt
        .as_ref()
        .is_none_or(|b| std::str::from_utf8(b).map(|s| s.trim().is_empty()).unwrap_or(false));

    if is_missing_or_blank {
        let seeded = default_personas();
        let _ = write_personas_file(dir, &seeded);
        let _ = write_seed_version(dir, CURRENT_SEED_VERSION);
        return seeded;
    }

    let existing: Vec<PersonaProfile> = raw_opt
        .as_deref()
        .and_then(|b| serde_json::from_slice(b).ok())
        .unwrap_or_default();

    if existing.is_empty() && seed_version.is_none_or(|v| v < CURRENT_SEED_VERSION) {
        let seeded = default_personas();
        let _ = write_personas_file(dir, &seeded);
        let _ = write_seed_version(dir, CURRENT_SEED_VERSION);
        return seeded;
    }

    if seed_version.is_none_or(|v| v < CURRENT_SEED_VERSION) {
        let merged = merge_defaults_non_destructive(existing);
        let _ = write_personas_file(dir, &merged);
        let _ = write_seed_version(dir, CURRENT_SEED_VERSION);
        return merged;
    }

    existing
}

pub fn agent_dir_in(dir: &Path, id: &str) -> PathBuf {
    dir.join("agents").join(slugify_id(id))
}

pub fn render_persona_agents_md(persona: &PersonaProfile) -> String {
    let mut out = format!("# {} (@{})\n", persona.name.trim(), persona.id.trim());
    if let Some(role) = persona.role.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str(&format!("\n- Papel: {role}"));
    } else {
        out.push_str("\n- Papel: Subagente vivo de perspectiva no chat em grupo");
    }
    if !persona.tags.is_empty() {
        out.push_str(&format!("\n- Tags: {}", persona.tags.join(", ")));
    }
    if let Some(focus) = persona.interest_filter.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str(&format!("\n- Foco: {focus}"));
    }
    out.push('\n');

    if !persona.identity.trim().is_empty() {
        out.push_str("\n## Identidade e voz (SOUL)\n");
        out.push_str(persona.identity.trim());
        out.push('\n');
    }
    if !persona.user_profile.trim().is_empty() {
        out.push_str("\n## Sobre quem anota (PROFILE)\n");
        out.push_str(persona.user_profile.trim());
        out.push('\n');
    }
    if !persona.rules.trim().is_empty() {
        out.push_str("\n## Regras e política de atuação (AGENTS)\n");
        out.push_str(persona.rules.trim());
        out.push('\n');
    }
    let persona_text = persona.persona.trim();
    let identity_first_line = persona.identity.trim().lines().next().unwrap_or("").trim();
    if !persona_text.is_empty() && (identity_first_line.is_empty() || !persona_text.contains(identity_first_line)) {
        out.push_str("\n## Definição adicional\n");
        out.push_str(persona_text);
        out.push('\n');
    }
    if let Some(example) = persona.mes_example.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str("\n## Exemplos de diálogo\n");
        out.push_str(example);
        out.push('\n');
    }
    if let Some(post) = persona.post_history_instructions.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str("\n## Instruções de turno\n");
        out.push_str(post);
        out.push('\n');
    }
    out
}

pub fn ensure_persona_agent_files_in(dir: &Path, personas: &[PersonaProfile]) {
    for p in personas {
        let p_dir = agent_dir_in(dir, &p.id);
        let agents_md = p_dir.join("AGENTS.md");
        if agents_md.exists() {
            continue;
        }
        if std::fs::create_dir_all(&p_dir).is_ok() {
            let _ = std::fs::write(&agents_md, render_persona_agents_md(p));
        }
    }
}

pub fn sync_persona_agent_files_overwrite_in(dir: &Path, personas: &[PersonaProfile]) {
    for p in personas {
        let p_dir = agent_dir_in(dir, &p.id);
        if std::fs::create_dir_all(&p_dir).is_ok() {
            let _ = std::fs::write(p_dir.join("AGENTS.md"), render_persona_agents_md(p));
        }
    }
}

pub fn load_persona_living_docs_in(dir: &Path, persona: &PersonaProfile) -> (String, Option<String>) {
    let p_dir = agent_dir_in(dir, &persona.id);
    let agents_path = p_dir.join("AGENTS.md");
    if !agents_path.exists() && std::fs::create_dir_all(&p_dir).is_ok() {
        let _ = std::fs::write(&agents_path, render_persona_agents_md(persona));
    }
    let agents_md = std::fs::read_to_string(&agents_path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| render_persona_agents_md(persona));

    let memory = std::fs::read_to_string(p_dir.join("MEMORY.md"))
        .ok()
        .map(|raw| {
            let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
            let tail_start = lines.len().saturating_sub(20);
            lines[tail_start..].join("\n")
        })
        .filter(|s| !s.trim().is_empty());

    (agents_md, memory)
}

pub fn append_persona_memory_in(dir: &Path, persona_id: &str, note: &str) -> Result<(), String> {
    let cleaned = note.trim();
    if cleaned.is_empty() {
        return Ok(());
    }
    let p_dir = agent_dir_in(dir, persona_id);
    std::fs::create_dir_all(&p_dir).map_err(|e| e.to_string())?;
    let mem_path = p_dir.join("MEMORY.md");
    let existing = std::fs::read_to_string(&mem_path).unwrap_or_default();
    let mut entries: Vec<String> = existing
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect();
    let entry = if cleaned.starts_with("- ") {
        cleaned.to_string()
    } else {
        format!("- {cleaned}")
    };
    let folded_entry = fold_pt_ascii(&entry);
    if entries.iter().any(|e| fold_pt_ascii(e) == folded_entry) {
        return Ok(());
    }
    entries.push(entry);
    if entries.len() > 30 {
        let drop_n = entries.len() - 30;
        entries.drain(0..drop_n);
    }
    let body = format!("# Memória viva de @{persona_id}\n\n{}\n", entries.join("\n"));
    std::fs::write(&mem_path, body).map_err(|e| e.to_string())
}

pub fn reset_default_personas_in(dir: &Path) -> Result<PersonasState, String> {
    let existing: Vec<PersonaProfile> = std::fs::read(dir.join("personas.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();

    let defaults = default_personas();
    let custom: Vec<PersonaProfile> = existing
        .into_iter()
        .filter(|p| !defaults.iter().any(|d| normalize_legacy_id_match(&p.id, &d.id)))
        .collect();
    let custom_has_primary = custom.iter().any(|p| p.is_primary_companion);

    let mut out = defaults;
    if custom_has_primary {
        for p in &mut out {
            p.is_primary_companion = false;
        }
    }
    out.extend(custom);
    ensure_single_primary(&mut out);

    write_personas_file(dir, &out)?;
    sync_persona_agent_files_overwrite_in(dir, &out);
    write_seed_version(dir, CURRENT_SEED_VERSION)?;
    Ok(PersonasState {
        personas: out,
        settings: load_comment_settings_in(dir),
    })
}

pub fn load_personas() -> Vec<PersonaProfile> {
    load_personas_in(&store_dir())
}

pub fn load_personas_in(dir: &Path) -> Vec<PersonaProfile> {
    let personas = ensure_default_personas_seeded(dir);
    ensure_persona_agent_files_in(dir, &personas);
    personas
}

pub fn save_personas(personas: &[PersonaProfile]) -> Result<(), String> {
    save_personas_in(&store_dir(), personas)
}

pub fn save_personas_in(dir: &Path, personas: &[PersonaProfile]) -> Result<(), String> {
    write_personas_file(dir, personas)?;
    sync_persona_agent_files_overwrite_in(dir, personas);
    if !dir.join(SEED_VERSION_FILE).exists() {
        write_seed_version(dir, CURRENT_SEED_VERSION)?;
    }
    Ok(())
}

pub fn load_comment_settings_in(dir: &Path) -> CommentSettings {
    let Ok(bytes) = std::fs::read(dir.join("comment_settings.json")) else {
        return CommentSettings::default();
    };
    let mut cfg: CommentSettings = serde_json::from_slice(&bytes).unwrap_or_default();
    cfg.max_comment_characters = cfg.max_comment_characters.clamp(1, MAX_COMMENT_CHARACTERS_LIMIT);
    cfg
}

pub fn save_comment_settings(mut settings: CommentSettings) -> Result<CommentSettings, String> {
    settings.max_comment_characters = settings.max_comment_characters.clamp(1, MAX_COMMENT_CHARACTERS_LIMIT);
    save_comment_settings_in(&store_dir(), &settings)?;
    Ok(settings)
}

pub fn save_comment_settings_in(dir: &Path, settings: &CommentSettings) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("comment_settings.json"), json).map_err(|e| e.to_string())
}

/// Mescla personas importadas por `id` (atualizando campos quando já existir o mesmo `id`).
pub fn merge_imported(existing: Vec<PersonaProfile>, imported: Vec<PersonaProfile>) -> Vec<PersonaProfile> {
    let mut out = existing;
    let imported_has_primary = imported.iter().any(|p| p.is_primary_companion);
    if imported_has_primary {
        for p in &mut out {
            p.is_primary_companion = false;
        }
    }
    for (idx, mut item) in imported.into_iter().enumerate() {
        if item.color.trim().is_empty() {
            item.color = default_color_for_id(&item.id)
                .unwrap_or(DEFAULT_COLORS[idx % DEFAULT_COLORS.len()])
                .to_string();
        }
        if let Some(pos) = out.iter().position(|p| p.id.eq_ignore_ascii_case(&item.id)) {
            out[pos] = item;
        } else {
            out.push(item);
        }
    }
    out
}

pub fn parse_import_json(raw: &str) -> Result<Vec<PersonaProfile>, String> {
    let trimmed = raw.trim().trim_start_matches('\u{feff}');
    if trimmed.is_empty() {
        return Err("Cole o JSON ou YAML da persona antes de importar.".into());
    }
    let val: Value = if trimmed.starts_with('{') || trimmed.starts_with('[') {
        serde_json::from_str(trimmed).map_err(|e| format!("JSON de persona inválido: {e}"))?
    } else {
        yaml_to_json_value(trimmed)?
    };
    parse_personas_value(&val)
}

fn parse_personas_value(val: &Value) -> Result<Vec<PersonaProfile>, String> {
    let items: Vec<&Value> = match val {
        Value::Array(arr) => arr.iter().collect(),
        Value::Object(map) => {
            if let Some(Value::Array(arr)) = map.get("personas").or_else(|| map.get("characters")) {
                arr.iter().collect()
            } else {
                vec![val]
            }
        }
        _ => return Err("O conteúdo deve ser um objeto de persona ou uma lista de personas.".into()),
    };
    if items.is_empty() {
        return Err("Nenhuma persona encontrada para importar.".into());
    }
    let mut parsed = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        parsed.push(parse_one_persona(item, i)?);
    }
    Ok(parsed)
}

/// Faz parse de um arquivo `.yaml` da Segunda Mente usando o nome do arquivo como `id` padrão.
pub fn parse_yaml_character_with_id(raw: &str, filename_id: &str) -> Result<PersonaProfile, String> {
    let mut val = yaml_to_json_value(raw)?;
    if let Value::Array(mut arr) = val {
        val = arr
            .drain(..)
            .next()
            .ok_or_else(|| "YAML de persona vazio.".to_string())?;
    }
    let id_slug = slugify_id(filename_id);
    if !id_slug.is_empty() {
        if let Value::Object(ref mut map) = val {
            map.entry("id").or_insert_with(|| Value::String(id_slug));
        }
    }
    let items = parse_personas_value(&val)?;
    items.into_iter().next().ok_or_else(|| "Nenhuma persona encontrada no YAML.".into())
}

// ── Parser YAML puro em Rust ──────────────────────────────────────────────────

pub fn yaml_to_json_value(raw: &str) -> Result<Value, String> {
    let normalized = raw.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let docs = split_yaml_documents(&normalized);
    if docs.is_empty() {
        return Err("YAML de persona vazio.".into());
    }
    let mut parsed_docs = Vec::with_capacity(docs.len());
    for doc in docs {
        let lines: Vec<&str> = doc.lines().collect();
        let mut parser = YamlLineParser { lines: &lines, pos: 0 };
        parser.skip_ignored();
        if parser.pos >= parser.lines.len() {
            continue;
        }
        let val = parser.parse_block(parser.current_indent())?;
        parser.skip_ignored();
        if parser.pos < parser.lines.len() {
            return Err(format!("Conteúdo YAML inesperado na linha {}", parser.pos + 1));
        }
        parsed_docs.push(val);
    }
    match parsed_docs.len() {
        0 => Err("YAML de persona vazio.".into()),
        1 => Ok(parsed_docs.into_iter().next().unwrap()),
        _ => Ok(Value::Array(parsed_docs)),
    }
}

fn split_yaml_documents(input: &str) -> Vec<String> {
    let mut docs = Vec::new();
    let mut current = Vec::new();
    let mut stopped = false;

    for line in input.lines() {
        let has_zero_indent = !line.starts_with(' ') && !line.starts_with('\t');
        let trimmed = line.trim();
        if has_zero_indent && (trimmed == "---" || trimmed.starts_with("--- ")) {
            if current.iter().any(|l: &&str| !strip_yaml_comment(l).trim().is_empty()) {
                docs.push(current.join("\n"));
            }
            current.clear();
            stopped = false;
            continue;
        }
        if has_zero_indent && (trimmed == "..." || trimmed.starts_with("... ")) {
            if current.iter().any(|l: &&str| !strip_yaml_comment(l).trim().is_empty()) {
                docs.push(current.join("\n"));
            }
            current.clear();
            stopped = true;
            continue;
        }
        if !stopped {
            current.push(line);
        }
    }
    if current.iter().any(|l| !strip_yaml_comment(l).trim().is_empty()) {
        docs.push(current.join("\n"));
    }
    docs
}

fn is_quote_opener(bytes: &[u8], idx: usize) -> bool {
    idx == 0 || matches!(bytes[idx - 1], b' ' | b'\t' | b':' | b'[' | b',')
}

fn strip_yaml_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    for (idx, &b) in bytes.iter().enumerate() {
        if in_double {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_double = false;
            }
            continue;
        }
        if in_single {
            if b == b'\'' {
                if bytes.get(idx + 1) == Some(&b'\'') {
                    continue;
                }
                if idx > 0 && bytes[idx - 1] == b'\'' {
                    continue;
                }
                in_single = false;
            }
            continue;
        }
        match b {
            b'"' if is_quote_opener(bytes, idx) => in_double = true,
            b'\'' if is_quote_opener(bytes, idx) => in_single = true,
            b'#' if idx == 0 || bytes[idx - 1] == b' ' || bytes[idx - 1] == b'\t' => {
                return line[..idx].trim_end();
            }
            _ => {}
        }
    }
    line.trim_end()
}

fn line_indent(line: &str) -> usize {
    line.bytes().take_while(|&b| b == b' ').count()
}

fn split_yaml_kv(clean: &str) -> Option<(&str, &str)> {
    let bytes = clean.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    for (idx, &b) in bytes.iter().enumerate() {
        if in_double {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_double = false;
            }
            continue;
        }
        if in_single {
            if b == b'\'' {
                in_single = false;
            }
            continue;
        }
        match b {
            b'"' if is_quote_opener(bytes, idx) => in_double = true,
            b'\'' if is_quote_opener(bytes, idx) => in_single = true,
            b':' => {
                let next = bytes.get(idx + 1).copied();
                if next.is_none() || next == Some(b' ') || next == Some(b'\t') {
                    let raw_key = clean[..idx].trim();
                    if raw_key.is_empty() {
                        return None;
                    }
                    let rest = clean[idx + 1..].trim_start();
                    return Some((raw_key, rest));
                }
            }
            _ => {}
        }
    }
    None
}

fn unquote_yaml_key(raw_key: &str) -> Result<String, String> {
    let trimmed = raw_key.trim();
    if trimmed.starts_with('"') {
        parse_double_quoted_yaml(trimmed)
    } else if trimmed.starts_with('\'') {
        parse_single_quoted_yaml(trimmed)
    } else {
        Ok(trimmed.to_string())
    }
}

fn parse_double_quoted_yaml(raw: &str) -> Result<String, String> {
    let mut chars = raw.chars();
    if chars.next() != Some('"') {
        return Err("String entre aspas duplas inválida no YAML.".into());
    }
    let mut out = String::new();
    let mut closed = false;
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                closed = true;
                break;
            }
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('/') => out.push('/'),
                Some('0') => out.push('\0'),
                Some(' ') => out.push(' '),
                Some('u') => {
                    let mut hex = String::with_capacity(4);
                    for _ in 0..4 {
                        hex.push(chars.next().ok_or("Escape \\u inválido no YAML.")?);
                    }
                    let code = u32::from_str_radix(&hex, 16)
                        .map_err(|_| "Escape \\u inválido no YAML.".to_string())?;
                    let decoded = char::from_u32(code).ok_or("Código Unicode inválido no YAML.")?;
                    out.push(decoded);
                }
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => return Err("Escape inacabado no final da string YAML.".into()),
            },
            other => out.push(other),
        }
    }
    if !closed {
        return Err("String entre aspas duplas sem fechamento no YAML.".into());
    }
    let trailing: String = chars.collect();
    if !trailing.trim().is_empty() {
        return Err("Caracteres inesperados após aspas duplas no YAML.".into());
    }
    Ok(out)
}

fn parse_single_quoted_yaml(raw: &str) -> Result<String, String> {
    let mut chars = raw.chars().peekable();
    if chars.next() != Some('\'') {
        return Err("String entre aspas simples inválida no YAML.".into());
    }
    let mut out = String::new();
    let mut closed = false;
    while let Some(ch) = chars.next() {
        if ch == '\'' {
            if chars.peek() == Some(&'\'') {
                chars.next();
                out.push('\'');
            } else {
                closed = true;
                break;
            }
        } else {
            out.push(ch);
        }
    }
    if !closed {
        return Err("String entre aspas simples sem fechamento no YAML.".into());
    }
    let trailing: String = chars.collect();
    if !trailing.trim().is_empty() {
        return Err("Caracteres inesperados após aspas simples no YAML.".into());
    }
    Ok(out)
}

fn parse_inline_yaml_list(raw: &str) -> Result<Value, String> {
    let inner = raw
        .trim()
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| "Lista YAML inline inválida.".to_string())?;
    if inner.trim().is_empty() {
        return Ok(Value::Array(Vec::new()));
    }
    let mut items = Vec::new();
    let mut start = 0usize;
    let bytes = inner.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    let mut depth = 0usize;

    for (idx, &b) in bytes.iter().enumerate() {
        if in_double {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_double = false;
            }
            continue;
        }
        if in_single {
            if b == b'\'' {
                in_single = false;
            }
            continue;
        }
        match b {
            b'"' => in_double = true,
            b'\'' => in_single = true,
            b'[' | b'{' => depth += 1,
            b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                let slice = inner[start..idx].trim();
                if !slice.is_empty() {
                    items.push(parse_yaml_scalar(slice)?);
                }
                start = idx + 1;
            }
            _ => {}
        }
    }
    let tail = inner[start..].trim();
    if !tail.is_empty() {
        items.push(parse_yaml_scalar(tail)?);
    }
    Ok(Value::Array(items))
}

fn parse_yaml_scalar(raw: &str) -> Result<Value, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Ok(Value::Null);
    }
    if s.starts_with('"') {
        return parse_double_quoted_yaml(s).map(Value::String);
    }
    if s.starts_with('\'') {
        return parse_single_quoted_yaml(s).map(Value::String);
    }
    if s == "[]" {
        return Ok(Value::Array(Vec::new()));
    }
    if s == "{}" {
        return Ok(Value::Object(Map::new()));
    }
    if s.starts_with('[') && s.ends_with(']') {
        return parse_inline_yaml_list(s);
    }
    if s.eq_ignore_ascii_case("true") {
        return Ok(Value::Bool(true));
    }
    if s.eq_ignore_ascii_case("false") {
        return Ok(Value::Bool(false));
    }
    if s == "~" || s.eq_ignore_ascii_case("null") {
        return Ok(Value::Null);
    }
    if let Ok(n) = s.parse::<i64>() {
        return Ok(Value::Number(n.into()));
    }
    if let Ok(f) = s.parse::<f64>() {
        if let Some(num) = serde_json::Number::from_f64(f) {
            return Ok(Value::Number(num));
        }
    }
    Ok(Value::String(s.to_string()))
}

struct YamlLineParser<'a> {
    lines: &'a [&'a str],
    pos: usize,
}

impl<'a> YamlLineParser<'a> {
    fn skip_ignored(&mut self) {
        while self.pos < self.lines.len() {
            let line = self.lines[self.pos];
            if strip_yaml_comment(line).trim().is_empty() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn current_indent(&self) -> usize {
        self.lines
            .get(self.pos)
            .map(|l| line_indent(l))
            .unwrap_or(0)
    }

    fn parse_block(&mut self, base_indent: usize) -> Result<Value, String> {
        self.skip_ignored();
        if self.pos >= self.lines.len() {
            return Ok(Value::Null);
        }
        let clean = strip_yaml_comment(self.lines[self.pos]).trim_start();
        if clean == "-" || clean.starts_with("- ") {
            self.parse_sequence(base_indent)
        } else if split_yaml_kv(clean).is_some() {
            self.parse_mapping(base_indent)
        } else {
            let val = parse_yaml_scalar(clean)?;
            self.pos += 1;
            Ok(val)
        }
    }

    fn parse_mapping(&mut self, base_indent: usize) -> Result<Value, String> {
        let mut map = Map::new();
        self.parse_mapping_into(base_indent, &mut map)?;
        Ok(Value::Object(map))
    }

    fn parse_mapping_into(&mut self, base_indent: usize, map: &mut Map<String, Value>) -> Result<(), String> {
        while self.pos < self.lines.len() {
            self.skip_ignored();
            if self.pos >= self.lines.len() {
                break;
            }
            let line = self.lines[self.pos];
            let indent = line_indent(line);
            if indent < base_indent {
                break;
            }
            if indent > base_indent {
                return Err(format!("Indentação YAML inválida na linha {}", self.pos + 1));
            }
            let clean = strip_yaml_comment(line).trim_start();
            if clean == "-" || clean.starts_with("- ") {
                break;
            }
            let Some((raw_key, rest)) = split_yaml_kv(clean) else {
                return Err(format!("Esperado 'chave: valor' na linha {}", self.pos + 1));
            };
            let key = unquote_yaml_key(raw_key)?;
            self.pos += 1;
            let value = self.parse_mapping_value(base_indent, rest)?;
            map.insert(key, value);
        }
        Ok(())
    }

    fn parse_mapping_value(&mut self, parent_indent: usize, rest: &str) -> Result<Value, String> {
        let rest = rest.trim();
        if rest.starts_with('|') || rest.starts_with('>') {
            let folded = rest.starts_with('>');
            let chomp = if rest.contains('-') {
                BlockChomp::Strip
            } else if rest.contains('+') {
                BlockChomp::Keep
            } else {
                BlockChomp::Clip
            };
            return Ok(Value::String(self.parse_block_scalar(parent_indent, folded, chomp)));
        }
        if !rest.is_empty() {
            return parse_yaml_scalar(rest);
        }

        self.skip_ignored();
        if self.pos >= self.lines.len() {
            return Ok(Value::Null);
        }
        let next_indent = self.current_indent();
        let next_clean = strip_yaml_comment(self.lines[self.pos]).trim_start();
        let is_zero_indent_seq = next_indent == parent_indent
            && (next_clean == "-" || next_clean.starts_with("- "));
        if next_indent > parent_indent || is_zero_indent_seq {
            self.parse_block(next_indent)
        } else {
            Ok(Value::Null)
        }
    }

    fn parse_sequence(&mut self, base_indent: usize) -> Result<Value, String> {
        let mut items = Vec::new();
        while self.pos < self.lines.len() {
            self.skip_ignored();
            if self.pos >= self.lines.len() {
                break;
            }
            let line = self.lines[self.pos];
            let indent = line_indent(line);
            if indent < base_indent {
                break;
            }
            if indent > base_indent {
                break;
            }
            let clean = strip_yaml_comment(line).trim_start();
            if !(clean == "-" || clean.starts_with("- ")) {
                break;
            }
            self.pos += 1;
            let item_rest = clean[1..].trim_start();
            if item_rest.is_empty() {
                self.skip_ignored();
                if self.pos < self.lines.len() && self.current_indent() > base_indent {
                    let child_indent = self.current_indent();
                    items.push(self.parse_block(child_indent)?);
                } else {
                    items.push(Value::Null);
                }
            } else if let Some((raw_k, rest_v)) = split_yaml_kv(item_rest) {
                let mut obj = Map::new();
                let key = unquote_yaml_key(raw_k)?;
                let val = self.parse_mapping_value(base_indent, rest_v)?;
                obj.insert(key, val);
                self.skip_ignored();
                if self.pos < self.lines.len() && self.current_indent() > base_indent {
                    let child_indent = self.current_indent();
                    self.parse_mapping_into(child_indent, &mut obj)?;
                }
                items.push(Value::Object(obj));
            } else {
                items.push(parse_yaml_scalar(item_rest)?);
            }
        }
        Ok(Value::Array(items))
    }

    fn parse_block_scalar(&mut self, parent_indent: usize, folded: bool, chomp: BlockChomp) -> String {
        let mut block_indent: Option<usize> = None;
        let mut raw_lines: Vec<String> = Vec::new();

        while self.pos < self.lines.len() {
            let line = self.lines[self.pos];
            if line.trim().is_empty() {
                raw_lines.push(String::new());
                self.pos += 1;
                continue;
            }
            let indent = line_indent(line);
            let min_allowed = block_indent.unwrap_or(parent_indent + 1);
            if indent < min_allowed {
                break;
            }
            let chosen = *block_indent.get_or_insert(indent);
            raw_lines.push(line[chosen..].to_string());
            self.pos += 1;
        }

        while raw_lines.last().is_some_and(|l| l.is_empty()) && chomp != BlockChomp::Keep {
            raw_lines.pop();
        }

        let mut text = if folded {
            let mut out = String::new();
            for (idx, line) in raw_lines.iter().enumerate() {
                if idx > 0 {
                    if line.is_empty() || raw_lines[idx - 1].is_empty() {
                        out.push('\n');
                    } else {
                        out.push(' ');
                    }
                }
                out.push_str(line);
            }
            out
        } else {
            raw_lines.join("\n")
        };

        if chomp == BlockChomp::Clip && !text.is_empty() {
            text.push('\n');
        }
        text
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockChomp {
    Clip,
    Strip,
    Keep,
}

fn get_str<'a>(val: &'a Value, keys: &[&str]) -> Option<&'a str> {
    for k in keys {
        if let Some(s) = val.get(*k).and_then(Value::as_str) {
            let t = s.trim();
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

fn get_str_fallback<'a>(primary: &'a Value, secondary: &'a Value, keys: &[&str]) -> Option<&'a str> {
    get_str(primary, keys).or_else(|| get_str(secondary, keys))
}

fn slugify_id(name: &str) -> String {
    let folded = fold_pt_ascii(name);
    let mut out = String::with_capacity(folded.len());
    let mut prev_dash = false;
    for ch in folded.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches(['-', '_']).to_string()
}

pub fn extract_comment_policy_from_text(text: &str) -> Option<String> {
    let mut in_policy = false;
    let mut collected = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(after_hashes) = trimmed.strip_prefix("##") {
            let heading = after_hashes.trim();
            if !in_policy {
                if heading.eq_ignore_ascii_case("Política de comentários")
                    || heading.eq_ignore_ascii_case("Politica de comentarios")
                    || heading.eq_ignore_ascii_case("Comment Policy")
                {
                    in_policy = true;
                }
                continue;
            } else {
                break;
            }
        }
        if in_policy {
            collected.push(line);
        }
    }
    let body = collected.join("\n").trim().to_string();
    (!body.is_empty()).then_some(body)
}

pub fn strip_comment_policy_from_text(text: &str) -> String {
    let mut out = Vec::new();
    let mut skipping = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(after_hashes) = trimmed.strip_prefix("##") {
            let heading = after_hashes.trim();
            if heading.eq_ignore_ascii_case("Política de comentários")
                || heading.eq_ignore_ascii_case("Politica de comentarios")
                || heading.eq_ignore_ascii_case("Comment Policy")
            {
                skipping = true;
                continue;
            } else if skipping {
                skipping = false;
            }
        }
        if !skipping {
            out.push(line);
        }
    }
    // Colapsa linhas em branco extras criadas ao remover a seção.
    let mut collapsed = String::new();
    let mut blank_run = 0usize;
    for line in out {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 1 && !collapsed.is_empty() {
                collapsed.push('\n');
            }
        } else {
            blank_run = 0;
            if !collapsed.is_empty() {
                collapsed.push('\n');
            }
            collapsed.push_str(line);
        }
    }
    collapsed.trim().to_string()
}

pub fn extract_comment_policy(persona: &PersonaProfile) -> String {
    if let Some(policy) = extract_comment_policy_from_text(&persona.persona) {
        return policy;
    }
    if let Some(policy) = extract_comment_policy_from_text(&persona.rules) {
        return policy;
    }
    persona.rules.trim().to_string()
}

fn parse_one_persona(val: &Value, index: usize) -> Result<PersonaProfile, String> {
    let is_tavern_v2 = val
        .get("data")
        .and_then(Value::as_object)
        .is_some_and(|data| {
            val.get("spec")
                .and_then(Value::as_str)
                .is_some_and(|s| s.to_ascii_lowercase().contains("chara_card"))
                || (val.get("name").is_none()
                    && val.get("nome").is_none()
                    && (data.contains_key("name") || data.contains_key("nome")))
        });
    let src = if is_tavern_v2 { &val["data"] } else { val };

    let name = get_str_fallback(src, val, &["name", "nome"])
        .ok_or_else(|| format!("Persona #{} está sem o campo `name`.", index + 1))?
        .to_string();
    let id = get_str_fallback(src, val, &["id"])
        .map(slugify_id)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| slugify_id(&name));
    if id.is_empty() {
        return Err(format!("Não foi possível gerar um identificador para a persona \"{name}\"."));
    }

    let tags = src
        .get("tags")
        .or_else(|| val.get("tags"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| match v {
                    Value::String(s) => Some(s.trim().to_string()),
                    Value::Number(n) => Some(n.to_string()),
                    _ => None,
                })
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let enabled = src
        .get("enabled")
        .or_else(|| src.get("ativa"))
        .or_else(|| val.get("enabled"))
        .or_else(|| val.get("ativa"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let is_primary_companion = src
        .get("isPrimaryCompanion")
        .or_else(|| src.get("is_primary_companion"))
        .or_else(|| src.get("primary"))
        .or_else(|| val.get("isPrimaryCompanion"))
        .or_else(|| val.get("is_primary_companion"))
        .or_else(|| val.get("primary"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let color = get_str_fallback(src, val, &["color", "cor"])
        .map(str::to_string)
        .unwrap_or_else(|| {
            default_color_for_id(&id)
                .unwrap_or(DEFAULT_COLORS[index % DEFAULT_COLORS.len()])
                .to_string()
        });

    let mut persona_text = if let Some(p) = get_str_fallback(src, val, &["persona"]) {
        p.to_string()
    } else if let Some(desc) = get_str_fallback(src, val, &["description"]) {
        let personality = get_str_fallback(src, val, &["personality"]).unwrap_or_default();
        let scenario = get_str_fallback(src, val, &["scenario"]).unwrap_or_default();
        if personality.is_empty() && scenario.is_empty() {
            desc.to_string()
        } else {
            let mut composed = format!("## Identity\n{desc}");
            if !personality.is_empty() {
                composed.push_str(&format!("\n\n## Personality\n{personality}"));
            }
            if !scenario.is_empty() {
                composed.push_str(&format!("\n\n## Scenario\n{scenario}"));
            }
            composed
        }
    } else {
        String::new()
    };

    if let Some(style_guide) = get_str_fallback(src, val, &["style_guide", "styleGuide"]) {
        if !persona_text.to_lowercase().contains("## voice") {
            if persona_text.is_empty() {
                persona_text = format!("## Voice\n{style_guide}");
            } else {
                persona_text.push_str(&format!("\n\n## Voice\n{style_guide}"));
            }
        }
    }

    let mut rules = get_str_fallback(src, val, &["rules", "agents", "AGENTS"])
        .unwrap_or_default()
        .to_string();
    if rules.is_empty() && !persona_text.is_empty() {
        if let Some(extracted) = extract_comment_policy_from_text(&persona_text) {
            rules = extracted;
        }
    }

    let mut identity = get_str_fallback(src, val, &["identity", "soul", "SOUL"])
        .unwrap_or_default()
        .to_string();
    if identity.is_empty() && !persona_text.is_empty() {
        identity = strip_comment_policy_from_text(&persona_text);
    }

    Ok(PersonaProfile {
        id,
        name,
        role: get_str_fallback(src, val, &["role", "title", "papel"]).map(str::to_string),
        identity,
        user_profile: get_str_fallback(src, val, &["userProfile", "user_profile", "profile", "PROFILE"])
            .unwrap_or_default()
            .to_string(),
        rules,
        persona: persona_text,
        system_prompt_override: get_str_fallback(
            src,
            val,
            &["systemPromptOverride", "system_prompt_override", "system_prompt"],
        )
        .map(str::to_string),
        post_history_instructions: get_str_fallback(
            src,
            val,
            &["postHistoryInstructions", "post_history_instructions"],
        )
        .map(str::to_string),
        mes_example: get_str_fallback(src, val, &["mesExample", "mes_example", "example_dialogue"])
            .map(str::to_string),
        tags,
        interest_filter: get_str_fallback(
            src,
            val,
            &["interestFilter", "interest_filter", "pkm_interest_filter"],
        )
        .map(str::to_string),
        avatar: get_str_fallback(src, val, &["avatar"]).map(str::to_string),
        first_message: get_str_fallback(src, val, &["firstMessage", "first_message", "first_mes"])
            .map(str::to_string),
        is_primary_companion,
        enabled,
        color,
    })
}

/// Hash estável para detectar se o conteúdo da nota mudou antes de gerar comentários automáticos.
pub fn content_hash(title: &str, body: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in title.trim().as_bytes().iter().chain([b'\n'].iter()).chain(body.trim().as_bytes()) {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub fn fold_pt_ascii(input: &str) -> String {
    input
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            other => other,
        })
        .collect()
}

fn first_mention_pos(folded_text: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    let pattern = format!("@{needle}");
    let mut search_from = 0usize;
    while let Some(rel) = folded_text[search_from..].find(&pattern) {
        let start = search_from + rel;
        let end = start + pattern.len();
        let prev_ok = start == 0
            || !folded_text[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
        let next_ok = end == folded_text.len()
            || !folded_text[end..]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
        if prev_ok && next_ok {
            return Some(start);
        }
        search_from = end;
    }
    None
}

/// Identifica menção `@id`, `@nome` ou `@primeiro_nome` (único) com fronteira lexical (ignora e-mails como `a@dominio.com`).
pub fn extract_mention<'a>(text: &str, active: &[&'a PersonaProfile]) -> Option<&'a PersonaProfile> {
    extract_all_mentions(text, active).into_iter().next()
}

/// Identifica todas as personas mencionadas na mensagem, em ordem de aparição no texto.
pub fn extract_all_mentions<'a>(text: &str, active: &[&'a PersonaProfile]) -> Vec<&'a PersonaProfile> {
    let folded_text = fold_pt_ascii(text);
    let mut found: Vec<(usize, &'a PersonaProfile)> = Vec::new();

    for (idx, p) in active.iter().enumerate() {
        let mut best_pos = [fold_pt_ascii(&p.id), fold_pt_ascii(&p.name)]
            .into_iter()
            .filter_map(|needle| first_mention_pos(&folded_text, &needle))
            .min();

        if best_pos.is_none() {
            if let Some(first_word) = fold_pt_ascii(&p.name)
                .split_whitespace()
                .next()
                .map(str::to_string)
                .filter(|w| w.len() >= 3)
            {
                let unique = active.iter().enumerate().all(|(other_idx, other)| {
                    other_idx == idx
                        || fold_pt_ascii(&other.name).split_whitespace().next() != Some(first_word.as_str())
                });
                if unique {
                    best_pos = first_mention_pos(&folded_text, &first_word);
                }
            }
        }

        if let Some(pos) = best_pos {
            found.push((pos, *p));
        }
    }

    found.sort_by_key(|(pos, _)| *pos);
    found.into_iter().map(|(_, p)| p).collect()
}

/// Ao responder a um comentário, caminha `reply_to_id` até descobrir qual persona iniciou o fio.
pub fn resolve_reply_target_character<'a>(
    comments: &[NoteComment],
    reply_to_id: Option<&str>,
    active: &[&'a PersonaProfile],
) -> Option<&'a PersonaProfile> {
    let mut current_id = reply_to_id?;
    let mut visited = HashSet::new();
    while visited.insert(current_id.to_string()) {
        let Some(comment) = comments.iter().find(|c| c.id == current_id) else {
            return None;
        };
        if comment.author_kind != "user" {
            return active.iter().copied().find(|p| p.id == comment.character_id);
        }
        let Some(parent) = comment.reply_to_id.as_deref() else {
            return None;
        };
        current_id = parent;
    }
    None
}

/// Quantos turnos seguidos de persona ainda restam desde a última fala do usuário.
pub fn remaining_persona_turns(comments: &[NoteComment], max_turns: usize) -> usize {
    let trailing_persona = comments
        .iter()
        .rev()
        .take_while(|c| c.author_kind != "user")
        .count();
    max_turns.saturating_sub(trailing_persona)
}

fn normalize_affinity_text(text: &str) -> String {
    let folded = fold_pt_ascii(text);
    let mut out = String::with_capacity(folded.len());
    let mut prev_space = true;
    for ch in folded.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    out.trim().to_string()
}

fn contains_affinity_phrase(padded_normalized: &str, phrase: &str) -> bool {
    let norm = normalize_affinity_text(phrase);
    !norm.is_empty() && padded_normalized.contains(&format!(" {norm} "))
}

fn is_interest_stop_word(word: &str) -> bool {
    const STOP_WORDS: &[&str] = &[
        "the", "and", "for", "that", "this", "with", "from", "about", "into", "like", "just",
        "only", "also", "more", "than", "when", "will", "what", "which", "their", "them", "they",
        "have", "been", "focus", "ignore", "unless", "based", "user",
        "foque", "foco", "em", "de", "do", "da", "dos", "das", "no", "na", "nos", "nas", "para",
        "por", "com", "sem", "que", "quando", "uma", "uns", "umas", "relacao", "sobre",
        "assuntos", "registros", "escolhas", "coisas", "itens", "algo", "tudo", "nada", "como",
        "entre", "ou", "se", "ao", "aos", "as", "os", "um", "nao", "mais", "menos", "evite",
    ];
    STOP_WORDS.contains(&word)
}

fn cut_before_negative_marker(folded: &str) -> &str {
    const MARKERS: &[&str] = &[
        "ignore:",
        "ignore ",
        "do not focus on",
        "nao foque em",
        "evite:",
        "evite ",
        "sem foco em",
    ];
    let mut end = folded.len();
    for marker in MARKERS {
        let mut search_from = 0usize;
        while let Some(rel) = folded[search_from..end].find(marker) {
            let pos = search_from + rel;
            let boundary_ok = pos == 0
                || !folded[..pos]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_ascii_alphanumeric());
            if boundary_ok {
                end = pos;
                break;
            }
            search_from = pos + marker.len();
        }
    }
    &folded[..end]
}

fn positive_interest_keywords(filter: &str) -> Vec<String> {
    let folded = fold_pt_ascii(filter);
    let positive_part = cut_before_negative_marker(&folded);
    let mut out = Vec::new();
    for token in positive_part.split(|c: char| !c.is_ascii_alphanumeric()) {
        if token.len() >= 3 && !is_interest_stop_word(token) && !out.iter().any(|w| w == token) {
            out.push(token.to_string());
        }
    }
    out
}

pub fn score_affinity(persona: &PersonaProfile, text: &str, tags: &[String]) -> i32 {
    let norm_text = normalize_affinity_text(text);
    let padded_text = format!(" {norm_text} ");
    let folded_tags: Vec<String> = tags.iter().map(|t| normalize_affinity_text(t)).collect();
    let mut score = 0i32;

    for p_tag in &persona.tags {
        let needle = normalize_affinity_text(p_tag);
        if needle.is_empty() {
            continue;
        }
        if folded_tags.iter().any(|t| t == &needle) || contains_affinity_phrase(&padded_text, &needle) {
            score += 3;
        }
    }

    let positive_kws = persona
        .interest_filter
        .as_deref()
        .map(positive_interest_keywords)
        .unwrap_or_default();
    for kw in &positive_kws {
        if contains_affinity_phrase(&padded_text, kw) {
            score += 2;
        }
    }

    let char_blob = format!(
        " {} ",
        normalize_affinity_text(&format!(
            "{} {} {} {} {} {} {}",
            persona.id,
            persona.name,
            persona.role.as_deref().unwrap_or(""),
            persona.tags.join(" "),
            positive_kws.join(" "),
            persona.identity,
            persona.rules
        ))
    );

    const HEALTH_SIGNALS: &[&str] = &[
        "sono", "cansado", "cansada", "cansaco", "exausto", "exausta", "insonia", "madrugada",
        "dormi", "dormir", "dor", "febre", "doente", "saude", "tired", "exhausted", "sleep",
        "insomnia", "headache", "sick",
    ];
    const VENTING_SIGNALS: &[&str] = &[
        "raiva", "frustrado", "frustrada", "irritado", "irritada", "odeio", "absurdo", "injusto",
        "que saco", "que droga", "saco cheio", "desabafo", "angry", "annoyed", "frustrated",
        "hate", "ugh", "pissed", "fed up",
    ];
    const REFLECTIVE_SIGNALS: &[&str] = &[
        "decisao", "decidir", "escolha", "escolher", "trade off", "rumo", "arquitetura", "produto",
        "carreira", "futuro", "objetivo", "proposito", "refletindo", "pensando sobre", "percebi",
        "duvida", "historico", "decision", "career", "future", "perspective", "growth",
    ];
    const EMOTIONAL_SIGNALS: &[&str] = &[
        "triste", "tristeza", "saudade", "solidao", "sozinho", "sozinha", "decepcao", "chateado",
        "chateada", "melancolia", "lembrei", "memoria", "silencio", "ansioso", "ansiosa",
        "emo", "emocao", "sad", "lonely", "miss", "nostalgia", "melancholy",
    ];

    if HEALTH_SIGNALS.iter().any(|s| contains_affinity_phrase(&padded_text, s))
        && [
            "guardiao_energia", "sono", "cansaco", "ritmo", "saude", "cuidado", "bem estar",
            "energy", "health", "care", "warmth",
        ]
        .iter()
        .any(|k| contains_affinity_phrase(&char_blob, k))
    {
        score += 4;
    }

    if VENTING_SIGNALS.iter().any(|s| contains_affinity_phrase(&padded_text, s))
        && [
            "amigo_bar", "informal", "humor", "desabafo", "companhia", "bestie", "venting",
            "company",
        ]
        .iter()
        .any(|k| contains_affinity_phrase(&char_blob, k))
    {
        score += 4;
    }

    if REFLECTIVE_SIGNALS.iter().any(|s| contains_affinity_phrase(&padded_text, s))
        && [
            "socio_tecnico", "advogado_diabo", "memoria_projeto", "decisao", "contraponto",
            "historico", "arquitetura", "produto", "wisdom", "mentor", "validation", "reflex",
        ]
        .iter()
        .any(|k| contains_affinity_phrase(&char_blob, k))
    {
        score += 4;
    }

    if EMOTIONAL_SIGNALS.iter().any(|s| contains_affinity_phrase(&padded_text, s))
        && [
            "escuta_atenta", "emocao", "apoio", "relacoes", "acolh", "diario", "attentive",
            "support", "nostalgia", "poetic",
        ]
        .iter()
        .any(|k| contains_affinity_phrase(&char_blob, k) || char_blob.contains(k))
    {
        score += 4;
    }

    score
}

fn rank_by_affinity<'a>(
    active: &[&'a PersonaProfile],
    text: &str,
    tags: &[String],
) -> Vec<(&'a PersonaProfile, i32)> {
    let mut scored: Vec<(&PersonaProfile, i32)> = active
        .iter()
        .map(|p| (*p, score_affinity(p, text, tags)))
        .collect();
    scored.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| b.0.is_primary_companion.cmp(&a.0.is_primary_companion))
            .then_with(|| a.0.id.cmp(&b.0.id))
    });
    scored
}

fn best_clear_affinity_match<'a>(
    active: &[&'a PersonaProfile],
    text: &str,
    tags: &[String],
) -> Option<&'a PersonaProfile> {
    let scored = rank_by_affinity(active, text, tags);
    if scored.is_empty() {
        return None;
    }
    if scored.len() == 1 {
        return (scored[0].1 > 0).then_some(scored[0].0);
    }
    let (best, best_score) = scored[0];
    let second_score = scored[1].1;
    if best_score > 0 && best_score - second_score >= 2 {
        Some(best)
    } else {
        None
    }
}

fn top_positive_affinity<'a>(
    active: &[&'a PersonaProfile],
    text: &str,
    tags: &[String],
    limit: usize,
) -> Vec<&'a PersonaProfile> {
    rank_by_affinity(active, text, tags)
        .into_iter()
        .filter(|(_, score)| *score > 0)
        .take(limit)
        .map(|(p, _)| p)
        .collect()
}

pub fn select_single_character<'a>(
    active: &[&'a PersonaProfile],
    text: &str,
    tags: &[String],
    prefer_non_primary: bool,
) -> Option<&'a PersonaProfile> {
    if active.is_empty() {
        return None;
    }
    if active.len() == 1 {
        return Some(active[0]);
    }
    if let Some(winner) = best_clear_affinity_match(active, text, tags) {
        return Some(winner);
    }
    if prefer_non_primary {
        if let Some(non_primary) = active.iter().copied().find(|p| !p.is_primary_companion) {
            return Some(non_primary);
        }
    }
    active
        .iter()
        .copied()
        .find(|p| p.is_primary_companion)
        .or_else(|| active.first().copied())
}

fn strip_list_prefix(s: &str) -> &str {
    let trimmed = s.trim();
    let without_bullet = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .unwrap_or(trimmed);
    let digit_end = without_bullet.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digit_end > 0 {
        let rest = &without_bullet[digit_end..];
        if let Some(after) = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')')) {
            return after.trim();
        }
    }
    without_bullet.trim()
}

fn push_unique_id(chosen: &mut Vec<String>, id: &str, limit: usize) -> bool {
    if chosen.iter().any(|existing| existing == id) {
        return chosen.len() >= limit;
    }
    chosen.push(id.to_string());
    chosen.len() >= limit
}

pub fn parse_ranked_character_ids(raw: &str, active: &[&PersonaProfile], limit: usize) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }
    let folded_active: Vec<(&PersonaProfile, String, String)> = active
        .iter()
        .map(|p| (*p, fold_pt_ascii(&p.id), fold_pt_ascii(&p.name)))
        .collect();

    let mut chosen = Vec::new();
    for segment in raw.split([',', '\n', ';', '|', '/']) {
        let stripped = strip_list_prefix(segment);
        let cleaned = stripped.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_' && c != ' ');
        let folded_seg = fold_pt_ascii(cleaned);
        if folded_seg.is_empty() {
            continue;
        }
        if let Some((p, _, _)) = folded_active
            .iter()
            .find(|(_, fid, fname)| fid == &folded_seg || fname == &folded_seg)
        {
            if push_unique_id(&mut chosen, &p.id, limit) {
                break;
            }
            continue;
        }
        let words: Vec<&str> = folded_seg
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_'))
            .filter(|w| !w.is_empty())
            .collect();
        let mut idx = 0usize;
        while idx < words.len() {
            let mut matched_advance = None;
            // Tenta casar nomes multi-palavra (mais longo primeiro) e ids exatos em ordem de aparição.
            for (p, fid, fname) in &folded_active {
                let name_words: Vec<&str> = fname.split_whitespace().collect();
                if !name_words.is_empty()
                    && idx + name_words.len() <= words.len()
                    && words[idx..idx + name_words.len()] == name_words[..]
                {
                    matched_advance = Some((p.id.as_str(), name_words.len()));
                    break;
                }
                if fid == words[idx] {
                    matched_advance = Some((p.id.as_str(), 1));
                    break;
                }
            }
            if let Some((id, adv)) = matched_advance {
                if push_unique_id(&mut chosen, id, limit) {
                    return chosen;
                }
                idx += adv;
            } else {
                idx += 1;
            }
        }
    }
    chosen
}

fn format_routing_persona_list(active: &[&PersonaProfile]) -> String {
    let mut list = String::new();
    for p in active {
        let role = p.role.as_deref().unwrap_or("companheira");
        let tags = if p.tags.is_empty() { "-".to_string() } else { p.tags.join(", ") };
        let policy = extract_comment_policy(p);
        let focus = p.interest_filter.as_deref().unwrap_or("-");
        if policy.is_empty() {
            list.push_str(&format!("- id: {} | nome: {} | papel: {} | tags: {} | foco: {}\n", p.id, p.name, role, tags, focus));
        } else {
            list.push_str(&format!(
                "- id: {} | nome: {} | papel: {} | tags: {} | foco: {}\n  política de comentários:\n  {}\n",
                p.id,
                p.name,
                role,
                tags,
                focus,
                policy.replace('\n', "\n  ")
            ));
        }
    }
    list
}

pub fn build_routing_prompt(active: &[&PersonaProfile], note_text: &str, max_chars: usize) -> (String, String) {
    let list = format_routing_persona_list(active);
    let multi_hint = if max_chars > 1 {
        format!(
            " Trata-se de um chat em grupo vivo: inclua TODAS as personas pertinentes cujo papel ou política acrescente um ângulo distinto, útil ou complementar (podem ser 2, 5 ou até {max_chars}, sem cortar artificialmente em 1)."
        )
    } else {
        String::new()
    };
    let system = format!(
        "Você coordena subagentes de IA de um chat em grupo sobre uma anotação do usuário e decide quais têm algo genuíno para contribuir. Respeite a política de cada persona (responda quando / pule quando).{multi_hint} Responda SOMENTE com os `id`s ordenados por relevância separados por vírgula, no máximo {max_chars}. Se nenhuma persona se encaixar pela política, responda exatamente uma string vazia."
    );
    let user = format!(
        "Subagentes/personas ativas:\n{list}\nLimite máximo: {max_chars}\n\nAnotação:\n{}\n\nResponda apenas os ids separados por vírgula (ou deixe vazio se nenhuma persona se encaixar pela política):",
        truncate_chars(note_text, 1200)
    );
    (system, user)
}

pub fn build_group_followup_routing_prompt(
    active: &[&PersonaProfile],
    note_text: &str,
    existing_comments: &[NoteComment],
    max_chars: usize,
) -> (String, String) {
    let list = format_routing_persona_list(active);
    let mut context = format!("Subagentes/personas candidatas a réplica:\n{list}\n");
    context.push_str(&format!("\nAnotação:\n{}\n", truncate_chars(note_text, 800)));
    context.push_str("\nConversa atual no grupo:\n");
    let start = existing_comments.len().saturating_sub(12);
    for c in &existing_comments[start..] {
        context.push_str(&format!(
            "- [{}] {}: {}\n",
            c.id,
            c.character_name,
            truncate_chars(c.content.trim(), 240)
        ));
    }
    context.push_str(&format!(
        "\nQuais personas acima (até {max_chars}) têm uma réplica direta, contraponto, complemento ou pergunta pertinente a um dos comentários acima? Responda SOMENTE os `id`s separados por vírgula (ou deixe vazio se a discussão já estiver bem fechada):"
    ));
    let system = format!(
        "Você coordena um chat em grupo vivo entre subagentes de personas. Escolha até {max_chars} personas que tenham reação genuína e não repetitiva aos comentários recém-feitos por outras personas ou pelo usuário. Se nada relevante acrescentar, responda string vazia."
    );
    (system, context)
}

pub fn build_reply_routing_prompt(
    active: &[&PersonaProfile],
    note_text: &str,
    user_comment: &str,
    existing_comments: &[NoteComment],
    max_chars: usize,
) -> (String, String) {
    let list = format_routing_persona_list(active);
    let mut participants = Vec::new();
    for c in existing_comments.iter().rev() {
        if c.author_kind != "user" && !participants.iter().any(|id| id == &c.character_id) {
            participants.push(c.character_id.clone());
        }
    }
    let mut context = format!("Personas ativas:\n{list}\n");
    if !participants.is_empty() {
        context.push_str(&format!("Personas que já comentaram nesta nota: {}\n", participants.join(", ")));
    }
    context.push_str(&format!("\nAnotação original:\n{}\n", truncate_chars(note_text, 800)));
    if !existing_comments.is_empty() {
        context.push_str("\nHistórico recente de comentários:\n");
        let start = existing_comments.len().saturating_sub(10);
        for c in &existing_comments[start..] {
            context.push_str(&format!("- [{}] {}: {}\n", c.id, c.character_name, truncate_chars(c.content.trim(), 220)));
        }
    }
    context.push_str(&format!(
        "\nNovo comentário do usuário:\n{}\n\nResponda apenas os ids das personas pertinentes para participar/responder (até {max_chars}, separados por vírgula):",
        truncate_chars(user_comment, 600)
    ));
    let system = format!(
        "Você coordena um chat em grupo vivo e escolhe quais personas ativas devem responder ao novo comentário do usuário (até {max_chars}). Dê preferência às personas já envolvidas na conversa ou cujo papel/política tenha algo pertinente a acrescentar. Responda SOMENTE com os `id`s ordenados por relevância separados por vírgula."
    );
    (system, context)
}

#[derive(Debug, Clone, Copy)]
pub struct SelectionResult<'a> {
    pub persona: &'a PersonaProfile,
    pub force_reply: bool,
}

/// Seleciona personas conforme regras da Segunda Mente: `@mention` -> fio de resposta -> configurações -> afinidade / roteamento LLM.
pub async fn select_characters_with<'a, F, Fut>(
    active: &[&'a PersonaProfile],
    note_text: &str,
    trigger_text: &str,
    existing_comments: &[NoteComment],
    reply_to_id: Option<&str>,
    settings: &CommentSettings,
    llm_route: F,
) -> Vec<SelectionResult<'a>>
where
    F: FnOnce(String, String) -> Fut,
    Fut: Future<Output = Result<String, String>>,
{
    if active.is_empty() {
        return Vec::new();
    }
    if let Some(mentioned) = extract_mention(trigger_text, active) {
        return vec![SelectionResult { persona: mentioned, force_reply: true }];
    }
    if let Some(owner) = resolve_reply_target_character(existing_comments, reply_to_id, active) {
        return vec![SelectionResult { persona: owner, force_reply: true }];
    }
    if !settings.enable_character_comment {
        return Vec::new();
    }
    let limit = settings.max_comment_characters.clamp(1, active.len());
    let combined_text = if trigger_text.trim().is_empty() || trigger_text == note_text {
        note_text.to_string()
    } else {
        format!("{note_text}\n{trigger_text}")
    };
    if active.len() == 1 || limit == 1 {
        return select_single_character(active, &combined_text, &[], false)
            .map(|p| vec![SelectionResult { persona: p, force_reply: false }])
            .unwrap_or_default();
    }

    let (sys, usr) = if !existing_comments.is_empty() && trigger_text != note_text && !trigger_text.trim().is_empty() {
        build_reply_routing_prompt(active, note_text, trigger_text, existing_comments, limit)
    } else {
        build_routing_prompt(active, note_text, limit)
    };
    if let Ok(raw) = llm_route(sys, usr).await {
        let ids = parse_ranked_character_ids(&raw, active, limit);
        if !ids.is_empty() {
            return ids
                .into_iter()
                .filter_map(|id| active.iter().copied().find(|p| p.id == id))
                .map(|persona| SelectionResult { persona, force_reply: false })
                .collect();
        }
        // Roteador retornou vazio ou sem IDs válidos: aproveita matches positivos de afinidade até `limit`,
        // respeitando silêncio quando nenhuma persona tem afinidade positiva.
        return top_positive_affinity(active, &combined_text, &[], limit)
            .into_iter()
            .map(|persona| SelectionResult { persona, force_reply: false })
            .collect();
    }

    rank_by_affinity(active, &combined_text, &[])
        .into_iter()
        .take(limit)
        .map(|(persona, _)| SelectionResult { persona, force_reply: false })
        .collect()
}

/// Seleciona personas que devem responder a um comentário direto do usuário na nota (suportando múltiplas menções e chat em grupo).
pub async fn select_reply_characters_with<'a, F, Fut>(
    active: &[&'a PersonaProfile],
    note_text: &str,
    user_comment: &str,
    existing_comments: &[NoteComment],
    reply_to_id: Option<&str>,
    settings: &CommentSettings,
    llm_route: F,
) -> Vec<SelectionResult<'a>>
where
    F: FnOnce(String, String) -> Fut,
    Fut: Future<Output = Result<String, String>>,
{
    if active.is_empty() {
        return Vec::new();
    }
    let mentioned = extract_all_mentions(user_comment, active);
    if !mentioned.is_empty() {
        let limit = settings.max_comment_characters.clamp(1, active.len());
        return mentioned
            .into_iter()
            .take(limit)
            .map(|persona| SelectionResult { persona, force_reply: true })
            .collect();
    }
    if let Some(owner) = resolve_reply_target_character(existing_comments, reply_to_id, active) {
        return vec![SelectionResult { persona: owner, force_reply: true }];
    }
    let has_persona_history = existing_comments.iter().any(|c| c.author_kind != "user");
    if !settings.enable_character_comment && !has_persona_history {
        return Vec::new();
    }

    let limit = settings.max_comment_characters.clamp(1, active.len());
    let (sys, usr) = build_reply_routing_prompt(active, note_text, user_comment, existing_comments, limit);
    if let Ok(raw) = llm_route(sys, usr).await {
        let ids = parse_ranked_character_ids(&raw, active, limit);
        if !ids.is_empty() {
            return ids
                .into_iter()
                .filter_map(|id| active.iter().copied().find(|p| p.id == id))
                .enumerate()
                .map(|(idx, persona)| SelectionResult {
                    persona,
                    force_reply: idx == 0,
                })
                .collect();
        }
    }

    if let Some(affinity_hit) = top_positive_affinity(active, user_comment, &[], 1).into_iter().next() {
        return vec![SelectionResult { persona: affinity_hit, force_reply: true }];
    }
    if let Some(last_speaker) = existing_comments
        .iter()
        .rev()
        .filter(|c| c.author_kind != "user")
        .find_map(|c| active.iter().copied().find(|p| p.id == c.character_id))
    {
        return vec![SelectionResult { persona: last_speaker, force_reply: true }];
    }
    let combined = format!("{note_text}\n{user_comment}");
    select_single_character(active, &combined, &[], false)
        .map(|persona| vec![SelectionResult { persona, force_reply: true }])
        .unwrap_or_default()
}

pub fn resolve_tavern_macros(text: &str, user_name: &str, char_name: &str) -> String {
    if !text.contains("{{") {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        if let Some(end) = after_open.find("}}") {
            let token = &after_open[..end];
            if token.eq_ignore_ascii_case("user") {
                out.push_str(user_name);
            } else if token.eq_ignore_ascii_case("char") {
                out.push_str(char_name);
            } else {
                out.push_str("{{");
                out.push_str(token);
                out.push_str("}}");
            }
            rest = &after_open[end + 2..];
        } else {
            out.push_str("{{");
            rest = after_open;
        }
    }
    out.push_str(rest);
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedGroupComment {
    pub content: String,
    pub reply_to_id: Option<String>,
    pub memory_note: Option<String>,
}

fn format_group_thread(existing_comments: &[NoteComment]) -> String {
    let mut out = String::from("\n<group_thread>\n");
    let by_id: std::collections::HashMap<&str, &NoteComment> =
        existing_comments.iter().map(|c| (c.id.as_str(), c)).collect();
    for c in existing_comments {
        let target_label = c
            .reply_to_id
            .as_deref()
            .and_then(|rid| by_id.get(rid))
            .map(|p| format!(" (em resposta a [{}] {})", p.id, p.character_name))
            .unwrap_or_default();
        out.push_str(&format!(
            "- [{}] {} (@{}){}: {}\n",
            c.id,
            c.character_name,
            c.character_id,
            target_label,
            c.content.trim()
        ));
    }
    out.push_str("</group_thread>\n");
    out
}

fn build_subagent_system_prompt(
    persona: &PersonaProfile,
    agents_md: &str,
    living_memory: Option<&str>,
    participants: &[&PersonaProfile],
) -> String {
    let m = |s: &str| resolve_tavern_macros(s, "Usuário", &persona.name);
    let mut system = String::new();
    if let Some(override_prompt) = &persona.system_prompt_override {
        system.push_str(&m(override_prompt));
        system.push('\n');
    } else {
        system.push_str(&format!(
            "Você é um subagente de IA vivo executando a definição AGENTS.md da persona \"{}\" (@{}) em português do Brasil.\n\
Você não é um bot estático nem uma resposta pronta: participe deste tópico como uma pessoa/agente real em um chat em grupo ativo sobre as notas do usuário.\n\
Reaja ao conteúdo, concorde, discorde, provoque, questione ou responda diretamente a outras personas presentes quando fizer sentido — sempre fiel à sua voz e às suas regras.\n",
            persona.name, persona.id
        ));
    }

    system.push_str("\n<persona_agents_md>\n");
    system.push_str(&m(agents_md.trim()));
    system.push_str("\n</persona_agents_md>\n");

    if let Some(mem) = living_memory.map(str::trim).filter(|s| !s.is_empty()) {
        system.push_str("\n<persona_living_memory>\n");
        system.push_str(&m(mem));
        system.push_str("\n</persona_living_memory>\n");
    }

    if participants.len() > 1 {
        system.push_str("\nParticipantes ativas neste chat em grupo (você pode citar com @id ou responder a uma delas): ");
        let roster: Vec<String> = participants
            .iter()
            .filter(|p| p.id != persona.id)
            .map(|p| format!("{} (@{})", p.name, p.id))
            .collect();
        system.push_str(&roster.join(", "));
        system.push('\n');
    }
    system
}

pub fn build_comment_prompt(
    persona: &PersonaProfile,
    note_title: &str,
    note_body: &str,
    summary: Option<&str>,
    trigger_user_comment: Option<&str>,
    existing_comments: &[NoteComment],
    force_reply: bool,
) -> (String, String) {
    let agents_md = render_persona_agents_md(persona);
    build_comment_prompt_from_docs(
        persona,
        &agents_md,
        None,
        &[persona],
        note_title,
        note_body,
        summary,
        trigger_user_comment,
        existing_comments,
        force_reply,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn build_comment_prompt_live(
    dir: &Path,
    persona: &PersonaProfile,
    participants: &[&PersonaProfile],
    note_title: &str,
    note_body: &str,
    summary: Option<&str>,
    trigger_user_comment: Option<&str>,
    existing_comments: &[NoteComment],
    force_reply: bool,
    target_comment: Option<&NoteComment>,
) -> (String, String) {
    let (agents_md, living_memory) = load_persona_living_docs_in(dir, persona);
    build_comment_prompt_from_docs(
        persona,
        &agents_md,
        living_memory.as_deref(),
        participants,
        note_title,
        note_body,
        summary,
        trigger_user_comment,
        existing_comments,
        force_reply,
        target_comment,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_comment_prompt_from_docs(
    persona: &PersonaProfile,
    agents_md: &str,
    living_memory: Option<&str>,
    participants: &[&PersonaProfile],
    note_title: &str,
    note_body: &str,
    summary: Option<&str>,
    trigger_user_comment: Option<&str>,
    existing_comments: &[NoteComment],
    force_reply: bool,
    target_comment: Option<&NoteComment>,
) -> (String, String) {
    let m = |s: &str| resolve_tavern_macros(s, "Usuário", &persona.name);
    let system = build_subagent_system_prompt(persona, agents_md, living_memory, participants);

    let mut user = format!(
        "<note_title>\n{}\n</note_title>\n\n<note_content>\n{}\n</note_content>\n",
        note_title.trim(),
        truncate_chars(note_body.trim(), 2400)
    );
    if let Some(sum) = summary.filter(|s| !s.trim().is_empty()) {
        user.push_str(&format!("\n<note_summary>\n{}\n</note_summary>\n", sum.trim()));
    }
    if !existing_comments.is_empty() {
        user.push_str(&format_group_thread(existing_comments));
    }
    if let Some(target) = target_comment {
        user.push_str(&format!(
            "\nVocê está entrando na conversa para responder diretamente ao comentário [{}] de {}:\n\"{}\"\nResponda diretamente a esse ponto sem repetir o que já foi dito.\n",
            target.id,
            target.character_name,
            target.content.trim()
        ));
    } else if let Some(reply) = trigger_user_comment.filter(|s| !s.trim().is_empty()) {
        user.push_str(&format!(
            "\nO usuário respondeu agora no tópico:\n\"{}\"\nResponda de forma direta, viva e natural a essa fala.\n",
            reply.trim()
        ));
    }
    if let Some(post) = &persona.post_history_instructions {
        user.push('\n');
        user.push_str(&m(post.trim()));
        user.push('\n');
    }

    let thread_hint = if !existing_comments.is_empty() && target_comment.is_none() {
        " Se estiver respondendo especificamente a um comentário acima, comece opcionalmente com `[REPLY:id_do_comentario]` (ex: `[REPLY:c1]`) ou mencione `@Pessoa`."
    } else {
        ""
    };
    let memory_hint = " Se surgir algum fato ou preferência duradoura do usuário para lembrar no futuro, você pode encerrar opcionalmente em nova linha com `[MEMORY: observação objetiva em 1 frase]`.";

    if force_reply {
        user.push_str(&format!(
            "\nEscreva um comentário curto (1 a 4 frases), vivo e útil no seu estilo próprio.{thread_hint}{memory_hint}"
        ));
    } else if !existing_comments.is_empty() {
        user.push_str(&format!(
            "\nVocê foi selecionada no chat em grupo para contribuir com sua perspectiva ou reagir a outra persona. Escreva de 1 a 4 frases vivas pelo seu próprio ângulo sem repetir o que já foi dito.{thread_hint}{memory_hint} Responda exatamente `[SKIP]` somente se sua política proibir estritamente comentar este assunto."
        ));
    } else {
        user.push_str(&format!(
            "\nEscreva um comentário curto (1 a 4 frases) acrescentando algo concreto sem repetir comentários já feitos.{memory_hint} Se realmente não houver nada útil ou pertinente a dizer, responda exatamente `[SKIP]`."
        ));
    }
    (system, user)
}

fn extract_tag_value(text: &str, tag: &str) -> (Option<String>, String) {
    let prefix = format!("[{tag}:");
    let mut remaining = String::with_capacity(text.len());
    let mut extracted: Option<String> = None;
    let mut rest = text;
    let lower_prefix = prefix.to_ascii_lowercase();

    while let Some(pos) = rest.to_ascii_lowercase().find(&lower_prefix) {
        remaining.push_str(&rest[..pos]);
        let after_prefix = &rest[pos + prefix.len()..];
        if let Some(close) = after_prefix.find(']') {
            let val = after_prefix[..close].trim();
            if !val.is_empty() && extracted.is_none() {
                extracted = Some(val.to_string());
            }
            rest = &after_prefix[close + 1..];
        } else {
            remaining.push_str(&rest[pos..]);
            rest = "";
            break;
        }
    }
    remaining.push_str(rest);
    (extracted, remaining.trim().to_string())
}

pub fn parse_group_comment_output(
    raw: &str,
    existing_comments: &[NoteComment],
    active: &[&PersonaProfile],
    force_reply: bool,
) -> Option<ParsedGroupComment> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (memory_note, without_mem) = extract_tag_value(trimmed, "MEMORY");
    let (raw_reply_id, without_tags) = extract_tag_value(&without_mem, "REPLY");
    let clean_content = without_tags.trim();
    if clean_content.is_empty() {
        return None;
    }
    if !force_reply
        && (clean_content == "[SKIP]"
            || clean_content.eq_ignore_ascii_case("skip")
            || clean_content.contains("[SKIP]"))
    {
        return None;
    }

    let valid_explicit_reply = raw_reply_id.and_then(|rid| {
        existing_comments
            .iter()
            .find(|c| c.id.eq_ignore_ascii_case(rid.trim()))
            .map(|c| c.id.clone())
    });

    let auto_mention_reply = if valid_explicit_reply.is_none() && !active.is_empty() && !existing_comments.is_empty() {
        extract_mention(clean_content, active).and_then(|mentioned| {
            existing_comments
                .iter()
                .rev()
                .find(|c| c.character_id == mentioned.id)
                .map(|c| c.id.clone())
        })
    } else {
        None
    };

    Some(ParsedGroupComment {
        content: clean_content.to_string(),
        reply_to_id: valid_explicit_reply.or(auto_mention_reply),
        memory_note,
    })
}

pub fn parse_comment_output(raw: &str, force_reply: bool) -> Option<String> {
    parse_group_comment_output(raw, &[], &[], force_reply).map(|parsed| parsed.content)
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

// ── Sincronização via ADB (Segunda Mente Android) ─────────────────────────────

pub fn find_adb_executable() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            let candidate = PathBuf::from(local_app_data)
                .join("Android")
                .join("Sdk")
                .join("platform-tools")
                .join("adb.exe");
            if candidate.is_file() {
                return candidate;
            }
        }
        for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
            if let Some(sdk) = std::env::var_os(var) {
                let candidate = PathBuf::from(sdk).join("platform-tools").join("adb.exe");
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
        if let Some(on_path) = crate::platform::find_on_path("adb") {
            return on_path;
        }
        PathBuf::from("adb")
    }
    #[cfg(not(windows))]
    {
        for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
            if let Some(sdk) = std::env::var_os(var) {
                let candidate = PathBuf::from(sdk).join("platform-tools").join("adb");
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
        crate::platform::find_on_path("adb").unwrap_or_else(|| PathBuf::from("adb"))
    }
}

fn run_adb(adb: &Path, serial: Option<&str>, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new(adb);
    crate::platform::no_console(&mut cmd);
    if let Some(s) = serial {
        cmd.args(["-s", s]);
    }
    cmd.args(args);
    let output = cmd
        .output()
        .map_err(|e| format!("Falha ao executar ADB ({}): {e}", adb.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(detail);
    }
    Ok(output.stdout)
}

fn list_adb_devices(adb: &Path) -> Result<Vec<String>, String> {
    let out = run_adb(adb, None, &["devices"])?;
    let text = String::from_utf8_lossy(&out);
    let mut devices = Vec::new();
    for line in text.lines().skip(1) {
        let mut parts = line.split_whitespace();
        let Some(serial) = parts.next() else { continue };
        let Some(state) = parts.next() else { continue };
        if state == "device" {
            devices.push(serial.to_string());
        }
    }
    Ok(devices)
}

fn is_safe_adb_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\0')
        && !path.contains('\n')
        && !path.contains('\r')
        && !path.split('/').any(|seg| seg == ".." || seg.starts_with('.'))
}

fn collect_yaml_files_from_device(adb: &Path, serial: &str) -> Result<Vec<(String, String)>, String> {
    const PACKAGES: &[&str] = &["app.segundamente.dev", "app.segundamente"];
    const LIST_YAML_SH: &str =
        r#"for f in app_flutter/workspace/*/Characters/*.yaml; do [ -f "$f" ] && printf "%s\n" "$f"; done"#;

    for pkg in PACKAGES {
        if let Ok(paths_bytes) = run_adb(adb, Some(serial), &["exec-out", "run-as", pkg, "sh", "-c", LIST_YAML_SH]) {
            let paths_text = String::from_utf8_lossy(&paths_bytes);
            if paths_text.contains("run-as:") || paths_text.contains("not debuggable") {
                continue;
            }
            let mut collected = Vec::new();
            for remote_path in paths_text.lines().map(str::trim).filter(|p| p.ends_with(".yaml") && is_safe_adb_path(p)) {
                let file_name = remote_path.rsplit('/').next().unwrap_or(remote_path);
                let stem = file_name.strip_suffix(".yaml").unwrap_or(file_name);
                let content_bytes = run_adb(adb, Some(serial), &["exec-out", "run-as", pkg, "cat", remote_path])?;
                let content = String::from_utf8(content_bytes)
                    .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
                collected.push((stem.to_string(), content));
            }
            if !collected.is_empty() {
                return Ok(collected);
            }
        }
    }

    // Fallback para builds sem `run-as` via armazenamento externo do app.
    for base in [
        "/sdcard/Android/data/app.segundamente/files",
        "/sdcard/Android/data/app.segundamente.dev/files",
    ] {
        let sh = format!(
            r#"for f in {base}/workspace/*/Characters/*.yaml; do [ -f "$f" ] && printf "%s\n" "$f"; done"#
        );
        if let Ok(paths_bytes) = run_adb(adb, Some(serial), &["exec-out", "sh", "-c", &sh]) {
            let paths_text = String::from_utf8_lossy(&paths_bytes);
            let mut collected = Vec::new();
            for remote_path in paths_text.lines().map(str::trim).filter(|p| p.ends_with(".yaml") && is_safe_adb_path(p)) {
                let file_name = remote_path.rsplit('/').next().unwrap_or(remote_path);
                let stem = file_name.strip_suffix(".yaml").unwrap_or(file_name);
                let content_bytes = run_adb(adb, Some(serial), &["exec-out", "cat", remote_path])?;
                let content = String::from_utf8(content_bytes)
                    .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
                collected.push((stem.to_string(), content));
            }
            if !collected.is_empty() {
                return Ok(collected);
            }
        }
    }

    Err("Nenhuma persona .yaml encontrada na Segunda Mente do aparelho conectado.".into())
}

pub fn sync_from_adb_in(dir: &Path) -> Result<PersonasState, String> {
    let adb = find_adb_executable();
    let devices = list_adb_devices(&adb)?;
    if devices.is_empty() {
        return Err("Nenhum celular Android conectado via ADB (verifique a depuração USB).".into());
    }

    let mut last_err = String::new();
    let mut raw_files = Vec::new();
    for serial in &devices {
        match collect_yaml_files_from_device(&adb, serial) {
            Ok(files) if !files.is_empty() => {
                raw_files = files;
                break;
            }
            Ok(_) => {}
            Err(err) => last_err = err,
        }
    }
    if raw_files.is_empty() {
        return Err(if last_err.is_empty() {
            "Nenhuma persona encontrada no celular conectado.".into()
        } else {
            last_err
        });
    }

    let existing: Vec<PersonaProfile> = std::fs::read(dir.join("personas.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();

    let mut imported = Vec::with_capacity(raw_files.len());
    for (stem, yaml) in raw_files {
        let mut profile = parse_yaml_character_with_id(&yaml, &stem)
            .map_err(|e| format!("Falha ao interpretar persona `{stem}.yaml`: {e}"))?;
        if let Some(prev) = existing.iter().find(|p| normalize_legacy_id_match(&p.id, &profile.id)) {
            if !profile.is_primary_companion && prev.is_primary_companion {
                profile.is_primary_companion = true;
            }
            if profile.user_profile.is_empty() && !prev.user_profile.is_empty() {
                profile.user_profile = prev.user_profile.clone();
            }
        }
        imported.push(profile);
    }

    // Remove eventuais aliases hifenizados antigos antes de mesclar os IDs oficiais com underscore.
    let cleaned_existing: Vec<PersonaProfile> = existing
        .into_iter()
        .filter(|old| {
            !imported
                .iter()
                .any(|imp| !old.id.eq_ignore_ascii_case(&imp.id) && normalize_legacy_id_match(&old.id, &imp.id))
        })
        .collect();

    let mut merged = merge_imported(cleaned_existing, imported);
    ensure_single_primary(&mut merged);

    write_personas_file(dir, &merged)?;
    write_seed_version(dir, CURRENT_SEED_VERSION)?;
    Ok(PersonasState {
        personas: merged,
        settings: load_comment_settings_in(dir),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_persona(id: &str, name: &str, primary: bool, tags: &[&str], filter: Option<&str>) -> PersonaProfile {
        PersonaProfile {
            id: id.into(),
            name: name.into(),
            role: Some("revisora sintética".into()),
            identity: "Tom calmo, objetivo e colaborativo.".into(),
            user_profile: "Pessoa fictícia de teste organizando notas técnicas.".into(),
            rules: "Comente em até 2 frases, sem bajulação.".into(),
            persona: String::new(),
            system_prompt_override: None,
            post_history_instructions: None,
            mes_example: None,
            tags: tags.iter().map(|s| s.to_string()).collect(),
            interest_filter: filter.map(str::to_string),
            avatar: None,
            first_message: None,
            is_primary_companion: primary,
            enabled: true,
            color: "#6C5CE7".into(),
        }
    }

    fn temp_personas_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lumen-personas-{tag}-{}-{}",
            std::process::id(),
            crate::flow::store::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn imports_qwenpaw_three_part_and_legacy_formats() {
        let json = r#"[
          {
            "id": "clara",
            "name": "Clara",
            "role": "Estrategista",
            "identity": "Direta e prática.",
            "userProfile": "Teste sintético.",
            "rules": "Foque no próximo passo.",
            "tags": ["produto", "arquitetura"],
            "interestFilter": "arquitetura, roadmap. Ignorar: fofoca",
            "isPrimaryCompanion": true
          },
          {
            "name": "Dante",
            "persona": "Questionador gentil.",
            "tags": ["diario"]
          }
        ]"#;
        let parsed = parse_import_json(json).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].identity, "Direta e prática.");
        assert_eq!(parsed[0].user_profile, "Teste sintético.");
        assert_eq!(parsed[0].rules, "Foque no próximo passo.");
        assert!(parsed[0].is_primary_companion);
        assert_eq!(parsed[1].id, "dante");
        assert_eq!(parsed[1].persona, "Questionador gentil.");
    }

    #[test]
    fn mention_overrides_disabled_auto_comments_and_ignores_emails() {
        let a = synthetic_persona("clara", "Clara", true, &["produto"], None);
        let b = synthetic_persona("dante", "Dante", false, &["codigo"], None);
        let active = vec![&a, &b];
        assert_eq!(extract_mention("envie para dev@clara.com depois", &active).map(|p| p.id.as_str()), None);
        assert_eq!(extract_mention("Ei @Dante, o que acha?", &active).map(|p| p.id.as_str()), Some("dante"));

        let rt = tokio::runtime::Runtime::new().unwrap();
        let disabled = CommentSettings { enable_character_comment: false, max_comment_characters: 2 };
        let auto_res = rt.block_on(select_characters_with(
            &active,
            "Nota sem menção",
            "Nota sem menção",
            &[],
            None,
            &disabled,
            |_, _| async { Ok("clara".into()) },
        ));
        assert!(auto_res.is_empty());

        let mention_res = rt.block_on(select_characters_with(
            &active,
            "Nota com @clara",
            "Nota com @clara",
            &[],
            None,
            &disabled,
            |_, _| async { Ok(String::new()) },
        ));
        assert_eq!(mention_res.len(), 1);
        assert_eq!(mention_res[0].persona.id, "clara");
        assert!(mention_res[0].force_reply);
    }

    #[test]
    fn multi_character_uses_llm_router_and_falls_back_on_error() {
        let a = synthetic_persona("clara", "Clara", true, &["produto"], Some("produto, backlog"));
        let b = synthetic_persona("dante", "Dante", false, &["codigo"], Some("codigo, rust"));
        let active = vec![&a, &b];
        let cfg = CommentSettings { enable_character_comment: true, max_comment_characters: 2 };
        let rt = tokio::runtime::Runtime::new().unwrap();

        let routed = rt.block_on(select_characters_with(
            &active,
            "Preciso revisar código em rust.",
            "Preciso revisar código em rust.",
            &[],
            None,
            &cfg,
            |_, _| async { Ok("dante, clara".into()) },
        ));
        assert_eq!(routed.iter().map(|r| r.persona.id.as_str()).collect::<Vec<_>>(), vec!["dante", "clara"]);

        let fallback = rt.block_on(select_characters_with(
            &active,
            "Ideia sobre codigo e rust",
            "Ideia sobre codigo e rust",
            &[],
            None,
            &cfg,
            |_, _| async { Err("offline".into()) },
        ));
        assert_eq!(fallback.len(), 2);
        assert_eq!(fallback[0].persona.id, "dante");

        // Quando o roteador LLM responde vazio e não há match claro de afinidade, respeita o silêncio.
        let empty_silent = rt.block_on(select_characters_with(
            &active,
            "Anotação neutra sem relação",
            "Anotação neutra sem relação",
            &[],
            None,
            &cfg,
            |_, _| async { Ok(String::new()) },
        ));
        assert!(empty_silent.is_empty());
    }

    #[test]
    fn reply_routing_and_turn_cap_work_as_expected() {
        let a = synthetic_persona("clara", "Clara", true, &[], None);
        let b = synthetic_persona("dante", "Dante", false, &[], None);
        let active = vec![&a, &b];
        let comments = vec![
            NoteComment {
                id: "c1".into(),
                character_id: "dante".into(),
                character_name: "Dante".into(),
                author_kind: "persona".into(),
                content: "Primeira resposta".into(),
                reply_to_id: None,
                created_at: 1,
            },
            NoteComment {
                id: "c2".into(),
                character_id: "user".into(),
                character_name: "Você".into(),
                author_kind: "user".into(),
                content: "Faz sentido!".into(),
                reply_to_id: Some("c1".into()),
                created_at: 2,
            },
        ];
        assert_eq!(
            resolve_reply_target_character(&comments, Some("c2"), &active).map(|p| p.id.as_str()),
            Some("dante")
        );
        assert_eq!(remaining_persona_turns(&comments, MAX_PERSONA_TURNS_PER_USER_MESSAGE), MAX_PERSONA_TURNS_PER_USER_MESSAGE);
    }

    #[test]
    fn store_roundtrip_and_skip_parsing() {
        let dir = temp_personas_dir("roundtrip");
        let a = synthetic_persona("clara", "Clara", true, &["notas"], None);
        save_personas_in(&dir, &[a.clone()]).unwrap();
        save_comment_settings_in(&dir, &CommentSettings { enable_character_comment: false, max_comment_characters: 25 }).unwrap();

        let state = load_state_in(&dir);
        assert_eq!(state.personas, vec![a]);
        assert!(!state.settings.enable_character_comment);
        assert_eq!(state.settings.max_comment_characters, MAX_COMMENT_CHARACTERS_LIMIT);
        assert_eq!(parse_comment_output("[SKIP]", false), None);
        assert_eq!(parse_comment_output("[SKIP]", true).as_deref(), Some("[SKIP]"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn seeds_all_15_official_personas_and_preserves_underscores() {
        let dir = temp_personas_dir("seed");
        let seeded = load_personas_in(&dir);
        assert_eq!(seeded.len(), 15);
        assert_eq!(
            std::fs::read_to_string(dir.join(SEED_VERSION_FILE)).unwrap().trim(),
            CURRENT_SEED_VERSION.to_string()
        );
        assert_eq!(slugify_id("socio_tecnico"), "socio_tecnico");
        assert_eq!(slugify_id("_revisor_chato_"), "revisor_chato");

        let colors: HashSet<&str> = seeded.iter().map(|p| p.color.as_str()).collect();
        assert_eq!(colors.len(), 15);
        assert_eq!(seeded.iter().filter(|p| p.is_primary_companion).count(), 1);
        assert_eq!(seeded[0].id, "socio_tecnico");
        assert!(seeded[0].is_primary_companion);
        assert_eq!(seeded[0].avatar.as_deref(), Some("asset://characters/socio_tecnico.png"));
        assert!(seeded[0].first_message.is_some());
        assert!(seeded[0].post_history_instructions.is_some());
        assert!(seeded[0].mes_example.is_some());
        assert!(seeded[0].interest_filter.is_some());
        assert!(seeded[0].persona.contains("## Política de comentários"));
        assert!(seeded[0].persona.contains("## Voice"));
        assert!(!seeded[0].rules.is_empty());
        assert!(!seeded[0].identity.contains("## Política de comentários"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_destructive_seed_upgrade_and_reset_keep_custom_personas() {
        let dir = temp_personas_dir("upgrade");
        let legacy_socio = PersonaProfile {
            user_profile: "Perfil de trabalho sintético".into(),
            enabled: false,
            is_primary_companion: false,
            ..synthetic_persona("socio-tecnico", "Sócio técnico", false, &[], None)
        };
        let custom = synthetic_persona("mentor_sintetico", "Mentor Sintético", true, &["custom"], None);
        write_personas_file(&dir, &[legacy_socio, custom.clone()]).unwrap();
        write_seed_version(&dir, 4).unwrap();

        let upgraded = load_personas_in(&dir);
        assert_eq!(upgraded.len(), 16);
        assert_eq!(
            std::fs::read_to_string(dir.join(SEED_VERSION_FILE)).unwrap().trim(),
            "6"
        );
        let socio = upgraded.iter().find(|p| p.id == "socio_tecnico").unwrap();
        assert!(!socio.enabled);
        assert!(!socio.is_primary_companion);
        assert_eq!(socio.user_profile, "Perfil de trabalho sintético");
        assert!(upgraded.iter().all(|p| p.id != "socio-tecnico"));
        assert!(upgraded.iter().any(|p| p.id == "mentor_sintetico" && p.is_primary_companion));

        let reset = reset_default_personas_in(&dir).unwrap();
        assert_eq!(reset.personas.len(), 16);
        let socio_reset = reset.personas.iter().find(|p| p.id == "socio_tecnico").unwrap();
        assert!(socio_reset.enabled);
        assert!(reset.personas.iter().any(|p| p.id == "mentor_sintetico"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_segundamente_yaml_multidoc_and_tavern_v2() {
        let yaml = r#"# Export Segunda Mente
name: Sócio técnico
tags:
  - produto
  - arquitetura
  - decisão
persona: "É um colega de produto.\n\n## Política de comentários\nResponda quando:\n- Decisão.\n\nPule quando:\n- Compra.\n\n## Voice\n1. Direto."
avatar: "asset://characters/socio_tecnico.png"
enabled: true
interest_filter: Foque em decisões de produto, arquitetura, escolha de tecnologia. Ignore compras e saúde.
first_message: Fala. O que está na mesa hoje?
post_history_instructions: "Responda como sócio técnico."
mes_example: "Usuário: Dúvida\nSócio técnico: Fica local."
"#;
        let parsed = parse_yaml_character_with_id(yaml, "socio_tecnico").unwrap();
        assert_eq!(parsed.id, "socio_tecnico");
        assert_eq!(parsed.name, "Sócio técnico");
        assert_eq!(parsed.tags, vec!["produto", "arquitetura", "decisão"]);
        assert_eq!(parsed.avatar.as_deref(), Some("asset://characters/socio_tecnico.png"));
        assert_eq!(parsed.first_message.as_deref(), Some("Fala. O que está na mesa hoje?"));
        assert!(parsed.rules.contains("Responda quando:"));
        assert!(!parsed.identity.contains("## Política de comentários"));
        assert!(parsed.identity.contains("## Voice"));

        let multi_yaml = r#"---
name: Primeira
tags: [a, b]
persona: |
  Linha um
  Linha dois
---
name: Segunda
tags: []
persona: 'Texto com ''aspas'' e # hashtag'
enabled: false
"#;
        let multi = parse_import_json(multi_yaml).unwrap();
        assert_eq!(multi.len(), 2);
        assert_eq!(multi[0].tags, vec!["a", "b"]);
        assert!(multi[0].persona.contains("Linha um\nLinha dois"));
        assert!(!multi[1].enabled);
        assert_eq!(multi[1].persona, "Texto com 'aspas' e # hashtag");

        let tavern_v2 = r#"{
          "spec": "chara_card_v2",
          "spec_version": "2.0",
          "data": {
            "name": "Ada Teste",
            "description": "Engenheira prática.",
            "personality": "Objetiva.",
            "scenario": "Revisando notas com {{user}}.",
            "first_mes": "Olá {{user}}, sou {{char}}.",
            "mes_example": "{{user}}: Oi\n{{char}}: Oi!",
            "system_prompt": "Ajude {{user}} como {{char}}.",
            "post_history_instructions": "Lembre {{user}} do foco.",
            "tags": ["engenharia"]
          }
        }"#;
        let tavern = parse_import_json(tavern_v2).unwrap();
        assert_eq!(tavern.len(), 1);
        assert_eq!(tavern[0].first_message.as_deref(), Some("Olá {{user}}, sou {{char}}."));
        assert_eq!(tavern[0].system_prompt_override.as_deref(), Some("Ajude {{user}} como {{char}}."));
        assert!(tavern[0].persona.contains("## Personality"));
    }

    #[test]
    fn affinity_routing_policy_and_tavern_macros_work_for_pt_br_catalog() {
        let catalog = default_personas();
        let refs: Vec<&PersonaProfile> = catalog.iter().collect();

        let sleep_winner = select_single_character(
            &refs,
            "Dormi apenas quatro horas e estou com muito cansaço hoje.",
            &[],
            false,
        )
        .unwrap();
        assert_eq!(sleep_winner.id, "guardiao_energia");

        let arch_winner = select_single_character(
            &refs,
            "Decisão de arquitetura: vale escolher tecnologia local pelos trade-offs?",
            &[],
            false,
        )
        .unwrap();
        assert_eq!(arch_winner.id, "socio_tecnico");

        let emotion_winner = select_single_character(
            &refs,
            "Estou sentindo muita tristeza e saudade depois daquela conversa.",
            &[],
            false,
        )
        .unwrap();
        assert_eq!(emotion_winner.id, "escuta_atenta");

        let socio = catalog.iter().find(|p| p.id == "socio_tecnico").unwrap();
        let policy = extract_comment_policy(socio);
        assert!(policy.contains("Responda quando:"));
        assert!(policy.contains("Pule quando:"));
        let (sys_route, _) = build_routing_prompt(&refs, "Qual banco escolher?", 2);
        assert!(sys_route.contains("string vazia"));

        let mut custom = synthetic_persona("ada", "Ada", false, &[], None);
        custom.identity = "Ajude {{USER}} a revisar o texto de {{Char}}.".into();
        custom.post_history_instructions = Some("Fale direto com {{uSeR}} como {{CHAR}}.".into());
        let (sys_prompt, usr_prompt) = build_comment_prompt(&custom, "Nota", "Corpo", None, None, &[], false);
        assert!(sys_prompt.contains("Ajude Usuário a revisar o texto de Ada."));
        assert!(usr_prompt.contains("Fale direto com Usuário como Ada."));
    }

    #[test]
    fn first_word_mentions_and_flexible_router_parsing_work() {
        let catalog = default_personas();
        let refs: Vec<&PersonaProfile> = catalog.iter().collect();

        assert_eq!(
            extract_mention("Concordo, @Sócio faz sentido?", &refs).map(|p| p.id.as_str()),
            Some("socio_tecnico")
        );
        assert_eq!(
            extract_mention("@advogado o que pode dar errado?", &refs).map(|p| p.id.as_str()),
            Some("advogado_diabo")
        );

        let dup1 = synthetic_persona("ana_clara", "Ana Clara", true, &[], None);
        let dup2 = synthetic_persona("ana_paula", "Ana Paula", false, &[], None);
        let dups = vec![&dup1, &dup2];
        assert_eq!(extract_mention("Oi @Ana", &dups), None);
        assert_eq!(
            extract_mention("Oi @Ana Paula", &dups).map(|p| p.id.as_str()),
            Some("ana_paula")
        );

        let numbered = parse_ranked_character_ids("1. Sócio técnico\n2. Advogado do diabo", &refs, 2);
        assert_eq!(numbered, vec!["socio_tecnico", "advogado_diabo"]);

        let joined = parse_ranked_character_ids("Socio tecnico e Revisor chato", &refs, 2);
        assert_eq!(joined, vec!["socio_tecnico", "revisor_chato"]);
    }

    #[test]
    fn multi_persona_affinity_fallback_and_open_user_reply_never_drop() {
        let a = synthetic_persona("clara", "Clara", true, &["produto"], Some("produto, backlog"));
        let b = synthetic_persona("dante", "Dante", false, &["codigo"], Some("codigo, rust"));
        let active = vec![&a, &b];
        let cfg = CommentSettings { enable_character_comment: true, max_comment_characters: 2 };
        let rt = tokio::runtime::Runtime::new().unwrap();

        // Quando o roteador retorna vazio mas múltiplas personas têm afinidade positiva, ambas respondem até o limite.
        let both = rt.block_on(select_characters_with(
            &active,
            "Decisão de produto e código em rust",
            "Decisão de produto e código em rust",
            &[],
            None,
            &cfg,
            |_, _| async { Ok(String::new()) },
        ));
        assert_eq!(both.len(), 2);

        // Comentário aberto do usuário (sem @mention nem reply_to_id) recebe resposta forçada mesmo sem match rígido do roteador.
        let comments = vec![NoteComment {
            id: "c1".into(),
            character_id: "dante".into(),
            character_name: "Dante".into(),
            author_kind: "persona".into(),
            content: "Confira os testes.".into(),
            reply_to_id: None,
            created_at: 1,
        }];
        let reply = rt.block_on(select_reply_characters_with(
            &active,
            "Nota sobre o projeto",
            "Beleza, mas por onde começo agora?",
            &comments,
            None,
            &cfg,
            |_, _| async { Ok(String::new()) },
        ));
        assert_eq!(reply.len(), 1);
        assert_eq!(reply[0].persona.id, "dante");
        assert!(reply[0].force_reply);

        // Quando automáticos estão desligados e ainda não há fio de personas nem menção/reply_to_id, mantém silêncio.
        let disabled = CommentSettings { enable_character_comment: false, max_comment_characters: 2 };
        let no_history_silent = rt.block_on(select_reply_characters_with(
            &active,
            "Nota",
            "Observação minha",
            &[],
            None,
            &disabled,
            |_, _| async { Ok("clara".into()) },
        ));
        assert!(no_history_silent.is_empty());

        // Se já havia comentário de persona na nota, continuar respondendo mesmo com auto-comentário desligado.
        let with_history_reply = rt.block_on(select_reply_characters_with(
            &active,
            "Nota",
            "E como testo isso?",
            &comments,
            None,
            &disabled,
            |_, _| async { Ok("dante".into()) },
        ));
        assert_eq!(with_history_reply.len(), 1);
        assert_eq!(with_history_reply[0].persona.id, "dante");
        assert!(with_history_reply[0].force_reply);
    }

    #[test]
    fn living_agents_md_memory_and_group_chat_threading_work() {
        let dir = temp_personas_dir("living_agents");
        let personas = load_personas_in(&dir);
        assert_eq!(personas.len(), 15);
        assert_eq!(CommentSettings::default().max_comment_characters, 15);

        // AGENTS.md vivo é criado para cada persona e edições locais são preservadas no load.
        let socio = personas.iter().find(|p| p.id == "socio_tecnico").unwrap();
        let agents_md = agent_dir_in(&dir, &socio.id).join("AGENTS.md");
        assert!(agents_md.is_file());
        std::fs::write(&agents_md, "# Sócio técnico vivo\nPrefira arquiteturas locais simples.").unwrap();
        let _reloaded = load_personas_in(&dir);
        assert!(std::fs::read_to_string(&agents_md).unwrap().contains("Sócio técnico vivo"));

        append_persona_memory_in(&dir, &socio.id, "Usuário prefere soluções offline-first.").unwrap();
        append_persona_memory_in(&dir, &socio.id, "Usuário prefere soluções offline-first.").unwrap();
        let refs: Vec<&PersonaProfile> = personas.iter().collect();
        let (sys, usr) = build_comment_prompt_live(
            &dir,
            socio,
            &refs,
            "Sync",
            "Pensando em manter tudo local.",
            None,
            None,
            &[],
            false,
            None,
        );
        assert!(sys.contains("Sócio técnico vivo"));
        assert!(sys.contains("soluções offline-first"));
        assert!(usr.contains("MEMORY:"));

        // Até 15 personas podem ser selecionadas pelo roteador quando pertinente.
        let all_ids = personas.iter().map(|p| p.id.as_str()).collect::<Vec<_>>().join(", ");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let selected_15 = rt.block_on(select_characters_with(
            &refs,
            "Grande nota estratégica envolvendo saúde, escopo, prazo e revisão",
            "Grande nota estratégica",
            &[],
            None,
            &CommentSettings::default(),
            |_, _| async { Ok(all_ids) },
        ));
        assert_eq!(selected_15.len(), 15);

        // Parseia [REPLY:id], [MEMORY: ...] e @menções entre personas.
        let thread = vec![
            NoteComment {
                id: "c1".into(),
                character_id: "socio_tecnico".into(),
                character_name: "Sócio técnico".into(),
                author_kind: "persona".into(),
                content: "Vamos salvar local.".into(),
                reply_to_id: None,
                created_at: 1,
            },
            NoteComment {
                id: "c2".into(),
                character_id: "advogado_diabo".into(),
                character_name: "Advogado do diabo".into(),
                author_kind: "persona".into(),
                content: "E se o disco falhar?".into(),
                reply_to_id: Some("c1".into()),
                created_at: 2,
            },
        ];
        let parsed = parse_group_comment_output(
            "[REPLY:c2] Faz sentido testar backup automático antes.\n[MEMORY: Usuário valoriza backup automático]",
            &thread,
            &refs,
            false,
        )
        .unwrap();
        assert_eq!(parsed.reply_to_id.as_deref(), Some("c2"));
        assert_eq!(parsed.content, "Faz sentido testar backup automático antes.");
        assert_eq!(parsed.memory_note.as_deref(), Some("Usuário valoriza backup automático"));

        // Sem [REPLY:...], @menção a uma persona do fio conecta o fio automaticamente.
        let mention_thread = parse_group_comment_output(
            "@Advogado concordo com o risco, vamos validar.",
            &thread,
            &refs,
            false,
        )
        .unwrap();
        assert_eq!(mention_thread.reply_to_id.as_deref(), Some("c2"));

        let multi_mentions = extract_all_mentions("Chamando @socio e @revisor para opinarem.", &refs);
        assert_eq!(
            multi_mentions.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["socio_tecnico", "revisor_chato"]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "Requer aparelho Android conectado via ADB com Segunda Mente instalada"]
    fn real_adb_sync_populates_local_personas_json() {
        let dir = store_dir();
        let state = sync_from_adb_in(&dir).expect("Sincronização ADB real falhou");
        assert!(
            state.personas.len() >= 15,
            "Esperava pelo menos 15 personas, veio {}",
            state.personas.len()
        );
        for p in &state.personas {
            assert!(!p.id.is_empty());
            assert!(!p.name.is_empty());
        }
        println!("ADB_SYNC_OK count={} path={}", state.personas.len(), dir.join("personas.json").display());
    }
}

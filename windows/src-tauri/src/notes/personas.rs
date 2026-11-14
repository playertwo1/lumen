// Personas locais (formato 3 partes inspiradas no QwenPaw + compatível com Segunda Mente)
// e regras de seleção/comentários em notas e ideias do Anotador.

use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DEFAULT_MAX_COMMENT_CHARACTERS: usize = 2;
pub const MAX_PERSONA_TURNS_PER_USER_MESSAGE: usize = 4;
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

pub fn load_personas() -> Vec<PersonaProfile> {
    load_personas_in(&store_dir())
}

pub fn load_personas_in(dir: &Path) -> Vec<PersonaProfile> {
    let Ok(bytes) = std::fs::read(dir.join("personas.json")) else {
        return Vec::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub fn save_personas(personas: &[PersonaProfile]) -> Result<(), String> {
    save_personas_in(&store_dir(), personas)
}

pub fn save_personas_in(dir: &Path, personas: &[PersonaProfile]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(personas).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("personas.json"), json).map_err(|e| e.to_string())
}

pub fn load_comment_settings_in(dir: &Path) -> CommentSettings {
    let Ok(bytes) = std::fs::read(dir.join("comment_settings.json")) else {
        return CommentSettings::default();
    };
    let mut cfg: CommentSettings = serde_json::from_slice(&bytes).unwrap_or_default();
    cfg.max_comment_characters = cfg.max_comment_characters.clamp(1, 6);
    cfg
}

pub fn save_comment_settings(mut settings: CommentSettings) -> Result<CommentSettings, String> {
    settings.max_comment_characters = settings.max_comment_characters.clamp(1, 6);
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
            item.color = DEFAULT_COLORS[idx % DEFAULT_COLORS.len()].to_string();
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
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Cole o JSON da persona antes de importar.".into());
    }
    let val: Value = serde_json::from_str(trimmed)
        .map_err(|e| format!("JSON de persona inválido: {e}"))?;
    let items: Vec<&Value> = match &val {
        Value::Array(arr) => arr.iter().collect(),
        Value::Object(map) => {
            if let Some(Value::Array(arr)) = map.get("personas").or_else(|| map.get("characters")) {
                arr.iter().collect()
            } else {
                vec![&val]
            }
        }
        _ => return Err("O JSON deve ser um objeto de persona ou uma lista de personas.".into()),
    };
    if items.is_empty() {
        return Err("Nenhuma persona encontrada no JSON.".into());
    }
    let mut parsed = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        parsed.push(parse_one_persona(item, i)?);
    }
    Ok(parsed)
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

fn slugify_id(name: &str) -> String {
    let folded = fold_pt_ascii(name);
    let mut out = String::with_capacity(folded.len());
    let mut prev_dash = false;
    for ch in folded.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn parse_one_persona(val: &Value, index: usize) -> Result<PersonaProfile, String> {
    let name = get_str(val, &["name", "nome"])
        .ok_or_else(|| format!("Persona #{} está sem o campo `name`.", index + 1))?
        .to_string();
    let id = get_str(val, &["id"])
        .map(slugify_id)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| slugify_id(&name));
    if id.is_empty() {
        return Err(format!("Não foi possível gerar um identificador para a persona \"{name}\"."));
    }
    let tags = val
        .get("tags")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let enabled = val
        .get("enabled")
        .or_else(|| val.get("ativa"))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let is_primary_companion = val
        .get("isPrimaryCompanion")
        .or_else(|| val.get("is_primary_companion"))
        .or_else(|| val.get("primary"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let color = get_str(val, &["color", "cor"])
        .map(str::to_string)
        .unwrap_or_else(|| DEFAULT_COLORS[index % DEFAULT_COLORS.len()].to_string());

    Ok(PersonaProfile {
        id,
        name,
        role: get_str(val, &["role", "title", "papel"]).map(str::to_string),
        identity: get_str(val, &["identity", "soul", "SOUL"]).unwrap_or_default().to_string(),
        user_profile: get_str(val, &["userProfile", "user_profile", "profile", "PROFILE"])
            .unwrap_or_default()
            .to_string(),
        rules: get_str(val, &["rules", "agents", "AGENTS"]).unwrap_or_default().to_string(),
        persona: get_str(val, &["persona", "description"]).unwrap_or_default().to_string(),
        system_prompt_override: get_str(val, &["systemPromptOverride", "system_prompt_override"])
            .map(str::to_string),
        post_history_instructions: get_str(
            val,
            &["postHistoryInstructions", "post_history_instructions"],
        )
        .map(str::to_string),
        mes_example: get_str(val, &["mesExample", "mes_example"]).map(str::to_string),
        tags,
        interest_filter: get_str(val, &["interestFilter", "interest_filter"]).map(str::to_string),
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

/// Identifica menção `@id` ou `@nome` com fronteira lexical (ignora e-mails como `a@dominio.com`).
pub fn extract_mention<'a>(text: &str, active: &[&'a PersonaProfile]) -> Option<&'a PersonaProfile> {
    let folded_text = fold_pt_ascii(text);
    for p in active {
        let candidates = [fold_pt_ascii(&p.id), fold_pt_ascii(&p.name)];
        for needle in candidates {
            if needle.is_empty() {
                continue;
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
                    return Some(*p);
                }
                search_from = end;
            }
        }
    }
    None
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

fn positive_interest_keywords(filter: &str) -> Vec<String> {
    let folded = fold_pt_ascii(filter);
    let mut cut = folded.as_str();
    for marker in ["ignore:", "do not focus on", "nao foque em", "evite:", "sem foco em"] {
        if let Some(pos) = cut.find(marker) {
            cut = &cut[..pos];
        }
    }
    cut.split([',', ';', '|', '\n', '.'])
        .map(str::trim)
        .filter(|s| s.len() >= 3)
        .map(str::to_string)
        .collect()
}

fn has_reflective_signal(text: &str) -> bool {
    let folded = fold_pt_ascii(text);
    const SIGNALS: &[&str] = &[
        "cansad", "ansios", "frustrad", "triste", "desabafo", "saude", "dor", "sono",
        "estress", "duvida", "medo", "gratid", "orgulho", "feliz", "reflexao", "sinto",
    ];
    SIGNALS.iter().any(|w| folded.contains(w))
}

pub fn score_affinity(persona: &PersonaProfile, text: &str, tags: &[String]) -> i32 {
    let folded_text = fold_pt_ascii(text);
    let folded_tags: Vec<String> = tags.iter().map(|t| fold_pt_ascii(t)).collect();
    let mut score = 0i32;

    for p_tag in &persona.tags {
        let needle = fold_pt_ascii(p_tag);
        if needle.is_empty() {
            continue;
        }
        if folded_tags.iter().any(|t| t == &needle) || folded_text.contains(&needle) {
            score += 3;
        }
    }

    if let Some(filter) = &persona.interest_filter {
        for kw in positive_interest_keywords(filter) {
            if folded_text.contains(&kw) {
                score += 2;
            }
        }
    }

    if has_reflective_signal(text) {
        let persona_blob = fold_pt_ascii(&format!(
            "{} {} {} {}",
            persona.role.as_deref().unwrap_or(""),
            persona.identity,
            persona.rules,
            persona.tags.join(" ")
        ));
        if ["cuidado", "bem-estar", "emoc", "saude", "acolh", "reflex", "diario", "compan"]
            .iter()
            .any(|k| persona_blob.contains(k))
        {
            score += 4;
        }
    }

    score
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
    let mut scored: Vec<(&PersonaProfile, i32)> = active
        .iter()
        .map(|p| (*p, score_affinity(p, text, tags)))
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.id.cmp(&b.0.id)));

    if scored.len() == 1 {
        return Some(scored[0].0);
    }
    let (best, best_score) = scored[0];
    let second_score = scored[1].1;
    if best_score > 0 && best_score - second_score >= 2 {
        return Some(best);
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

pub fn parse_ranked_character_ids(raw: &str, active: &[&PersonaProfile], limit: usize) -> Vec<String> {
    let mut chosen = Vec::new();
    for token in raw.split([',', '\n', ';', '|']) {
        let cleaned = token
            .trim()
            .trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
        if cleaned.is_empty() {
            continue;
        }
        if let Some(p) = active
            .iter()
            .find(|p| p.id.eq_ignore_ascii_case(cleaned) || p.name.eq_ignore_ascii_case(cleaned))
        {
            if !chosen.iter().any(|id| id == &p.id) {
                chosen.push(p.id.clone());
                if chosen.len() >= limit {
                    break;
                }
            }
        }
    }
    chosen
}

pub fn build_routing_prompt(active: &[&PersonaProfile], note_text: &str, max_chars: usize) -> (String, String) {
    let mut list = String::new();
    for p in active {
        let role = p.role.as_deref().unwrap_or("companheira");
        let tags = if p.tags.is_empty() { "-".to_string() } else { p.tags.join(", ") };
        let focus = p.interest_filter.as_deref().unwrap_or("-");
        list.push_str(&format!("- id: {} | nome: {} | papel: {} | tags: {} | foco: {}\n", p.id, p.name, role, tags, focus));
    }
    let system = "Você decide quais personas têm algo genuíno para comentar sobre uma anotação do usuário. Responda SOMENTE com os `id`s ordenados por relevância separados por vírgula, no máximo o limite pedido.".to_string();
    let user = format!(
        "Personas ativas:\n{list}\nLimite máximo: {max_chars}\n\nAnotação:\n{}\n\nResponda apenas os ids separados por vírgula:",
        truncate_chars(note_text, 1200)
    );
    (system, user)
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
    if active.len() == 1 || limit == 1 {
        return select_single_character(active, note_text, &[], false)
            .map(|p| vec![SelectionResult { persona: p, force_reply: false }])
            .unwrap_or_default();
    }

    let (sys, usr) = build_routing_prompt(active, note_text, limit);
    if let Ok(raw) = llm_route(sys, usr).await {
        let ids = parse_ranked_character_ids(&raw, active, limit);
        if !ids.is_empty() {
            return ids
                .into_iter()
                .filter_map(|id| active.iter().copied().find(|p| p.id == id))
                .map(|persona| SelectionResult { persona, force_reply: false })
                .collect();
        }
    }

    let mut ranked: Vec<&PersonaProfile> = active.to_vec();
    ranked.sort_by(|a, b| {
        score_affinity(b, note_text, &[])
            .cmp(&score_affinity(a, note_text, &[]))
            .then_with(|| b.is_primary_companion.cmp(&a.is_primary_companion))
            .then_with(|| a.id.cmp(&b.id))
    });
    ranked
        .into_iter()
        .take(limit)
        .map(|persona| SelectionResult { persona, force_reply: false })
        .collect()
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
    let mut system = String::new();
    if let Some(override_prompt) = &persona.system_prompt_override {
        system.push_str(override_prompt);
    } else {
        system.push_str(&format!("Você é {}", persona.name));
        if let Some(role) = &persona.role {
            system.push_str(&format!(" ({role})"));
        }
        system.push_str(". Responda em português do Brasil.\n");
        if !persona.identity.trim().is_empty() {
            system.push_str("\n[Identidade e voz]\n");
            system.push_str(persona.identity.trim());
            system.push('\n');
        }
        if !persona.user_profile.trim().is_empty() {
            system.push_str("\n[Sobre quem anota]\n");
            system.push_str(persona.user_profile.trim());
            system.push('\n');
        }
        if !persona.rules.trim().is_empty() {
            system.push_str("\n[Regras de resposta]\n");
            system.push_str(persona.rules.trim());
            system.push('\n');
        }
        if !persona.persona.trim().is_empty() {
            system.push_str("\n[Definição adicional]\n");
            system.push_str(persona.persona.trim());
            system.push('\n');
        }
    }
    if let Some(example) = &persona.mes_example {
        system.push_str("\n[Exemplo de estilo]\n");
        system.push_str(example.trim());
        system.push('\n');
    }

    let mut user = format!(
        "<note_title>\n{}\n</note_title>\n\n<note_content>\n{}\n</note_content>\n",
        note_title.trim(),
        truncate_chars(note_body.trim(), 2400)
    );
    if let Some(sum) = summary.filter(|s| !s.trim().is_empty()) {
        user.push_str(&format!("\n<note_summary>\n{}\n</note_summary>\n", sum.trim()));
    }
    if !existing_comments.is_empty() {
        user.push_str("\n<existing_comments>\n");
        for c in existing_comments {
            user.push_str(&format!("- {}: {}\n", c.character_name, c.content.trim()));
        }
        user.push_str("</existing_comments>\n");
    }
    if let Some(reply) = trigger_user_comment.filter(|s| !s.trim().is_empty()) {
        user.push_str(&format!(
            "\nO usuário respondeu agora no tópico:\n\"{}\"\nResponda de forma direta e natural a essa fala.\n",
            reply.trim()
        ));
    }
    if let Some(post) = &persona.post_history_instructions {
        user.push_str("\n");
        user.push_str(post.trim());
        user.push('\n');
    }
    if force_reply {
        user.push_str("\nEscreva um comentário curto (1 a 3 frases), direto e útil no seu estilo próprio.");
    } else {
        user.push_str("\nEscreva um comentário curto (1 a 3 frases) acrescentando algo concreto sem repetir comentários já feitos. Se realmente não houver nada útil ou pertinente a dizer, responda exatamente `[SKIP]`.");
    }
    (system, user)
}

pub fn parse_comment_output(raw: &str, force_reply: bool) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if !force_reply && (trimmed == "[SKIP]" || trimmed.eq_ignore_ascii_case("skip") || trimmed.contains("[SKIP]")) {
        return None;
    }
    Some(trimmed.to_string())
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
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
            is_primary_companion: primary,
            enabled: true,
            color: "#6C5CE7".into(),
        }
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
        assert_eq!(remaining_persona_turns(&comments, MAX_PERSONA_TURNS_PER_USER_MESSAGE), 4);
    }

    #[test]
    fn store_roundtrip_and_skip_parsing() {
        let dir = std::env::temp_dir().join(format!("lumen-personas-test-{}", crate::flow::store::now_ms()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = synthetic_persona("clara", "Clara", true, &["notas"], None);
        save_personas_in(&dir, &[a.clone()]).unwrap();
        save_comment_settings_in(&dir, &CommentSettings { enable_character_comment: false, max_comment_characters: 9 }).unwrap();

        let state = load_state_in(&dir);
        assert_eq!(state.personas, vec![a]);
        assert!(!state.settings.enable_character_comment);
        assert_eq!(state.settings.max_comment_characters, 6);
        assert_eq!(parse_comment_output("[SKIP]", false), None);
        assert_eq!(parse_comment_output("[SKIP]", true).as_deref(), Some("[SKIP]"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

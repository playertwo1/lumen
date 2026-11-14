//! Proposta de tarefa por IA ou fallback conservador (L4.3 / F11-05).
//! Regra central: data ou hora ambígua nunca é inferida sem confirmação explícita do dono.

use std::future::Future;

use serde::Deserialize;

use super::model::{
    civil_from_days, days_from_civil, fold_pt, is_valid_date, is_valid_time, local_parts_at,
    TodoPriority, TodoProposal, TodoProposalDateState, TodoProposalTimeState, TODO_TIME_ZONE,
};

#[derive(Debug, Deserialize)]
struct RawProposalJson {
    #[serde(default)]
    title: String,
    #[serde(default)]
    date_state: Option<String>,
    #[serde(default)]
    due_date: Option<String>,
    #[serde(default)]
    date_phrase: Option<String>,
    #[serde(default)]
    time_state: Option<String>,
    #[serde(default)]
    due_time: Option<String>,
    #[serde(default)]
    time_phrase: Option<String>,
    #[serde(default)]
    priority: Option<String>,
}

pub fn build_proposal_prompt(input: &str, now_ms: i64, time_zone: &str) -> (String, String) {
    let (y, m, d, hh, mm) = local_parts_at(now_ms, time_zone);
    let system = format!(
        "Você extrai UMA proposta de tarefa em JSON estrito (sem texto fora do JSON) a partir da fala ou frase do dono.\n\
Data/hora local atual ({time_zone}): {y:04}-{m:02}-{d:02} {hh:02}:{mm:02}.\n\
Regras obrigatórias:\n\
1. Nunca grave nem suponha tarefa confirmada; você só prepara o cartão de confirmação.\n\
2. Nunca infira data ou hora ambígua: dia da semana sem data (ex.: \"sexta\"), \"semana que vem\", \"depois\" -> date_state=\"ambiguous\", preserve date_phrase e OMITA due_date.\n\
3. Só use date_state=\"explicit\" quando a data exata YYYY-MM-DD estiver clara no texto (ex.: \"hoje\", \"amanhã\", \"18/11\"). Sem menção de dia -> date_state=\"unspecified\".\n\
4. Só use time_state=\"explicit\" (HH:mm) quando houver hora exata e data explícita. Frases vagas (\"de manhã\", \"à tarde\", \"à noite\", \"mais tarde\") -> time_state=\"ambiguous\" preservando time_phrase e OMITINDO due_time. Sem menção -> \"unspecified\".\n\
5. Mantenha o título fiel em pt-BR, curto e limpo (sem as palavras soltas do comando tipo \"me lembra de\").\n\
Formato exato:\n\
{{\"title\":\"...\",\"date_state\":\"unspecified|explicit|ambiguous\",\"due_date\":\"YYYY-MM-DD ou null\",\"date_phrase\":\"... ou null\",\"time_state\":\"unspecified|explicit|ambiguous\",\"due_time\":\"HH:mm ou null\",\"time_phrase\":\"... ou null\",\"priority\":\"low|normal|high|urgent\"}}"
    );
    let user = format!("Frase do dono: {}", input.trim());
    (system, user)
}

pub fn parse_llm_proposal(raw: &str, original_input: &str) -> Result<TodoProposal, String> {
    let json_slice = extract_json_object(raw).ok_or_else(|| "Resposta sem JSON de proposta.".to_string())?;
    let parsed: RawProposalJson =
        serde_json::from_str(json_slice).map_err(|e| format!("JSON inválido na proposta: {e}"))?;
    let title = {
        let t = parsed.title.trim();
        if t.is_empty() {
            original_input.trim().to_string()
        } else {
            t.to_string()
        }
    };
    if title.is_empty() {
        return Err("Descreva a tarefa antes de continuar.".into());
    }

    let mut date_state = match parsed.date_state.as_deref() {
        Some("explicit") => TodoProposalDateState::Explicit,
        Some("ambiguous") => TodoProposalDateState::Ambiguous,
        _ => TodoProposalDateState::Unspecified,
    };
    let mut due_date = parsed
        .due_date
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if date_state != TodoProposalDateState::Explicit || !due_date.as_deref().is_some_and(is_valid_date) {
        if due_date.is_some() && date_state == TodoProposalDateState::Explicit {
            date_state = TodoProposalDateState::Ambiguous;
        }
        due_date = None;
    }

    let mut time_state = match parsed.time_state.as_deref() {
        Some("explicit") => TodoProposalTimeState::Explicit,
        Some("ambiguous") => TodoProposalTimeState::Ambiguous,
        _ => TodoProposalTimeState::Unspecified,
    };
    let mut due_time = parsed
        .due_time
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if time_state != TodoProposalTimeState::Explicit
        || date_state == TodoProposalDateState::Unspecified
        || !due_time.as_deref().is_some_and(is_valid_time)
    {
        if time_state == TodoProposalTimeState::Explicit && due_time.as_deref().is_some_and(is_valid_time) {
            time_state = TodoProposalTimeState::Ambiguous;
        } else if time_state == TodoProposalTimeState::Explicit {
            time_state = TodoProposalTimeState::Unspecified;
        }
        due_time = None;
    }

    let priority = match parsed.priority.as_deref() {
        Some("low") => TodoPriority::Low,
        Some("high") => TodoPriority::High,
        Some("urgent") => TodoPriority::Urgent,
        _ => TodoPriority::Normal,
    };

    let proposal = TodoProposal {
        id: crate::flow::text::new_id(),
        title,
        date_state,
        due_date,
        date_phrase: clean_phrase(parsed.date_phrase),
        time_state,
        due_time,
        time_phrase: clean_phrase(parsed.time_phrase),
        priority,
    };
    proposal.validate()?;
    Ok(proposal)
}

/// Fallback determinístico em pt-BR: só fixa `explicit` quando "hoje", "amanhã" ou "HH:mm"
/// são inequívocos; palavras vagas ("sexta", "de manhã") ficam em `ambiguous`.
pub fn fallback_proposal(input: &str, now_ms: i64, time_zone: &str) -> Result<TodoProposal, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Descreva a tarefa antes de continuar.".into());
    }
    let folded = fold_pt(trimmed);
    let (y, m, d, _, _) = local_parts_at(now_ms, time_zone);
    let today_days = days_from_civil(y, m, d);

    let (date_state, due_date, date_phrase) = if contains_word(&folded, "hoje") {
        let (ny, nm, nd) = civil_from_days(today_days);
        (TodoProposalDateState::Explicit, Some(format!("{ny:04}-{nm:02}-{nd:02}")), Some("hoje".into()))
    } else if contains_word(&folded, "amanha") {
        let (ny, nm, nd) = civil_from_days(today_days + 1);
        (TodoProposalDateState::Explicit, Some(format!("{ny:04}-{nm:02}-{nd:02}")), Some("amanhã".into()))
    } else if let Some(amb) = find_ambiguous_date(&folded) {
        (TodoProposalDateState::Ambiguous, None, Some(amb.to_string()))
    } else {
        (TodoProposalDateState::Unspecified, None, None)
    };

    let explicit_time = find_explicit_time(&folded);
    let (time_state, due_time, time_phrase) = match (explicit_time, date_state) {
        (Some(hhmm), TodoProposalDateState::Explicit | TodoProposalDateState::Ambiguous) => {
            (TodoProposalTimeState::Explicit, Some(hhmm.clone()), Some(hhmm))
        }
        (Some(hhmm), TodoProposalDateState::Unspecified) => {
            (TodoProposalTimeState::Ambiguous, None, Some(hhmm))
        }
        (None, _) => {
            if let Some(amb) = find_ambiguous_time(&folded) {
                (TodoProposalTimeState::Ambiguous, None, Some(amb.to_string()))
            } else {
                (TodoProposalTimeState::Unspecified, None, None)
            }
        }
    };

    let priority = if contains_word(&folded, "urgente") {
        TodoPriority::Urgent
    } else {
        TodoPriority::Normal
    };

    let proposal = TodoProposal {
        id: crate::flow::text::new_id(),
        title: trimmed.to_string(),
        date_state,
        due_date,
        date_phrase,
        time_state,
        due_time,
        time_phrase,
        priority,
    };
    proposal.validate()?;
    Ok(proposal)
}

pub async fn propose_with<F, Fut>(
    input: &str,
    now_ms: i64,
    time_zone: &str,
    call_llm: F,
) -> Result<TodoProposal, String>
where
    F: FnOnce(String, String) -> Fut,
    Fut: Future<Output = Result<String, String>>,
{
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Descreva a tarefa antes de continuar.".into());
    }
    let zone = if time_zone.trim().is_empty() { TODO_TIME_ZONE } else { time_zone };
    let (sys, usr) = build_proposal_prompt(trimmed, now_ms, zone);
    if let Ok(raw) = call_llm(sys, usr).await {
        if let Ok(prop) = parse_llm_proposal(&raw, trimmed) {
            return Ok(prop);
        }
    }
    fallback_proposal(trimmed, now_ms, zone)
}

fn clean_phrase(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn extract_json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    (end >= start).then(|| &raw[start..=end])
}

fn contains_word(haystack: &str, word: &str) -> bool {
    haystack
        .split(|c: char| !c.is_alphanumeric())
        .any(|token| token == word)
}

fn find_ambiguous_date(folded: &str) -> Option<&'static str> {
    for phrase in [
        "semana que vem",
        "proxima semana",
        "mes que vem",
        "fim de semana",
        "depois de amanha",
    ] {
        if folded.contains(phrase) {
            return Some(phrase);
        }
    }
    for day in ["segunda", "terca", "quarta", "quinta", "sexta", "sabado", "domingo"] {
        if contains_word(folded, day) {
            return Some(day);
        }
    }
    None
}

fn find_ambiguous_time(folded: &str) -> Option<&'static str> {
    for phrase in ["de manha", "pela manha", "a tarde", "de tarde", "a noite", "de noite", "mais tarde", "logo"] {
        if folded.contains(phrase) {
            return Some(phrase);
        }
    }
    None
}

fn find_explicit_time(folded: &str) -> Option<String> {
    for token in folded.split(|c: char| c.is_whitespace() || c == ',' || c == ';') {
        let tok = token.trim_matches(|c: char| !c.is_alphanumeric() && c != ':');
        if let Some((h, m)) = tok.split_once(':') {
            if let (Ok(hh), Ok(mm)) = (h.parse::<u8>(), m.parse::<u8>()) {
                if hh < 24 && mm < 60 {
                    return Some(format!("{hh:02}:{mm:02}"));
                }
            }
        }
        if let Some(rest) = tok.strip_suffix('h') {
            if let Ok(hh) = rest.parse::<u8>() {
                if hh < 24 {
                    return Some(format!("{hh:02}:00"));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::model::utc_ms_from_local;

    #[test]
    fn fallback_never_guesses_ambiguous_weekday_or_morning() {
        let now = utc_ms_from_local(2026, 11, 14, 10, 0, TODO_TIME_ZONE);
        let prop = fallback_proposal("Ligar para o dentista sexta de manhã", now, TODO_TIME_ZONE).unwrap();
        assert_eq!(prop.date_state, TodoProposalDateState::Ambiguous);
        assert_eq!(prop.due_date, None);
        assert_eq!(prop.date_phrase.as_deref(), Some("sexta"));
        assert_eq!(prop.time_state, TodoProposalTimeState::Ambiguous);
        assert_eq!(prop.due_time, None);
        assert_eq!(prop.time_phrase.as_deref(), Some("de manha"));
    }

    #[test]
    fn fallback_extracts_explicit_today_and_hour() {
        let now = utc_ms_from_local(2026, 11, 14, 10, 0, TODO_TIME_ZONE);
        let prop = fallback_proposal("Pagar conta hoje às 14h", now, TODO_TIME_ZONE).unwrap();
        assert_eq!(prop.date_state, TodoProposalDateState::Explicit);
        assert_eq!(prop.due_date.as_deref(), Some("2026-11-14"));
        assert_eq!(prop.time_state, TodoProposalTimeState::Explicit);
        assert_eq!(prop.due_time.as_deref(), Some("14:00"));
    }

    #[test]
    fn llm_parser_strips_inferred_date_if_state_is_not_explicit() {
        let raw = r#"```json
{"title":"Revisar contrato","date_state":"ambiguous","due_date":"2026-11-20","date_phrase":"sexta","time_state":"explicit","due_time":"09:00","priority":"high"}
```"#;
        let prop = parse_llm_proposal(raw, "Revisar contrato sexta às 9h").unwrap();
        assert_eq!(prop.date_state, TodoProposalDateState::Ambiguous);
        assert_eq!(prop.due_date, None);
        assert_eq!(prop.time_state, TodoProposalTimeState::Explicit);
        assert_eq!(prop.due_time.as_deref(), Some("09:00"));
        assert_eq!(prop.priority, TodoPriority::High);
    }
}

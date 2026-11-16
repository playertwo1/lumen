//! Registro local de auditoria do Mochi com redação anti-vazamento (M1.4).
//!
//! Invariantes:
//! - Persistido exclusivamente em `%LOCALAPPDATA%\Coucou\mochi-audit.json` via escrita atômica.
//! - Mantém no máximo `MAX_AUDIT_ENTRIES` (1000) registros mais recentes.
//! - `AuditLog::record` não aceita argumentos brutos (`args: &Value`) na assinatura e aplica
//!   `redact_summary` para remover controles, caminhos pessoais e segredos/tokens antes de persistir.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::permissions::PermissionDecision;
use super::registry::{is_valid_tool_id, RiskLevel};

/// Nome do arquivo de auditoria em `platform::local_dir()`.
pub const AUDIT_FILE_NAME: &str = "mochi-audit.json";

/// Limite máximo de entradas mantidas no arquivo local (rotação FIFO das mais antigas).
pub const MAX_AUDIT_ENTRIES: usize = 1000;

/// Limite máximo de caracteres Unicode em `AuditEntry::summary`.
pub const MAX_SUMMARY_CHARS: usize = 160;

const REDACTED_TOKEN: &str = "[redigido]";
const REDACTED_PATH: &str = "[caminho]";

/// Retorna o caminho padrão de `mochi-audit.json` (`%LOCALAPPDATA%\Coucou\mochi-audit.json`).
pub fn default_audit_path() -> PathBuf {
    crate::platform::local_dir().join(AUDIT_FILE_NAME)
}

/// Decisão registrada na camada de execução/permissão do Mochi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuditDecision {
    AutoLow,
    AllowedOnce,
    AllowedAlways,
    AlreadyAlwaysAllowed,
    Denied,
    TimedOut,
    BlockedPaused,
    BlockedFailClosed,
    Cancelled,
    Rejected,
}

impl From<PermissionDecision> for AuditDecision {
    fn from(value: PermissionDecision) -> Self {
        match value {
            PermissionDecision::AutoLow => Self::AutoLow,
            PermissionDecision::AllowedOnce => Self::AllowedOnce,
            PermissionDecision::AllowedAlways => Self::AllowedAlways,
            PermissionDecision::AlreadyAlwaysAllowed => Self::AlreadyAlwaysAllowed,
        }
    }
}

/// Resultado final de uma tentativa de invocação de ferramenta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuditResult {
    Success,
    Failed,
    Blocked,
    Cancelled,
}

/// Entrada individual persistida em `mochi-audit.json` (sem argumentos crus).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub timestamp_ms: u64,
    pub iso_time: String,
    pub tool_id: String,
    pub risk: RiskLevel,
    pub decision: AuditDecision,
    pub result: AuditResult,
    pub summary: String,
}

/// Registro local redigido e limitado a `MAX_AUDIT_ENTRIES` entradas.
#[derive(Debug)]
pub struct AuditLog {
    path: Option<PathBuf>,
    entries: Mutex<Vec<AuditEntry>>,
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new_default()
    }
}

impl AuditLog {
    /// Cria o registro apontando para `%LOCALAPPDATA%\Coucou\mochi-audit.json`.
    pub fn new_default() -> Self {
        Self::load_from(default_audit_path())
    }

    /// Carrega entradas existentes a partir de `path` (ou inicia vazio se ausente/corrompido).
    pub fn load_from(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let entries = load_entries_from(&path);
        Self {
            path: Some(path),
            entries: Mutex::new(entries),
        }
    }

    /// Instância isolada em memória para testes unitários do runtime que não inspecionam disco.
    #[cfg(test)]
    pub(crate) fn new_in_memory() -> Self {
        Self {
            path: None,
            entries: Mutex::new(Vec::new()),
        }
    }

    /// Caminho persistido em disco, quando habilitado.
    #[allow(dead_code)]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Registra uma tentativa de ferramenta com `summary` obrigatório e previamente redigido.
    ///
    /// Por contrato de segurança, esta função nunca aceita `args: &serde_json::Value`.
    pub fn record(
        &self,
        tool_id: &str,
        risk: RiskLevel,
        decision: AuditDecision,
        result: AuditResult,
        summary: &str,
    ) -> Result<AuditEntry, String> {
        let timestamp_ms = now_ms();
        let iso_time =
            crate::whatsapp::gateway::iso_utc(i64::try_from(timestamp_ms).unwrap_or(i64::MAX));
        let entry = AuditEntry {
            timestamp_ms,
            iso_time,
            tool_id: sanitize_tool_id_for_audit(tool_id),
            risk,
            decision,
            result,
            summary: redact_summary(summary),
        };

        let mut guard = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let mut next = guard.clone();
        next.push(entry.clone());
        trim_oldest_entries(&mut next);
        if let Some(path) = &self.path {
            write_entries_atomic(path, &next)?;
        }
        *guard = next;
        Ok(entry)
    }

    /// Retorna as entradas gravadas da mais recente para a mais antiga.
    pub fn list(&self, limit: Option<usize>) -> Vec<AuditEntry> {
        let guard = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let take = limit.unwrap_or(usize::MAX);
        guard.iter().rev().take(take).cloned().collect()
    }

    /// Zera o registro em memória e grava `[]` atomicamente em disco.
    pub fn clear(&self) -> Result<(), String> {
        let mut guard = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(path) = &self.path {
            write_entries_atomic(path, &[])?;
        }
        guard.clear();
        Ok(())
    }
}

/// Higieniza e mascara caminhos pessoais e possíveis segredos em textos de resumo/erro.
pub fn redact_summary(raw: &str) -> String {
    let normalized = normalize_controls_and_spaces(raw);
    if normalized.is_empty() {
        return String::new();
    }

    let redacted = scan_and_redact(&normalized);
    let collapsed = collapse_spaces(&redacted);
    truncate_chars(&collapsed, MAX_SUMMARY_CHARS)
}

fn sanitize_tool_id_for_audit(tool_id: &str) -> String {
    if is_valid_tool_id(tool_id) && !looks_like_secret_token(tool_id) {
        return tool_id.to_string();
    }
    let clean = redact_summary(tool_id);
    if clean.is_empty() {
        "unknown".to_string()
    } else {
        clean
    }
}

fn looks_like_secret_token(token: &str) -> bool {
    if token.len() >= 24 && token.bytes().all(is_long_token_byte) {
        return true;
    }
    has_secret_prefix(token).is_some()
}

fn normalize_controls_and_spaces(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_space = true;

    for ch in raw.chars() {
        if matches!(ch, '\r' | '\n' | '\t' | '\u{000B}' | '\u{000C}' | '\u{0085}' | '\u{2028}' | '\u{2029}') {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
            continue;
        }
        if is_control_or_invisible(ch) {
            continue;
        }
        if ch == ' ' {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
            continue;
        }
        out.push(ch);
        last_space = false;
    }

    if out.ends_with(' ') {
        out.pop();
    }
    out
}

fn is_control_or_invisible(ch: char) -> bool {
    let code = ch as u32;
    matches!(
        code,
        0x0000..=0x001F
            | 0x007F..=0x009F
            | 0x00AD
            | 0x034F
            | 0x061C
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x206F
            | 0xFEFF
            | 0xE0000..=0xE007F
    )
}

fn collapse_spaces(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_space = true;
    for ch in input.chars() {
        if ch == ' ' {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    let count = input.chars().count();
    if count <= max_chars {
        return input.to_string();
    }
    input
        .chars()
        .take(max_chars)
        .collect::<String>()
        .trim_end()
        .to_string()
}

fn scan_and_redact(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;

    while i < s.len() {
        let rest = &s[i..];
        let prev = s[..i].chars().next_back();

        if let Some(end) = match_path_span(rest, prev) {
            out.push_str(REDACTED_PATH);
            i += end;
            continue;
        }

        if let Some((prefix_end, end)) = match_secret_kv(rest, prev) {
            out.push_str(&rest[..prefix_end]);
            out.push_str(REDACTED_TOKEN);
            i += end;
            continue;
        }

        if let Some(end) = match_bearer_or_auth(rest, prev) {
            out.push_str(REDACTED_TOKEN);
            i += end;
            continue;
        }

        if let Some(end) = match_prefixed_or_long_token(rest, prev) {
            out.push_str(REDACTED_TOKEN);
            i += end;
            continue;
        }

        let ch = rest.chars().next().expect("i < s.len() garante char válido");
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

fn is_boundary_char(ch: Option<char>) -> bool {
    ch.is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
}

fn is_path_boundary(ch: Option<char>) -> bool {
    ch.is_none_or(|c| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\'' | '`' | '(' | '[' | '{' | '<' | '=' | ':' | ',' | ';' | '|'
            )
    })
}

fn starts_with_ascii_ci(haystack: &str, needle: &str) -> bool {
    haystack
        .get(..needle.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(needle))
}

fn match_path_span(rest: &str, prev: Option<char>) -> Option<usize> {
    if !is_path_boundary(prev) {
        return None;
    }

    // 1) URI file://...
    if starts_with_ascii_ci(rest, "file://") {
        let len = consume_path(rest, prev);
        if len > "file://".len() {
            return Some(len);
        }
    }

    // 2) Caminho UNC \\server\share\...
    if rest.starts_with("\\\\") {
        let after = &rest[2..];
        let first = after.chars().next()?;
        if !first.is_whitespace() && !matches!(first, '\\' | '/' | '"' | '\'' | '<' | '>' | ')' | ']') {
            let len = consume_path(rest, prev);
            if len > 2 {
                return Some(len);
            }
        }
    }

    // 3) Drive Windows C:\... ou C:/...
    let bytes = rest.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return Some(consume_path(rest, prev));
    }

    // 4) Variáveis de ambiente Windows (%USERPROFILE%..., %LOCALAPPDATA%..., etc.)
    const ENV_VARS: &[&str] = &[
        "%USERPROFILE%",
        "%LOCALAPPDATA%",
        "%APPDATA%",
        "%HOMEPATH%",
        "%ONEDRIVE%",
        "%USERNAME%",
        "%TEMP%",
        "%TMP%",
    ];
    for var in ENV_VARS {
        if starts_with_ascii_ci(rest, var) {
            let tail = &rest[var.len()..];
            if tail.is_empty()
                || tail.starts_with('\\')
                || tail.starts_with('/')
                || tail.chars().next().is_some_and(|c| !c.is_ascii_alphanumeric())
            {
                let extra = if tail.starts_with('\\') || tail.starts_with('/') {
                    consume_path(tail, prev)
                } else {
                    0
                };
                return Some(var.len() + extra);
            }
        }
    }

    // 5) Caminhos POSIX pessoais/raiz (/home/..., /Users/..., /root/..., ~/..., /var/folders/..., /tmp/...)
    const POSIX_PREFIXES: &[&str] = &[
        "/home/",
        "/Users/",
        "/users/",
        "/root/",
        "/private/var/",
        "/var/folders/",
        "/tmp/",
        "~/",
    ];
    for prefix in POSIX_PREFIXES {
        if rest.starts_with(prefix) {
            let len = consume_path(rest, prev);
            if len > prefix.len() || *prefix != "~/" {
                return Some(len);
            }
        }
    }

    None
}

fn consume_path(rest: &str, prev: Option<char>) -> usize {
    // Quando o caminho vem imediatamente após aspa simples ou dupla fechada na mesma frase,
    // inclui espaços internos até a aspa correspondente.
    if let Some(quote @ ('"' | '\'')) = prev {
        if let Some(close_idx) = rest.find(quote) {
            if close_idx > 0 {
                return close_idx;
            }
        }
    }

    let mut end = 0;
    for (idx, ch) in rest.char_indices() {
        if ch.is_whitespace() || matches!(ch, '"' | '\'' | '`' | ')' | ']' | '}' | '>' | ',' | ';') {
            break;
        }
        end = idx + ch.len_utf8();
    }

    while end > 3 && rest[..end].ends_with('.') {
        end -= 1;
    }
    end
}

fn match_bearer_or_auth(rest: &str, prev: Option<char>) -> Option<usize> {
    if !is_boundary_char(prev) {
        return None;
    }
    if !starts_with_ascii_ci(rest, "bearer ") {
        return None;
    }

    let after = &rest["bearer ".len()..];
    if after.starts_with(REDACTED_TOKEN) {
        return None;
    }

    let val_len = consume_secret_value(after)?;
    if val_len == 0 {
        return None;
    }
    Some("bearer ".len() + val_len)
}

fn match_secret_kv(rest: &str, prev: Option<char>) -> Option<(usize, usize)> {
    if !is_boundary_char(prev) {
        return None;
    }

    const SECRET_KEYS: &[&str] = &[
        "access_token",
        "refresh_token",
        "bearer_token",
        "auth_token",
        "client_secret",
        "private_key",
        "secret_key",
        "x-api-key",
        "id_token",
        "password",
        "api_key",
        "api-key",
        "apikey",
        "passwd",
        "secret",
        "token",
        "pwd",
    ];

    // Suporta chave sem aspas (`token=abc`) e chave JSON/aspas (`"token": "abc"`).
    let (key_total_len, key_quoted) = if let Some(q @ ('"' | '\'')) = prev {
        let matched_key = SECRET_KEYS
            .iter()
            .find(|k| starts_with_ascii_ci(rest, k) && rest[k.len()..].starts_with(q))?;
        (matched_key.len() + 1, true)
    } else {
        let matched_key = SECRET_KEYS.iter().find(|k| starts_with_ascii_ci(rest, k))?;
        (matched_key.len(), false)
    };

    let after_key = &rest[key_total_len..];
    let after_key_trim_len = after_key.len() - after_key.trim_start_matches(' ').len();
    let sep_slice = &after_key[after_key_trim_len..];
    let sep_ch = sep_slice.chars().next()?;
    if sep_ch != '=' && sep_ch != ':' {
        return None;
    }

    // Evita consumir frases em texto corrido como "token: invalido" quando não há aspas nem `=`;
    // para `:` sem aspas na chave, só mascara se houver valor contíguo não trivial ou aspas no valor.
    let mut prefix_len = key_total_len + after_key_trim_len + sep_ch.len_utf8();
    let after_sep = &rest[prefix_len..];
    let spaces_after_sep = after_sep.len() - after_sep.trim_start_matches(' ').len();
    if sep_ch == '=' && spaces_after_sep > 0 && !after_sep[spaces_after_sep..].starts_with(['"', '\'']) {
        prefix_len += spaces_after_sep;
    } else if sep_ch == ':' {
        if !key_quoted && spaces_after_sep == 0 && after_sep.starts_with("//") {
            return None;
        }
        prefix_len += spaces_after_sep;
    }

    let val_slice = &rest[prefix_len..];
    if val_slice.is_empty() || val_slice.starts_with(REDACTED_TOKEN) {
        return None;
    }

    if let Some(q @ ('"' | '\'')) = val_slice.chars().next() {
        let close = find_closing_quote(&val_slice[1..], q)?;
        let inner = &val_slice[1..1 + close];
        if inner.trim().is_empty() || inner == REDACTED_TOKEN {
            return None;
        }
        return Some((prefix_len + 1, prefix_len + 1 + close));
    }

    let val_len = consume_kv_unquoted_value(val_slice)?;
    if val_len == 0 {
        return None;
    }
    Some((prefix_len, prefix_len + val_len))
}

fn find_closing_quote(s: &str, quote: char) -> Option<usize> {
    let mut escaped = false;
    for (idx, ch) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == quote {
            return Some(idx);
        }
    }
    None
}

fn consume_kv_unquoted_value(s: &str) -> Option<usize> {
    let mut end = 0;
    for (idx, ch) in s.char_indices() {
        if ch.is_whitespace() || matches!(ch, ',' | ';' | '&' | ')' | ']' | '}' | '>' | '"' | '\'') {
            break;
        }
        end = idx + ch.len_utf8();
    }
    while end > 0 && s[..end].ends_with('.') {
        end -= 1;
    }
    if end == 0 {
        None
    } else {
        Some(end)
    }
}

fn has_secret_prefix(rest: &str) -> Option<usize> {
    const PREFIXES: &[(&str, usize, bool)] = &[
        ("github_pat_", 4, false),
        ("sk-proj-", 4, false),
        ("sk-ant-", 4, false),
        ("sk-or-", 4, false),
        ("sk_live_", 4, false),
        ("sk_test_", 4, false),
        ("pk_live_", 4, false),
        ("pk_test_", 4, false),
        ("rk_live_", 4, false),
        ("rk_test_", 4, false),
        ("ghp_", 4, false),
        ("gho_", 4, false),
        ("ghu_", 4, false),
        ("ghs_", 4, false),
        ("ghr_", 4, false),
        ("AIza", 4, false),
        ("xoxb-", 4, false),
        ("xoxp-", 4, false),
        ("xoxa-", 4, false),
        ("xoxr-", 4, false),
        ("xoxs-", 4, false),
        ("glpat-", 4, false),
        ("ntn_", 4, false),
        ("sk-", 4, false),
        ("ya29.", 4, true),
        ("AKIA", 12, false),
        ("ASIA", 12, false),
        ("eyJ", 5, true),
    ];

    for (prefix, min_tail, allow_dots) in PREFIXES {
        if rest.starts_with(prefix) {
            let tail = &rest[prefix.len()..];
            let mut end = prefix.len();
            for (idx, ch) in tail.char_indices() {
                let valid = if *allow_dots {
                    is_token_or_dot_char(ch)
                } else {
                    is_token_body_char(ch)
                };
                if !valid {
                    break;
                }
                end = prefix.len() + idx + ch.len_utf8();
            }
            while end > prefix.len() && rest[..end].ends_with('.') {
                end -= 1;
            }
            if end - prefix.len() >= *min_tail {
                return Some(end);
            }
        }
    }
    None
}

fn match_prefixed_or_long_token(rest: &str, prev: Option<char>) -> Option<usize> {
    if !is_boundary_char(prev) {
        return None;
    }

    if let Some(end) = has_secret_prefix(rest) {
        return Some(end);
    }

    // Sequências longas >= 24 chars de token hex / base64url / alfanumérico com hífen/underscore.
    let bytes = rest.as_bytes();
    let mut len = 0;
    while len < bytes.len() && is_long_token_byte(bytes[len]) {
        len += 1;
    }
    if len >= 22 {
        while len < bytes.len() && bytes[len] == b'=' {
            len += 1;
        }
    }
    if len >= 24 {
        let next = rest[len..].chars().next();
        if next.is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '-') {
            return Some(len);
        }
    }

    None
}

fn consume_secret_value(rest: &str) -> Option<usize> {
    if let Some(q @ ('"' | '\'')) = rest.chars().next() {
        let close = find_closing_quote(&rest[1..], q)?;
        if close > 0 {
            return Some(close + 2);
        }
    }
    let mut end = 0;
    for (idx, ch) in rest.char_indices() {
        if ch.is_whitespace() || matches!(ch, ',' | ';' | ')' | ']' | '}' | '>' | '"' | '\'') {
            break;
        }
        end = idx + ch.len_utf8();
    }
    while end > 0 && rest[..end].ends_with('.') {
        end -= 1;
    }
    if end == 0 {
        None
    } else {
        Some(end)
    }
}

fn is_token_body_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '=')
}

fn is_token_or_dot_char(ch: char) -> bool {
    is_token_body_char(ch) || ch == '.'
}

fn is_long_token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn trim_oldest_entries(entries: &mut Vec<AuditEntry>) {
    if entries.len() > MAX_AUDIT_ENTRIES {
        let excess = entries.len() - MAX_AUDIT_ENTRIES;
        entries.drain(..excess);
    }
}

fn load_entries_from(path: &Path) -> Vec<AuditEntry> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == ErrorKind::NotFound => return Vec::new(),
        Err(_) => return Vec::new(),
    };
    let Ok(raw) = std::str::from_utf8(&bytes) else {
        return Vec::new();
    };
    let trimmed = raw.strip_prefix('\u{FEFF}').unwrap_or(raw).trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let Ok(mut parsed) = serde_json::from_str::<Vec<AuditEntry>>(trimmed) else {
        return Vec::new();
    };
    trim_oldest_entries(&mut parsed);
    parsed
}

fn write_entries_atomic(path: &Path, entries: &[AuditEntry]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            crate::platform::ensure_private_dir(parent).map_err(|e| e.to_string())?;
        }
    }
    let mut bytes = serde_json::to_vec_pretty(entries).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    crate::tasks::store::write_atomic(path, &bytes)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    struct TempAuditDir {
        dir: PathBuf,
    }

    impl TempAuditDir {
        fn new(tag: &str) -> Self {
            static SEQ: AtomicUsize = AtomicUsize::new(0);
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "lumen-mochi-audit-{tag}-{}-{nanos}-{n}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self { dir }
        }

        fn audit_path(&self) -> PathBuf {
            self.dir.join(AUDIT_FILE_NAME)
        }
    }

    impl Drop for TempAuditDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn redact_summary_masks_secrets_paths_controls_and_caps_length() {
        let cases = [
            ("sk-secret-token-998877665544332211", &["sk-secret", "998877665544332211"][..], "[redigido]"),
            ("Bearer eyJhbGciOi...", &["eyJhbGciOi"][..], "[redigido]"),
            (
                "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.sig123456",
                &["eyJhbGci", "sig123456"],
                "[redigido]",
            ),
            ("ghp_1234567890abcdef1234567890", &["1234567890abcdef"], "[redigido]"),
            ("gho_abcdefghijklmnopqrstuvwxyz", &["abcdefghijklmnopqrstuvwxyz"], "[redigido]"),
            ("AIzaSyDaGmWKa4JsXZ-HjGw7ISLn_3namBGt", &["AIzaSyDa", "3namBGt"], "[redigido]"),
            ("xoxb-123456789012-abcdef", &["123456789012", "abcdef"], "[redigido]"),
            ("token=meuSegredoSuperForte", &["meuSegredoSuperForte"], "[redigido]"),
            ("api_key=chave123 apikey=chave456", &["chave123", "chave456"], "[redigido]"),
            ("secret=segredo1 password=\"minha senha secreta\"", &["segredo1", "minha senha secreta"], "[redigido]"),
            ("0123456789abcdef0123456789abcdef", &["0123456789abcdef"], "[redigido]"),
            ("token_alfa_numerico_longo_998877", &["token_alfa_numerico_longo"], "[redigido]"),
            (r"C:\Users\alice\Desktop\segredo.txt", &["alice", "segredo.txt", r"C:\Users"], "[caminho]"),
            (r"D:\projetos\privado\dados.json", &["privado", "dados.json", r"D:\"], "[caminho]"),
            (r"\\servidor\compartilhamento\segredo.docx", &["servidor", "segredo.docx"], "[caminho]"),
            (r"%USERPROFILE%\Documents\chaves.txt", &["USERPROFILE", "chaves.txt"], "[caminho]"),
            ("/home/alice/.ssh/id_rsa", &["alice", "id_rsa", "/home/"], "[caminho]"),
            ("/Users/alice/Documents/notas.md", &["alice", "notas.md", "/Users/"], "[caminho]"),
        ];

        for (raw, forbidden, marker) in cases {
            let clean = redact_summary(raw);
            assert!(
                clean.contains(marker),
                "esperava marcador {marker} em {clean:?} para entrada {raw:?}"
            );
            for secret in forbidden {
                assert!(
                    !clean.contains(secret),
                    "vazamento detectado ({secret}) em {clean:?} para entrada {raw:?}"
                );
            }
        }

        // Controles C0/C1 e quebras de linha viram espaço único e nunca corrompem o JSON.
        let with_controls = "linha1\r\nlinha2\n\t\u{0007}\u{001B}\u{0085}\u{2028}linha3";
        assert_eq!(redact_summary(with_controls), "linha1 linha2 linha3");

        // Bidi/ZWSP dentro de token não burla a redação.
        let obfuscated = "s\u{200B}k-secret-token-998877665544332211";
        assert_eq!(redact_summary(obfuscated), "[redigido]");

        // Frases legítimas não sofrem falsos positivos.
        assert_eq!(redact_summary("ping local"), "ping local");
        assert_eq!(
            redact_summary("Permitir/Negar ação no assistente"),
            "Permitir/Negar ação no assistente"
        );

        // Limite estrito de 160 caracteres Unicode (sem panics em multibyte).
        let long_pt = "ação ".repeat(60);
        let truncated = redact_summary(&long_pt);
        assert!(truncated.chars().count() <= MAX_SUMMARY_CHARS);
    }

    #[test]
    fn record_persists_redacted_entry_rotates_at_max_and_clear_writes_empty_array() {
        let dir = TempAuditDir::new("rotate_clear");
        let log = AuditLog::load_from(dir.audit_path());

        let first = log
            .record(
                "ping",
                RiskLevel::Low,
                AuditDecision::AutoLow,
                AuditResult::Success,
                r"Erro simulado em C:\Users\alice\Desktop\segredo.txt com sk-secret-token-998877665544332211 e /home/alice/.ssh/id_rsa",
            )
            .unwrap();

        assert_eq!(first.tool_id, "ping");
        assert_eq!(first.risk, RiskLevel::Low);
        assert_eq!(first.decision, AuditDecision::AutoLow);
        assert_eq!(first.result, AuditResult::Success);
        assert!(first.iso_time.ends_with('Z') && first.iso_time.len() == 20);
        assert!(!first.summary.contains("alice"));
        assert!(!first.summary.contains("sk-secret"));
        assert!(!first.summary.contains("id_rsa"));

        let disk = std::fs::read_to_string(dir.audit_path()).unwrap();
        assert!(!disk.contains("alice"));
        assert!(!disk.contains("sk-secret"));
        assert!(!disk.contains("id_rsa"));
        assert!(disk.contains("\"toolId\": \"ping\""));
        assert!(disk.contains("\"decision\": \"autoLow\""));
        assert!(disk.contains("\"result\": \"success\""));

        // Simula lote de > MAX_AUDIT_ENTRIES para validar rotação mantendo as mais recentes.
        for idx in 1..=(MAX_AUDIT_ENTRIES + 5) {
            log.record(
                "ping",
                RiskLevel::Low,
                AuditDecision::AutoLow,
                AuditResult::Success,
                &format!("entrada {idx}"),
            )
            .unwrap();
        }

        let all = log.list(None);
        assert_eq!(all.len(), MAX_AUDIT_ENTRIES);
        assert_eq!(all[0].summary, format!("entrada {}", MAX_AUDIT_ENTRIES + 5));
        assert_eq!(all[MAX_AUDIT_ENTRIES - 1].summary, "entrada 6");
        assert_eq!(log.list(Some(3)).len(), 3);

        let reloaded = AuditLog::load_from(dir.audit_path());
        assert_eq!(reloaded.list(None).len(), MAX_AUDIT_ENTRIES);
        assert_eq!(reloaded.list(Some(1))[0].summary, format!("entrada {}", MAX_AUDIT_ENTRIES + 5));

        reloaded.clear().unwrap();
        assert!(reloaded.list(None).is_empty());
        assert_eq!(std::fs::read_to_string(dir.audit_path()).unwrap().trim(), "[]");

        let after_clear = AuditLog::load_from(dir.audit_path());
        assert!(after_clear.list(None).is_empty());
    }

    #[test]
    fn load_from_recovers_safely_from_corrupt_or_empty_file() {
        let dir = TempAuditDir::new("corrupt");
        std::fs::write(dir.audit_path(), b"{invalid json").unwrap();
        let log = AuditLog::load_from(dir.audit_path());
        assert!(log.list(None).is_empty());

        std::fs::write(dir.audit_path(), b"   \n").unwrap();
        let empty = AuditLog::load_from(dir.audit_path());
        assert!(empty.list(None).is_empty());
    }
}

//! Instalador e desinstalador seguro de hooks HTTP (Claude Code) e command/MCP (Codex)
//! para permitir respostas pela placa ESP32-S3 AMOLED (subfase L11.6).
//!
//! Todas as funções recebem `home_dir`, `hook_exe` e `port` por parâmetro e utilizam
//! `config_file` para diffs unificados, backups datados e escritas atômicas com verificação
//! rigorosa de fingerprint. Nunca acessa caminhos reais do usuário diretamente.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::config_file::{self, FileEdit};

const CLAUDE_WAITING: &str = "Aguardando resposta na placa…";
const CODEX_WAITING: &str = "Aguardando resposta na placa…";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Esp32HookFilePreview {
    pub id: String,
    pub path: String,
    pub exists: bool,
    pub changed: bool,
    pub diff: String,
    pub after: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Esp32HooksPreview {
    pub files: Vec<Esp32HookFilePreview>,
    pub backup_notice: String,
}

/// Identificadores dos 3 arquivos manipulados.
pub const FILE_ID_CLAUDE_SETTINGS: &str = "claude_settings";
pub const FILE_ID_CODEX_HOOKS: &str = "codex_hooks";
pub const FILE_ID_CODEX_CONFIG: &str = "codex_config";

pub fn claude_settings_path(home_dir: &Path) -> PathBuf {
    home_dir.join(".claude").join("settings.json")
}

pub fn codex_hooks_path(home_dir: &Path) -> PathBuf {
    home_dir.join(".codex").join("hooks.json")
}

pub fn codex_config_path(home_dir: &Path) -> PathBuf {
    home_dir.join(".codex").join("config.toml")
}

// ── Verificação de instalação ────────────────────────────────────────────────

pub fn is_installed(home_dir: &Path) -> bool {
    let claude_path = claude_settings_path(home_dir);
    if let Ok(Some(bytes)) = config_file::read(&claude_path) {
        if let Ok(val) = config_file::parse_json(Some(&bytes), "settings.json") {
            if let Some(hooks) = val.get("hooks").and_then(Value::as_object) {
                for list in hooks.values().filter_map(Value::as_array) {
                    if list.iter().any(is_esp32_claude_entry) {
                        return true;
                    }
                }
            }
        }
    }

    let codex_hooks_p = codex_hooks_path(home_dir);
    if let Ok(Some(bytes)) = config_file::read(&codex_hooks_p) {
        if let Ok(val) = config_file::parse_json(Some(&bytes), "hooks.json") {
            if let Some(hooks) = val.get("hooks").and_then(Value::as_object) {
                if let Some(perm) = hooks.get("PermissionRequest").and_then(Value::as_array) {
                    if perm.iter().any(is_esp32_codex_hook_entry) {
                        return true;
                    }
                }
            }
        }
    }

    let codex_config_p = codex_config_path(home_dir);
    if let Ok(Some(bytes)) = config_file::read(&codex_config_p) {
        if let Ok(text) = std::str::from_utf8(&bytes) {
            if text.contains("[mcp_servers.lumen_placa]") {
                return true;
            }
        }
    }

    false
}

// ── Preview e Apply ──────────────────────────────────────────────────────────

/// Gera o preview estruturado para cada um dos 3 arquivos.
pub fn preview_hooks(
    home_dir: &Path,
    hook_exe: &Path,
    port: u16,
    enable: bool,
) -> Result<Esp32HooksPreview, String> {
    let edits_with_id = build_edits_with_id(home_dir, hook_exe, port, enable);
    let mut files = Vec::new();

    for (id, file_edit) in edits_with_id {
        let current = config_file::read(&file_edit.path)?;
        let exists = current.is_some();
        let change = (file_edit.edit)(current.as_deref())?;
        let after_str = change.after.clone().unwrap_or_default();
        let diff = config_file::unified_diff(&change.before, after_str.trim_end_matches('\n'));
        let changed = change.before != after_str;
        let fp = match current {
            Some(ref bytes) => config_file::fingerprint(bytes),
            None => "none".to_string(),
        };

        files.push(Esp32HookFilePreview {
            id: id.to_string(),
            path: file_edit.path.display().to_string(),
            exists,
            changed,
            diff,
            after: after_str,
            fingerprint: fp,
        });
    }

    Ok(Esp32HooksPreview {
        files,
        backup_notice: "Backups datados (.bak-*) serão gravados ao lado dos arquivos existentes antes de qualquer alteração.".to_string(),
    })
}

/// Aplica as alterações aos 3 arquivos garantindo que nenhum foi modificado desde o preview.
pub fn apply_hooks(
    home_dir: &Path,
    hook_exe: &Path,
    port: u16,
    enable: bool,
    fingerprints: &HashMap<String, String>,
) -> Result<Vec<PathBuf>, String> {
    let edits_with_id = build_edits_with_id(home_dir, hook_exe, port, enable);

    let mut prints = Vec::new();
    let mut raw_edits = Vec::new();

    for (id, file_edit) in edits_with_id {
        let expected_fp = fingerprints
            .get(id)
            .ok_or_else(|| format!("Fingerprint ausente para o arquivo {id}"))?;
        prints.push(expected_fp.as_str());
        raw_edits.push(file_edit);
    }

    let joined_fingerprints = prints.join(":");
    config_file::apply(&raw_edits, &joined_fingerprints)
}

fn build_edits_with_id<'a>(
    home_dir: &Path,
    hook_exe: &Path,
    port: u16,
    enable: bool,
) -> Vec<(&'static str, FileEdit<'a>)> {
    let p_claude = claude_settings_path(home_dir);
    let p_codex_hooks = codex_hooks_path(home_dir);
    let p_codex_config = codex_config_path(home_dir);

    let hook_exe_cloned = hook_exe.to_path_buf();
    let hook_exe_cloned2 = hook_exe.to_path_buf();

    let edit_claude = FileEdit {
        path: p_claude.clone(),
        edit: config_file::json_edit(p_claude.display().to_string(), move |current| {
            edit_claude_settings(current, port, enable)
        }),
    };

    let edit_codex_hooks = FileEdit {
        path: p_codex_hooks.clone(),
        edit: config_file::json_edit(p_codex_hooks.display().to_string(), move |current| {
            edit_codex_hooks(current, &hook_exe_cloned, port, enable)
        }),
    };

    let edit_codex_config = FileEdit {
        path: p_codex_config.clone(),
        edit: config_file::text_edit(p_codex_config.display().to_string(), move |current| {
            let label = p_codex_config.display().to_string();
            edit_codex_config_toml(current, &hook_exe_cloned2, port, enable, &label)
        }),
    };

    vec![
        (FILE_ID_CLAUDE_SETTINGS, edit_claude),
        (FILE_ID_CODEX_HOOKS, edit_codex_hooks),
        (FILE_ID_CODEX_CONFIG, edit_codex_config),
    ]
}

// ── Arquivo 1: Claude Code (.claude/settings.json) ───────────────────────────

fn is_esp32_claude_hook_item(h: &Value) -> bool {
    h.get("type").and_then(Value::as_str) == Some("http")
        && h.get("url").and_then(Value::as_str).map_or(false, |u| {
            u.contains("/api/hook/question") || u.contains("/api/hook/permission")
        })
}

fn is_esp32_claude_entry(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .map_or(false, |arr| arr.iter().any(is_esp32_claude_hook_item))
}

fn filter_claude_entries(list: &[Value]) -> Vec<Value> {
    list.iter()
        .filter_map(|entry| {
            let Some(mut obj) = entry.as_object().cloned() else {
                return Some(entry.clone());
            };
            if let Some(hooks_arr) = obj.get("hooks").and_then(Value::as_array) {
                let kept: Vec<Value> = hooks_arr
                    .iter()
                    .filter(|h| !is_esp32_claude_hook_item(h))
                    .cloned()
                    .collect();
                if kept.is_empty() {
                    return None;
                }
                obj.insert("hooks".to_string(), Value::Array(kept));
                Some(Value::Object(obj))
            } else {
                Some(entry.clone())
            }
        })
        .collect()
}

fn edit_claude_settings(current: &Value, port: u16, enable: bool) -> Result<Option<Value>, String> {
    let mut root = current.as_object().cloned().unwrap_or_default();
    let mut hooks_obj = match root.get("hooks") {
        None => Map::new(),
        Some(Value::Object(h)) => h.clone(),
        Some(_) => return Err("settings.json: \"hooks\" não é um objeto JSON".to_string()),
    };

    let mut pre_list = match hooks_obj.get("PreToolUse") {
        None => Vec::new(),
        Some(Value::Array(arr)) => filter_claude_entries(arr),
        Some(_) => return Err("settings.json: \"hooks\".\"PreToolUse\" não é uma lista".to_string()),
    };

    let mut perm_list = match hooks_obj.get("PermissionRequest") {
        None => Vec::new(),
        Some(Value::Array(arr)) => filter_claude_entries(arr),
        Some(_) => return Err("settings.json: \"hooks\".\"PermissionRequest\" não é uma lista".to_string()),
    };

    if enable {
        pre_list.push(json!({
            "matcher": "AskUserQuestion",
            "hooks": [
                {
                    "type": "http",
                    "url": format!("http://127.0.0.1:{port}/api/hook/question"),
                    "timeout": 120,
                    "statusMessage": CLAUDE_WAITING
                }
            ]
        }));

        perm_list.push(json!({
            "matcher": ".*",
            "hooks": [
                {
                    "type": "http",
                    "url": format!("http://127.0.0.1:{port}/api/hook/permission"),
                    "timeout": 120,
                    "statusMessage": CLAUDE_WAITING
                }
            ]
        }));
    }

    if pre_list.is_empty() {
        hooks_obj.remove("PreToolUse");
    } else {
        hooks_obj.insert("PreToolUse".to_string(), Value::Array(pre_list));
    }

    if perm_list.is_empty() {
        hooks_obj.remove("PermissionRequest");
    } else {
        hooks_obj.insert("PermissionRequest".to_string(), Value::Array(perm_list));
    }

    if hooks_obj.is_empty() {
        root.remove("hooks");
    } else {
        root.insert("hooks".to_string(), Value::Object(hooks_obj));
    }

    Ok(Some(Value::Object(root)))
}

// ── Arquivo 2: Codex Hooks (.codex/hooks.json) ───────────────────────────────

fn is_esp32_codex_hook_item(h: &Value) -> bool {
    h.get("command")
        .and_then(Value::as_str)
        .map_or(false, |c| c.contains("--esp32-codex-permission"))
}

fn is_esp32_codex_hook_entry(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .map_or(false, |arr| arr.iter().any(is_esp32_codex_hook_item))
}

fn filter_codex_entries(list: &[Value]) -> Vec<Value> {
    list.iter()
        .filter_map(|entry| {
            let Some(mut obj) = entry.as_object().cloned() else {
                return Some(entry.clone());
            };
            if let Some(hooks_arr) = obj.get("hooks").and_then(Value::as_array) {
                let kept: Vec<Value> = hooks_arr
                    .iter()
                    .filter(|h| !is_esp32_codex_hook_item(h))
                    .cloned()
                    .collect();
                if kept.is_empty() {
                    return None;
                }
                obj.insert("hooks".to_string(), Value::Array(kept));
                Some(Value::Object(obj))
            } else {
                Some(entry.clone())
            }
        })
        .collect()
}

fn codex_permission_cmd(hook_exe: &Path, port: u16) -> String {
    let p = hook_exe.to_string_lossy();
    if p.contains(' ') || p.contains('\\') || p.contains('/') {
        format!("\"{p}\" --esp32-codex-permission {port}")
    } else {
        format!("{p} --esp32-codex-permission {port}")
    }
}

fn edit_codex_hooks(current: &Value, hook_exe: &Path, port: u16, enable: bool) -> Result<Option<Value>, String> {
    let mut root = current.as_object().cloned().unwrap_or_default();
    let mut hooks_obj = match root.get("hooks") {
        None => Map::new(),
        Some(Value::Object(h)) => h.clone(),
        Some(_) => return Err("hooks.json: \"hooks\" não é um objeto JSON".to_string()),
    };

    let mut perm_list = match hooks_obj.get("PermissionRequest") {
        None => Vec::new(),
        Some(Value::Array(arr)) => filter_codex_entries(arr),
        Some(_) => return Err("hooks.json: \"hooks\".\"PermissionRequest\" não é uma lista".to_string()),
    };

    if enable {
        let cmd = codex_permission_cmd(hook_exe, port);
        perm_list.push(json!({
            "hooks": [
                {
                    "type": "command",
                    "command": cmd,
                    "timeout": 125,
                    "statusMessage": CODEX_WAITING
                }
            ]
        }));
    }

    if perm_list.is_empty() {
        hooks_obj.remove("PermissionRequest");
    } else {
        hooks_obj.insert("PermissionRequest".to_string(), Value::Array(perm_list));
    }

    if hooks_obj.is_empty() {
        root.remove("hooks");
    } else {
        root.insert("hooks".to_string(), Value::Object(hooks_obj));
    }

    Ok(Some(Value::Object(root)))
}

// ── Arquivo 3: Codex Config (.codex/config.toml) ─────────────────────────────

fn edit_codex_config_toml(
    current: Option<&str>,
    hook_exe: &Path,
    port: u16,
    enable: bool,
    _label: &str,
) -> Result<Option<String>, String> {
    let (bom, raw) = match current {
        Some(s) => {
            if s.starts_with('\u{FEFF}') {
                ("\u{FEFF}", &s['\u{FEFF}'.len_utf8()..])
            } else {
                ("", s)
            }
        }
        None => {
            if !enable {
                return Ok(None);
            }
            ("", "")
        }
    };

    let eol = if raw.contains("\r\n") { "\r\n" } else { "\n" };

    let hook_path_str = hook_exe.to_string_lossy().replace('\\', "\\\\");
    let lumen_block = format!(
        "[mcp_servers.lumen_placa]{eol}command = \"{hook_path_str}\"{eol}args = [\"--esp32-codex-mcp\", \"{port}\"]{eol}tool_timeout_sec = 130{eol}"
    );

    let lines: Vec<&str> = raw.split('\n').collect();

    // Encontra intervalos de seções
    // section_header: linha que começa com '[' (excluindo comentários)
    let mut sections: Vec<(String, usize, usize)> = Vec::new(); // (nome_seção, linha_inicio, linha_fim)
    let mut current_sec_name: Option<String> = None;
    let mut current_sec_start = 0;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && !trimmed.starts_with("[[") {
            if let Some(closing) = trimmed.find(']') {
                let name = trimmed[1..closing].trim().to_string();
                if let Some(prev_name) = current_sec_name.take() {
                    sections.push((prev_name, current_sec_start, i));
                }
                current_sec_name = Some(name);
                current_sec_start = i;
            }
        }
    }
    if let Some(last_name) = current_sec_name {
        sections.push((last_name, current_sec_start, lines.len()));
    }

    let mut result_lines: Vec<String> = lines.iter().map(|s| s.trim_end_matches('\r').to_string()).collect();

    if enable {
        // 1. Garante `codex_hooks = true` sob `[features]`
        let features_sec = sections.iter().find(|(name, _, _)| name == "features");
        match features_sec {
            Some((_, start, end)) => {
                // Checa se já tem `codex_hooks`
                let has_codex_hooks = (start + 1..*end).any(|idx| {
                    let tr = result_lines[idx].trim();
                    tr.starts_with("codex_hooks") && tr.contains('=')
                });
                if !has_codex_hooks {
                    result_lines.insert(start + 1, "codex_hooks = true".to_string());
                }
            }
            None => {
                // Insere no topo ou antes da primeira seção
                let insert_pos = if let Some(first_sec) = sections.first() {
                    first_sec.1
                } else {
                    result_lines.len()
                };
                result_lines.insert(insert_pos, "[features]".to_string());
                result_lines.insert(insert_pos + 1, "codex_hooks = true".to_string());
                result_lines.insert(insert_pos + 2, String::new());
            }
        }

        // Recalcula seções após possível inserção
        sections.clear();
        let mut cur_name = None;
        let mut cur_start = 0;
        for (i, line) in result_lines.iter().enumerate() {
            let tr = line.trim();
            if tr.starts_with('[') && !tr.starts_with("[[") {
                if let Some(closing) = tr.find(']') {
                    let name = tr[1..closing].trim().to_string();
                    if let Some(prev) = cur_name.take() {
                        sections.push((prev, cur_start, i));
                    }
                    cur_name = Some(name);
                    cur_start = i;
                }
            }
        }
        if let Some(last) = cur_name {
            sections.push((last, cur_start, result_lines.len()));
        }

        // 2. Insere ou substitui `[mcp_servers.lumen_placa]`
        let lumen_sec = sections.iter().find(|(name, _, _)| name == "mcp_servers.lumen_placa");
        if let Some((_, start, end)) = lumen_sec {
            let replacement_lines: Vec<String> = lumen_block
                .split(eol)
                .map(|s| s.trim_end_matches('\r').to_string())
                .filter(|s| !s.is_empty())
                .collect();
            result_lines.splice(*start..*end, replacement_lines);
        } else {
            // Adiciona no final
            if !result_lines.is_empty() && !result_lines.last().unwrap().is_empty() {
                result_lines.push(String::new());
            }
            for l in lumen_block.split(eol) {
                let trimmed = l.trim_end_matches('\r');
                if !trimmed.is_empty() {
                    result_lines.push(trimmed.to_string());
                }
            }
        }
    } else {
        // Remoção limpa de `[mcp_servers.lumen_placa]`
        let lumen_sec = sections.iter().find(|(name, _, _)| name == "mcp_servers.lumen_placa");
        if let Some((_, start, end)) = lumen_sec {
            result_lines.drain(*start..*end);
            // Remove linhas vazias residuais no final
            while result_lines.last().map_or(false, |l| l.trim().is_empty()) {
                result_lines.pop();
            }
        }
    }

    let mut out = result_lines.join(eol);
    if !out.is_empty() && !out.ends_with(eol) {
        out.push_str(eol);
    }

    Ok(Some(format!("{bom}{out}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_test_env(test_name: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("coucou_test_hooks_{}_{}", test_name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let hook_exe = dir.join("bin").join("coucou-hook.exe");
        fs::create_dir_all(hook_exe.parent().unwrap()).unwrap();
        fs::write(&hook_exe, b"fake-exe").unwrap();
        (dir, hook_exe)
    }

    #[test]
    fn clean_install_creates_all_three_files_and_preserves_on_clean_remove() {
        let (home, hook_exe) = make_test_env("clean_install");
        let port = 8787;

        assert!(!is_installed(&home));

        // 1. Preview de instalação em diretório limpo
        let preview = preview_hooks(&home, &hook_exe, port, true).unwrap();
        assert_eq!(preview.files.len(), 3);
        for f in &preview.files {
            assert!(!f.exists);
            assert!(f.changed);
            assert_eq!(f.fingerprint, "none");
        }

        // Mapeia fingerprints para apply
        let mut fps = HashMap::new();
        for f in &preview.files {
            fps.insert(f.id.clone(), f.fingerprint.clone());
        }

        // 2. Aplica instalação
        let backups = apply_hooks(&home, &hook_exe, port, true, &fps).unwrap();
        assert!(backups.is_empty()); // Nenhum backup pois os arquivos não existiam antes
        assert!(is_installed(&home));

        // Confere conteúdo do settings.json do Claude
        let claude_val: Value = serde_json::from_str(&fs::read_to_string(claude_settings_path(&home)).unwrap()).unwrap();
        let pre_hooks = claude_val["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre_hooks[0]["matcher"], "AskUserQuestion");
        assert_eq!(pre_hooks[0]["hooks"][0]["type"], "http");
        assert!(pre_hooks[0]["hooks"][0]["url"].as_str().unwrap().contains("/api/hook/question"));

        let perm_hooks = claude_val["hooks"]["PermissionRequest"].as_array().unwrap();
        assert_eq!(perm_hooks[0]["matcher"], ".*");
        assert_eq!(perm_hooks[0]["hooks"][0]["type"], "http");
        assert!(perm_hooks[0]["hooks"][0]["url"].as_str().unwrap().contains("/api/hook/permission"));

        // Confere conteúdo do hooks.json do Codex
        let codex_hooks_val: Value = serde_json::from_str(&fs::read_to_string(codex_hooks_path(&home)).unwrap()).unwrap();
        let cx_perm = codex_hooks_val["hooks"]["PermissionRequest"].as_array().unwrap();
        let cmd = cx_perm[0]["hooks"][0]["command"].as_str().unwrap();
        assert!(cmd.contains("--esp32-codex-permission 8787"));

        // Confere conteúdo do config.toml do Codex
        let toml_str = fs::read_to_string(codex_config_path(&home)).unwrap();
        assert!(toml_str.contains("[features]"));
        assert!(toml_str.contains("codex_hooks = true"));
        assert!(toml_str.contains("[mcp_servers.lumen_placa]"));
        assert!(toml_str.contains("--esp32-codex-mcp"));

        // 3. Preview de remoção
        let remove_preview = preview_hooks(&home, &hook_exe, port, false).unwrap();
        let mut remove_fps = HashMap::new();
        for f in &remove_preview.files {
            assert!(f.exists);
            assert!(f.changed);
            assert_ne!(f.fingerprint, "none");
            remove_fps.insert(f.id.clone(), f.fingerprint.clone());
        }

        // 4. Aplica remoção e confere criação de backups
        let remove_backups = apply_hooks(&home, &hook_exe, port, false, &remove_fps).unwrap();
        assert_eq!(remove_backups.len(), 3);
        assert!(!is_installed(&home));

        for b in &remove_backups {
            assert!(b.exists());
            assert!(b.to_string_lossy().contains(".bak-"));
        }

        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn preserves_existing_coucou_hook_and_other_settings() {
        let (home, hook_exe) = make_test_env("preserves_coucou");
        let port = 8787;

        // Prepara Claude settings.json existente com coucou-hook e chaves extras
        let claude_p = claude_settings_path(&home);
        fs::create_dir_all(claude_p.parent().unwrap()).unwrap();
        let existing_claude = json!({
            "theme": "dark",
            "hooks": {
                "PreToolUse": [
                    { "hooks": [{ "type": "command", "command": "coucou-hook PreToolUse", "timeout": 10 }] }
                ],
                "Stop": [
                    { "hooks": [{ "type": "command", "command": "coucou-hook Stop", "timeout": 10 }] }
                ]
            }
        });
        fs::write(&claude_p, serde_json::to_string_pretty(&existing_claude).unwrap()).unwrap();

        // Prepara Codex hooks.json existente com coucou-hook
        let codex_h_p = codex_hooks_path(&home);
        fs::create_dir_all(codex_h_p.parent().unwrap()).unwrap();
        let existing_codex_hooks = json!({
            "hooks": {
                "SessionStart": [
                    { "hooks": [{ "type": "command", "command": "coucou-hook --agent codex SessionStart", "timeout": 10 }] }
                ],
                "PermissionRequest": [
                    { "hooks": [{ "type": "command", "command": "coucou-hook --agent codex PermissionRequest", "timeout": 120 }] }
                ]
            }
        });
        fs::write(&codex_h_p, serde_json::to_string_pretty(&existing_codex_hooks).unwrap()).unwrap();

        // Prepara Codex config.toml existente com outras opções e comentários
        let codex_c_p = codex_config_path(&home);
        fs::create_dir_all(codex_c_p.parent().unwrap()).unwrap();
        let existing_toml = "# Meu arquivo de config\nmodel = \"o3-mini\"\n\n[features]\nminha_feature = true\n\n[mcp_servers.meu_servidor]\ncommand = \"node\"\nargs = [\"server.js\"]\n";
        fs::write(&codex_c_p, existing_toml).unwrap();

        // Aplica instalação
        let preview = preview_hooks(&home, &hook_exe, port, true).unwrap();
        let mut fps = HashMap::new();
        for f in &preview.files {
            fps.insert(f.id.clone(), f.fingerprint.clone());
        }
        apply_hooks(&home, &hook_exe, port, true, &fps).unwrap();

        // Verifica que `coucou-hook` e `"theme": "dark"` continuam lá
        let claude_after: Value = serde_json::from_str(&fs::read_to_string(&claude_p).unwrap()).unwrap();
        assert_eq!(claude_after["theme"], "dark");
        let pre_arr = claude_after["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre_arr.len(), 2);
        assert!(pre_arr[0]["hooks"][0]["command"].as_str().unwrap().contains("coucou-hook"));
        assert!(pre_arr[1]["hooks"][0]["url"].as_str().unwrap().contains("/api/hook/question"));

        // Verifica que no Codex hooks continuam o coucou-hook de PermissionRequest
        let codex_h_after: Value = serde_json::from_str(&fs::read_to_string(&codex_h_p).unwrap()).unwrap();
        let perm_arr = codex_h_after["hooks"]["PermissionRequest"].as_array().unwrap();
        assert_eq!(perm_arr.len(), 2);
        assert!(perm_arr[0]["hooks"][0]["command"].as_str().unwrap().contains("coucou-hook"));
        assert!(perm_arr[1]["hooks"][0]["command"].as_str().unwrap().contains("--esp32-codex-permission"));

        // Verifica que config.toml preservou comentários e servidores anteriores
        let toml_after = fs::read_to_string(&codex_c_p).unwrap();
        assert!(toml_after.contains("# Meu arquivo de config"));
        assert!(toml_after.contains("minha_feature = true"));
        assert!(toml_after.contains("codex_hooks = true"));
        assert!(toml_after.contains("[mcp_servers.meu_servidor]"));
        assert!(toml_after.contains("[mcp_servers.lumen_placa]"));

        // Agora desinstala
        let rem_prev = preview_hooks(&home, &hook_exe, port, false).unwrap();
        let mut rem_fps = HashMap::new();
        for f in &rem_prev.files {
            rem_fps.insert(f.id.clone(), f.fingerprint.clone());
        }
        apply_hooks(&home, &hook_exe, port, false, &rem_fps).unwrap();

        // Confere que remoção restaurou coucou-hook sem os hooks da placa
        let claude_restored: Value = serde_json::from_str(&fs::read_to_string(&claude_p).unwrap()).unwrap();
        let pre_restored = claude_restored["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre_restored.len(), 1);
        assert!(pre_restored[0]["hooks"][0]["command"].as_str().unwrap().contains("coucou-hook"));
        assert!(claude_restored["hooks"].get("PermissionRequest").is_none());

        let codex_h_restored: Value = serde_json::from_str(&fs::read_to_string(&codex_h_p).unwrap()).unwrap();
        let perm_restored = codex_h_restored["hooks"]["PermissionRequest"].as_array().unwrap();
        assert_eq!(perm_restored.len(), 1);
        assert!(perm_restored[0]["hooks"][0]["command"].as_str().unwrap().contains("coucou-hook"));

        let toml_restored = fs::read_to_string(&codex_c_p).unwrap();
        assert!(!toml_restored.contains("[mcp_servers.lumen_placa]"));
        assert!(toml_restored.contains("[mcp_servers.meu_servidor]"));

        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn rejects_outdated_fingerprint() {
        let (home, hook_exe) = make_test_env("conflict_fp");
        let port = 8787;

        let claude_p = claude_settings_path(&home);
        fs::create_dir_all(claude_p.parent().unwrap()).unwrap();
        fs::write(&claude_p, b"{\"original\":true}").unwrap();

        let preview = preview_hooks(&home, &hook_exe, port, true).unwrap();
        let mut fps = HashMap::new();
        for f in &preview.files {
            fps.insert(f.id.clone(), f.fingerprint.clone());
        }

        // Simula outra ferramenta modificando o arquivo após o preview
        fs::write(&claude_p, b"{\"original\":true,\"alguem_alterou\":true}").unwrap();

        let result = apply_hooks(&home, &hook_exe, port, true, &fps);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("changed since the preview") || err.contains("modificado após"));

        let _ = fs::remove_dir_all(&home);
    }
}

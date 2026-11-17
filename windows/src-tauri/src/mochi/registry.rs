//! Contrato de ferramentas, validação estrita de schema JSON e registro deny-by-default do Mochi.
//!
//! Invariantes da M1.1:
//! - O modelo só enxerga e executa ferramentas registradas e habilitadas explicitamente (`ToolRegistry`).
//! - Ferramentas indisponíveis (`Availability::Unavailable`) são omitidas de `list_available` e
//!   recusadas em `get_if_available` (regra Understudy).
//! - Nenhuma ferramenta executa texto livre como comando de shell ou PowerShell.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::permissions::{PermissionDecision, PermitToken};

/// Nível de risco da ferramenta, usado para decidir a exigência de clique na camada de permissões.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

/// Disponibilidade operacional de uma ferramenta no contexto atual.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Availability {
    Available,
    Unavailable { reason: String },
}

/// Erros padronizados do contrato, registro e execução de ferramentas do Mochi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolError {
    UnknownTool(String),
    DisabledTool(String),
    PrerequisiteMissing(String),
    InvalidInput(String),
    Timeout,
    Cancelled,
    Paused,
    PermissionDenied(String),
    VerificationFailed(String),
    ExecutionFailed(String),
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownTool(id) => write!(f, "Ferramenta desconhecida: {id}."),
            Self::DisabledTool(id) => write!(f, "Ferramenta desabilitada: {id}."),
            Self::PrerequisiteMissing(reason) => {
                write!(f, "Pré-requisito ausente para a ferramenta: {reason}.")
            }
            Self::InvalidInput(reason) => {
                write!(f, "Argumentos inválidos para a ferramenta: {reason}.")
            }
            Self::Timeout => write!(f, "Tempo limite esgotado ao executar a ferramenta."),
            Self::Cancelled => write!(f, "Execução da ferramenta cancelada."),
            Self::Paused => {
                write!(f, "O assistente está em pausa; nenhuma ferramenta pode ser executada.")
            }
            Self::PermissionDenied(reason) => {
                write!(f, "Permissão negada para executar a ferramenta: {reason}.")
            }
            Self::VerificationFailed(reason) => {
                write!(f, "A verificação pós-execução falhou: {reason}.")
            }
            Self::ExecutionFailed(reason) => {
                write!(f, "Falha ao executar a ferramenta: {reason}.")
            }
        }
    }
}

impl std::error::Error for ToolError {}

/// Contexto ambiental consultado para verificar pré-requisitos e pausa rápida.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolContext {
    pub paused: bool,
    pub capabilities: BTreeSet<String>,
}

impl ToolContext {
    /// Cria um contexto padrão ativo e sem capacidades extras declaradas.
    pub fn new() -> Self {
        Self::default()
    }

    /// Define o sinal de pausa rápida neste contexto.
    pub fn with_paused(mut self, paused: bool) -> Self {
        self.paused = paused;
        self
    }

    /// Adiciona uma capacidade/adaptador disponível ao contexto (útil para testes e adaptadores).
    pub fn with_capability(mut self, capability: impl Into<String>) -> Self {
        self.capabilities.insert(capability.into());
        self
    }

    /// Informa se uma capacidade/adaptador está disponível no contexto atual.
    pub fn has_capability(&self, capability: &str) -> bool {
        self.capabilities.contains(capability)
    }
}

/// Contexto entregue à função `execute` de uma ferramenta.
///
/// Só pode ser construído a partir de um `&PermitToken` válido emitido por `PermissionEngine`
/// para a mesma ferramenta em execução.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub cancel_rx: tokio::sync::watch::Receiver<bool>,
    permit_tool_id: String,
    permit_decision: PermissionDecision,
    _private: (),
}

impl ExecutionContext {
    /// Cria um contexto de execução validando que o `PermitToken` pertence à ferramenta esperada.
    pub fn for_tool(
        cancel_rx: tokio::sync::watch::Receiver<bool>,
        permit: &PermitToken,
        expected_tool_id: &str,
    ) -> Result<Self, ToolError> {
        if permit.tool_id() != expected_tool_id {
            return Err(ToolError::PermissionDenied(format!(
                "token emitido para '{}' não autoriza '{}'",
                permit.tool_id(),
                expected_tool_id
            )));
        }
        Ok(Self {
            cancel_rx,
            permit_tool_id: permit.tool_id().to_string(),
            permit_decision: permit.decision(),
            _private: (),
        })
    }

    /// Indica se o cancelamento já foi sinalizado.
    pub fn is_cancelled(&self) -> bool {
        *self.cancel_rx.borrow()
    }

    /// Identificador da ferramenta autorizada pelo `PermitToken`.
    pub fn permit_tool_id(&self) -> &str {
        &self.permit_tool_id
    }

    /// Decisão registrada que originou o `PermitToken`.
    pub fn permit_decision(&self) -> PermissionDecision {
        self.permit_decision
    }
}

/// Futuro retornado pela etapa `execute` de uma ferramenta.
pub type ToolFuture = Pin<Box<dyn Future<Output = Result<Value, ToolError>> + Send>>;

/// Contrato completo de uma ferramenta registrável no Mochi.
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub id: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
    pub risk: RiskLevel,
    pub timeout: Duration,
    pub check_availability: fn(&ToolContext) -> Availability,
    pub dry_run: fn(&Value) -> Result<String, ToolError>,
    pub execute: fn(Value, ExecutionContext) -> ToolFuture,
    pub verify: fn(&Value, &Value) -> Result<(), ToolError>,
    pub audit_summary: fn(&Value, &Result<Value, ToolError>) -> String,
}

/// Metadados públicos de uma ferramenta disponível entregues ao modelo e ao servidor MCP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDescriptor {
    pub id: String,
    pub description: String,
    pub risk: RiskLevel,
    pub input_schema: Value,
}

impl From<&ToolDefinition> for ToolDescriptor {
    fn from(def: &ToolDefinition) -> Self {
        Self {
            id: def.id.to_string(),
            description: def.description.to_string(),
            risk: def.risk,
            input_schema: def.input_schema.clone(),
        }
    }
}

/// Valida identificadores no padrão `[a-z0-9_-]{1,64}`.
pub fn is_valid_tool_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-'))
}

/// Registro deny-by-default das ferramentas declaradas do Mochi.
#[derive(Debug, Clone, Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<ToolDefinition>>,
    enabled: BTreeSet<String>,
}

impl ToolRegistry {
    /// Cria um registro vazio, com nenhuma ferramenta registrada nem habilitada.
    pub fn new() -> Self {
        Self::default()
    }

    /// Cria o registro padrão: `ping` (M1) e as ferramentas nativas do Windows da M2.
    pub fn with_default_tools() -> Self {
        let mut registry = Self::new();
        registry
            .register_and_enable(ping_tool())
            .expect("definição padrão de ping deve ser válida");
        super::tools::register_m2(&mut registry);
        registry
    }

    /// Registra uma ferramenta sem habilitá-la automaticamente (deny-by-default).
    pub fn register(&mut self, tool: ToolDefinition) -> Result<(), ToolError> {
        if !is_valid_tool_id(tool.id) {
            return Err(ToolError::InvalidInput(format!(
                "identificador de ferramenta inválido: '{}'",
                tool.id
            )));
        }
        if tool.timeout.is_zero() {
            return Err(ToolError::InvalidInput(format!(
                "timeout inválido para a ferramenta '{}'",
                tool.id
            )));
        }
        if self.tools.contains_key(tool.id) {
            return Err(ToolError::InvalidInput(format!(
                "ferramenta já registrada: '{}'",
                tool.id
            )));
        }
        self.tools.insert(tool.id.to_string(), Arc::new(tool));
        Ok(())
    }

    /// Habilita uma ferramenta já registrada para que possa ser listada e chamada.
    pub fn enable(&mut self, id: &str) -> Result<(), ToolError> {
        if !self.tools.contains_key(id) {
            return Err(ToolError::UnknownTool(id.to_string()));
        }
        self.enabled.insert(id.to_string());
        Ok(())
    }

    /// Desabilita uma ferramenta sem removê-la do registro.
    pub fn disable(&mut self, id: &str) -> bool {
        self.enabled.remove(id)
    }

    /// Registra e habilita explicitamente uma ferramenta no registro.
    pub fn register_and_enable(&mut self, tool: ToolDefinition) -> Result<(), ToolError> {
        let id = tool.id;
        self.register(tool)?;
        self.enable(id)
    }

    /// Indica se uma ferramenta existe e está explicitamente habilitada.
    pub fn is_enabled(&self, id: &str) -> bool {
        self.tools.contains_key(id) && self.enabled.contains(id)
    }

    /// Lista apenas as ferramentas habilitadas cujo `check_availability` retorna `Available`.
    pub fn list_available(&self, ctx: &ToolContext) -> Vec<ToolDescriptor> {
        self.tools
            .iter()
            .filter(|(id, def)| {
                self.enabled.contains(id.as_str())
                    && (def.check_availability)(ctx) == Availability::Available
            })
            .map(|(_, def)| ToolDescriptor::from(def.as_ref()))
            .collect()
    }

    /// Obtém uma ferramenta somente se existir, estiver habilitada e disponível no contexto.
    pub fn get_if_available(
        &self,
        id: &str,
        ctx: &ToolContext,
    ) -> Result<Arc<ToolDefinition>, ToolError> {
        let def = self
            .tools
            .get(id)
            .ok_or_else(|| ToolError::UnknownTool(id.to_string()))?;

        if !self.enabled.contains(id) {
            return Err(ToolError::DisabledTool(id.to_string()));
        }

        match (def.check_availability)(ctx) {
            Availability::Available => Ok(Arc::clone(def)),
            Availability::Unavailable { reason } => Err(ToolError::PrerequisiteMissing(reason)),
        }
    }
}

/// Valida `input` estritamente contra um JSON Schema de ferramenta.
///
/// Regras principais:
/// - Em objetos, `additionalProperties` é tratado como `false` quando omitido; só aceita
///   campos extras se `additionalProperties: true` explícito (ou subschema objeto válido).
/// - Suporta `type`, `required`, `properties`, `enum`, `minLength`, `maxLength`,
///   `minimum`, `maximum`, `items`, `minItems`, `maxItems`.
/// - Rejeita floats fracionários para `integer` e qualquer número não finito.
pub fn validate_schema(schema: &Value, input: &Value) -> Result<(), ToolError> {
    validate_node(schema, input, "$")
}

fn validate_node(schema: &Value, input: &Value, path: &str) -> Result<(), ToolError> {
    let schema_obj = schema.as_object().ok_or_else(|| {
        ToolError::InvalidInput(format!("schema inválido em '{path}': esperado objeto"))
    })?;

    if let Value::Number(num) = input {
        let _ = JsonNumber::parse(num, path)?;
    }

    validate_type_constraint(schema_obj, input, path)?;
    validate_enum_constraint(schema_obj, input, path)?;

    match input {
        Value::String(s) => validate_string_constraints(schema_obj, s, path)?,
        Value::Number(n) => validate_number_constraints(schema_obj, n, path)?,
        Value::Array(arr) => validate_array_constraints(schema_obj, arr, path)?,
        Value::Object(map) => validate_object_constraints(schema_obj, map, path)?,
        Value::Bool(_) | Value::Null => {}
    }

    Ok(())
}

fn validate_type_constraint(
    schema_obj: &Map<String, Value>,
    input: &Value,
    path: &str,
) -> Result<(), ToolError> {
    let Some(type_val) = schema_obj.get("type") else {
        return Ok(());
    };

    match type_val {
        Value::String(expected) => {
            if !matches_json_type(expected, input, path)? {
                return Err(ToolError::InvalidInput(format!(
                    "campo '{path}' deve ser do tipo '{expected}'"
                )));
            }
            Ok(())
        }
        Value::Array(expected_list) if !expected_list.is_empty() => {
            let mut allowed = Vec::with_capacity(expected_list.len());
            for item in expected_list {
                let ty = item.as_str().ok_or_else(|| {
                    ToolError::InvalidInput(format!(
                        "schema inválido em '{path}': item de 'type' deve ser string"
                    ))
                })?;
                if matches_json_type(ty, input, path)? {
                    return Ok(());
                }
                allowed.push(ty);
            }
            Err(ToolError::InvalidInput(format!(
                "campo '{path}' deve corresponder a um dos tipos: {}",
                allowed.join(", ")
            )))
        }
        _ => Err(ToolError::InvalidInput(format!(
            "schema inválido em '{path}': propriedade 'type' malformada"
        ))),
    }
}

fn matches_json_type(expected: &str, input: &Value, path: &str) -> Result<bool, ToolError> {
    match expected {
        "object" => Ok(input.is_object()),
        "string" => Ok(input.is_string()),
        "boolean" => Ok(input.is_boolean()),
        "array" => Ok(input.is_array()),
        "null" => Ok(input.is_null()),
        "number" => match input {
            Value::Number(n) => Ok(JsonNumber::parse(n, path).is_ok()),
            _ => Ok(false),
        },
        "integer" => match input {
            Value::Number(n) => Ok(is_json_integer(n, path)?),
            _ => Ok(false),
        },
        other => Err(ToolError::InvalidInput(format!(
            "schema inválido em '{path}': tipo desconhecido '{other}'"
        ))),
    }
}

fn is_json_integer(num: &serde_json::Number, path: &str) -> Result<bool, ToolError> {
    match JsonNumber::parse(num, path)? {
        JsonNumber::Int(_) => Ok(true),
        JsonNumber::Float(f) => Ok(f.is_finite() && f.fract() == 0.0),
    }
}

fn validate_enum_constraint(
    schema_obj: &Map<String, Value>,
    input: &Value,
    path: &str,
) -> Result<(), ToolError> {
    let Some(enum_val) = schema_obj.get("enum") else {
        return Ok(());
    };
    let choices = enum_val.as_array().ok_or_else(|| {
        ToolError::InvalidInput(format!(
            "schema inválido em '{path}': 'enum' deve ser uma lista"
        ))
    })?;
    if choices.is_empty() {
        return Err(ToolError::InvalidInput(format!(
            "schema inválido em '{path}': 'enum' não pode ser vazio"
        )));
    }
    if choices.iter().any(|candidate| json_values_equal(candidate, input)) {
        Ok(())
    } else {
        Err(ToolError::InvalidInput(format!(
            "valor fora das opções permitidas em '{path}'"
        )))
    }
}

fn validate_string_constraints(
    schema_obj: &Map<String, Value>,
    value: &str,
    path: &str,
) -> Result<(), ToolError> {
    let char_len = value.chars().count();
    if let Some(min_val) = schema_obj.get("minLength") {
        let min_len = parse_usize_constraint(min_val, "minLength", path)?;
        if char_len < min_len {
            return Err(ToolError::InvalidInput(format!(
                "campo '{path}' deve ter pelo menos {min_len} caractere(s)"
            )));
        }
    }
    if let Some(max_val) = schema_obj.get("maxLength") {
        let max_len = parse_usize_constraint(max_val, "maxLength", path)?;
        if char_len > max_len {
            return Err(ToolError::InvalidInput(format!(
                "campo '{path}' deve ter no máximo {max_len} caractere(s)"
            )));
        }
    }
    Ok(())
}

fn validate_number_constraints(
    schema_obj: &Map<String, Value>,
    value: &serde_json::Number,
    path: &str,
) -> Result<(), ToolError> {
    let parsed = JsonNumber::parse(value, path)?;
    if let Some(min_val) = schema_obj.get("minimum") {
        let bound = parse_number_constraint(min_val, "minimum", path)?;
        if parsed.lt(&bound) {
            return Err(ToolError::InvalidInput(format!(
                "campo '{path}' está abaixo do mínimo permitido ({min_val})"
            )));
        }
    }
    if let Some(max_val) = schema_obj.get("maximum") {
        let bound = parse_number_constraint(max_val, "maximum", path)?;
        if parsed.gt(&bound) {
            return Err(ToolError::InvalidInput(format!(
                "campo '{path}' está acima do máximo permitido ({max_val})"
            )));
        }
    }
    Ok(())
}

fn validate_array_constraints(
    schema_obj: &Map<String, Value>,
    items_val: &[Value],
    path: &str,
) -> Result<(), ToolError> {
    if let Some(min_val) = schema_obj.get("minItems") {
        let min_items = parse_usize_constraint(min_val, "minItems", path)?;
        if items_val.len() < min_items {
            return Err(ToolError::InvalidInput(format!(
                "campo '{path}' deve conter pelo menos {min_items} item(ns)"
            )));
        }
    }
    if let Some(max_val) = schema_obj.get("maxItems") {
        let max_items = parse_usize_constraint(max_val, "maxItems", path)?;
        if items_val.len() > max_items {
            return Err(ToolError::InvalidInput(format!(
                "campo '{path}' deve conter no máximo {max_items} item(ns)"
            )));
        }
    }
    if let Some(item_schema) = schema_obj.get("items") {
        if !item_schema.is_object() {
            return Err(ToolError::InvalidInput(format!(
                "schema inválido em '{path}': 'items' deve ser um objeto"
            )));
        }
        for (idx, elem) in items_val.iter().enumerate() {
            validate_node(item_schema, elem, &format!("{path}[{idx}]"))?;
        }
    }
    Ok(())
}

enum AdditionalPropertiesPolicy<'a> {
    Reject,
    Allow,
    Subschema(&'a Value),
}

fn validate_object_constraints(
    schema_obj: &Map<String, Value>,
    map: &Map<String, Value>,
    path: &str,
) -> Result<(), ToolError> {
    if let Some(req_val) = schema_obj.get("required") {
        let req_list = req_val.as_array().ok_or_else(|| {
            ToolError::InvalidInput(format!(
                "schema inválido em '{path}': 'required' deve ser uma lista"
            ))
        })?;
        for item in req_list {
            let key = item.as_str().ok_or_else(|| {
                ToolError::InvalidInput(format!(
                    "schema inválido em '{path}': item de 'required' deve ser string"
                ))
            })?;
            if !map.contains_key(key) {
                return Err(ToolError::InvalidInput(format!(
                    "campo obrigatório ausente: '{path}.{key}'"
                )));
            }
        }
    }

    let props = match schema_obj.get("properties") {
        Some(val) => Some(val.as_object().ok_or_else(|| {
            ToolError::InvalidInput(format!(
                "schema inválido em '{path}': 'properties' deve ser um objeto"
            ))
        })?),
        None => None,
    };

    let additional_policy = match schema_obj.get("additionalProperties") {
        None | Some(Value::Bool(false)) => AdditionalPropertiesPolicy::Reject,
        Some(Value::Bool(true)) => AdditionalPropertiesPolicy::Allow,
        Some(obj) if obj.is_object() => AdditionalPropertiesPolicy::Subschema(obj),
        Some(_) => {
            return Err(ToolError::InvalidInput(format!(
                "schema inválido em '{path}': 'additionalProperties' inválido"
            )));
        }
    };

    for (key, value) in map {
        let child_path = format!("{path}.{key}");
        if let Some(prop_schema) = props.and_then(|p| p.get(key)) {
            validate_node(prop_schema, value, &child_path)?;
        } else {
            match additional_policy {
                AdditionalPropertiesPolicy::Reject => {
                    return Err(ToolError::InvalidInput(format!(
                        "campo não permitido: '{child_path}'"
                    )));
                }
                AdditionalPropertiesPolicy::Allow => {}
                AdditionalPropertiesPolicy::Subschema(sub) => {
                    validate_node(sub, value, &child_path)?;
                }
            }
        }
    }

    Ok(())
}

fn parse_usize_constraint(val: &Value, name: &str, path: &str) -> Result<usize, ToolError> {
    let raw = val.as_u64().ok_or_else(|| {
        ToolError::InvalidInput(format!(
            "schema inválido em '{path}': '{name}' deve ser inteiro não negativo"
        ))
    })?;
    usize::try_from(raw).map_err(|_| {
        ToolError::InvalidInput(format!(
            "schema inválido em '{path}': '{name}' excede o limite suportado"
        ))
    })
}

fn parse_number_constraint(val: &Value, name: &str, path: &str) -> Result<JsonNumber, ToolError> {
    let num = val.as_number().ok_or_else(|| {
        ToolError::InvalidInput(format!(
            "schema inválido em '{path}': '{name}' deve ser numérico"
        ))
    })?;
    JsonNumber::parse(num, path)
}

#[derive(Debug, Clone, Copy)]
enum JsonNumber {
    Int(i128),
    Float(f64),
}

impl JsonNumber {
    fn parse(num: &serde_json::Number, path: &str) -> Result<Self, ToolError> {
        if let Some(i) = num.as_i64() {
            Ok(Self::Int(i128::from(i)))
        } else if let Some(u) = num.as_u64() {
            Ok(Self::Int(i128::from(u)))
        } else if let Some(f) = num.as_f64() {
            if f.is_finite() {
                Ok(Self::Float(f))
            } else {
                Err(ToolError::InvalidInput(format!(
                    "número não finito não permitido em '{path}'"
                )))
            }
        } else {
            Err(ToolError::InvalidInput(format!(
                "número fora do intervalo suportado em '{path}'"
            )))
        }
    }

    fn lt(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a < b,
            (Self::Float(a), Self::Float(b)) => a < b,
            (Self::Int(a), Self::Float(b)) => compare_int_float(*a, *b).is_lt(),
            (Self::Float(a), Self::Int(b)) => compare_int_float(*b, *a).is_gt(),
        }
    }

    fn gt(&self, other: &Self) -> bool {
        other.lt(self)
    }

    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a == b,
            (Self::Int(a), Self::Float(b)) | (Self::Float(b), Self::Int(a)) => {
                compare_int_float(*a, *b).is_eq()
            }
        }
    }
}

fn compare_int_float(int_val: i128, float_val: f64) -> std::cmp::Ordering {
    // Valores fora da faixa exata de i128 em f64 não devem sofrer overflow no cast.
    const MAX_EXACT_SAFE: f64 = 9_007_199_254_740_992.0; // 2^53
    if float_val.fract() == 0.0 && (-MAX_EXACT_SAFE..=MAX_EXACT_SAFE).contains(&float_val) {
        int_val.cmp(&(float_val as i128))
    } else if float_val < (i64::MIN as f64) {
        if int_val >= i128::from(i64::MIN) {
            std::cmp::Ordering::Greater
        } else {
            (int_val as f64)
                .partial_cmp(&float_val)
                .unwrap_or(std::cmp::Ordering::Equal)
        }
    } else if float_val > (i64::MAX as f64) {
        if int_val <= i128::from(i64::MAX) {
            std::cmp::Ordering::Less
        } else {
            (int_val as f64)
                .partial_cmp(&float_val)
                .unwrap_or(std::cmp::Ordering::Equal)
        }
    } else {
        (int_val as f64)
            .partial_cmp(&float_val)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}

fn json_values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(na), Value::Number(nb)) => {
            match (JsonNumber::parse(na, "$"), JsonNumber::parse(nb, "$")) {
                (Ok(ja), Ok(jb)) => ja.eq(&jb),
                _ => false,
            }
        }
        (Value::Array(aa), Value::Array(ab)) => {
            aa.len() == ab.len() && aa.iter().zip(ab).all(|(x, y)| json_values_equal(x, y))
        }
        (Value::Object(oa), Value::Object(ob)) => {
            oa.len() == ob.len()
                && oa
                    .iter()
                    .all(|(k, va)| ob.get(k).is_some_and(|vb| json_values_equal(va, vb)))
        }
        _ => a == b,
    }
}

/// Constrói a ferramenta de teste inofensiva `ping` da M1.
pub fn ping_tool() -> ToolDefinition {
    ToolDefinition {
        id: "ping",
        description: "Verifica se o núcleo local do assistente Lumen está ativo (ping local inofensivo, sem efeitos colaterais).",
        input_schema: json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }),
        risk: RiskLevel::Low,
        timeout: Duration::from_secs(5),
        check_availability: |_| Availability::Available,
        dry_run: |_| Ok("Executar ping local inofensivo do Lumen.".to_string()),
        execute: ping_execute,
        verify: ping_verify,
        audit_summary: |_, _| "ping local".to_string(),
    }
}

fn ping_expected_output() -> Value {
    json!({
        "status": "ok",
        "pong": "lumen-mochi-m1"
    })
}

fn ping_execute(_args: Value, _ctx: ExecutionContext) -> ToolFuture {
    Box::pin(async move { Ok(ping_expected_output()) })
}

fn ping_verify(_args: &Value, output: &Value) -> Result<(), ToolError> {
    if output == &ping_expected_output() {
        Ok(())
    } else {
        Err(ToolError::VerificationFailed(
            "a resposta do ping não corresponde ao retorno esperado".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noop_execute(_args: Value, _ctx: ExecutionContext) -> ToolFuture {
        Box::pin(async move { Ok(json!({ "ok": true })) })
    }

    fn unavailable_probe() -> ToolDefinition {
        ToolDefinition {
            id: "unavailable_probe",
            description: "Ferramenta de teste que exige adaptador ausente.",
            input_schema: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
            risk: RiskLevel::Low,
            timeout: Duration::from_secs(2),
            check_availability: |ctx| {
                if ctx.has_capability("mock_adapter") {
                    Availability::Available
                } else {
                    Availability::Unavailable {
                        reason: "adaptador ausente".to_string(),
                    }
                }
            },
            dry_run: |_| Ok("Executar sonda dependente de adaptador.".to_string()),
            execute: noop_execute,
            verify: |_, _| Ok(()),
            audit_summary: |_, _| "sonda indisponível".to_string(),
        }
    }

    #[test]
    fn default_registry_exposes_only_ping() {
        let registry = ToolRegistry::with_default_tools();
        let ctx = ToolContext::new();
        let listed = registry.list_available(&ctx);

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "ping");
        assert_eq!(listed[0].risk, RiskLevel::Low);
        assert!(registry.get_if_available("ping", &ctx).is_ok());
    }

    #[test]
    fn registry_rejects_unknown_and_disabled_tools() {
        let mut registry = ToolRegistry::new();
        registry
            .register(ping_tool())
            .expect("registro sem habilitar deve funcionar");

        let ctx = ToolContext::new();
        assert!(registry.list_available(&ctx).is_empty());
        assert_eq!(
            registry.get_if_available("ping", &ctx).unwrap_err(),
            ToolError::DisabledTool("ping".to_string())
        );
        assert_eq!(
            registry.get_if_available("nao_existe", &ctx).unwrap_err(),
            ToolError::UnknownTool("nao_existe".to_string())
        );

        registry.enable("ping").unwrap();
        assert!(registry.is_enabled("ping"));
        assert!(registry.get_if_available("ping", &ctx).is_ok());
        assert!(registry.disable("ping"));
        assert_eq!(
            registry.get_if_available("ping", &ctx).unwrap_err(),
            ToolError::DisabledTool("ping".to_string())
        );
    }

    #[test]
    fn understudy_hides_and_refuses_unavailable_tools() {
        let mut registry = ToolRegistry::with_default_tools();
        registry.register_and_enable(unavailable_probe()).unwrap();

        let missing_ctx = ToolContext::new();
        let listed = registry.list_available(&missing_ctx);
        assert_eq!(listed.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["ping"]);
        assert_eq!(
            registry
                .get_if_available("unavailable_probe", &missing_ctx)
                .unwrap_err(),
            ToolError::PrerequisiteMissing("adaptador ausente".to_string())
        );

        let ready_ctx = ToolContext::new().with_capability("mock_adapter");
        let listed_ready = registry.list_available(&ready_ctx);
        assert_eq!(
            listed_ready
                .iter()
                .map(|t| t.id.as_str())
                .collect::<Vec<_>>(),
            vec!["ping", "unavailable_probe"]
        );
        assert!(registry
            .get_if_available("unavailable_probe", &ready_ctx)
            .is_ok());
    }

    #[test]
    fn registry_validates_tool_ids_and_timeouts() {
        let mut registry = ToolRegistry::new();
        for invalid in ["", "PingUpper", "bad space", "dot.name"] {
            let mut def = ping_tool();
            def.id = invalid;
            assert!(matches!(
                registry.register(def),
                Err(ToolError::InvalidInput(_))
            ));
        }

        let mut zero_timeout = ping_tool();
        zero_timeout.timeout = Duration::ZERO;
        assert!(matches!(
            registry.register(zero_timeout),
            Err(ToolError::InvalidInput(_))
        ));
    }

    #[test]
    fn validate_schema_rejects_extra_fields_by_default_and_checks_types() {
        let ping_schema = ping_tool().input_schema;
        assert!(validate_schema(&ping_schema, &json!({})).is_ok());
        assert!(matches!(
            validate_schema(&ping_schema, &json!({ "cmd": "whoami" })),
            Err(ToolError::InvalidInput(_))
        ));
        assert!(matches!(
            validate_schema(&ping_schema, &json!(["not", "object"])),
            Err(ToolError::InvalidInput(_))
        ));

        let omitted_additional = json!({
            "type": "object",
            "required": ["action", "count"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["start", "stop"],
                    "minLength": 4,
                    "maxLength": 5
                },
                "count": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 10
                },
                "ratio": {
                    "type": "number",
                    "minimum": 0.0,
                    "maximum": 1.0
                },
                "active": { "type": "boolean" },
                "note": { "type": ["string", "null"] },
                "tags": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 3,
                    "items": { "type": "string", "minLength": 2 }
                }
            }
        });

        assert!(validate_schema(
            &omitted_additional,
            &json!({
                "action": "start",
                "count": 3,
                "ratio": 0.5,
                "active": true,
                "note": null,
                "tags": ["ok"]
            })
        )
        .is_ok());

        // Campo extra omitindo `additionalProperties` deve ser recusado.
        assert!(matches!(
            validate_schema(
                &omitted_additional,
                &json!({ "action": "start", "count": 1, "extra": 123 })
            ),
            Err(ToolError::InvalidInput(_))
        ));

        // Obrigatório ausente, enum inválido, float fracionário em integer, bounds.
        assert!(matches!(
            validate_schema(&omitted_additional, &json!({ "action": "start" })),
            Err(ToolError::InvalidInput(_))
        ));
        assert!(matches!(
            validate_schema(&omitted_additional, &json!({ "action": "pause", "count": 1 })),
            Err(ToolError::InvalidInput(_))
        ));
        assert!(matches!(
            validate_schema(&omitted_additional, &json!({ "action": "start", "count": 2.5 })),
            Err(ToolError::InvalidInput(_))
        ));
        assert!(matches!(
            validate_schema(&omitted_additional, &json!({ "action": "start", "count": 0 })),
            Err(ToolError::InvalidInput(_))
        ));
        assert!(matches!(
            validate_schema(&omitted_additional, &json!({ "action": "start", "count": 11 })),
            Err(ToolError::InvalidInput(_))
        ));
        assert!(matches!(
            validate_schema(
                &omitted_additional,
                &json!({ "action": "start", "count": 1, "tags": [] })
            ),
            Err(ToolError::InvalidInput(_))
        ));
        assert!(matches!(
            validate_schema(
                &omitted_additional,
                &json!({ "action": "start", "count": 1, "tags": ["x"] })
            ),
            Err(ToolError::InvalidInput(_))
        ));
    }

    #[test]
    fn validate_schema_allows_extra_only_when_explicitly_true() {
        let explicit_true = json!({
            "type": "object",
            "properties": { "a": { "type": "string" } },
            "additionalProperties": true
        });
        assert!(validate_schema(&explicit_true, &json!({ "a": "ok", "b": 42 })).is_ok());
    }

    #[test]
    fn risk_level_serializes_lowercase_and_errors_display_in_pt_br() {
        assert_eq!(serde_json::to_string(&RiskLevel::Low).unwrap(), "\"low\"");
        assert_eq!(
            serde_json::to_string(&RiskLevel::Medium).unwrap(),
            "\"medium\""
        );
        assert_eq!(serde_json::to_string(&RiskLevel::High).unwrap(), "\"high\"");

        let cases = [
            ToolError::UnknownTool("x".into()),
            ToolError::DisabledTool("x".into()),
            ToolError::PrerequisiteMissing("x".into()),
            ToolError::InvalidInput("x".into()),
            ToolError::Timeout,
            ToolError::Cancelled,
            ToolError::Paused,
            ToolError::PermissionDenied("x".into()),
            ToolError::VerificationFailed("x".into()),
            ToolError::ExecutionFailed("x".into()),
        ];
        for err in cases {
            assert!(!err.to_string().trim().is_empty());
        }
    }
}

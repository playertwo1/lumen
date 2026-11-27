use base64::Engine;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::digest;
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

pub const MAX_BRIDGE_BYTES: usize = 65_536;

pub fn generate_bridge_key_hex() -> Result<String, String> {
    let rng = SystemRandom::new();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes)
        .map_err(|e| format!("Failed to generate random bytes: {:?}", e))?;
    let mut hex_str = String::with_capacity(64);
    for b in &bytes {
        hex_str.push_str(&format!("{:02x}", b));
    }
    Ok(hex_str)
}

pub fn valid_room(room: &str) -> bool {
    if room.len() < 8 || room.len() > 64 {
        return false;
    }
    room.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '_' || c == '-'
    })
}

pub fn derive_aes_key(bridge_key_ascii: &str) -> [u8; 32] {
    let digest = digest::digest(&digest::SHA256, bridge_key_ascii.as_bytes());
    let mut key = [0u8; 32];
    key.copy_from_slice(digest.as_ref());
    key
}

#[derive(Serialize, Deserialize)]
struct BridgePayloadJson {
    iv: String,
    ct: String,
    tag: String,
}

pub fn encrypt_bridge_payload(bridge_key_ascii: &str, plaintext: &str) -> Result<String, String> {
    let aes_key_bytes = derive_aes_key(bridge_key_ascii);
    let unbound_key = UnboundKey::new(&AES_256_GCM, &aes_key_bytes)
        .map_err(|e| format!("Failed to create unbound key: {:?}", e))?;
    let key = LessSafeKey::new(unbound_key);

    let rng = SystemRandom::new();
    let mut iv_bytes = [0u8; 12];
    rng.fill(&mut iv_bytes)
        .map_err(|e| format!("Failed to generate IV: {:?}", e))?;

    let nonce = Nonce::assume_unique_for_key(iv_bytes);

    let mut in_out = plaintext.as_bytes().to_vec();
    key.seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
        .map_err(|e| format!("Failed to encrypt: {:?}", e))?;

    // in_out now contains ciphertext + 16-byte tag at the end
    let split_idx = in_out.len() - 16;
    let ct = &in_out[..split_idx];
    let tag = &in_out[split_idx..];

    let iv_b64 = base64::engine::general_purpose::STANDARD.encode(iv_bytes);
    let ct_b64 = base64::engine::general_purpose::STANDARD.encode(ct);
    let tag_b64 = base64::engine::general_purpose::STANDARD.encode(tag);

    let payload = BridgePayloadJson {
        iv: iv_b64,
        ct: ct_b64,
        tag: tag_b64,
    };

    let json = serde_json::to_string(&payload)
        .map_err(|e| format!("Failed to serialize payload JSON: {:?}", e))?;

    if json.len() > MAX_BRIDGE_BYTES {
        return Err(format!(
            "Encrypted payload exceeds MAX_BRIDGE_BYTES: {} > {}",
            json.len(),
            MAX_BRIDGE_BYTES
        ));
    }

    Ok(json)
}

pub fn decrypt_bridge_payload(bridge_keys: &[String], wire_json: &str) -> Result<String, String> {
    if wire_json.len() > MAX_BRIDGE_BYTES {
        return Err("Wire JSON exceeds MAX_BRIDGE_BYTES".into());
    }

    let payload: BridgePayloadJson = serde_json::from_str(wire_json)
        .map_err(|e| format!("Failed to parse wire JSON: {:?}", e))?;

    let iv_bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.iv.as_bytes())
        .map_err(|e| format!("Failed to decode IV base64: {:?}", e))?;
    let mut ct_bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.ct.as_bytes())
        .map_err(|e| format!("Failed to decode CT base64: {:?}", e))?;
    let tag_bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.tag.as_bytes())
        .map_err(|e| format!("Failed to decode Tag base64: {:?}", e))?;

    if iv_bytes.len() != 12 {
        return Err(format!("Invalid IV length: {}", iv_bytes.len()));
    }
    if tag_bytes.len() != 16 {
        return Err(format!("Invalid Tag length: {}", tag_bytes.len()));
    }

    let mut iv_arr = [0u8; 12];
    iv_arr.copy_from_slice(&iv_bytes);

    // Append tag to ciphertext for ring's open_in_place
    ct_bytes.extend_from_slice(&tag_bytes);

    let mut last_err = "No bridge keys provided".to_string();

    for key_ascii in bridge_keys {
        let aes_key_bytes = derive_aes_key(key_ascii);
        let unbound_key = match UnboundKey::new(&AES_256_GCM, &aes_key_bytes) {
            Ok(k) => k,
            Err(e) => {
                last_err = format!("UnboundKey error: {:?}", e);
                continue;
            }
        };
        let key = LessSafeKey::new(unbound_key);

        let nonce = Nonce::assume_unique_for_key(iv_arr);
        let mut work_buf = ct_bytes.clone();
        match key.open_in_place(nonce, Aad::empty(), &mut work_buf) {
            Ok(plaintext_slice) => {
                let s = match String::from_utf8(plaintext_slice.to_vec()) {
                    Ok(s) => s,
                    Err(e) => return Err(format!("Decrypted bytes are not valid UTF-8: {:?}", e)),
                };
                return Ok(s);
            }
            Err(e) => {
                last_err = format!("Decryption failed with key: {:?}", e);
            }
        }
    }

    Err(last_err)
}

// -----------------------------------------------------------------------------
// MQTT protocol crypto and pairing types (docs/roadmap/mqtt/PROTOCOLO.md)
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MqttPairCode {
    pub t: String,
    pub v: u32,
    pub uri: String,
    pub u: String,
    pub p: String,
    pub sala: String,
    pub k: String,
}

pub fn extract_broker_host(uri: &str) -> String {
    let without_scheme = if let Some(s) = uri.strip_prefix("mqtts://") {
        s
    } else if let Some(s) = uri.strip_prefix("mqtt://") {
        s
    } else {
        uri
    };
    let host_and_port = without_scheme.split('/').next().unwrap_or("");
    let host = host_and_port.split(':').next().unwrap_or("");
    host.to_string()
}

pub fn parse_mqtt_pair_code(raw: &str) -> Result<MqttPairCode, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Código MQTT vazio".into());
    }
    let code: MqttPairCode = serde_json::from_str(trimmed)
        .map_err(|e| format!("JSON do código MQTT inválido: {e}"))?;

    if code.t != "sm-mqtt" {
        return Err(format!("Tipo esperado 'sm-mqtt', recebido '{}'", code.t));
    }
    if code.v != 1 {
        return Err(format!("Versão esperada 1, recebida {}", code.v));
    }

    let uri = code.uri.trim();
    let is_mqtts = uri.starts_with("mqtts://");
    let is_local_test = uri.starts_with("mqtt://127.0.0.1") || uri.starts_with("mqtt://localhost");
    if !is_mqtts && !is_local_test {
        return Err("URI do broker deve usar o protocolo mqtts://".into());
    }
    let host = extract_broker_host(uri);
    if host.is_empty() {
        return Err("URI do broker inválida (host vazio)".into());
    }

    if code.u.trim().is_empty() {
        return Err("Usuário do broker vazio".into());
    }
    if code.p.trim().is_empty() {
        return Err("Senha do broker vazia".into());
    }

    if code.sala.len() != 12
        || !code
            .sala
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        return Err("Identificador de sala deve conter exatamente 12 caracteres [a-z0-9]".into());
    }

    if code.k.len() != 64 || !code.k.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Chave da sala deve conter exatamente 64 caracteres hexadecimais".into());
    }

    Ok(code)
}

#[derive(Serialize, Deserialize)]
struct MqttEnvelopeJson {
    v: u32,
    from: String,
    iv: String,
    ct: String,
    tag: String,
}

pub fn seal_mqtt_envelope(
    key_text: &str,
    from: &str,
    plain_json: &str,
) -> Result<String, String> {
    if from.trim().is_empty() {
        return Err("Campo 'from' não pode ser vazio".into());
    }
    let aes_key_bytes = derive_aes_key(key_text);
    let unbound_key = UnboundKey::new(&AES_256_GCM, &aes_key_bytes)
        .map_err(|e| format!("Failed to create unbound key: {:?}", e))?;
    let key = LessSafeKey::new(unbound_key);

    let rng = SystemRandom::new();
    let mut iv_bytes = [0u8; 12];
    rng.fill(&mut iv_bytes)
        .map_err(|e| format!("Failed to generate IV: {:?}", e))?;

    let nonce = Nonce::assume_unique_for_key(iv_bytes);

    let mut in_out = plain_json.as_bytes().to_vec();
    let aad = Aad::from(from.as_bytes());

    key.seal_in_place_append_tag(nonce, aad, &mut in_out)
        .map_err(|e| format!("Failed to encrypt MQTT payload: {:?}", e))?;

    let split_idx = in_out.len() - 16;
    let ct = &in_out[..split_idx];
    let tag = &in_out[split_idx..];

    let iv_b64 = base64::engine::general_purpose::STANDARD.encode(iv_bytes);
    let ct_b64 = base64::engine::general_purpose::STANDARD.encode(ct);
    let tag_b64 = base64::engine::general_purpose::STANDARD.encode(tag);

    let env = MqttEnvelopeJson {
        v: 1,
        from: from.to_string(),
        iv: iv_b64,
        ct: ct_b64,
        tag: tag_b64,
    };

    serde_json::to_string(&env).map_err(|e| format!("Failed to serialize MQTT envelope: {e}"))
}

pub fn open_mqtt_envelope(
    key_text: &str,
    env_json: &str,
) -> Result<(String, String), String> {
    let env: MqttEnvelopeJson = serde_json::from_str(env_json)
        .map_err(|e| format!("Failed to parse MQTT envelope JSON: {e}"))?;

    if env.v != 1 {
        return Err(format!("Unsupported envelope version: {}", env.v));
    }
    if env.from.trim().is_empty() {
        return Err("Missing or empty 'from' field in envelope".into());
    }

    let iv_bytes = base64::engine::general_purpose::STANDARD
        .decode(env.iv.as_bytes())
        .map_err(|e| format!("Failed to decode IV base64: {e}"))?;
    if iv_bytes.len() != 12 {
        return Err(format!("Invalid IV length: {}", iv_bytes.len()));
    }

    let mut ct_bytes = base64::engine::general_purpose::STANDARD
        .decode(env.ct.as_bytes())
        .map_err(|e| format!("Failed to decode CT base64: {e}"))?;

    let tag_bytes = base64::engine::general_purpose::STANDARD
        .decode(env.tag.as_bytes())
        .map_err(|e| format!("Failed to decode Tag base64: {e}"))?;
    if tag_bytes.len() != 16 {
        return Err(format!("Invalid Tag length: {}", tag_bytes.len()));
    }

    let mut iv_arr = [0u8; 12];
    iv_arr.copy_from_slice(&iv_bytes);
    ct_bytes.extend_from_slice(&tag_bytes);

    let aes_key_bytes = derive_aes_key(key_text);
    let unbound_key = UnboundKey::new(&AES_256_GCM, &aes_key_bytes)
        .map_err(|e| format!("Failed to create unbound key: {:?}", e))?;
    let key = LessSafeKey::new(unbound_key);

    let nonce = Nonce::assume_unique_for_key(iv_arr);
    let aad = Aad::from(env.from.as_bytes());

    let mut work_buf = ct_bytes;
    match key.open_in_place(nonce, aad, &mut work_buf) {
        Ok(plaintext_slice) => {
            let plain = String::from_utf8(plaintext_slice.to_vec())
                .map_err(|e| format!("Decrypted bytes are not valid UTF-8: {:?}", e))?;
            Ok((env.from, plain))
        }
        Err(e) => Err(format!("Decryption failed: {:?}", e)),
    }
}

pub fn voice_bearer_hex(key_text: &str) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, key_text.as_bytes());
    let tag = hmac::sign(&key, b"voice");
    let mut out = String::with_capacity(64);
    for b in tag.as_ref() {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_bridge_key_hex() {
        let key = generate_bridge_key_hex().unwrap();
        assert_eq!(key.len(), 64);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn test_valid_room() {
        assert!(!valid_room("short")); // < 8
        assert!(valid_room("room-123_ABC")); // valid
        assert!(!valid_room("room with space"));
        assert!(!valid_room(&"a".repeat(65))); // > 64
    }

    #[test]
    fn test_round_trip_and_multi_key() {
        let key1 = generate_bridge_key_hex().unwrap();
        let key2 = generate_bridge_key_hex().unwrap();
        let plaintext = "Hello, ESP32 Bridge!";

        let encrypted = encrypt_bridge_payload(&key2, plaintext).unwrap();

        // Decrypt with multi-key list where correct key is second
        let decrypted = decrypt_bridge_payload(&[key1.clone(), key2.clone()], &encrypted).unwrap();
        assert_eq!(decrypted, plaintext);

        // Decrypt with single correct key
        let decrypted_single = decrypt_bridge_payload(&[key2.clone()], &encrypted).unwrap();
        assert_eq!(decrypted_single, plaintext);
    }

    #[test]
    fn test_tampering_rejection() {
        let key = generate_bridge_key_hex().unwrap();
        let encrypted = encrypt_bridge_payload(&key, "Secret message").unwrap();

        // Tamper with JSON fields
        let mut val: serde_json::Value = serde_json::from_str(&encrypted).unwrap();

        // Tamper IV
        let orig_iv = val["iv"].as_str().unwrap().to_string();
        val["iv"] = serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(b"wrongiv12345"));
        let tampered_iv = val.to_string();
        assert!(decrypt_bridge_payload(&[key.clone()], &tampered_iv).is_err());
        val["iv"] = serde_json::Value::String(orig_iv);

        // Tamper CT
        let orig_ct = val["ct"].as_str().unwrap().to_string();
        val["ct"] = serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(b"tamperedciphertext"));
        let tampered_ct = val.to_string();
        assert!(decrypt_bridge_payload(&[key.clone()], &tampered_ct).is_err());
        val["ct"] = serde_json::Value::String(orig_ct);

        // Tamper Tag
        let _orig_tag = val["tag"].as_str().unwrap().to_string();
        val["tag"] = serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(b"wrongtag12345678"));
        let tampered_tag = val.to_string();
        assert!(decrypt_bridge_payload(&[key.clone()], &tampered_tag).is_err());
    }

    #[test]
    fn test_max_bridge_bytes_exceeded() {
        let key = generate_bridge_key_hex().unwrap();
        let huge_plaintext = "a".repeat(MAX_BRIDGE_BYTES + 100);
        let res = encrypt_bridge_payload(&key, &huge_plaintext);
        assert!(res.is_err());
    }

    #[test]
    fn test_parse_mqtt_pair_code() {
        let valid_json = r#"{
            "t": "sm-mqtt",
            "v": 1,
            "uri": "mqtts://broker.example.com:8883",
            "u": "user123",
            "p": "pass456",
            "sala": "abc123def456",
            "k": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        }"#;
        let parsed = parse_mqtt_pair_code(valid_json).expect("valid code");
        assert_eq!(parsed.sala, "abc123def456");
        assert_eq!(extract_broker_host(&parsed.uri), "broker.example.com");

        // Test rejection: wrong t
        assert!(parse_mqtt_pair_code(&valid_json.replace("sm-mqtt", "other")).is_err());
        // Test rejection: wrong v
        assert!(parse_mqtt_pair_code(&valid_json.replace("\"v\": 1", "\"v\": 2")).is_err());
        // Test rejection: non-mqtts (and not local)
        assert!(parse_mqtt_pair_code(&valid_json.replace("mqtts://", "http://")).is_err());
        // Test rejection: sala length != 12
        assert!(parse_mqtt_pair_code(&valid_json.replace("abc123def456", "short")).is_err());
        // Test rejection: sala uppercase
        assert!(parse_mqtt_pair_code(&valid_json.replace("abc123def456", "ABC123DEF456")).is_err());
        // Test rejection: k length != 64
        assert!(parse_mqtt_pair_code(&valid_json.replace("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef", "shortkey")).is_err());
    }

    #[test]
    fn test_mqtt_envelope_roundtrip_and_tampering() {
        let key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let wrong_key = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
        let plain = r#"{"contract_version":1,"entity_id":"t1","payload":{"text":"Comprar leite"}}"#;

        let env_json = seal_mqtt_envelope(key, "lumen", plain).expect("seal");
        let (from, decrypted) = open_mqtt_envelope(key, &env_json).expect("open");
        assert_eq!(from, "lumen");
        assert_eq!(decrypted, plain);

        // Wrong key failure
        assert!(open_mqtt_envelope(wrong_key, &env_json).is_err());

        // Tamper from (AAD mismatch)
        let tampered_from = env_json.replace("\"from\":\"lumen\"", "\"from\":\"placa\"");
        assert!(open_mqtt_envelope(key, &tampered_from).is_err());

        // Missing from
        let mut no_from: serde_json::Value = serde_json::from_str(&env_json).unwrap();
        no_from.as_object_mut().unwrap().remove("from");
        assert!(open_mqtt_envelope(key, &no_from.to_string()).is_err());

        // Tamper ciphertext
        let mut v: serde_json::Value = serde_json::from_str(&env_json).unwrap();
        v["ct"] = serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(b"corrupted"));
        assert!(open_mqtt_envelope(key, &v.to_string()).is_err());
    }

    #[test]
    fn test_voice_bearer_hex() {
        let key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let bearer = voice_bearer_hex(key);
        // Verified with node / RFC HMAC-SHA256
        assert_eq!(
            bearer,
            "3717157b1cbcf20a0ec4f22acbbe162bd033066c6528db618d6c7155a67f6e8b"
        );
    }
}

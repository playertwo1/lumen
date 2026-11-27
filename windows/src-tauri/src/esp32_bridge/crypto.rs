use base64::Engine;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::digest;
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
}

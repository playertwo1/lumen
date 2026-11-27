use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct RelayBridgeMsg {
    pub id: String,
    pub body: String,
    pub ts: u64,
}

#[derive(Debug, Clone)]
pub struct RelayClient {
    client: reqwest::Client,
}

impl RelayClient {
    pub fn new() -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .map_err(|e| format!("Failed to build reqwest client: {:?}", e))?;
        Ok(Self { client })
    }

    pub async fn pull_bridge(
        &self,
        base_url: &str,
        relay_key: &str,
        room: &str,
    ) -> Result<Vec<RelayBridgeMsg>, String> {
        if !super::crypto::valid_room(room) {
            return Err(format!("Invalid room name: {}", room));
        }
        if relay_key.trim().is_empty() {
            return Err("Relay key cannot be empty".into());
        }

        let url = format!("{}/v1/bridge/{}", base_url.trim_end_matches('/'), room);

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", relay_key))
            .send()
            .await
            .map_err(|e| format!("Network error pulling bridge: {:?}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Relay pull failed with status {}: {}", status, text));
        }

        #[derive(Deserialize)]
        struct PullResponse {
            messages: Vec<RelayBridgeMsg>,
        }

        let pull_resp: PullResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse bridge JSON response: {:?}", e))?;

        Ok(pull_resp.messages)
    }

    pub async fn post_bridge(
        &self,
        base_url: &str,
        relay_key: &str,
        room: &str,
        body_json: &str,
    ) -> Result<(), String> {
        if !super::crypto::valid_room(room) {
            return Err(format!("Invalid room name: {}", room));
        }
        if body_json.len() > super::crypto::MAX_BRIDGE_BYTES {
            return Err(format!(
                "Body JSON exceeds MAX_BRIDGE_BYTES: {} > {}",
                body_json.len(),
                super::crypto::MAX_BRIDGE_BYTES
            ));
        }
        if relay_key.trim().is_empty() {
            return Err("Relay key cannot be empty".into());
        }

        let url = format!("{}/v1/bridge/{}", base_url.trim_end_matches('/'), room);

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", relay_key))
            .header("Content-Type", "application/json")
            .body(body_json.to_string())
            .send()
            .await
            .map_err(|e| format!("Network error posting bridge: {:?}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Relay post failed with status {}: {}", status, text));
        }

        Ok(())
    }

    pub async fn put_usage(
        &self,
        base_url: &str,
        relay_key: &str,
        payload: &serde_json::Value,
    ) -> Result<(), String> {
        if relay_key.trim().is_empty() {
            return Err("Relay key cannot be empty".into());
        }

        let serialized = serde_json::to_string(payload)
            .map_err(|e| format!("Failed to serialize usage payload: {:?}", e))?;

        if serialized.len() > 512 {
            return Err(format!(
                "Usage payload exceeds 512 bytes: {} > 512",
                serialized.len()
            ));
        }

        let url = format!("{}/v1/usage", base_url.trim_end_matches('/'));

        let resp = self
            .client
            .put(&url)
            .header("Authorization", format!("Bearer {}", relay_key))
            .header("Content-Type", "application/json")
            .body(serialized)
            .send()
            .await
            .map_err(|e| format!("Network error putting usage: {:?}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Relay put usage failed with status {}: {}", status, text));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn spawn_test_server() -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}", addr);

        let handle = tokio::spawn(async move {
            loop {
                let (mut socket, _) = match listener.accept().await {
                    Ok(s) => s,
                    Err(_) => break,
                };

                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let n = match socket.read(&mut buf).await {
                        Ok(n) if n > 0 => n,
                        _ => return,
                    };
                    let request = String::from_utf8_lossy(&buf[..n]);
                    let mut lines = request.lines();
                    let request_line = lines.next().unwrap_or("");
                    
                    let mut auth_ok = false;
                    for line in lines {
                        if line.to_lowercase().starts_with("authorization: bearer secret-token") {
                            auth_ok = true;
                        }
                    }

                    if !auth_ok {
                        let response = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 9\r\n\r\nUnauthorized";
                        let _ = socket.write_all(response.as_bytes()).await;
                        return;
                    }

                    if request_line.starts_with("GET /v1/bridge/") {
                        let json_resp = r#"{"messages":[{"id":"msg-1","body":"hello","ts":123456}]}"#;
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                            json_resp.len(),
                            json_resp
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                    } else if request_line.starts_with("POST /v1/bridge/") {
                        let response = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";
                        let _ = socket.write_all(response.as_bytes()).await;
                    } else if request_line.starts_with("PUT /v1/usage") {
                        let response = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";
                        let _ = socket.write_all(response.as_bytes()).await;
                    } else {
                        let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nNot Found";
                        let _ = socket.write_all(response.as_bytes()).await;
                    }
                });
            }
        });

        (base_url, handle)
    }

    #[tokio::test]
    async fn test_relay_client_flow() {
        let (base_url, _server_handle) = spawn_test_server().await;
        let client = RelayClient::new().unwrap();
        let room = "testroom123";
        let relay_key = "secret-token";

        // Test pull_bridge
        let msgs = client.pull_bridge(&base_url, relay_key, room).await.unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].id, "msg-1");
        assert_eq!(msgs[0].body, "hello");

        // Test post_bridge
        client.post_bridge(&base_url, relay_key, room, "{\"test\":true}").await.unwrap();

        // Test put_usage
        let usage_payload = serde_json::json!({"active": true});
        client.put_usage(&base_url, relay_key, &usage_payload).await.unwrap();

        // Test usage size limit (> 512 bytes)
        let big_payload = serde_json::json!({"data": "a".repeat(600)});
        let usage_res = client.put_usage(&base_url, relay_key, &big_payload).await;
        assert!(usage_res.is_err());

        // Test invalid room
        let invalid_pull = client.pull_bridge(&base_url, relay_key, "short").await;
        assert!(invalid_pull.is_err());
    }
}

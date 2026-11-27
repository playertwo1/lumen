use std::time::Duration;

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

        if serialized.len() > 2048 {
            return Err(format!(
                "Usage payload exceeds 2048 bytes: {} > 2048",
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

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
                        let response = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\n\r\nUnauthorized";
                        let _ = socket.write_all(response.as_bytes()).await;
                        return;
                    }

                    if request_line.starts_with("PUT /v1/usage") {
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
    async fn test_relay_put_usage_flow() {
        let (base_url, _server_handle) = spawn_test_server().await;
        let client = RelayClient::new().unwrap();
        let relay_key = "secret-token";

        let usage_payload = serde_json::json!({"active": true});
        client.put_usage(&base_url, relay_key, &usage_payload).await.unwrap();

        let big_payload = serde_json::json!({"data": "a".repeat(2100)});
        assert!(client.put_usage(&base_url, relay_key, &big_payload).await.is_err());

        assert!(client.put_usage(&base_url, "wrong-token", &usage_payload).await.is_err());
    }
}

use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest,
    http::{HeaderValue, Request},
};

/// 华北2（北京）固定 WebSocket 地址。
pub const ASR_WS_URL: &str = "wss://dashscope.aliyuncs.com/api-ws/v1/inference";

pub fn ws_request(api_key: &str) -> Result<Request<()>, String> {
    let mut request = ASR_WS_URL.into_client_request().map_err(|e| e.to_string())?;
    let headers = request.headers_mut();
    headers.insert(
        "Authorization",
        HeaderValue::from_str(&format!("Bearer {}", api_key.trim())).map_err(|e| e.to_string())?,
    );
    Ok(request)
}

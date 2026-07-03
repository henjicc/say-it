use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest,
    http::{HeaderName, HeaderValue, Request},
};

/// 华北2（北京）固定 WebSocket 地址。
pub const ASR_WS_URL: &str = "wss://dashscope.aliyuncs.com/api-ws/v1/inference";
pub const QWEN_REALTIME_WS_URL: &str = "wss://dashscope.aliyuncs.com/api-ws/v1/realtime";

pub fn ws_request(api_key: &str) -> Result<Request<()>, String> {
    ws_request_with_url(api_key, ASR_WS_URL, &[])
}

pub fn qwen_realtime_request(api_key: &str, model: &str) -> Result<Request<()>, String> {
    let url = format!("{}?model={}", QWEN_REALTIME_WS_URL, model.trim());
    ws_request_with_url(api_key, &url, &[("OpenAI-Beta", "realtime=v1")])
}

fn ws_request_with_url(
    api_key: &str,
    url: &str,
    extra_headers: &[(&str, &str)],
) -> Result<Request<()>, String> {
    let mut request = url.into_client_request().map_err(|e| e.to_string())?;
    let headers = request.headers_mut();
    headers.insert(
        "Authorization",
        HeaderValue::from_str(&format!("Bearer {}", api_key.trim())).map_err(|e| e.to_string())?,
    );
    for (key, value) in extra_headers {
        headers.insert(
            HeaderName::from_bytes(key.as_bytes()).map_err(|e| e.to_string())?,
            HeaderValue::from_str(value).map_err(|e| e.to_string())?,
        );
    }
    Ok(request)
}

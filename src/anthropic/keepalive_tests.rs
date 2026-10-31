//! 429 无限立即重试打开时，流式请求在等待上游响应头期间的下游保活测试。
//!
//! 上游全部走 loopback 的 axum mock：先连续回 429，之后回 200 事件流。凭据是合成
//! API Key（`kiroApiKey` 直接作为 Bearer），不触发 token 刷新，整个过程没有任何外网访问。
use super::{
    handlers::post_messages,
    middleware::{AppState, KeyContext},
};
use crate::{
    admin::trace_db::TraceKeySource,
    kiro::{
        endpoint::{KiroEndpoint, RequestContext},
        model::credentials::KiroCredentials,
        parser::crc::crc32,
        provider::KiroProvider,
        token_manager::MultiTokenManager,
    },
    model::config::{Config, ToolCompatibilityMode},
};
use axum::{Extension, Json, body::Body, extract::State, http::StatusCode, response::Response};
use bytes::Bytes;
use futures::StreamExt;
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

/// 上游 mock 行为：每次响应前延迟 `delay`，前 `throttle_times` 次（或恒）回容量类 429。
struct MockConfig {
    throttle_times: usize,
    always_throttle: bool,
    delay: Duration,
}

struct MockUpstream {
    url: String,
    calls: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for MockUpstream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl MockUpstream {
    async fn start(throttle_times: usize, always_throttle: bool, delay: Duration) -> Self {
        let calls = Arc::new(AtomicUsize::new(0));
        let state = (
            calls.clone(),
            Arc::new(MockConfig {
                throttle_times,
                always_throttle,
                delay,
            }),
        );
        let app = axum::Router::new().fallback(mock_handler).with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self { url, calls, task }
    }

    fn count(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

async fn mock_handler(
    State((calls, config)): State<(Arc<AtomicUsize>, Arc<MockConfig>)>,
) -> Response {
    let call = calls.fetch_add(1, Ordering::Relaxed);
    tokio::time::sleep(config.delay).await;
    let throttled = config.always_throttle || call < config.throttle_times;
    if throttled {
        // 容量类 429：不会冷却凭据，无限立即重试在同一凭据上直接重来。
        return Response::builder()
            .status(StatusCode::TOO_MANY_REQUESTS)
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"message":"Insufficient model capacity","reason":"INSUFFICIENT_MODEL_CAPACITY"}"#,
            ))
            .unwrap();
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&frame(
        "assistantResponseEvent",
        json!({"content": "hello from upstream"}),
    ));
    bytes.extend_from_slice(&frame(
        "metadataEvent",
        json!({"tokenUsage": {"uncachedInputTokens": 13, "outputTokens": 17, "cacheReadInputTokens": 0, "cacheWriteInputTokens": 0}}),
    ));
    Response::new(Body::from(bytes))
}

fn frame(event: &str, payload: serde_json::Value) -> Bytes {
    let mut headers = Vec::new();
    for (name, value) in [(":message-type", "event"), (":event-type", event)] {
        headers.push(name.len() as u8);
        headers.extend_from_slice(name.as_bytes());
        headers.push(7);
        headers.extend_from_slice(&(value.len() as u16).to_be_bytes());
        headers.extend_from_slice(value.as_bytes());
    }
    let payload = serde_json::to_vec(&payload).unwrap();
    let mut out = Vec::new();
    out.extend_from_slice(&((16 + headers.len() + payload.len()) as u32).to_be_bytes());
    out.extend_from_slice(&(headers.len() as u32).to_be_bytes());
    out.extend_from_slice(&crc32(&out).to_be_bytes());
    out.extend(headers);
    out.extend(payload);
    out.extend_from_slice(&crc32(&out).to_be_bytes());
    Bytes::from(out)
}

struct LoopbackEndpoint(String);

impl KiroEndpoint for LoopbackEndpoint {
    fn name(&self) -> &'static str {
        "loopback"
    }
    fn api_url(&self, _: &RequestContext<'_>) -> String {
        self.0.clone()
    }
    fn mcp_url(&self, _: &RequestContext<'_>) -> String {
        self.0.clone()
    }
    fn decorate_api(
        &self,
        request: reqwest::RequestBuilder,
        _: &RequestContext<'_>,
    ) -> reqwest::RequestBuilder {
        request
    }
    fn decorate_mcp(
        &self,
        request: reqwest::RequestBuilder,
        _: &RequestContext<'_>,
    ) -> reqwest::RequestBuilder {
        request
    }
    fn transform_api_body(&self, body: &str, _: &RequestContext<'_>) -> String {
        body.to_owned()
    }
}

fn state(upstream: &MockUpstream, infinite_retry: bool) -> AppState {
    let credential: KiroCredentials = serde_json::from_value(json!({
        "id": 1,
        "kiroApiKey": "fixture-only-keepalive",
        "machineId": "0000000000000000000000000000000000000000000000000000000000000000"
    }))
    .unwrap();
    let mut config = Config::default();
    config.model_api_429_infinite_retry = infinite_retry;
    let manager =
        Arc::new(MultiTokenManager::new(config, vec![credential], None, None, false).unwrap());
    let endpoints = HashMap::from([(
        "loopback".to_owned(),
        Arc::new(LoopbackEndpoint(upstream.url.clone())) as Arc<dyn KiroEndpoint>,
    )]);
    let provider = KiroProvider::new(manager, endpoints, "loopback".to_owned());
    AppState::new(false, ToolCompatibilityMode::Raw).with_shared_kiro_provider(Arc::new(provider))
}

fn key() -> KeyContext {
    KeyContext {
        key_id: 0,
        group: None,
        key_source: TraceKeySource::MasterApiKey,
        client_ip: None,
    }
}

async fn stream_response(state: AppState) -> Response {
    let payload = json!({
        "model": "gpt-5.6-sol",
        "max_tokens": 64,
        "stream": true,
        "messages": [{"role": "user", "content": "hello"}]
    });
    post_messages(
        State(state),
        Extension(key()),
        Json(serde_json::from_value(payload).unwrap()),
    )
    .await
}

#[tokio::test]
async fn infinite_retry_stream_keeps_downstream_alive_with_pings() {
    // 前 10 次 429，每次耗时 300ms，总等待约 3s。
    let upstream = MockUpstream::start(10, false, Duration::from_millis(300)).await;
    let started = Instant::now();
    let response = stream_response(state(&upstream, true)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        started.elapsed() < Duration::from_millis(200),
        "响应头应在等待上游期间立即返回，实际耗时 {:?}",
        started.elapsed()
    );

    let mut body = response.into_body().into_data_stream();
    let mut text = String::new();
    let mut pings = 0;
    let mut first_ping_at = None;
    let mut message_start_at = None;
    while let Some(chunk) = tokio::time::timeout(Duration::from_secs(10), body.next())
        .await
        .expect("等待保活期间连接被断开")
    {
        let chunk = chunk.unwrap();
        let piece = String::from_utf8_lossy(&chunk).to_string();
        if piece.contains("event: ping") {
            pings += piece.matches("event: ping").count();
            first_ping_at.get_or_insert_with(|| started.elapsed());
        }
        if piece.contains("event: message_start") {
            message_start_at.get_or_insert_with(|| started.elapsed());
        }
        text.push_str(&piece);
        if text.contains("event: message_stop") {
            break;
        }
    }

    // (a) 3s 内连接未断：流一路读到 message_stop 即证明；首个 ping 必须在 3s 之前到达。
    assert!(
        first_ping_at.expect("等待期没有收到任何 ping") < Duration::from_secs(3),
        "首个 ping 到得太晚"
    );
    // (b) 等待期至少 2 个 ping。
    assert!(pings >= 2, "等待期只收到 {pings} 个 ping");
    // 上游 429 持续约 3s：message_start 只能在重试结束后到达。
    assert!(
        message_start_at.expect("缺少 message_start") >= Duration::from_secs(2),
        "message_start 早于上游 429 结束"
    );
    // (c) 收到正常内容事件且无流内 error。
    assert!(text.contains("event: content_block_delta"), "{text}");
    assert!(text.contains("hello from upstream"), "{text}");
    assert!(text.contains("event: message_stop"), "{text}");
    assert!(!text.contains("event: error"), "{text}");
    // 顺序：初始事件（message_start）必须早于任何内容事件。
    assert!(
        text.find("event: message_start").unwrap()
            < text.find("event: content_block_delta").unwrap()
    );
    assert_eq!(upstream.count(), 11);
}

#[tokio::test]
async fn infinite_retry_stream_pings_stop_after_client_disconnect() {
    let upstream = MockUpstream::start(0, true, Duration::from_millis(100)).await;
    let response = stream_response(state(&upstream, true)).await;
    assert_eq!(response.status(), StatusCode::OK);

    let mut body = response.into_body().into_data_stream();
    // 读掉第一个 ping，说明保活链路已经跑起来了。
    let mut saw_ping = false;
    while let Some(chunk) = tokio::time::timeout(Duration::from_secs(5), body.next())
        .await
        .expect("等待首个 ping 超时")
    {
        if String::from_utf8_lossy(&chunk.unwrap()).contains("event: ping") {
            saw_ping = true;
            break;
        }
    }
    assert!(saw_ping, "等待期没有收到 ping");
    drop(body);

    // 给已经发出但尚未被 mock 计入的那一次请求留出落地时间，再取基线。
    tokio::time::sleep(Duration::from_millis(300)).await;
    let baseline = upstream.count();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        upstream.count(),
        baseline,
        "下游断开后上游仍在被重试（300ms 内新增 {} 次）",
        upstream.count() - baseline
    );
}

#[tokio::test]
async fn stream_without_infinite_retry_sends_no_ping_before_headers() {
    // 开关关闭：上游 1.5s 后才返回 200，响应头也应当等这么久才发出。
    let upstream = MockUpstream::start(0, false, Duration::from_millis(1500)).await;
    let started = Instant::now();
    let response = stream_response(state(&upstream, false)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        started.elapsed() >= Duration::from_millis(1400),
        "开关关闭时不应提前发出响应头，实际耗时 {:?}",
        started.elapsed()
    );

    let mut body = response.into_body().into_data_stream();
    let mut before_content = String::new();
    while let Some(chunk) = tokio::time::timeout(Duration::from_secs(5), body.next())
        .await
        .expect("等待内容超时")
    {
        let piece = String::from_utf8_lossy(&chunk.unwrap()).to_string();
        if piece.contains("hello from upstream") {
            break;
        }
        before_content.push_str(&piece);
    }
    assert!(
        !before_content.contains("event: ping"),
        "开关关闭时不应发送 ping: {before_content}"
    );
    assert!(
        before_content.contains("event: message_start"),
        "{before_content}"
    );
}

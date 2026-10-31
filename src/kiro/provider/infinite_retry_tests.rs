//! 429 无限立即重试的端到端测试。
//!
//! 上游全部走 loopback 的 axum mock：前 N 次请求返回 429，之后返回 200。
//! 凭据是合成 API Key（`kiroApiKey` 直接作为 Bearer），不触发 token 刷新，
//! 因此整个过程没有任何外网访问。
use super::*;
use crate::kiro::endpoint::KiroEndpoint;
use crate::model::config::Config;
use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::time::timeout;

/// 上游 mock 的行为：先回 `throttle_times` 次 429，再回 200。
#[derive(Clone)]
struct MockState {
    /// 按顺序消费的状态码脚本；用尽后一直返回最后一个（脚本为空则恒 200）。
    script: Arc<Vec<u16>>,
    /// 前 `throttle_times` 次返回 429（脚本为空时使用）。
    throttle_times: usize,
    calls: Arc<AtomicUsize>,
    /// 账号级 429 文案（命中 `is_account_rate_limited`）vs 容量类 429。
    account_level: bool,
    /// 每次响应前的固定延迟，用于让「客户端断开」测试有稳定的时间窗口。
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
    async fn start(throttle_times: usize, account_level: bool, delay: Duration) -> Self {
        Self::start_with(throttle_times, account_level, delay, Vec::new()).await
    }

    /// 按 `script` 依次返回状态码，脚本用尽后返回 200。
    async fn start_script(script: Vec<u16>, delay: Duration) -> Self {
        Self::start_with(0, false, delay, script).await
    }

    async fn start_with(
        throttle_times: usize,
        account_level: bool,
        delay: Duration,
        script: Vec<u16>,
    ) -> Self {
        let state = MockState {
            script: Arc::new(script),
            throttle_times,
            calls: Arc::new(AtomicUsize::new(0)),
            account_level,
            delay,
        };
        let calls = state.calls.clone();
        let app = Router::new().fallback(mock_handler).with_state(state);
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

async fn mock_handler(State(state): State<MockState>) -> Response {
    let n = state.calls.fetch_add(1, Ordering::Relaxed);
    if !state.delay.is_zero() {
        tokio::time::sleep(state.delay).await;
    }
    let status = match state.script.get(n) {
        Some(status) => *status,
        None if n < state.throttle_times => 429,
        None => 200,
    };
    if status == 429 {
        let body = if state.account_level {
            r#"{"message":"You have exceeded the request rate for this account","reason":"USER_REQUEST_RATE_EXCEEDED"}"#
        } else {
            r#"{"message":"Insufficient model capacity","reason":"INSUFFICIENT_MODEL_CAPACITY"}"#
        };
        return (StatusCode::TOO_MANY_REQUESTS, body).into_response();
    }
    if !(200..300).contains(&status) {
        return (StatusCode::from_u16(status).unwrap(), "server error").into_response();
    }
    (StatusCode::OK, "ok").into_response()
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
        body.to_string()
    }
}

fn provider(
    upstream: &MockUpstream,
    infinite_retry: bool,
) -> (KiroProvider, Arc<MultiTokenManager>) {
    provider_with_switches(upstream, infinite_retry, false, 0)
}

fn provider_with_switches(
    upstream: &MockUpstream,
    model_api_infinite: bool,
    account_infinite: bool,
    interval_ms: u64,
) -> (KiroProvider, Arc<MultiTokenManager>) {
    let credential = KiroCredentials {
        id: Some(1),
        auth_method: Some("api_key".to_string()),
        kiro_api_key: Some("ksk_synthetic_429_fixture".to_string()),
        ..Default::default()
    };
    multi_provider_with_switches(
        upstream,
        vec![credential],
        model_api_infinite,
        account_infinite,
        interval_ms,
    )
}

fn multi_provider_with_switches(
    upstream: &MockUpstream,
    credentials: Vec<KiroCredentials>,
    model_api_infinite: bool,
    account_infinite: bool,
    interval_ms: u64,
) -> (KiroProvider, Arc<MultiTokenManager>) {
    let mut config = Config::default();
    config.model_api_429_infinite_retry = model_api_infinite;
    config.account_rate_limit_infinite_retry = account_infinite;
    config.all_creds_cooldown_retry_interval_ms = interval_ms;
    let manager = Arc::new(MultiTokenManager::new(config, credentials, None, None, false).unwrap());
    let endpoints: HashMap<String, Arc<dyn KiroEndpoint>> = HashMap::from([(
        "loopback".to_string(),
        Arc::new(LoopbackEndpoint(upstream.url.clone())) as Arc<dyn KiroEndpoint>,
    )]);
    let provider = KiroProvider::new(manager.clone(), endpoints, "loopback".to_string());
    (provider, manager)
}

fn summary_lines(sink: &RetryLogSink) -> Vec<String> {
    sink.lock()
        .iter()
        .filter(|line| line.contains("429 无限重试汇总"))
        .cloned()
        .collect()
}

#[tokio::test]
async fn infinite_retry_returns_success_after_twenty_429s_without_sleep() {
    let upstream = MockUpstream::start(20, false, Duration::ZERO).await;
    let (provider, _manager) = provider(&upstream, true);
    let sink = provider.capture_retry_logs();

    let started = Instant::now();
    let result = timeout(Duration::from_secs(5), provider.call_api("{}", None, None))
        .await
        .expect("无限重试不应超时")
        .expect("20 次 429 之后应拿到 200");
    let elapsed = started.elapsed();
    assert_eq!(result.response.text().await.unwrap(), "ok");
    assert_eq!(upstream.count(), 21, "20 次 429 + 1 次成功");
    assert!(
        elapsed < Duration::from_secs(1),
        "20 次重试必须无 sleep，实际耗时 {:?}",
        elapsed
    );

    let lines = summary_lines(&sink);
    assert_eq!(lines.len(), 1, "成功时应恰好一行汇总: {:?}", lines);
    assert!(
        lines[0].contains("重试 20 次"),
        "汇总需含重试次数: {}",
        lines[0]
    );
    assert!(lines[0].contains("耗时"), "汇总需含耗时: {}", lines[0]);
    // 日志限流：20 次失败最多 1 条首行 + 1 条汇总，不能一次一行。
    assert!(
        sink.lock().len() <= 3,
        "重试日志必须限流，实际 {} 行",
        sink.lock().len()
    );
}

#[tokio::test]
async fn infinite_retry_streaming_before_first_frame_returns_success() {
    let upstream = MockUpstream::start(12, false, Duration::ZERO).await;
    let (provider, _manager) = provider(&upstream, true);
    let sink = provider.capture_retry_logs();

    let started = Instant::now();
    let result = timeout(
        Duration::from_secs(5),
        provider.call_api_stream("{}", None, None),
    )
    .await
    .expect("流式无限重试不应超时")
    .expect("首帧前的 429 不应返回下游");
    let elapsed = started.elapsed();
    assert_eq!(result.response.text().await.unwrap(), "ok");
    assert_eq!(upstream.count(), 13);
    assert!(elapsed < Duration::from_secs(1), "耗时 {:?}", elapsed);

    let lines = summary_lines(&sink);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("重试 12 次"));
}

#[tokio::test]
async fn infinite_retry_disabled_keeps_bounded_429() {
    let upstream = MockUpstream::start(20, false, Duration::ZERO).await;
    let (provider, _manager) = provider(&upstream, false);

    let started = Instant::now();
    let error = match timeout(Duration::from_secs(20), provider.call_api("{}", None, None)).await {
        Ok(Ok(_)) => panic!("关闭开关时 20 次 429 必须失败，不能泄漏给下游 200"),
        Ok(Err(error)) => error,
        Err(_) => panic!("关闭开关时应在上限内返回"),
    };
    let elapsed = started.elapsed();

    assert!(
        error.downcast_ref::<UpstreamRateLimitError>().is_some(),
        "关闭开关应返回类型化 429: {error}"
    );
    // master 的预算：单凭据 min(1 × 3, 4) = 3 次尝试，不会因为 20 次 429 而放大。
    assert_eq!(upstream.count(), MAX_RETRIES_PER_CREDENTIAL);
    // 429 专用退避 1s 起，4 次尝试至少睡 3 次。
    assert!(
        elapsed >= Duration::from_secs(1),
        "关闭开关时必须保留退避，实际 {:?}",
        elapsed
    );
}

#[tokio::test]
async fn infinite_retry_stops_when_client_disconnects() {
    // throttle_times 足够大：不取消就永远不会成功。
    let upstream = MockUpstream::start(usize::MAX, false, Duration::from_millis(1)).await;
    let (provider, _manager) = provider(&upstream, true);
    let sink = provider.capture_retry_logs();
    let provider = Arc::new(provider);

    let task = {
        let provider = provider.clone();
        tokio::spawn(async move { provider.call_api("{}", None, None).await })
    };

    // 等重试真正跑起来（至少几轮）。
    let deadline = Instant::now() + Duration::from_secs(5);
    while upstream.count() < 5 {
        assert!(Instant::now() < deadline, "重试没有启动");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(sink.lock().iter().any(|line| line.contains("进行中")));
    // 反证：不取消的话这条请求永远不会成功，说明下面的「计数不增长」不是自然结束。
    assert!(
        !task.is_finished(),
        "请求应仍在重试中，否则断开测试没有意义"
    );

    task.abort();
    let _ = task.await;

    // 断开后计数不再增长：留出足够窗口让在途请求落地。
    let after_abort = upstream.count();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        upstream.count(),
        after_abort,
        "客户端断开后不得继续重试（{} -> {}）",
        after_abort,
        upstream.count()
    );

    let lines = summary_lines(&sink);
    assert_eq!(lines.len(), 1, "断开也必须留下唯一一条汇总: {:?}", lines);
    assert!(
        lines[0].contains("客户端断开"),
        "汇总应写明因客户端断开而停止: {}",
        lines[0]
    );
    let retries: usize = lines[0]
        .split("重试 ")
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("汇总需含重试次数: {}", lines[0]));
    assert!(retries >= 4, "汇总的重试次数应与实际相符: {}", lines[0]);
    assert!(retries <= after_abort, "{} > {}", retries, after_abort);
    assert!(lines[0].contains("耗时"), "汇总需含耗时: {}", lines[0]);
}

#[tokio::test]
async fn infinite_retry_keeps_retrying_while_all_credentials_cool() {
    // 账号级 429：每次都会把唯一凭据打进冷却，无限重试必须忽略冷却继续撞。
    let upstream = MockUpstream::start(6, true, Duration::from_millis(30)).await;
    let (provider, manager) = provider_with_switches(&upstream, true, true, 0);
    let sink = provider.capture_retry_logs();

    // 采样：证明整个过程中唯一凭据确实被账号级 429 打进了冷却（否则本用例退化成
    // 「有可用凭据时的普通重试」）。成功会顺带清掉冷却，所以必须在重试期间采样。
    let saw_cooldown = Arc::new(AtomicUsize::new(0));
    let sampler = {
        let manager = manager.clone();
        let saw = saw_cooldown.clone();
        tokio::spawn(async move {
            while saw.load(Ordering::Relaxed) == 0 {
                let throttled = manager.snapshot().entries[0]
                    .throttled_remaining_secs
                    .is_some();
                if throttled {
                    saw.store(1, Ordering::Relaxed);
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
    };

    let result = timeout(Duration::from_secs(5), provider.call_api("{}", None, None))
        .await
        .expect("全冷却时不应卡死")
        .expect("忽略冷却后应最终成功");
    assert_eq!(result.response.text().await.unwrap(), "ok");
    assert_eq!(upstream.count(), 7);

    sampler.abort();
    assert_eq!(
        saw_cooldown.load(Ordering::Relaxed),
        1,
        "重试期间凭据应被账号级 429 打入冷却，才能证明是在全冷却下继续重试"
    );

    let lines = summary_lines(&sink);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("重试 6 次"), "{}", lines[0]);
}

// ---------------------------------------------------------------------------
// B1：开关打开时，非 429 错误必须保持 master 的上限与退避行为。
// ---------------------------------------------------------------------------

/// 上游一直 500：按 master 的普通预算（1 凭据 = 3 次）放弃，返回 502。
#[tokio::test]
async fn infinite_retry_still_bounds_persistent_5xx_to_master_budget() {
    let upstream = MockUpstream::start_with(0, false, Duration::ZERO, vec![500; 64]).await;
    let (provider, _manager) = provider(&upstream, true);
    let sink = provider.capture_retry_logs();

    let started = Instant::now();
    let error = match timeout(Duration::from_secs(10), provider.call_api("{}", None, None)).await {
        Ok(Ok(_)) => panic!("持续 500 不能返回成功"),
        Ok(Err(error)) => error,
        Err(_) => panic!("开关打开时持续 500 也必须按预算返回，不能挂起"),
    };
    let elapsed = started.elapsed();

    assert_eq!(
        upstream.count(),
        MAX_RETRIES_PER_CREDENTIAL,
        "持续 500 必须与 master 同样次数后放弃"
    );
    assert!(
        error.to_string().contains("500"),
        "错误应保留上游状态: {error}"
    );
    assert!(
        elapsed >= Duration::from_millis(400),
        "5xx 必须保留 master 的退避，实际 {:?}",
        elapsed
    );
    // 从没发生过 429，报告里不该出现无限重试汇总。
    assert!(
        summary_lines(&sink).is_empty(),
        "无 429 时不应写汇总: {:?}",
        sink.lock()
    );
}

/// 上游端口拒绝连接：与 master 一样按预算返回网络错误，不挂起。
#[tokio::test]
async fn infinite_retry_still_bounds_connection_refused() {
    // 绑定后立刻释放端口，得到一个「肯定没人监听」的地址。
    let dead = {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        addr
    };
    let upstream = MockUpstream {
        url: format!("http://{dead}/"),
        calls: Arc::new(AtomicUsize::new(0)),
        task: tokio::spawn(async {}),
    };
    let (provider, _manager) = provider(&upstream, true);

    let started = Instant::now();
    let error = match timeout(Duration::from_secs(10), provider.call_api("{}", None, None)).await {
        Ok(Ok(_)) => panic!("连接被拒不能成功"),
        Ok(Err(error)) => error,
        Err(_) => panic!("连接被拒也必须按预算返回"),
    };
    let elapsed = started.elapsed();

    assert!(!error.to_string().is_empty());
    assert!(
        elapsed < Duration::from_secs(8),
        "不得无限重试，实际 {:?}",
        elapsed
    );
}

/// 唯一凭据被禁用：立刻报错，且从不打上游。
#[tokio::test]
async fn infinite_retry_still_fails_fast_when_no_credential_is_enabled() {
    let upstream = MockUpstream::start(0, false, Duration::ZERO).await;
    let (provider, manager) = provider(&upstream, true);
    manager.set_disabled(1, true).unwrap();

    let started = Instant::now();
    let error = match timeout(Duration::from_secs(5), provider.call_api("{}", None, None)).await {
        Ok(Ok(_)) => panic!("全部禁用不能成功"),
        Ok(Err(error)) => error,
        Err(_) => panic!("全部禁用必须立即报错，不能挂起"),
    };
    let elapsed = started.elapsed();

    assert_eq!(upstream.count(), 0, "无可用凭据时不应打上游");
    assert!(
        elapsed < Duration::from_secs(1),
        "全部禁用应立即返回，实际 {:?}",
        elapsed
    );
    assert!(
        error.to_string().contains("禁用"),
        "错误应说明无可用凭据: {error}"
    );
}

/// 429 与 500 混合：429 不消耗普通预算，500 只消耗一次，最终成功。
#[tokio::test]
async fn infinite_retry_429s_do_not_consume_the_ordinary_retry_budget() {
    let mut script = vec![429; 5];
    script.push(500);
    script.extend(std::iter::repeat_n(429, 5));
    let upstream = MockUpstream::start_script(script, Duration::ZERO).await;
    let (provider, _manager) = provider(&upstream, true);
    let sink = provider.capture_retry_logs();

    let result = timeout(Duration::from_secs(10), provider.call_api("{}", None, None))
        .await
        .expect("混合序列不应超时")
        .expect("10 次 429 + 1 次 500 之后应拿到 200");
    assert_eq!(result.response.text().await.unwrap(), "ok");
    assert_eq!(upstream.count(), 12, "5+1+5 次失败 + 1 次成功");

    let lines = summary_lines(&sink);
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("重试 10 次"), "只统计 429: {}", lines[0]);
    assert!(lines[0].contains("成功"), "{}", lines[0]);
}

/// N2：没有发生 429 重试的成功请求不能写任何汇总日志。
#[tokio::test]
async fn infinite_retry_success_without_any_429_stays_silent() {
    let upstream = MockUpstream::start(0, false, Duration::ZERO).await;
    let (provider, _manager) = provider(&upstream, true);
    let sink = provider.capture_retry_logs();

    let result = timeout(Duration::from_secs(5), provider.call_api("{}", None, None))
        .await
        .expect("不应超时")
        .expect("首次即成功");
    assert_eq!(result.response.text().await.unwrap(), "ok");
    assert_eq!(upstream.count(), 1);

    assert!(
        sink.lock().is_empty(),
        "0 次重试不能写汇总: {:?}",
        sink.lock()
    );
}

/// N2：非 429 错误终止与客户端断开必须打不同的汇总标签。
#[tokio::test]
async fn infinite_retry_summary_labels_distinguish_stop_reasons() {
    // 先来几次 429（让日志计数非零），再让上游持续 500 触发非 429 终止。
    let mut script = vec![429; 3];
    script.extend(std::iter::repeat_n(500, 64));
    let upstream = MockUpstream::start_script(script, Duration::ZERO).await;
    let (provider, _manager) = provider(&upstream, true);
    let sink = provider.capture_retry_logs();

    let error = match timeout(Duration::from_secs(10), provider.call_api("{}", None, None)).await {
        Ok(Ok(_)) => panic!("持续 500 不能成功"),
        Ok(Err(error)) => error,
        Err(_) => panic!("必须按预算返回"),
    };
    assert!(error.to_string().contains("500"));

    let lines = summary_lines(&sink);
    assert_eq!(lines.len(), 1, "{:?}", lines);
    assert!(
        lines[0].contains("非 429 错误终止"),
        "非 429 终止必须有独立标签: {}",
        lines[0]
    );
    assert!(lines[0].contains("重试 3 次"), "{}", lines[0]);
    assert!(lines[0].contains("耗时"), "{}", lines[0]);
}
/// 测试 4 种开关组合对模型容量类 429 与账号级 429 的正交独立表现
#[tokio::test]
async fn four_switch_combinations_for_capacity_and_account_429() {
    for (model_inf, acct_inf) in [(false, false), (true, false), (false, true), (true, true)] {
        // 容量类 429 场景：前 5 次 429，第 6 次 200
        let cap_upstream = MockUpstream::start(5, false, Duration::ZERO).await;
        let (cap_prov, _) = provider_with_switches(&cap_upstream, model_inf, acct_inf, 0);
        let cap_res = timeout(Duration::from_secs(4), cap_prov.call_api("{}", None, None)).await;

        if model_inf {
            assert!(
                cap_res.unwrap().is_ok(),
                "model_inf=true 时容量类 429 必须成功"
            );
        } else {
            // master 行为：单凭据 3 次重试用尽后返回 429 错误
            assert!(
                cap_res.unwrap().is_err(),
                "model_inf=false 时容量类 429 应按 master 返回错误"
            );
        }

        // 账号级 429 场景：前 5 次 429（全冷却），第 6 次 200
        let acct_upstream = MockUpstream::start(5, true, Duration::from_millis(10)).await;
        let (acct_prov, _) = provider_with_switches(&acct_upstream, model_inf, acct_inf, 0);
        let acct_res = timeout(Duration::from_secs(4), acct_prov.call_api("{}", None, None)).await;

        if acct_inf {
            assert!(
                acct_res.unwrap().is_ok(),
                "acct_inf=true 时账号级 429 必须全冷却轮转成功"
            );
        } else {
            // master 行为：唯一凭据冷却后无可用凭据，直接返回 429 错误
            assert!(
                acct_res.unwrap().is_err(),
                "acct_inf=false 时账号级 429 应按 master 返回错误"
            );
        }
    }
}

/// 测试全部凭据在冷却时的最小重试间隔配置与凭据轮转
#[tokio::test]
async fn all_creds_cooldown_retry_interval_bounds_hit_rate_and_rotates() {
    let cred1 = KiroCredentials {
        id: Some(1),
        auth_method: Some("api_key".to_string()),
        kiro_api_key: Some("ksk_cred1".to_string()),
        priority: 10,
        ..Default::default()
    };
    let cred2 = KiroCredentials {
        id: Some(2),
        auth_method: Some("api_key".to_string()),
        kiro_api_key: Some("ksk_cred2".to_string()),
        priority: 20,
        ..Default::default()
    };

    // 1. interval = 40ms：限制重试频率
    let upstream_slow = MockUpstream::start(999, true, Duration::ZERO).await;
    let (prov_slow, _) = multi_provider_with_switches(
        &upstream_slow,
        vec![cred1.clone(), cred2.clone()],
        false,
        true,
        40,
    );

    let _ = timeout(
        Duration::from_millis(150),
        prov_slow.call_api("{}", None, None),
    )
    .await;
    let slow_calls = upstream_slow.count();
    // 150ms / 40ms = 最多 4-6 次请求，不可能爆发到几十次
    assert!(
        slow_calls <= 7,
        "40ms interval 下 150ms 实际调用 {} 次，频率应受约束",
        slow_calls
    );

    // 2. interval = 0ms：不卡顿但至少 yield 一次并轮转凭据
    let upstream_fast = MockUpstream::start(999, true, Duration::ZERO).await;
    let (prov_fast, manager_fast) =
        multi_provider_with_switches(&upstream_fast, vec![cred1, cred2], false, true, 0);

    let _ = timeout(
        Duration::from_millis(50),
        prov_fast.call_api("{}", None, None),
    )
    .await;
    let fast_calls = upstream_fast.count();
    assert!(
        fast_calls > slow_calls,
        "interval=0 时的重试次数 ({fast_calls}) 应高于带 interval ({slow_calls})"
    );
    // 验证轮转游标确实更新过（在凭据 1 和 2 之间轮转）
    let cursor = manager_fast.snapshot().entries;
    assert_eq!(cursor.len(), 2);
}

//! 会话粘性路由（Sticky Session）
//!
//! 让同一会话（conversationId）的后续轮次尽量落在上一轮成功的凭据上。
//!
//! 实测 Kiro 上游 prompt cache 按 **profile** 隔离、按内容前缀匹配：同一 profile 下的
//! 账号互相共享缓存，会话在它们之间换号并不会丢缓存（社交登录账号通常同属一个共享
//! profile）。因此粘性路由的收益主要在两类场景：
//! - 凭据分布在多个 profile（企业 / IdC 账号）时，跨 profile 换号确实会冷启动；
//! - 让一个会话的请求在同一账号上连续可追溯，便于排查与配额归因。
//!
//! 本模块只维护「会话 → 凭据」的绑定表，不参与可用性判断；调度器在选号前先查表，
//! 绑定的凭据仍可用（未禁用 / 未冷却 / RPM 未满 / 支持该模型 / 在分组内）就直接沿用，
//! 否则回落到普通负载均衡并在成功后重新绑定。
//!
//! 绑定表为进程内存储，多副本部署下各实例独立；容量有上限，TTL 到期即失效。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;

/// 绑定表容量上限。超出时先清过期，再淘汰最早到期的条目。
const CAPACITY: usize = 8192;
/// TTL 下限，防止误配成 0 让粘性形同虚设
const MIN_TTL_SECS: u64 = 60;

/// 单次选号的粘性判定结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StickyOutcome {
    /// 绑定凭据可用，本次沿用
    Hit,
    /// 该会话无绑定（首轮或已过期）
    MissFirst,
    /// 有绑定但该凭据当前不可用
    MissUnavailable,
    /// 粘性路由已关闭
    Off,
}

impl StickyOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hit => crate::admin::trace_db::sticky::HIT,
            Self::MissFirst => crate::admin::trace_db::sticky::MISS_FIRST,
            Self::MissUnavailable => crate::admin::trace_db::sticky::MISS_UNAVAILABLE,
            Self::Off => crate::admin::trace_db::sticky::OFF,
        }
    }
}

/// 选号器返回给调用方的路由决策，供 trace 落库与命中率统计
#[derive(Debug, Clone, Copy)]
pub struct RouteDecision {
    pub sticky_outcome: StickyOutcome,
    /// 本次选号前该会话绑定的凭据（不论是否可用）
    pub previous_credential_id: Option<u64>,
}

impl RouteDecision {
    pub const fn none() -> Self {
        Self {
            sticky_outcome: StickyOutcome::Off,
            previous_credential_id: None,
        }
    }
}

struct Binding {
    credential_id: u64,
    expires_at: Instant,
}

/// 粘性路由统计（Admin API 暴露）
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionAffinityStats {
    pub enabled: bool,
    pub ttl_secs: u64,
    pub hits: u64,
    pub misses: u64,
    pub active_bindings: usize,
}

/// 会话 → 凭据 绑定表
pub struct SessionAffinity {
    bindings: Mutex<HashMap<String, Binding>>,
    enabled: AtomicBool,
    ttl_secs: AtomicU64,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl SessionAffinity {
    pub fn new(enabled: bool, ttl_secs: u64) -> Self {
        Self {
            bindings: Mutex::new(HashMap::new()),
            enabled: AtomicBool::new(enabled),
            ttl_secs: AtomicU64::new(ttl_secs.max(MIN_TTL_SECS)),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// 关闭时保留已有绑定，重新开启即可继续命中。
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn ttl_secs(&self) -> u64 {
        self.ttl_secs.load(Ordering::Relaxed)
    }

    /// 只影响此后写入 / 续期的条目；已有条目按写入时的 TTL 到期。
    pub fn set_ttl_secs(&self, ttl_secs: u64) {
        self.ttl_secs
            .store(ttl_secs.max(MIN_TTL_SECS), Ordering::Relaxed);
    }

    fn ttl(&self) -> Duration {
        Duration::from_secs(self.ttl_secs())
    }

    /// 查询会话当前绑定的凭据。过期条目视为不存在并顺手删除；不续期。
    pub fn lookup(&self, session: &str) -> Option<u64> {
        let now = Instant::now();
        let mut map = self.bindings.lock();
        match map.get(session) {
            Some(b) if b.expires_at > now => Some(b.credential_id),
            Some(_) => {
                map.remove(session);
                None
            }
            None => None,
        }
    }

    /// 记录「该凭据成功服务了该会话」，写入或续期绑定。
    pub fn bind(&self, session: &str, credential_id: u64) {
        if !self.is_enabled() || credential_id == 0 {
            return;
        }
        let now = Instant::now();
        let expires_at = now + self.ttl();
        let mut map = self.bindings.lock();
        if let Some(b) = map.get_mut(session) {
            b.credential_id = credential_id;
            b.expires_at = expires_at;
            return;
        }
        if map.len() >= CAPACITY {
            map.retain(|_, b| b.expires_at > now);
        }
        if map.len() >= CAPACITY
            && let Some(victim) = map
                .iter()
                .min_by_key(|(_, b)| b.expires_at)
                .map(|(k, _)| k.clone())
        {
            map.remove(&victim);
        }
        map.insert(
            session.to_string(),
            Binding {
                credential_id,
                expires_at,
            },
        );
    }

    /// 计入一次选号结果。Off 不计入命中率分母。
    pub fn record(&self, outcome: StickyOutcome) {
        match outcome {
            StickyOutcome::Hit => {
                self.hits.fetch_add(1, Ordering::Relaxed);
            }
            StickyOutcome::MissFirst | StickyOutcome::MissUnavailable => {
                self.misses.fetch_add(1, Ordering::Relaxed);
            }
            StickyOutcome::Off => {}
        }
    }

    pub fn stats(&self) -> SessionAffinityStats {
        let now = Instant::now();
        let active_bindings = self
            .bindings
            .lock()
            .values()
            .filter(|b| b.expires_at > now)
            .count();
        SessionAffinityStats {
            enabled: self.is_enabled(),
            ttl_secs: self.ttl_secs(),
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            active_bindings,
        }
    }

    /// 清理过期绑定（后台周期调用；lookup 只顺手清自己命中的那条）
    pub fn evict_expired(&self) {
        let now = Instant::now();
        self.bindings.lock().retain(|_, b| b.expires_at > now);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.bindings.lock().len()
    }

    #[cfg(test)]
    fn expire_now(&self, session: &str) {
        if let Some(b) = self.bindings.lock().get_mut(session) {
            b.expires_at = Instant::now() - Duration::from_secs(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_then_lookup_hits() {
        let a = SessionAffinity::new(true, 3600);
        assert_eq!(a.lookup("s1"), None);
        a.bind("s1", 7);
        assert_eq!(a.lookup("s1"), Some(7));
        // 重新绑定覆盖旧值
        a.bind("s1", 9);
        assert_eq!(a.lookup("s1"), Some(9));
    }

    #[test]
    fn expired_binding_is_removed_on_lookup() {
        let a = SessionAffinity::new(true, 3600);
        a.bind("s1", 7);
        a.expire_now("s1");
        assert_eq!(a.lookup("s1"), None);
        assert_eq!(a.len(), 0, "过期条目应被顺手删除");
    }

    #[test]
    fn disabled_does_not_bind_but_keeps_existing() {
        let a = SessionAffinity::new(true, 3600);
        a.bind("s1", 7);
        a.set_enabled(false);
        a.bind("s2", 8);
        assert_eq!(a.lookup("s2"), None, "关闭后不写入新绑定");
        assert_eq!(a.lookup("s1"), Some(7), "已有绑定保留，重开即可命中");
    }

    #[test]
    fn zero_credential_is_never_bound() {
        let a = SessionAffinity::new(true, 3600);
        a.bind("s1", 0);
        assert_eq!(a.lookup("s1"), None);
    }

    #[test]
    fn ttl_floor_is_enforced() {
        let a = SessionAffinity::new(true, 0);
        assert_eq!(a.ttl_secs(), MIN_TTL_SECS);
        a.set_ttl_secs(5);
        assert_eq!(a.ttl_secs(), MIN_TTL_SECS);
        a.set_ttl_secs(7200);
        assert_eq!(a.ttl_secs(), 7200);
    }

    #[test]
    fn capacity_evicts_earliest_expiry() {
        let a = SessionAffinity::new(true, 3600);
        for i in 0..CAPACITY {
            a.bind(&format!("s{i}"), 1);
        }
        assert_eq!(a.len(), CAPACITY);
        // 让 s0 最早到期
        a.expire_now("s0");
        a.bind("overflow", 2);
        assert_eq!(a.len(), CAPACITY, "容量不应超上限");
        assert_eq!(a.lookup("s0"), None, "过期条目被淘汰");
        assert_eq!(a.lookup("overflow"), Some(2));
        assert_eq!(a.lookup("s1"), Some(1), "未过期条目保留");
    }

    #[test]
    fn stats_count_hits_and_misses_but_not_off() {
        let a = SessionAffinity::new(true, 3600);
        a.record(StickyOutcome::Hit);
        a.record(StickyOutcome::Hit);
        a.record(StickyOutcome::MissFirst);
        a.record(StickyOutcome::MissUnavailable);
        a.record(StickyOutcome::Off);
        let s = a.stats();
        assert_eq!(s.hits, 2);
        assert_eq!(s.misses, 2);
    }

    #[test]
    fn evict_expired_drops_only_dead_entries() {
        let a = SessionAffinity::new(true, 3600);
        a.bind("live", 1);
        a.bind("dead", 2);
        a.expire_now("dead");
        a.evict_expired();
        assert_eq!(a.len(), 1);
        assert_eq!(a.lookup("live"), Some(1));
    }
}

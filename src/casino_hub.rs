// casino_hub.rs — 봇 안의 카지노 운영 상태: 테이블 저장소, 개인 링크 세션, 코인↔칩 연동,
// 영속 저장(casino.json), 변경 알림. Discord 중계와 웹 API가 이 허브를 공유한다.

use crate::stats;
use anyhow::{Context as AnyhowContext, Result};
use dashmap::DashMap;
use mafia_remake::casino::{
    CasinoCommand, CasinoError, CasinoEvent, CasinoTable, GameKind, TableSettings, TableView,
    format_chips, table_view,
};
use mafia_remake::stats::StatsFile;
use mafia_remake::system_random;
use poise::serenity_prelude as serenity;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{RwLock, broadcast};

/// 개인 링크 세션 수명.
pub const CASINO_SESSION_TTL_SECONDS: u64 = 12 * 60 * 60;
pub const MAX_TABLES: usize = 12;

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// 테이블에 연결된 Discord 채널 정보.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TableBinding {
    pub guild_id: u64,
    pub channel_id: u64,
    #[serde(default)]
    pub status_message_id: Option<u64>,
    /// 여기까지의 테이블 채팅을 채널로 보냈다.
    #[serde(default)]
    pub relayed_message_seq: u64,
    /// 여기까지의 핸드 결과를 채널에 알렸다 (history id).
    #[serde(default)]
    pub announced_result_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CasinoSession {
    pub user_id: u64,
    pub name: String,
    pub expires_at: Instant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredTable {
    table: CasinoTable,
    binding: TableBinding,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CasinoFile {
    #[serde(default)]
    house: i64,
    #[serde(default)]
    tables: Vec<StoredTable>,
}

/// 테이블이 바뀌었다는 알림 (웹소켓 푸시·Discord 중계용).
#[derive(Debug, Clone)]
pub struct CasinoUpdate {
    pub table_id: String,
}

pub struct CasinoHub {
    pub tables: DashMap<String, Arc<RwLock<CasinoTable>>>,
    pub bindings: DashMap<String, TableBinding>,
    pub sessions: DashMap<String, CasinoSession>,
    pub house: AtomicI64,
    pub stats: Arc<RwLock<StatsFile>>,
    pub stats_path: Arc<PathBuf>,
    pub path: PathBuf,
    pub updates: broadcast::Sender<CasinoUpdate>,
    /// 테이블 채널별 채팅 웹훅 (메모리 캐시; 재시작 후에는 채널의 웹훅 목록에서 다시 찾는다).
    pub webhooks: DashMap<u64, serenity::Webhook>,
    /// 웹훅 채팅에 쓰는 참가자 아바타 URL 캐시 (user_id → url).
    pub avatars: DashMap<u64, String>,
    save_lock: tokio::sync::Mutex<()>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TableSummary {
    pub id: String,
    pub kind: GameKind,
    pub kind_text: String,
    pub name: String,
    pub seated: usize,
    pub seat_count: usize,
    pub playing: bool,
    pub phase_text: Option<String>,
    pub channel_id: Option<u64>,
    /// 판돈 ("블라인드 50/100", "베팅 100~5,000").
    pub stakes: String,
}

impl CasinoHub {
    pub fn load(path: PathBuf, stats: Arc<RwLock<StatsFile>>, stats_path: Arc<PathBuf>) -> Self {
        let (updates, _) = broadcast::channel(256);
        let hub = Self {
            tables: DashMap::new(),
            bindings: DashMap::new(),
            sessions: DashMap::new(),
            house: AtomicI64::new(0),
            stats,
            stats_path,
            path,
            updates,
            webhooks: DashMap::new(),
            avatars: DashMap::new(),
            save_lock: tokio::sync::Mutex::new(()),
        };
        match load_file(&hub.path) {
            Ok(file) => {
                hub.house.store(file.house, Ordering::Relaxed);
                for stored in file.tables {
                    let id = stored.table.id.clone();
                    hub.bindings.insert(id.clone(), stored.binding);
                    hub.tables.insert(id, Arc::new(RwLock::new(stored.table)));
                }
            }
            Err(error) => eprintln!("failed to load casino state: {error:?}"),
        }
        hub
    }

    pub fn notify(&self, table_id: &str) {
        let _ = self.updates.send(CasinoUpdate {
            table_id: table_id.to_string(),
        });
    }

    /// 테이블 목록 스냅샷. DashMap 가드를 await 너머로 들고 있지 않기 위해 Arc만 복사한다.
    fn table_handles(&self) -> Vec<(String, Arc<RwLock<CasinoTable>>)> {
        self.tables
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect()
    }

    /// casino.json 저장 (직렬화는 잠금 안에서, 쓰기는 블로킹 스레드에서).
    pub async fn save(&self) {
        let _guard = self.save_lock.lock().await;
        let mut tables = Vec::new();
        for (id, handle) in self.table_handles() {
            let table = handle.read().await.clone();
            let binding = self.binding(&id).unwrap_or_default();
            tables.push(StoredTable { table, binding });
        }
        tables.sort_by(|left, right| left.table.created_at.cmp(&right.table.created_at));
        let file = CasinoFile {
            house: self.house.load(Ordering::Relaxed),
            tables,
        };
        let path = self.path.clone();
        match tokio::task::spawn_blocking(move || save_file(&path, &file)).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => eprintln!("failed to save casino state: {error:?}"),
            Err(error) => eprintln!("failed to join casino save task: {error:?}"),
        }
    }

    pub async fn save_stats(&self) {
        let snapshot = self.stats.read().await.clone();
        let path = self.stats_path.clone();
        match tokio::task::spawn_blocking(move || stats::save_stats(&*path, &snapshot)).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => eprintln!("failed to save stats after casino change: {error:?}"),
            Err(error) => {
                eprintln!("failed to join stats save task after casino change: {error:?}")
            }
        }
    }

    // ------------------------------------------------------------ 세션

    pub fn issue_session(&self, user_id: u64, name: String) -> String {
        let now = Instant::now();
        self.sessions.retain(|_, session| session.expires_at > now);
        let mut bytes = [0u8; 32];
        system_random::fill_bytes(&mut bytes);
        let mut token = String::with_capacity(64);
        for byte in bytes {
            let _ = write!(&mut token, "{byte:02x}");
        }
        self.sessions.insert(
            token.clone(),
            CasinoSession {
                user_id,
                name,
                expires_at: now + Duration::from_secs(CASINO_SESSION_TTL_SECONDS),
            },
        );
        token
    }

    pub fn session(&self, token: &str) -> Option<CasinoSession> {
        let session = self.sessions.get(token)?.clone();
        if session.expires_at <= Instant::now() {
            drop(session);
            self.sessions.remove(token);
            return None;
        }
        Some(session)
    }

    // ------------------------------------------------------------ 테이블

    pub fn table(&self, table_id: &str) -> Option<Arc<RwLock<CasinoTable>>> {
        self.tables.get(table_id).map(|entry| entry.value().clone())
    }

    /// 이름 또는 id로 테이블을 찾는다 (대소문자 무시).
    pub async fn find_table(&self, query: &str) -> Option<(String, Arc<RwLock<CasinoTable>>)> {
        let query = query.trim().to_lowercase();
        if let Some(handle) = self.table(&query) {
            return Some((query, handle));
        }
        for (id, handle) in self.table_handles() {
            let name = handle.read().await.name.to_lowercase();
            if name == query {
                return Some((id, handle));
            }
        }
        None
    }

    pub async fn summaries(&self) -> Vec<TableSummary> {
        let mut summaries = Vec::new();
        for (id, handle) in self.table_handles() {
            let table = handle.read().await;
            summaries.push(TableSummary {
                id: table.id.clone(),
                kind: table.kind,
                kind_text: table.kind.value().to_string(),
                name: table.name.clone(),
                seated: table.seats.iter().flatten().count(),
                seat_count: table.seats.len(),
                playing: table.playing(),
                phase_text: table
                    .round
                    .as_ref()
                    .map(|round| round.phase.value().to_string()),
                channel_id: self.binding(&id).map(|binding| binding.channel_id),
                stakes: table.settings.stakes(table.kind),
            });
        }
        summaries.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        summaries
    }

    pub fn create_table(
        &self,
        kind: GameKind,
        name: &str,
        created_by: u64,
        binding: TableBinding,
        settings: TableSettings,
    ) -> Result<String, String> {
        if self.tables.len() >= MAX_TABLES {
            return Err(format!(
                "테이블은 최대 {MAX_TABLES}개까지 만들 수 있습니다."
            ));
        }
        let name = name.trim();
        if name.chars().count() < 2 || name.chars().count() > 24 {
            return Err("테이블 이름은 2~24자로 입력하세요.".to_string());
        }
        let id = format!("{}-{}", kind.key(), short_id());
        let table =
            CasinoTable::new(id.clone(), kind, name, created_by, now_ms()).with_settings(settings);
        self.bindings.insert(id.clone(), binding);
        self.tables.insert(id.clone(), Arc::new(RwLock::new(table)));
        Ok(id)
    }

    /// 테이블을 닫고 모든 칩을 코인으로 돌려준다. 반환: 캐시아웃 목록.
    pub async fn close_table(&self, table_id: &str) -> Vec<CasinoEvent> {
        let Some((_, table)) = self.tables.remove(table_id) else {
            return Vec::new();
        };
        let events = table.write().await.close();
        self.apply_events(&events).await;
        if let Some((_, binding)) = self.bindings.remove(table_id) {
            self.webhooks.remove(&binding.channel_id);
        }
        self.save().await;
        self.save_stats().await;
        events
    }

    /// 어떤 테이블이든 이 사용자가 앉아 있는 곳.
    pub async fn seated_table_of(&self, user_id: u64) -> Option<String> {
        for (id, handle) in self.table_handles() {
            if handle.read().await.seat_index(user_id).is_some() {
                return Some(id);
            }
        }
        None
    }

    // ------------------------------------------------------------ 코인 연동

    /// 코인 잔액 (없으면 0).
    pub async fn coins_of(&self, user_id: u64) -> i64 {
        self.stats
            .read()
            .await
            .users
            .get(&user_id.to_string())
            .map_or(0, |entry| entry.coins)
    }

    /// 캐시아웃·정산 이벤트를 코인·하우스에 반영한다.
    pub async fn apply_events(&self, events: &[CasinoEvent]) {
        let mut touched = false;
        for event in events {
            match event {
                CasinoEvent::CashOut {
                    user_id,
                    name,
                    amount,
                } => {
                    let mut stats_file = self.stats.write().await;
                    stats::refund_coins(&mut stats_file, *user_id, name, *amount);
                    touched = true;
                }
                CasinoEvent::RoundSettled { house_delta, .. } => {
                    self.house.fetch_add(*house_delta, Ordering::Relaxed);
                }
                CasinoEvent::Joined { .. } => {}
            }
        }
        if touched {
            self.save_stats().await;
        }
    }

    /// 명령 적용: 바이인은 코인을 먼저 빼고(실패 시 환불), 결과 이벤트를 코인에 반영한다.
    pub async fn apply(
        &self,
        table_id: &str,
        user_id: u64,
        user_name: &str,
        command: &CasinoCommand,
        expected_version: Option<u64>,
    ) -> Result<Vec<CasinoEvent>, CasinoError> {
        let table = self
            .table(table_id)
            .ok_or_else(|| CasinoError::new("NOT_FOUND", "테이블을 찾을 수 없습니다."))?;
        let now = now_ms();
        // 좌석 이름은 항상 Discord 이름을 쓴다 (웹이 보낸 닉네임은 무시; 채널 웹훅 이름과 맞춘다).
        let sanitized_join;
        let command = match command {
            CasinoCommand::Join { seat, amount, .. } => {
                sanitized_join = CasinoCommand::Join {
                    seat: *seat,
                    amount: *amount,
                    name: seat_name(user_name),
                };
                &sanitized_join
            }
            other => other,
        };
        let mut reserved = 0;
        if let CasinoCommand::Join { amount, .. } = command {
            if let Some(other) = self.seated_table_of(user_id).await {
                if other != table_id {
                    return Err(CasinoError::invalid("이미 다른 테이블에 앉아 있습니다."));
                }
            }
            let rules = table.read().await.settings;
            if *amount < rules.min_buy_in || *amount > rules.max_buy_in {
                return Err(CasinoError::invalid(format!(
                    "바이인은 {}~{} 칩입니다.",
                    format_chips(rules.min_buy_in),
                    format_chips(rules.max_buy_in)
                )));
            }
            let mut stats_file = self.stats.write().await;
            stats::reserve_coins(&mut stats_file, user_id, user_name, *amount)
                .map_err(|message| CasinoError::new("INSUFFICIENT_COINS", message))?;
            reserved = *amount;
        }
        let result =
            table
                .write()
                .await
                .apply_command(user_id, user_name, command, expected_version, now);
        let events = match result {
            Ok(events) => events,
            Err(error) => {
                if reserved > 0 {
                    let mut stats_file = self.stats.write().await;
                    stats::refund_coins(&mut stats_file, user_id, user_name, reserved);
                }
                return Err(error);
            }
        };
        if reserved > 0 {
            self.save_stats().await;
        }
        self.apply_events(&events).await;
        self.notify(table_id);
        self.save().await;
        Ok(events)
    }

    /// 봇 쪽(Discord 채널)에서 온 채팅을 테이블 채팅에 넣는다.
    pub async fn relay_chat_from_discord(
        &self,
        table_id: &str,
        user_id: u64,
        name: &str,
        text: &str,
    ) {
        let Some(table) = self.table(table_id) else {
            return;
        };
        table
            .write()
            .await
            .relay_chat(user_id, name, text, now_ms());
        self.notify(table_id);
    }

    /// 모든 테이블의 시간 초과·유휴 정리. 바뀐 테이블 id 목록.
    pub async fn tick_all(&self) -> Vec<String> {
        let now = now_ms();
        let mut changed = Vec::new();
        for (id, table) in self.table_handles() {
            let (before, result) = {
                let mut table = table.write().await;
                let before = table.version;
                let result = table.tick(now);
                (before, result)
            };
            match result {
                Ok(events) => {
                    self.apply_events(&events).await;
                    let guard = table.read().await;
                    if guard.version != before {
                        changed.push(id);
                    } else if guard.is_revealing(now) {
                        // 카드 연출 중에는 상태가 안 바뀌어도 화면을 자주 밀어 준다.
                        self.notify(&id);
                    }
                }
                Err(error) => eprintln!("casino tick failed for {id}: {error}"),
            }
        }
        for id in &changed {
            self.notify(id);
        }
        if !changed.is_empty() {
            self.save().await;
        }
        changed
    }

    /// 웹·중계용 화면 상태.
    pub async fn view_for(&self, table_id: &str, viewer: Option<u64>) -> Option<TableView> {
        let table = self.table(table_id)?;
        let table = table.read().await;
        Some(table_view(&table, viewer, now_ms()))
    }

    pub fn binding(&self, table_id: &str) -> Option<TableBinding> {
        self.bindings.get(table_id).map(|binding| binding.clone())
    }

    pub fn update_binding(&self, table_id: &str, update: impl FnOnce(&mut TableBinding)) {
        if let Some(mut binding) = self.bindings.get_mut(table_id) {
            update(&mut binding);
        }
    }

    pub fn table_id_for_channel(&self, channel_id: u64) -> Option<String> {
        self.bindings
            .iter()
            .find(|entry| entry.value().channel_id == channel_id)
            .map(|entry| entry.key().clone())
    }
}

/// 테이블 좌석에 쓰는 이름: 제어 문자 제거, 16자 제한, 2자 미만이면 보정.
fn seat_name(name: &str) -> String {
    let mut cleaned: String = name
        .trim()
        .chars()
        .filter(|ch| !ch.is_control())
        .take(16)
        .collect();
    if cleaned.chars().count() < 2 {
        cleaned.push('님');
    }
    if cleaned.chars().count() < 2 {
        cleaned = "플레이어".to_string();
    }
    cleaned
}

fn short_id() -> String {
    let mut bytes = [0u8; 4];
    system_random::fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn load_file(path: &Path) -> Result<CasinoFile> {
    if !path.exists() {
        return Ok(CasinoFile::default());
    }
    let text = std::fs::read_to_string(path).context("casino.json 읽기 실패")?;
    if text.trim().is_empty() {
        return Ok(CasinoFile::default());
    }
    serde_json::from_str(&text).context("casino.json 파싱 실패")
}

fn save_file(path: &Path, file: &CasinoFile) -> Result<()> {
    let text = serde_json::to_string_pretty(file).context("casino.json 직렬화 실패")?;
    let temp_path = path.with_file_name(format!(
        "{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("casino.json")
    ));
    std::fs::write(&temp_path, text).context("casino.json 임시 파일 쓰기 실패")?;
    std::fs::rename(&temp_path, path).context("casino.json 교체 실패")?;
    Ok(())
}

/// 채널 이름용 슬러그.
pub fn table_channel_name(name: &str) -> String {
    let slug = crate::channel::sanitize_channel_part(name);
    format!("카지노-{slug}")
}

/// 카지노 개인 링크의 기준 주소. CASINO_BASE_URL이 없으면 WEB_SETTINGS_BASE_URL의 호스트에
/// Activity 포트를 붙여 만든다 (같은 서버·같은 도메인에서 돌기 때문).
pub fn casino_base_url(web_host: &str, activity_port: u16, tls: bool) -> String {
    if let Ok(base_url) = std::env::var("CASINO_BASE_URL")
        && !base_url.trim().is_empty()
    {
        return base_url.trim_end_matches('/').to_string();
    }
    let scheme = if tls { "https" } else { "http" };
    if let Some(host) = std::env::var("WEB_SETTINGS_BASE_URL")
        .ok()
        .as_deref()
        .and_then(public_host_of)
    {
        return format!("{scheme}://{host}:{activity_port}");
    }
    let display_host = if matches!(web_host, "0.0.0.0" | "::") {
        "localhost"
    } else {
        web_host
    };
    format!("{scheme}://{display_host}:{activity_port}")
}

/// `https://example.com:8443/path` → `example.com`. localhost나 IP만 있으면 None.
pub fn public_host_of(url: &str) -> Option<String> {
    let without_scheme = url
        .trim()
        .strip_prefix("https://")
        .or_else(|| url.trim().strip_prefix("http://"))?;
    let authority = without_scheme.split('/').next()?;
    let host = authority
        .rsplit_once(':')
        .map_or(authority, |(host, _)| host)
        .trim_matches(|ch| ch == '[' || ch == ']');
    if host.is_empty()
        || host == "localhost"
        || host
            .chars()
            .all(|ch| ch.is_ascii_digit() || ch == '.' || ch == ':')
    {
        return None;
    }
    Some(host.to_string())
}

pub fn personal_link(base_url: &str, token: &str, table_id: &str) -> String {
    format!(
        "{}/casino/{token}?table={table_id}",
        base_url.trim_end_matches('/')
    )
}

/// 서버 재시작 직후 등 세션이 없는 요청에 쓰는 안내.
pub fn session_expired_message() -> &'static str {
    "링크가 만료됐거나 서버가 다시 시작됐습니다. Discord에서 `/카지노입장`으로 새 링크를 받으세요."
}

pub type SharedHub = Arc<CasinoHub>;

#[cfg(test)]
mod tests {
    use super::public_host_of;

    #[test]
    fn public_host_is_taken_from_the_settings_url() {
        assert_eq!(
            public_host_of("https://o4.example.kr:8443").as_deref(),
            Some("o4.example.kr")
        );
        assert_eq!(
            public_host_of("http://example.com/path").as_deref(),
            Some("example.com")
        );
        assert_eq!(public_host_of("http://localhost:8800"), None);
        assert_eq!(public_host_of("http://127.0.0.1:8800"), None);
        assert_eq!(public_host_of("not a url"), None);
    }
}

/// 웹 API 응답용 요약 (테이블 목록 + 내 정보).
#[derive(Debug, Clone, Serialize)]
pub struct CasinoMe {
    pub user_id: String,
    pub name: String,
    pub coins: i64,
    pub seated_table: Option<String>,
}

impl CasinoHub {
    pub async fn me(&self, session: &CasinoSession) -> CasinoMe {
        CasinoMe {
            user_id: session.user_id.to_string(),
            name: session.name.clone(),
            coins: self.coins_of(session.user_id).await,
            seated_table: self.seated_table_of(session.user_id).await,
        }
    }

    /// 상태 임베드용: 테이블 채팅 중 아직 중계하지 않은 메시지.
    pub async fn unrelayed_messages(
        &self,
        table_id: &str,
    ) -> Vec<mafia_remake::casino::ChatMessage> {
        let since = self
            .binding(table_id)
            .map_or(0, |binding| binding.relayed_message_seq);
        let Some(table) = self.table(table_id) else {
            return Vec::new();
        };
        let table = table.read().await;
        table
            .messages
            .iter()
            .filter(|message| message.seq > since)
            .cloned()
            .collect()
    }
}

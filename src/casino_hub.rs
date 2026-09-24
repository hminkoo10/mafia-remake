// casino_hub.rs — 봇 안의 카지노 운영 상태: 테이블 저장소, 개인 링크 세션, 코인↔칩 연동,
// 영속 저장(casino.json), 변경 알림. Discord 중계와 웹 API가 이 허브를 공유한다.

use crate::stats;
use anyhow::{Context as AnyhowContext, Result};
use dashmap::DashMap;
use mafia_remake::casino::{
    CasinoCommand, CasinoError, CasinoEvent, CasinoTable, GameKind, Phase, TableSettings,
    TableView, format_chips, shown_phase, table_view,
};
use mafia_remake::stats::StatsFile;
use mafia_remake::system_random;
use poise::serenity_prelude as serenity;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{Notify, RwLock, broadcast};

/// 개인 링크 세션 수명.
pub const CASINO_SESSION_TTL_SECONDS: u64 = 12 * 60 * 60;
pub const MAX_TABLES: usize = 12;
/// 모아서 저장할 때 마지막 변경 뒤 기다리는 시간.
const SAVE_COALESCE: Duration = Duration::from_millis(300);

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
    /// 봇이 마지막으로 정한 채널 이름. 테이블 이름과 어긋나면(이름 변경이 Discord 제한에
    /// 걸렸던 경우 등) 다음 설정 제출 때 다시 맞춘다. 예전 저장본은 None.
    #[serde(default)]
    pub channel_name: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PanelBinding {
    pub guild_id: u64,
    pub channel_id: u64,
    pub message_id: u64,
}

#[derive(Debug, Clone)]
pub struct CasinoSession {
    pub user_id: u64,
    pub name: String,
    /// 만료 시각 (unix ms). 봇이 다시 시작해도 이어지도록 저장한다.
    pub expires_at: i64,
}

/// casino.json에 남기는 세션. 토큰 원문은 쓰지 않고 SHA-256만 남긴다.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSession {
    token_sha256: String,
    user_id: u64,
    name: String,
    /// 만료 시각 (unix ms).
    expires_at: i64,
}

/// 개인 링크 토큰의 SHA-256 (hex). 세션 표는 이 값으로 찾는다.
fn token_hash(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
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
    #[serde(default)]
    panel: Option<PanelBinding>,
    /// 아직 유효한 개인 링크 세션 (재시작 뒤에도 링크가 이어지게).
    #[serde(default)]
    sessions: Vec<StoredSession>,
}

/// 알림 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateKind {
    /// 테이블 상태가 바뀌었다.
    Changed,
    /// 상태는 그대로이고, 예약해 둔 공개 시각(연출 끝·결과 안내·당첨금)이 지났다.
    /// 웹 화면만 다시 보내면 되고, Discord 상태 임베드는 새로 중계할 것이 있을 때만 고친다.
    RevealTick,
}

/// 테이블이 바뀌었다는 알림 (웹소켓 푸시·Discord 중계용).
#[derive(Debug, Clone)]
pub struct CasinoUpdate {
    pub table_id: String,
    pub kind: UpdateKind,
}

/// 모아서 저장하기: 변경마다 casino.json 전체를 기다려 쓰지 않고, 잠깐 모았다가 한 번 쓴다.
#[derive(Default)]
struct SaveSignal {
    dirty: AtomicBool,
    wake: Notify,
    started: AtomicBool,
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
    pub panel: Mutex<Option<PanelBinding>>,
    panel_text: Mutex<Option<String>>,
    /// 테이블 이름 확인과 예약·이름 변경을 한 번에 하나씩 해 같은 이름이 둘 생기지 않게 한다.
    names: tokio::sync::Mutex<()>,
    /// 채널을 만드는 중이라 아직 테이블 목록에 없는 이름 (소문자).
    reserved_names: Mutex<std::collections::HashSet<String>>,
    /// 채널별 최근 이름 변경 시각 (Discord는 10분에 두 번까지만 허용한다).
    channel_renames: Mutex<std::collections::HashMap<u64, Vec<Instant>>>,
    save_lock: tokio::sync::Mutex<()>,
    saver: Arc<SaveSignal>,
    /// 테이블별로 화면을 마지막으로 밀어 준 시각. 이 뒤로 공개 시각이 지나면 한 번 더 민다.
    reveal_marks: Mutex<std::collections::HashMap<String, i64>>,
    /// 카드가 아직 놓이는 중인 라운드의 하우스 손익 (테이블 id, reveal_until, 손익). 공개 시각이 지나면
    /// `house`에 더한다. 저장 파일에는 `house`에 더한 값으로 들어간다 (`save`).
    pending_house: Mutex<Vec<(String, i64, i64)>>,
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
    /// casino.json을 불러온다. 파일이 없으면 빈 카지노다. 파일이 있는데 읽거나 파싱하지 못하면
    /// 오류를 돌려준다: 빈 상태로 시작하면 다음 저장이 테이블과 앉은 사람들의 칩을 지운다.
    pub fn load(
        path: PathBuf,
        stats: Arc<RwLock<StatsFile>>,
        stats_path: Arc<PathBuf>,
    ) -> Result<Self> {
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
            panel: Mutex::new(None),
            panel_text: Mutex::new(None),
            names: tokio::sync::Mutex::new(()),
            reserved_names: Mutex::new(std::collections::HashSet::new()),
            channel_renames: Mutex::new(std::collections::HashMap::new()),
            save_lock: tokio::sync::Mutex::new(()),
            saver: Arc::new(SaveSignal::default()),
            reveal_marks: Mutex::new(std::collections::HashMap::new()),
            pending_house: Mutex::new(Vec::new()),
        };
        let file = load_file(&hub.path)?;
        hub.house.store(file.house, Ordering::Relaxed);
        *hub.panel
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = file.panel;
        for stored in file.tables {
            let id = stored.table.id.clone();
            hub.bindings.insert(id.clone(), stored.binding);
            hub.tables.insert(id, Arc::new(RwLock::new(stored.table)));
        }
        // 만료된 세션은 버린다.
        let now = now_ms();
        for stored in file.sessions {
            if stored.expires_at > now {
                hub.sessions.insert(
                    stored.token_sha256,
                    CasinoSession {
                        user_id: stored.user_id,
                        name: stored.name,
                        expires_at: stored.expires_at,
                    },
                );
            }
        }
        Ok(hub)
    }

    /// 테이블 상태가 바뀌었다고 알린다.
    pub fn notify(&self, table_id: &str) {
        self.mark_pushed(table_id, now_ms());
        let _ = self.updates.send(CasinoUpdate {
            table_id: table_id.to_string(),
            kind: UpdateKind::Changed,
        });
    }

    /// 공개 시각이 지나 화면만 다시 보내면 된다고 알린다.
    fn notify_reveal(&self, table_id: &str) {
        let _ = self.updates.send(CasinoUpdate {
            table_id: table_id.to_string(),
            kind: UpdateKind::RevealTick,
        });
    }

    /// `at`까지의 상태를 화면에 보냈다고 기록한다 (그 전의 공개 시각은 다시 알리지 않는다).
    fn mark_pushed(&self, table_id: &str, at: i64) {
        let mut marks = self
            .reveal_marks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mark = marks.entry(table_id.to_string()).or_insert(at);
        *mark = (*mark).max(at);
    }

    /// 지난번에 화면을 보낸 뒤 `now`까지 새로 드러난 것이 있으면 true (그리고 `now`까지
    /// 보냈다고 기록한다). 공개 시각 하나마다 알림이 한 번만 나간다.
    fn take_reveal_crossing(&self, table_id: &str, table: &CasinoTable, now: i64) -> bool {
        let mut marks = self
            .reveal_marks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // 처음 보는 테이블(재시작 직후 등)은 지금까지 보낸 것으로 친다.
        let mark = marks.entry(table_id.to_string()).or_insert(now);
        let crossed = table.reveals_between(*mark, now);
        *mark = (*mark).max(now);
        crossed
    }

    // ------------------------------------------------------------ 저장

    /// 모아서 저장하는 작업을 띄운다 (한 번만). 저장 요청 뒤 잠깐 기다렸다가 한 번에 쓴다.
    pub fn start_saver(self: &Arc<Self>) {
        if self.saver.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let signal = self.saver.clone();
        let hub = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                signal.wake.notified().await;
                tokio::time::sleep(SAVE_COALESCE).await;
                let Some(hub) = hub.upgrade() else {
                    break;
                };
                hub.flush_pending_save().await;
            }
        });
    }

    /// 곧 저장하도록 요청한다 (기다리지 않는다).
    pub fn request_save(&self) {
        self.saver.dirty.store(true, Ordering::SeqCst);
        self.saver.wake.notify_one();
    }

    /// 아직 쓰지 않은 저장 요청이 있으면 지금 쓴다 (종료 직전 등).
    pub async fn flush_pending_save(&self) {
        if self.saver.dirty.load(Ordering::SeqCst) {
            self.save().await;
        }
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
        // 이 뒤의 변경은 다시 저장 요청을 남긴다. 그 전의 변경은 아래 스냅샷에 들어간다.
        self.saver.dirty.store(false, Ordering::SeqCst);
        let mut tables = Vec::new();
        for (id, handle) in self.table_handles() {
            let table = handle.read().await.clone();
            let binding = self.binding(&id).unwrap_or_default();
            tables.push(StoredTable { table, binding });
        }
        tables.sort_by(|left, right| left.table.created_at.cmp(&right.table.created_at));
        let now = now_ms();
        let mut sessions = self
            .sessions
            .iter()
            .filter(|entry| entry.value().expires_at > now)
            .map(|entry| StoredSession {
                token_sha256: entry.key().clone(),
                user_id: entry.value().user_id,
                name: entry.value().name.clone(),
                expires_at: entry.value().expires_at,
            })
            .collect::<Vec<_>>();
        sessions.sort_by(|left, right| left.token_sha256.cmp(&right.token_sha256));
        // 공개 전 손익(`pending_house`)도 더해 저장한다: 연출 중에 재시작해도 빠지지 않는다 (파일은 화면에
        // 보이지 않는다). 정리할 때 잠금을 쥔 채 `house`에 더하므로 같은 잠금 안에서 읽어야 두 번 세지 않는다.
        let house = {
            let pending = self
                .pending_house
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            self.house.load(Ordering::Relaxed)
                + pending.iter().map(|(_, _, delta)| delta).sum::<i64>()
        };
        let file = CasinoFile {
            house,
            tables,
            panel: self.panel(),
            sessions,
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

    /// 개인 링크 토큰을 만든다. 세션 표와 casino.json에는 토큰의 SHA-256만 남긴다.
    pub fn issue_session(&self, user_id: u64, name: String) -> String {
        let now = now_ms();
        self.sessions.retain(|_, session| session.expires_at > now);
        let mut bytes = [0u8; 32];
        system_random::fill_bytes(&mut bytes);
        let mut token = String::with_capacity(64);
        for byte in bytes {
            let _ = write!(&mut token, "{byte:02x}");
        }
        self.sessions.insert(
            token_hash(&token),
            CasinoSession {
                user_id,
                name,
                expires_at: now + CASINO_SESSION_TTL_SECONDS as i64 * 1000,
            },
        );
        self.request_save();
        token
    }

    pub fn session(&self, token: &str) -> Option<CasinoSession> {
        let key = token_hash(token);
        let session = self.sessions.get(&key)?.clone();
        if session.expires_at <= now_ms() {
            self.sessions.remove(&key);
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
        let now = now_ms();
        for (id, handle) in self.table_handles() {
            let table = handle.read().await;
            // 결과를 만든 카드가 아직 놓이는 중이면 정산 전 단계로 보여 준다 (화면과 같은 기준).
            let phase = table
                .round
                .as_ref()
                .map(|round| shown_phase(table.kind, round, now));
            summaries.push(TableSummary {
                id: table.id.clone(),
                kind: table.kind,
                kind_text: table.kind.value().to_string(),
                name: table.name.clone(),
                seated: table.seats.iter().flatten().count(),
                seat_count: table.seats.len(),
                playing: phase.is_some_and(|phase| phase != Phase::Complete),
                phase_text: phase.map(|phase| phase.value().to_string()),
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
        self.check_new_table(name)?;
        let name = name.trim();
        let id = format!("{}-{}", kind.key(), short_id());
        let table =
            CasinoTable::new(id.clone(), kind, name, created_by, now_ms()).with_settings(settings);
        self.bindings.insert(id.clone(), binding);
        self.tables.insert(id.clone(), Arc::new(RwLock::new(table)));
        Ok(id)
    }

    /// 새 테이블을 만들 수 있는지 (개수 한도, 이름 길이). 채널을 만들기 전에도 부른다.
    pub fn check_new_table(&self, name: &str) -> Result<(), String> {
        if self.tables.len() >= MAX_TABLES {
            return Err(format!(
                "테이블은 최대 {MAX_TABLES}개까지 만들 수 있습니다."
            ));
        }
        let count = name.trim().chars().count();
        if !(2..=24).contains(&count) {
            return Err("테이블 이름은 2~24자로 입력하세요.".to_string());
        }
        Ok(())
    }

    /// 다른 테이블이나 만드는 중인 테이블이 이 이름(대소문자 무시)을 쓰는지.
    async fn name_taken(&self, name: &str, except_id: Option<&str>) -> bool {
        let key = name.trim().to_lowercase();
        if self
            .reserved_names
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(&key)
        {
            return true;
        }
        for (id, handle) in self.table_handles() {
            if Some(id.as_str()) != except_id && handle.read().await.name.to_lowercase() == key {
                return true;
            }
        }
        false
    }

    /// 채널을 만드는 동안 이름을 맡아 둔다. 돌려받은 값을 버리면(테이블 등록 뒤) 풀린다.
    /// 이름 잠금은 확인과 예약 사이에만 쥐어, 느린 채널 생성이 다른 생성·이름 변경을 막지 않는다.
    pub async fn reserve_table_name(
        self: &Arc<Self>,
        name: &str,
    ) -> Result<NameReservation, String> {
        let _names = self.names.lock().await;
        self.check_new_table(name)?;
        // 만드는 중인 테이블도 개수에 넣는다 (동시에 만들어 한도를 넘긴 채널이 생기지 않게).
        let reserved = self
            .reserved_names
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len();
        if self.tables.len() + reserved >= MAX_TABLES {
            return Err(format!(
                "테이블은 최대 {MAX_TABLES}개까지 만들 수 있습니다."
            ));
        }
        if self.name_taken(name, None).await {
            return Err("같은 이름의 테이블이 이미 있습니다.".to_string());
        }
        let key = name.trim().to_lowercase();
        self.reserved_names
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key.clone());
        Ok(NameReservation {
            hub: self.clone(),
            key,
        })
    }

    /// 이 채널 이름을 지금 바꿔도 되는지 보고, 되면 기록한다. Discord는 채널 이름을 10분에
    /// 두 번까지만 바꾸게 하고, 넘으면 요청이 몇 분씩 멈춰 같은 채널의 다른 요청(삭제 등)까지
    /// 막는다. 그래서 넘을 것 같으면 아예 보내지 않는다.
    pub fn take_channel_rename(&self, channel_id: u64) -> bool {
        const WINDOW: Duration = Duration::from_secs(10 * 60);
        let now = Instant::now();
        let mut renames = self
            .channel_renames
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let recent = renames.entry(channel_id).or_default();
        recent.retain(|at| now.saturating_duration_since(*at) < WINDOW);
        if recent.len() >= 2 {
            return false;
        }
        recent.push(now);
        true
    }

    pub async fn rename_table(&self, table_id: &str, new_name: &str) -> Result<bool, String> {
        let _names = self.names.lock().await;
        let name = new_name.trim();
        if name.chars().count() < 2 || name.chars().count() > 24 {
            return Err("테이블 이름은 2~24자로 입력하세요.".to_string());
        }
        if self.name_taken(name, Some(table_id)).await {
            return Err("같은 이름의 테이블이 이미 있습니다.".to_string());
        }
        let Some(table) = self.table(table_id) else {
            return Err("테이블을 찾을 수 없습니다.".to_string());
        };
        let mut table = table.write().await;
        if table.name == name {
            return Ok(false);
        }
        table.name = name.to_string();
        table.version += 1;
        drop(table);
        self.notify(table_id);
        Ok(true)
    }

    pub async fn update_settings(
        &self,
        table_id: &str,
        settings: TableSettings,
    ) -> Result<bool, String> {
        let Some(table) = self.table(table_id) else {
            return Err("테이블을 찾을 수 없습니다.".to_string());
        };
        let applied = table.write().await.change_settings(settings, now_ms());
        self.notify(table_id);
        Ok(applied)
    }

    /// 테이블을 닫고 모든 칩을 코인으로 돌려준다. 반환: 캐시아웃 목록.
    pub async fn close_table(&self, table_id: &str) -> Vec<CasinoEvent> {
        let Some((_, table)) = self.tables.remove(table_id) else {
            return Vec::new();
        };
        let events = table.write().await.close();
        self.apply_events(&events).await;
        self.flush_pending_house(Some(table_id), now_ms());
        if let Some((_, binding)) = self.bindings.remove(table_id) {
            self.webhooks.remove(&binding.channel_id);
        }
        self.reveal_marks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(table_id);
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

    /// 화면에 보여 줄 하우스 누적. 카드를 여는 중인 라운드의 손익은 `pending_house`에 있다가
    /// 공개 시각이 지나야 `house`에 더해지므로 결과가 먼저 새지 않는다.
    pub async fn visible_house(&self) -> i64 {
        self.house.load(Ordering::Relaxed)
    }

    fn flush_pending_house(&self, table_id: Option<&str>, now: i64) {
        let mut pending = self
            .pending_house
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut applied = 0;
        pending.retain(|(id, reveal_until, delta)| {
            let matches = table_id.is_none_or(|wanted| wanted == id);
            if matches && (table_id.is_some() || now >= *reveal_until) {
                applied += *delta;
                false
            } else {
                true
            }
        });
        if applied != 0 {
            self.house.fetch_add(applied, Ordering::Relaxed);
        }
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
                CasinoEvent::RoundSettled {
                    house_delta,
                    reveal_until,
                    table_id,
                    ..
                } => {
                    self.pending_house
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push((table_id.clone(), *reveal_until, *house_delta));
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
        let (result, changed) = {
            let mut table = table.write().await;
            let version = table.version;
            let result = table.apply_command(user_id, user_name, command, expected_version, now);
            (result, table.version != version)
        };
        let events = match result {
            Ok(events) => events,
            Err(error) => {
                if reserved > 0 {
                    let mut stats_file = self.stats.write().await;
                    stats::refund_coins(&mut stats_file, user_id, user_name, reserved);
                }
                // 실패했어도 테이블이 바뀌었으면(예: 기다리던 방 설정 적용) 알리고 저장한다.
                if changed {
                    self.notify(table_id);
                    self.request_save();
                }
                return Err(error);
            }
        };
        if reserved > 0 {
            self.save_stats().await;
        }
        self.apply_events(&events).await;
        self.notify(table_id);
        // 코인이 오간 변경(바이인·캐시아웃)은 좌석과 코인이 어긋나지 않게 바로 저장한다.
        // 나머지는 모아서 저장해 버튼 응답이 파일 쓰기를 기다리지 않게 한다.
        if reserved > 0 || moves_coins(&events) {
            self.save().await;
        } else {
            self.request_save();
        }
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
    /// 상태가 그대로인 테이블은 예약된 공개 시각(연출 끝·결과 안내·당첨금)이 지났을 때만
    /// 한 번 `RevealTick`을 보낸다 (연출 중 250ms마다 전체 화면을 밀던 것을 대신한다).
    pub async fn tick_all(&self) -> Vec<String> {
        let now = now_ms();
        self.flush_pending_house(None, now);
        let mut changed = Vec::new();
        let mut revealed = Vec::new();
        let mut coins_moved = false;
        for (id, table) in self.table_handles() {
            let (before, result) = {
                let mut table = table.write().await;
                let before = table.version;
                let result = table.tick(now);
                (before, result)
            };
            match result {
                Ok(events) => {
                    coins_moved |= moves_coins(&events);
                    self.apply_events(&events).await;
                    let guard = table.read().await;
                    if guard.version != before {
                        changed.push(id);
                    } else if self.take_reveal_crossing(&id, &guard, now) {
                        revealed.push(id);
                    }
                }
                Err(error) => eprintln!("casino tick failed for {id}: {error}"),
            }
        }
        self.flush_pending_house(None, now);
        for id in &changed {
            self.notify(id);
        }
        for id in &revealed {
            self.notify_reveal(id);
        }
        if coins_moved {
            self.save().await;
        } else if !changed.is_empty() {
            self.request_save();
        }
        changed
    }

    /// 웹·중계용 화면 상태와, 그 화면을 만든 서버 시각. 시각은 테이블을 읽은 뒤에 한 번만 잰다
    /// (화면의 공개 판정과 클라이언트에 보내는 `server_time`이 같은 시각을 쓰게).
    pub async fn view_now(&self, table_id: &str, viewer: Option<u64>) -> Option<(TableView, i64)> {
        let table = self.table(table_id)?;
        let table = table.read().await;
        let now = now_ms();
        Some((table_view(&table, viewer, now), now))
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

    pub fn panel(&self) -> Option<PanelBinding> {
        self.panel
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// 새 패널 연결과 그 패널에 그린 본문을 한 번에 기록하고, 바뀌기 전 연결을 돌려준다.
    /// 잠금 순서는 항상 panel → panel_text. 두 관리자가 동시에 패널을 올려도 돌려받은 옛
    /// 패널을 지우면 고정된 패널이 하나만 남는다.
    pub fn set_panel_with_text(
        &self,
        panel: Option<PanelBinding>,
        text: Option<String>,
    ) -> Option<PanelBinding> {
        let mut current = self
            .panel
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = std::mem::replace(&mut *current, panel);
        *self
            .panel_text
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = text;
        previous
    }

    pub fn panel_text(&self) -> Option<String> {
        self.panel_text
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// 패널이 아직 `message_id`를 가리킬 때만 연결을 끊는다. 갱신이 지워진 옛 패널을 고치다
    /// 실패하는 동안 새 패널이 올라왔으면 새 연결은 그대로 둔다. 끊었으면 true.
    pub fn clear_panel_if(&self, message_id: u64) -> bool {
        let mut panel = self
            .panel
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if panel.as_ref().map(|panel| panel.message_id) != Some(message_id) {
            return false;
        }
        *panel = None;
        // panel 잠금을 쥔 채로 지워, 그 사이 새 패널 본문이 끼어들지 않게 한다.
        *self
            .panel_text
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        true
    }

    /// 패널이 아직 `message_id`를 가리킬 때만 마지막으로 그린 본문을 기록한다.
    pub fn record_panel_text(&self, message_id: u64, text: String) {
        let panel = self
            .panel
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if panel.as_ref().map(|panel| panel.message_id) == Some(message_id) {
            // panel 잠금을 쥔 채로 써서, 확인과 기록 사이에 새 패널이 올라오지 못하게 한다.
            *self
                .panel_text
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(text);
        }
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
    if mafia_remake::atomic_file::is_missing(path) {
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

#[cfg(test)]
mod panel_tests {
    use super::*;

    fn temp_hub(name: &str) -> (CasinoHub, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "mafia-casino-hub-{name}-{}-{}",
            std::process::id(),
            mafia_remake::atomic_file::next_seq()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let hub = CasinoHub::load(
            dir.join("casino.json"),
            Arc::new(RwLock::new(StatsFile::default())),
            Arc::new(dir.join("stats.json")),
        )
        .unwrap();
        (hub, dir)
    }

    #[tokio::test]
    async fn renaming_rejects_duplicates_and_bad_lengths() {
        let (hub, dir) = temp_hub("rename");
        let table = |kind, name: &str| {
            hub.create_table(
                kind,
                name,
                1,
                TableBinding::default(),
                TableSettings::default(),
            )
            .unwrap()
        };
        let first = table(GameKind::Holdem, "하이롤러");
        table(GameKind::Blackjack, "Lucky");

        // 다른 테이블 이름은 대소문자만 달라도 못 쓴다.
        assert!(hub.rename_table(&first, "lucky").await.is_err());
        assert!(hub.rename_table(&first, "x").await.is_err());
        assert!(hub.rename_table(&first, &"가".repeat(25)).await.is_err());
        assert!(hub.rename_table("missing", "새 이름").await.is_err());
        // 같은 이름은 바뀐 것이 없다.
        assert_eq!(hub.rename_table(&first, "하이롤러").await, Ok(false));
        assert_eq!(hub.rename_table(&first, "  새 이름 ").await, Ok(true));
        let renamed = hub.table(&first).unwrap().read().await.name.clone();
        assert_eq!(renamed, "새 이름");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn a_reserved_name_blocks_creates_and_renames_until_released() {
        let (hub, dir) = temp_hub("reserve");
        let hub = Arc::new(hub);
        let other = hub
            .create_table(
                GameKind::Holdem,
                "하이롤러",
                1,
                TableBinding::default(),
                TableSettings::default(),
            )
            .unwrap();
        let reservation = hub.reserve_table_name(" VIP ").await.unwrap();
        // 채널을 만드는 동안 같은 이름(대소문자 무시)은 예약도, 이름 변경도 안 된다.
        assert!(hub.reserve_table_name("vip").await.is_err());
        assert!(hub.rename_table(&other, "Vip").await.is_err());
        assert!(hub.reserve_table_name("하이롤러").await.is_err());
        assert!(hub.reserve_table_name("x").await.is_err());
        drop(reservation);
        assert_eq!(hub.rename_table(&other, "Vip").await, Ok(true));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn tables_being_created_count_toward_the_limit() {
        let (hub, dir) = temp_hub("reserve-limit");
        let hub = Arc::new(hub);
        for index in 0..MAX_TABLES - 1 {
            hub.create_table(
                GameKind::Holdem,
                &format!("테이블{index}"),
                1,
                TableBinding::default(),
                TableSettings::default(),
            )
            .unwrap();
        }
        let last = hub.reserve_table_name("마지막").await.unwrap();
        assert!(hub.reserve_table_name("하나 더").await.is_err());
        drop(last);
        assert!(hub.reserve_table_name("하나 더").await.is_ok());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn channel_renames_are_limited_to_two_per_channel() {
        let (hub, dir) = temp_hub("renames");
        assert!(hub.take_channel_rename(10));
        assert!(hub.take_channel_rename(10));
        assert!(!hub.take_channel_rename(10));
        // 다른 채널은 따로 센다.
        assert!(hub.take_channel_rename(11));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_stale_panel_refresh_does_not_clear_a_new_panel() {
        let (hub, dir) = temp_hub("panel-race");
        let panel = |message_id| PanelBinding {
            guild_id: 1,
            channel_id: 2,
            message_id,
        };
        assert!(hub.set_panel_with_text(Some(panel(20)), None).is_none());
        // 새 패널을 올리면 옛 연결을 돌려준다 (그 메시지를 지운다).
        assert_eq!(
            hub.set_panel_with_text(Some(panel(30)), None)
                .map(|old| old.message_id),
            Some(20)
        );
        // 옛 패널(20)을 고치다 404가 났어도 새 패널(30) 연결은 남는다.
        assert!(!hub.clear_panel_if(20));
        hub.record_panel_text(20, "old".to_string());
        assert_eq!(hub.panel().unwrap().message_id, 30);
        assert_eq!(hub.panel_text(), None);
        hub.record_panel_text(30, "new".to_string());
        assert_eq!(hub.panel_text().as_deref(), Some("new"));
        assert!(hub.clear_panel_if(30));
        assert!(hub.panel().is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn casino_files_from_before_the_panel_still_load() {
        let (_, dir) = temp_hub("panel");
        let path = dir.join("casino.json");
        std::fs::write(&path, "{\"house\": 7, \"tables\": []}").unwrap();
        let load = || {
            CasinoHub::load(
                path.clone(),
                Arc::new(RwLock::new(StatsFile::default())),
                Arc::new(dir.join("stats.json")),
            )
            .unwrap()
        };
        let hub = load();
        assert!(hub.panel().is_none());
        assert_eq!(hub.house.load(Ordering::Relaxed), 7);

        // 패널 위치는 저장했다가 다시 불러온다.
        hub.set_panel_with_text(
            Some(PanelBinding {
                guild_id: 1,
                channel_id: 2,
                message_id: 3,
            }),
            None,
        );
        hub.save().await;
        let panel = load().panel().unwrap();
        assert_eq!(
            (panel.guild_id, panel.channel_id, panel.message_id),
            (1, 2, 3)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
mod hub_tests {
    use super::*;
    use mafia_remake::casino::ChatMessage;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mafia-casino-hub-{name}-{}-{}",
            std::process::id(),
            mafia_remake::atomic_file::next_seq()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn load_hub(dir: &Path) -> CasinoHub {
        CasinoHub::load(
            dir.join("casino.json"),
            Arc::new(RwLock::new(StatsFile::default())),
            Arc::new(dir.join("stats.json")),
        )
        .unwrap()
    }

    fn scheduled(seq: u64, at: i64, text: &str) -> ChatMessage {
        ChatMessage {
            id: format!("m{seq}"),
            seq,
            name: "딜러".to_string(),
            text: text.to_string(),
            at,
            dealer: true,
            user_id: None,
            from_discord: false,
        }
    }

    /// 봇 바이너리의 테스트는 엔진을 실제 연출 시간으로 돌린다 (엔진 단위 테스트는 0).
    /// 실제 시간표에서 모든 착지가 비행 시간 + 80ms 이상 떨어져 공중에 카드가 한 장뿐인지 본다.
    #[test]
    fn live_timeline_keeps_one_card_in_the_air() {
        use mafia_remake::casino::{
            CARD_FLIGHT_MS, CasinoCommand, DEAL_LEAD_MS, LAND_SETTLE_MS, live_timing,
        };
        assert_eq!(
            CARD_FLIGHT_MS,
            live_timing::CARD_FLIGHT_MS,
            "실제 시간으로 돈다"
        );
        let join = |table: &mut CasinoTable, user: u64, seat: usize| {
            table
                .apply_command(
                    user,
                    "손님",
                    &CasinoCommand::Join {
                        seat,
                        amount: 10_000,
                        name: format!("손님{user}"),
                    },
                    None,
                    0,
                )
                .unwrap();
        };
        let deck = |cards: &[&str]| {
            let mut deck = cards
                .iter()
                .map(|card| card.to_string())
                .collect::<Vec<_>>();
            deck.reverse();
            deck
        };
        let assert_spaced = |mut landings: Vec<i64>, what: &str| {
            landings.sort_unstable();
            for pair in landings.windows(2) {
                assert!(
                    pair[1] - pair[0] >= CARD_FLIGHT_MS + 80,
                    "{what}: {landings:?}"
                );
            }
        };

        // 블랙잭 3명: 딜, 히트, 딜러 드로.
        let mut table = CasinoTable::new("bj", GameKind::Blackjack, "블랙잭", 1, 0);
        for (user, seat) in [(1, 0), (2, 1), (3, 2)] {
            join(&mut table, user, seat);
        }
        table
            .start_with_deck(
                1,
                deck(&[
                    "2h", "3h", "4h", "6c", "5h", "6h", "7h", "Tc", "2d", "3d", "4d", "2c",
                ]),
                1_000,
            )
            .unwrap();
        for (user, at) in [(1, 2_000), (2, 2_100), (3, 2_200)] {
            table
                .apply_command(
                    user,
                    "손님",
                    &CasinoCommand::Bet {
                        amount: 100,
                        pairs: 0,
                        plus3: 0,
                    },
                    None,
                    at,
                )
                .unwrap();
        }
        let round = table.round.as_ref().unwrap();
        assert_eq!(
            round.dealer_reveal_at[0],
            2_200 + DEAL_LEAD_MS + 3 * live_timing::DEAL_CARD_MS
        );
        assert_eq!(
            round.reveal_until,
            round.dealer_reveal_at[1] + LAND_SETTLE_MS
        );
        // 차례대로 한 번씩 히트하고 스탠드 (연출이 끝난 뒤에만 받는다).
        for user in [1, 2, 3] {
            for command in [CasinoCommand::Hit, CasinoCommand::Stand] {
                let at = table.round.as_ref().unwrap().reveal_until;
                table
                    .apply_command(user, "손님", &command, None, at)
                    .unwrap();
            }
        }
        let round = table.round.as_ref().unwrap();
        assert_eq!(round.phase, mafia_remake::casino::Phase::Complete);
        assert!(
            round.dealer.len() > 2,
            "딜러가 드로한다: {:?}",
            round.dealer
        );
        let mut landings = round.dealer_reveal_at.clone();
        for seat in table.seats.iter().flatten() {
            for hand in &seat.hands {
                landings.extend(hand.reveal_at.iter().copied());
            }
        }
        assert_spaced(landings, "블랙잭");

        // 홀덤 3명: 홀 카드, 번 카드, 보드.
        let mut table = CasinoTable::new("th", GameKind::Holdem, "홀덤", 1, 0);
        for (user, seat) in [(1, 0), (2, 1), (3, 2)] {
            join(&mut table, user, seat);
        }
        table
            .start_with_deck(
                1,
                deck(&[
                    "Ah", "Kh", "Qh", "Ad", "Kd", "Qd", "2c", "7h", "8d", "9s", "3c", "Jd", "4c",
                    "5h",
                ]),
                1_000,
            )
            .unwrap();
        // 끝까지 콜·체크한다. 차례인 사람이 콜할 게 있으면 콜, 없으면 체크.
        while table.playing() {
            let round = table.round.as_ref().unwrap();
            let (turn, at) = (round.turn, round.reveal_until);
            let user = table.seat(turn).unwrap().user_id;
            let legal = table.poker_legal_for(turn).unwrap();
            let command = if legal.can_check {
                CasinoCommand::Check
            } else {
                CasinoCommand::Call
            };
            table
                .apply_command(user, "손님", &command, None, at)
                .unwrap();
        }
        let round = table.round.as_ref().unwrap();
        assert_eq!(round.burn_at.len(), 3);
        let mut landings = round.board_reveal_at.clone();
        landings.extend(round.burn_at.iter().copied());
        for seat in table.seats.iter().flatten() {
            landings.extend(seat.cards_reveal_at.iter().copied());
        }
        assert_spaced(landings, "홀덤");
    }

    #[tokio::test]
    async fn sessions_survive_a_restart_without_writing_raw_tokens() {
        let dir = temp_dir("sessions");
        let hub = load_hub(&dir);
        let token = hub.issue_session(7, "세션".to_string());
        assert_eq!(hub.session(&token).map(|session| session.user_id), Some(7));
        assert!(hub.session("not-a-token").is_none());
        hub.save().await;
        let text = std::fs::read_to_string(dir.join("casino.json")).unwrap();
        assert!(!text.contains(&token), "토큰 원문은 디스크에 쓰지 않는다");
        assert!(text.contains(&token_hash(&token)));

        // 저장 파일에 만료된 세션을 섞어도 불러올 때 버린다.
        let mut file: serde_json::Value = serde_json::from_str(&text).unwrap();
        file["sessions"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "token_sha256": token_hash("expired-token"),
                "user_id": 8,
                "name": "만료",
                "expires_at": now_ms() - 1,
            }));
        std::fs::write(dir.join("casino.json"), file.to_string()).unwrap();

        let restarted = load_hub(&dir);
        let session = restarted
            .session(&token)
            .expect("재시작 뒤에도 링크가 이어진다");
        assert_eq!((session.user_id, session.name.as_str()), (7, "세션"));
        assert!(restarted.session("expired-token").is_none());
        assert_eq!(restarted.sessions.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn expired_sessions_are_rejected_and_not_saved() {
        let dir = temp_dir("sessions-expired");
        let hub = load_hub(&dir);
        let expired = || CasinoSession {
            user_id: 9,
            name: "지난".to_string(),
            expires_at: now_ms() - 1,
        };
        hub.sessions.insert(token_hash("old"), expired());
        assert!(hub.session("old").is_none());
        hub.sessions.insert(token_hash("old"), expired());
        hub.save().await;
        let text = std::fs::read_to_string(dir.join("casino.json")).unwrap();
        assert!(!text.contains(&token_hash("old")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn reveal_ticks_fire_once_per_crossed_reveal_time() {
        let dir = temp_dir("reveal-tick");
        let hub = load_hub(&dir);
        let id = hub
            .create_table(
                GameKind::Blackjack,
                "공개 알림",
                1,
                TableBinding::default(),
                TableSettings::default(),
            )
            .unwrap();
        let mut updates = hub.updates.subscribe();
        // 처음 보는 테이블은 지금까지 보낸 것으로 친다.
        hub.tick_all().await;
        assert!(updates.try_recv().is_err());

        // 카드가 다 놓인 뒤에 뜰 안내를 예약한다.
        let at = now_ms() + 40;
        hub.table(&id)
            .unwrap()
            .write()
            .await
            .messages
            .push(scheduled(1, at, "결과"));
        hub.tick_all().await;
        assert!(updates.try_recv().is_err(), "공개 시각 전에는 밀지 않는다");
        tokio::time::sleep(Duration::from_millis(80)).await;
        hub.tick_all().await;
        let update = updates.try_recv().unwrap();
        assert_eq!(update.table_id, id);
        assert_eq!(update.kind, UpdateKind::RevealTick);
        hub.tick_all().await;
        assert!(
            updates.try_recv().is_err(),
            "같은 공개 시각으로 두 번 알리지 않는다"
        );
        // 상태가 바뀐 알림은 Changed다.
        hub.relay_chat_from_discord(&id, 9, "손님", "안녕").await;
        assert_eq!(updates.try_recv().unwrap().kind, UpdateKind::Changed);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn summaries_and_house_wait_for_the_cards_to_land() {
        use mafia_remake::casino::{CasinoCommand, Phase};
        let dir = temp_dir("summary-phase");
        let hub = load_hub(&dir);
        let id = hub
            .create_table(
                GameKind::Blackjack,
                "단계",
                1,
                TableBinding::default(),
                TableSettings::default(),
            )
            .unwrap();
        hub.house.store(50_000, Ordering::Relaxed);
        let now = now_ms();
        let (house_delta, mut events) = {
            let table = hub.table(&id).unwrap();
            let mut table = table.write().await;
            table
                .apply_command(
                    1,
                    "손님",
                    &CasinoCommand::Join {
                        seat: 0,
                        amount: 10_000,
                        name: "손님1".to_string(),
                    },
                    None,
                    now,
                )
                .unwrap();
            // 딜러 내추럴 (업카드 T): 딜하자마자 정산되지만 카드는 아직 날아가는 중이다.
            let mut deck = ["9h", "Td", "7c", "As"]
                .iter()
                .map(|card| card.to_string())
                .collect::<Vec<_>>();
            deck.reverse();
            table.start_with_deck(1, deck, now).unwrap();
            let events = table
                .apply_command(
                    1,
                    "손님",
                    &CasinoCommand::Bet {
                        amount: 500,
                        pairs: 0,
                        plus3: 0,
                    },
                    None,
                    now,
                )
                .unwrap();
            assert_eq!(table.round.as_ref().unwrap().phase, Phase::Complete);
            let house_delta = events
                .iter()
                .find_map(|event| match event {
                    CasinoEvent::RoundSettled { house_delta, .. } => Some(*house_delta),
                    _ => None,
                })
                .unwrap();
            (house_delta, events)
        };
        assert_eq!(house_delta, 500);
        // 정산 이벤트가 하우스에 더해져도 카드를 다 열기 전에는 보여 주지 않는다.
        async fn summary(hub: &CasinoHub, id: &str) -> TableSummary {
            hub.summaries()
                .await
                .into_iter()
                .find(|summary| summary.id == id)
                .unwrap()
        }
        let during = summary(&hub, &id).await;
        assert!(during.playing);
        assert_eq!(during.phase_text.as_deref(), Some("플레이 중"));
        assert_eq!(hub.visible_house().await, 50_000);
        // 공개 전 손익은 화면에 없지만 저장 파일에는 들어가 연출 중에 재시작해도 빠지지 않는다.
        let mut later = events.clone();
        for event in &mut later {
            if let CasinoEvent::RoundSettled { reveal_until, .. } = event {
                *reveal_until = now + 60_000;
            }
        }
        hub.apply_events(&later).await;
        hub.tick_all().await;
        assert_eq!(hub.visible_house().await, 50_000);
        hub.save().await;
        assert_eq!(load_hub(&dir).house.load(Ordering::Relaxed), 50_500);
        // 카드를 다 연 뒤에는 라운드 종료, 하우스 누적에도 더해진다.
        {
            let table = hub.table(&id).unwrap();
            let mut table = table.write().await;
            let round = table.round.as_mut().unwrap();
            round.dealer_flip_at = now - 2;
            round.reveal_until = now - 1;
        }
        for event in &mut events {
            if let CasinoEvent::RoundSettled {
                reveal_until: value,
                ..
            } = event
            {
                *value = now - 1;
            }
        }
        hub.apply_events(&events).await;
        hub.tick_all().await;
        let after = summary(&hub, &id).await;
        assert!(!after.playing);
        assert_eq!(after.phase_text.as_deref(), Some("라운드 종료"));
        assert_eq!(hub.visible_house().await, 50_500);
        assert_eq!(hub.house.load(Ordering::Relaxed), 50_500);
        // 테이블을 닫으면 공개 전 손익(위의 1분 뒤 것과 아래 것)도 바로 더한다.
        hub.apply_events(&events).await;
        hub.close_table(&id).await;
        assert_eq!(hub.house.load(Ordering::Relaxed), 51_500);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn scheduled_messages_are_relayed_in_order_once_visible() {
        let dir = temp_dir("relay-order");
        let hub = load_hub(&dir);
        let id = hub
            .create_table(
                GameKind::Blackjack,
                "중계 순서",
                1,
                TableBinding::default(),
                TableSettings::default(),
            )
            .unwrap();
        let now = now_ms();
        {
            let table = hub.table(&id).unwrap();
            let mut table = table.write().await;
            table.messages.push(scheduled(1, now - 10, "앞"));
            table.messages.push(scheduled(2, now + 60_000, "결과"));
            table.messages.push(scheduled(3, now, "채팅"));
        }
        // 아직 뜨지 않은 결과 안내에서 멈춰야, 그 뒤 메시지 때문에 결과를 건너뛰지 않는다.
        let texts = hub
            .unrelayed_messages(&id)
            .await
            .into_iter()
            .map(|message| message.text)
            .collect::<Vec<_>>();
        assert_eq!(texts, vec!["앞".to_string()]);
        assert!(hub.has_relay_work(&id).await);
        hub.update_binding(&id, |binding| binding.relayed_message_seq = 1);
        assert!(!hub.has_relay_work(&id).await);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn coalesced_saves_are_written_shortly_after_the_request() {
        let dir = temp_dir("saver");
        let path = dir.join("casino.json");
        let hub = Arc::new(load_hub(&dir));
        hub.start_saver();
        hub.create_table(
            GameKind::Holdem,
            "모아 저장",
            1,
            TableBinding::default(),
            TableSettings::default(),
        )
        .unwrap();
        hub.request_save();
        hub.request_save();
        assert!(!path.exists(), "요청하자마자 쓰지 않는다");
        tokio::time::sleep(Duration::from_millis(800)).await;
        let text = std::fs::read_to_string(&path).expect("잠시 뒤 한 번에 쓴다");
        assert!(text.contains("모아 저장"));

        // 저장기가 없어도 종료 직전 flush가 남은 요청을 쓴다.
        let other_dir = temp_dir("flush");
        let other = load_hub(&other_dir);
        other.flush_pending_save().await;
        assert!(
            !other_dir.join("casino.json").exists(),
            "요청이 없으면 쓰지 않는다"
        );
        other.request_save();
        other.flush_pending_save().await;
        assert!(other_dir.join("casino.json").exists());
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&other_dir).unwrap();
    }
}

/// 채널을 만드는 동안 맡아 둔 테이블 이름. 버리면 풀린다.
pub struct NameReservation {
    hub: Arc<CasinoHub>,
    key: String,
}

impl Drop for NameReservation {
    fn drop(&mut self) {
        self.hub
            .reserved_names
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.key);
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

    /// 상태 임베드용: 테이블 채팅 중 아직 중계하지 않은 메시지. 공개 시각이 안 된 메시지
    /// (카드가 다 놓인 뒤 뜰 결과 안내 등)에서 멈춘다. 그 뒤 메시지를 먼저 보내면 중계 위치
    /// (`relayed_message_seq`)가 앞질러 그 안내를 영영 건너뛰기 때문이다.
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
        let now = now_ms();
        table
            .messages
            .iter()
            .filter(|message| message.seq > since)
            .take_while(|message| message.at <= now)
            .cloned()
            .collect()
    }

    /// Discord 채널에 새로 중계할 것(공개된 채팅·결과)이 있는지. 공개 시각 알림(`RevealTick`)은
    /// 이때만 상태 임베드를 고친다.
    pub async fn has_relay_work(&self, table_id: &str) -> bool {
        let Some(binding) = self.binding(table_id) else {
            return false;
        };
        let Some(table) = self.table(table_id) else {
            return false;
        };
        let table = table.read().await;
        let now = now_ms();
        let chat = table
            .messages
            .iter()
            .filter(|message| message.seq > binding.relayed_message_seq)
            .take_while(|message| message.at <= now)
            .any(|message| !message.from_discord);
        let result = table
            .visible_history(now)
            .first()
            .is_some_and(|result| binding.announced_result_id.as_deref() != Some(&result.id));
        chat || result
    }
}

/// 코인이 오가는 이벤트(캐시아웃)가 있는지.
fn moves_coins(events: &[CasinoEvent]) -> bool {
    events
        .iter()
        .any(|event| matches!(event, CasinoEvent::CashOut { .. }))
}

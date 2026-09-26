// stock_hub.rs — 봇 안의 주식 시장 운영: 시장 상태 저장(stocks.json, 봉은 stocks-candles.json),
// 5초 틱, 코인 연동, 서버 활동 연동. Discord 명령과 웹 API가 이 허브를 같이 쓴다.
//
// 코인: 엔진은 코인 이동을 장부(journal)에 남기고, 허브가 그 장부를 통계 파일의 코인에 반영한다.
// 반영한 마지막 번호를 통계 파일(`stock_ledger`)에 함께 저장하므로 같은 이동은 한 번만 반영된다.
// 저장은 시장 파일을 먼저, 통계 파일을 나중에 쓴다: 그 사이에 멈추면 다음 시작 때 시장 파일의
// 장부를 다시 반영하고, 둘 다 저장된 뒤에만 장부를 비운다.

use crate::stats;
use anyhow::{Context as AnyhowContext, Result};
use mafia_remake::config::BotConfig;
use mafia_remake::stats::{EconomyRules, StatsFile};
use mafia_remake::stocks::*;
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

pub type SharedStocks = Arc<StockHub>;

/// 가격 상태만 바뀐 틱은 이 간격으로 모아서 저장한다.
const MARKET_SAVE_EVERY: Duration = Duration::from_secs(30);
/// 봉 파일 저장 간격.
const CANDLE_SAVE_EVERY: Duration = Duration::from_secs(60);

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// 서버 활동 누적 (게임 연동 종목의 실적 재료). 마피아 판·카지노 라운드가 끝날 때 더한다.
#[derive(Debug, Default)]
pub struct ServerActivity {
    pub mafia_games: AtomicI64,
    pub casino_hands: AtomicI64,
    pub casino_house: AtomicI64,
}

/// Discord 연결 (시세판 메시지, 뉴스 채널).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct StockBindings {
    #[serde(default)]
    pub panel_channel: u64,
    #[serde(default)]
    pub panel_message: u64,
    #[serde(default)]
    pub news_channel: u64,
    /// 마지막으로 올린 시세판 본문 (같으면 고치지 않는다).
    #[serde(default)]
    pub panel_text: String,
}

#[derive(Serialize, Deserialize)]
struct StockFile {
    market: StockMarket,
    #[serde(default)]
    bindings: StockBindings,
}

/// 코인이 필요한 정도.
pub enum Need {
    /// 코인이 필요 없다 (매도 등).
    Nothing,
    /// 이 금액이 모두 있어야 한다 (설립, 청약, 신주 행사).
    Exactly(i64),
    /// 이 금액까지 쓸 수 있다 (시장가 매수는 가진 만큼 체결).
    UpTo(i64),
}

pub struct StockHub {
    pub market: RwLock<StockMarket>,
    stats: Arc<RwLock<StatsFile>>,
    stats_path: Arc<PathBuf>,
    path: PathBuf,
    candles_path: PathBuf,
    config: OnceLock<Arc<RwLock<BotConfig>>>,
    bet_locks: OnceLock<crate::BetLocks>,
    activity: OnceLock<Arc<ServerActivity>>,
    activity_seen: Mutex<(i64, i64, i64)>,
    pub bindings: Mutex<StockBindings>,
    save_lock: tokio::sync::Mutex<()>,
    rng: Mutex<StdRng>,
    dirty: AtomicBool,
    last_market_save: Mutex<Instant>,
    last_candle_save: Mutex<Instant>,
}

impl StockHub {
    /// stocks.json을 불러온다. 없으면 새 시장을 연다. 있는데 읽지 못하면 오류 (빈 시장으로 시작하면
    /// 다음 저장이 사람들의 주식을 지운다).
    pub fn load(
        path: PathBuf,
        stats: Arc<RwLock<StatsFile>>,
        stats_path: Arc<PathBuf>,
    ) -> Result<Self> {
        // stocks.json → stocks-candles.json (개발 서버의 stocks-dev.json → stocks-dev-candles.json).
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("stocks");
        let candles_path = path.with_file_name(format!("{stem}-candles.json"));
        let (mut market, bindings) = if mafia_remake::atomic_file::is_missing(&path) {
            (
                StockMarket::new(now_ms(), &StockRules::default()),
                StockBindings::default(),
            )
        } else {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("주식 파일을 읽지 못했습니다: {}", path.display()))?;
            let file: StockFile = serde_json::from_str(&text)
                .with_context(|| format!("주식 파일을 해석하지 못했습니다: {}", path.display()))?;
            (file.market, file.bindings)
        };
        // 봉은 없어도 시장을 돌릴 수 있다 (읽지 못하면 빈 차트로 시작한다).
        if let Ok(text) = std::fs::read_to_string(&candles_path) {
            match serde_json::from_str::<CandleStore>(&text) {
                Ok(candles) => market.candles = candles,
                Err(error) => eprintln!("failed to read stock candles, starting empty: {error:?}"),
            }
        }
        Ok(Self {
            market: RwLock::new(market),
            stats,
            stats_path,
            path,
            candles_path,
            config: OnceLock::new(),
            bet_locks: OnceLock::new(),
            activity: OnceLock::new(),
            activity_seen: Mutex::new((0, 0, 0)),
            bindings: Mutex::new(bindings),
            save_lock: tokio::sync::Mutex::new(()),
            rng: Mutex::new(StdRng::from_os_rng()),
            dirty: AtomicBool::new(false),
            last_market_save: Mutex::new(Instant::now()),
            last_candle_save: Mutex::new(Instant::now()),
        })
    }

    /// 운영 설정·배팅 잠금·서버 활동을 연결한다 (봇 시작 때 한 번).
    pub fn connect(
        &self,
        config: Arc<RwLock<BotConfig>>,
        bet_locks: crate::BetLocks,
        activity: Arc<ServerActivity>,
    ) {
        let _ = self.config.set(config);
        let _ = self.bet_locks.set(bet_locks);
        let seen = (
            activity.mafia_games.load(Ordering::Relaxed),
            activity.casino_hands.load(Ordering::Relaxed),
            activity.casino_house.load(Ordering::Relaxed),
        );
        *self
            .activity_seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = seen;
        let _ = self.activity.set(activity);
    }

    pub async fn rules(&self) -> (StockRules, EconomyRules) {
        match self.config.get() {
            Some(config) => {
                let config = config.read().await;
                (config.stock_rules(), config.economy_rules())
            }
            None => (StockRules::default(), EconomyRules::default()),
        }
    }

    fn locked_bet(&self, user: u64) -> i64 {
        self.bet_locks
            .get()
            .map_or(0, |locks| crate::locked_bet(locks, user))
    }

    /// 시작 때: 저장된 장부 중 코인에 아직 반영하지 않은 것을 반영한다 (저장 도중 멈췄을 때).
    pub async fn recover(&self) {
        let (_, econ) = self.rules().await;
        let snapshot = {
            let market = self.market.read().await;
            let mut stats_file = self.stats.write().await;
            let applied = apply_journal(&market, &mut stats_file, &econ);
            if applied == 0 {
                None
            } else {
                eprintln!("stock ledger: re-applied {applied} coin transfers after restart");
                Some((market.clone(), stats_file.clone()))
            }
        };
        if let Some((market, stats_file)) = snapshot {
            self.persist(market, stats_file).await;
        }
    }

    // ------------------------------------------------------------ 저장

    /// 시장 파일을 쓰고, 그다음 통계 파일을 쓰고, 둘 다 쓴 뒤에 반영한 장부를 비운다.
    async fn persist(&self, market: StockMarket, stats_file: StatsFile) {
        let _guard = self.save_lock.lock().await;
        let watermark = stats_file.stock_ledger;
        if !self.write_market(market).await {
            return;
        }
        let stats_path = self.stats_path.clone();
        match tokio::task::spawn_blocking(move || stats::save_stats(&*stats_path, &stats_file))
            .await
        {
            Ok(Ok(())) => {
                self.market.write().await.prune_journal(watermark);
            }
            Ok(Err(error)) => eprintln!("failed to save stats after stock change: {error:?}"),
            Err(error) => eprintln!("failed to join stats save after stock change: {error:?}"),
        }
    }

    async fn write_market(&self, market: StockMarket) -> bool {
        let bindings = self
            .bindings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let path = self.path.clone();
        let seq = mafia_remake::atomic_file::next_seq();
        let result = tokio::task::spawn_blocking(move || -> Result<()> {
            let text = serde_json::to_string(&StockFile { market, bindings })
                .context("주식 파일 JSON을 만들지 못했습니다")?;
            mafia_remake::atomic_file::replace(&path, text.as_bytes(), Some(seq))?;
            Ok(())
        })
        .await;
        match result {
            Ok(Ok(())) => {
                self.dirty.store(false, Ordering::Relaxed);
                *self
                    .last_market_save
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Instant::now();
                true
            }
            Ok(Err(error)) => {
                eprintln!("failed to save stock market: {error:?}");
                false
            }
            Err(error) => {
                eprintln!("failed to join stock market save: {error:?}");
                false
            }
        }
    }

    async fn write_candles(&self) {
        let candles = self.market.read().await.candles.clone();
        let path = self.candles_path.clone();
        let result = tokio::task::spawn_blocking(move || -> Result<()> {
            let text = serde_json::to_string(&candles).context("봉 JSON을 만들지 못했습니다")?;
            mafia_remake::atomic_file::replace(&path, text.as_bytes(), None)?;
            Ok(())
        })
        .await;
        if let Ok(Err(error)) = result {
            eprintln!("failed to save stock candles: {error:?}");
        }
        *self
            .last_candle_save
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Instant::now();
    }

    /// Discord 연결 정보만 바뀌었을 때 저장한다.
    pub async fn save_bindings(&self) {
        let market = self.market.read().await.clone();
        let _guard = self.save_lock.lock().await;
        self.write_market(market).await;
    }

    /// 종료 직전 등: 가격 상태와 봉을 지금 쓴다.
    pub async fn flush(&self) {
        let market = self.market.read().await.clone();
        {
            let _guard = self.save_lock.lock().await;
            self.write_market(market).await;
        }
        self.write_candles().await;
    }

    // ------------------------------------------------------------ 거래

    /// 코인이 오가는 작업: 필요한 코인을 확인하고(마피아 판에 걸린 배팅액은 뺀다), 엔진 작업을 한 뒤,
    /// 장부를 코인에 반영하고 저장한다.
    pub async fn transact<T>(
        &self,
        user: u64,
        need: impl FnOnce(&StockMarket, &StockRules) -> std::result::Result<Need, String>,
        op: impl FnOnce(&mut StockMarket, i64, &StockRules, i64) -> std::result::Result<T, String>,
    ) -> std::result::Result<T, String> {
        let (rules, econ) = self.rules().await;
        if !rules.enabled {
            return Err("지금은 주식 시장이 닫혀 있습니다.".to_string());
        }
        let (result, market_snapshot, stats_snapshot) = {
            let mut market = self.market.write().await;
            let now = market.clock(now_ms());
            let need = need(&market, &rules)?;
            let mut stats_file = self.stats.write().await;
            let budget = match need {
                Need::Nothing => 0,
                Need::Exactly(amount) | Need::UpTo(amount) => {
                    let coins = stats_file
                        .users
                        .get(&user.to_string())
                        .map_or(0, |entry| entry.coins);
                    let locked = self.locked_bet(user);
                    let available = (coins - locked).max(0);
                    match need {
                        Need::Exactly(amount) if amount > available => {
                            let tail = if locked > 0 {
                                format!(
                                    " (진행 중인 게임에 배팅 {}이 걸려 있습니다)",
                                    stats::coin_text(locked)
                                )
                            } else {
                                String::new()
                            };
                            return Err(format!(
                                "코인이 부족합니다. 필요 {} / 쓸 수 있는 코인 {}{tail}",
                                stats::coin_text(amount),
                                stats::coin_text(available)
                            ));
                        }
                        _ => amount.min(available),
                    }
                }
            };
            let result = op(&mut market, budget, &rules, now)?;
            apply_journal(&market, &mut stats_file, &econ);
            (result, market.clone(), stats_file.clone())
        };
        self.persist(market_snapshot, stats_snapshot).await;
        Ok(result)
    }

    /// 매수·매도 주문.
    pub async fn order(
        &self,
        user: u64,
        name: &str,
        code: &str,
        side: Side,
        qty: i64,
        limit: Option<i64>,
    ) -> std::result::Result<OrderResult, String> {
        let request = OrderRequest {
            user,
            name: name.to_string(),
            code: code.to_string(),
            side,
            qty,
            limit,
        };
        self.transact(
            user,
            |market, rules| match side {
                Side::Buy => market.max_buy_cost(code, qty, limit, rules).map(Need::UpTo),
                Side::Sell => Ok(Need::Nothing),
            },
            move |market, budget, rules, now| market.place_order(&request, budget, now, rules),
        )
        .await
    }

    pub async fn cancel(&self, user: u64, order_id: u64) -> std::result::Result<(), String> {
        self.transact(
            user,
            |_, _| Ok(Need::Nothing),
            |market, _, _, _| market.cancel_order(user, order_id),
        )
        .await
    }

    // ------------------------------------------------------------ 틱

    /// 5초마다: 서버 활동을 넘기고, 시장을 돌리고, 생긴 코인 이동을 반영해 저장한다.
    pub async fn tick(&self) -> TickReport {
        let (rules, econ) = self.rules().await;
        let activity = self.take_activity();
        let (report, snapshot) = {
            let mut market = self.market.write().await;
            let now = market.clock(now_ms());
            if activity != (0, 0, 0) {
                market.record_activity(activity.0, activity.1, activity.2);
            }
            let report = {
                let mut rng = self
                    .rng
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                market.tick(now, &rules, &mut *rng)
            };
            let snapshot = if market.journal.is_empty() {
                None
            } else {
                let mut stats_file = self.stats.write().await;
                apply_journal(&market, &mut stats_file, &econ);
                Some((market.clone(), stats_file.clone()))
            };
            (report, snapshot)
        };
        if report.changed {
            self.dirty.store(true, Ordering::Relaxed);
        }
        match snapshot {
            Some((market, stats_file)) => self.persist(market, stats_file).await,
            None => {
                let due = self
                    .last_market_save
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .elapsed()
                    >= MARKET_SAVE_EVERY;
                if due && self.dirty.load(Ordering::Relaxed) {
                    let market = self.market.read().await.clone();
                    let _guard = self.save_lock.lock().await;
                    self.write_market(market).await;
                }
            }
        }
        let candles_due = self
            .last_candle_save
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .elapsed()
            >= CANDLE_SAVE_EVERY;
        if candles_due {
            self.write_candles().await;
        }
        report
    }

    /// 지난 틱 뒤로 늘어난 서버 활동.
    fn take_activity(&self) -> (i64, i64, i64) {
        let Some(activity) = self.activity.get() else {
            return (0, 0, 0);
        };
        let now = (
            activity.mafia_games.load(Ordering::Relaxed),
            activity.casino_hands.load(Ordering::Relaxed),
            activity.casino_house.load(Ordering::Relaxed),
        );
        let mut seen = self
            .activity_seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let delta = (now.0 - seen.0, now.1 - seen.1, now.2 - seen.2);
        *seen = now;
        delta
    }

    /// 로그 채널로 보낼 운영 기록(체결·주문·청약·회사 작업·배당 등)을 꺼낸다.
    pub async fn take_logs(&self) -> Vec<String> {
        self.market.write().await.take_logs()
    }

    /// 뉴스 채널에 올릴 새 뉴스·공시를 꺼낸다 (틱에서 생긴 것과 명령으로 생긴 공시 모두).
    pub async fn take_news(&self) -> Vec<NewsItem> {
        self.market.write().await.take_news_outbox()
    }

    // ------------------------------------------------------------ 조회

    /// 주식에 쓸 수 있는 코인 (가진 코인에서 진행 중인 마피아 판에 걸린 배팅을 뺀다).
    pub async fn coins_of(&self, user: u64) -> i64 {
        let coins = self
            .stats
            .read()
            .await
            .users
            .get(&user.to_string())
            .map_or(0, |entry| entry.coins);
        (coins - self.locked_bet(user)).max(0)
    }

    /// 주식 시장에 있는 재산 (평가액 + 묶인 코인). 구조금 기준에 넣는다.
    pub async fn portfolio_value(&self, user: u64) -> i64 {
        let market = self.market.read().await;
        market.portfolio_value(user, market.clock(now_ms()))
    }

    /// 시장 시각 (관리자가 게임일을 넘긴 만큼 실제 시각보다 앞선다).
    pub async fn now(&self) -> i64 {
        self.market.read().await.clock(now_ms())
    }

    /// 관리자: 게임일을 넘긴다. 시장 시계를 옮기고 그 사이를 바로 따라잡은 뒤 저장한다.
    /// 돌려주는 값은 (지금 게임일 번호, 앞당긴 시간 ms).
    pub async fn skip_game_days(&self, days: i64) -> std::result::Result<(i64, i64), String> {
        let (rules, _) = self.rules().await;
        if !rules.enabled {
            return Err("지금은 주식 시장이 닫혀 있습니다.".to_string());
        }
        let shift_before = {
            let mut market = self.market.write().await;
            let before = market.time_shift_ms;
            let now = market.clock(now_ms());
            market.skip_game_days(now, days, &rules);
            before
        };
        self.tick().await;
        self.flush().await;
        let market = self.market.read().await;
        let day = rules.day_of(market.clock(now_ms()));
        Ok((day, market.time_shift_ms - shift_before))
    }

    pub async fn find_code(&self, query: &str) -> Option<String> {
        self.market
            .read()
            .await
            .find_company(query)
            .map(|company| company.code.clone())
    }
}

/// 장부 중 아직 반영하지 않은 코인 이동을 통계 파일에 반영한다. 반영한 개수를 돌려준다.
pub fn apply_journal(
    market: &StockMarket,
    stats_file: &mut StatsFile,
    econ: &EconomyRules,
) -> usize {
    let mut applied = 0;
    let already = stats_file.stock_ledger;
    for transfer in market
        .journal
        .iter()
        .filter(|transfer| transfer.id > already)
    {
        if transfer.user == TREASURY_USER {
            stats::treasury_deposit(stats_file, transfer.amount, econ);
        } else {
            let entry = stats_file
                .users
                .entry(transfer.user.to_string())
                .or_default();
            if !transfer.name.is_empty() {
                entry.name = transfer.name.clone();
            }
            let next = entry.coins.saturating_add(transfer.amount);
            if next < 0 {
                // 허브가 미리 잔액을 확인하므로 오지 않아야 한다. 오면 0에서 멈추고 기록을 남긴다.
                eprintln!(
                    "stock ledger: transfer {} would make user {} negative ({} + {})",
                    transfer.id, transfer.user, entry.coins, transfer.amount
                );
            }
            entry.coins = next.max(0);
        }
        stats_file.stock_ledger = stats_file.stock_ledger.max(transfer.id);
        applied += 1;
    }
    applied
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mafia-stock-hub-{name}-{}-{}",
            std::process::id(),
            mafia_remake::atomic_file::next_seq()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn hub_with_coins(dir: &Path, coins: i64) -> StockHub {
        let mut stats_file = StatsFile::default();
        stats::refund_coins(&mut stats_file, 1, "Alpha", coins);
        StockHub::load(
            dir.join("stocks.json"),
            Arc::new(RwLock::new(stats_file)),
            Arc::new(dir.join("stats.json")),
        )
        .unwrap()
    }

    async fn coins(hub: &StockHub, user: u64) -> i64 {
        hub.stats
            .read()
            .await
            .users
            .get(&user.to_string())
            .map_or(0, |entry| entry.coins)
    }

    #[tokio::test]
    async fn skipping_a_game_day_moves_the_market_clock_and_saves_it() {
        let dir = temp_dir("skip");
        let hub = hub_with_coins(&dir, 1_000_000);
        let (rules, _) = hub.rules().await;
        let before = hub.now().await;
        let (day, skipped) = hub.skip_game_days(1).await.unwrap();
        let after = hub.now().await;
        assert_eq!(day, rules.day_of(before) + 1);
        assert!(skipped > 0 && skipped <= rules.day_ms());
        assert!(after >= (rules.day_of(before) + 1) * rules.day_ms());
        // 따라잡은 시각까지 시장을 돌렸고, 알림 뉴스가 채널로 갈 준비가 됐다.
        assert!(hub.market.read().await.last_tick > before);
        assert!(
            hub.take_news()
                .await
                .iter()
                .any(|item| item.headline.contains("게임일을 1일 넘겼습니다"))
        );
        // 시장 파일에 앞당긴 시계가 남아 다시 켜도 이어진다.
        let reloaded = StockHub::load(
            dir.join("stocks.json"),
            Arc::new(RwLock::new(StatsFile::default())),
            Arc::new(dir.join("stats.json")),
        )
        .unwrap();
        assert_eq!(
            reloaded.market.read().await.time_shift_ms,
            hub.market.read().await.time_shift_ms
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_buy_moves_coins_once_and_both_files_agree() {
        let dir = temp_dir("buy");
        let hub = hub_with_coins(&dir, 1_000_000);
        let result = hub
            .order(1, "Alpha", "100070", Side::Buy, 10, None)
            .await
            .unwrap();
        assert_eq!(result.filled, 10);
        let spent = result.notional + result.fee;
        assert_eq!(coins(&hub, 1).await, 1_000_000 - spent);
        // 수수료는 금고로 (그중 일부는 잭팟).
        let treasury = hub.stats.read().await.treasury.clone();
        assert_eq!(treasury.balance + treasury.jackpot, result.fee);
        // 저장한 뒤 장부는 비고, 두 파일이 같은 번호를 가리킨다.
        assert!(hub.market.read().await.journal.is_empty());
        let saved: StatsFile =
            serde_json::from_str(&std::fs::read_to_string(dir.join("stats.json")).unwrap())
                .unwrap();
        assert_eq!(saved.users["1"].coins, 1_000_000 - spent);
        assert!(saved.stock_ledger > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn exact_costs_need_the_coins_and_leave_nothing_behind_on_refusal() {
        let dir = temp_dir("exact");
        let hub = hub_with_coins(&dir, 500_000);
        let result = hub
            .transact(
                1,
                |_, rules| Ok(Need::Exactly(StockMarket::founding_cost(1_000_000, rules))),
                |market, _, rules, now| {
                    market.found_company(
                        1,
                        "Alpha",
                        "부족회사",
                        Sector::Game,
                        1_000_000,
                        now,
                        rules,
                    )
                },
            )
            .await;
        assert!(result.unwrap_err().contains("코인이 부족"));
        assert_eq!(coins(&hub, 1).await, 500_000);
        assert!(
            hub.market
                .read()
                .await
                .companies
                .values()
                .all(|company| !company.is_player())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_mafia_bet_that_is_still_riding_cannot_be_spent_on_stocks() {
        let dir = temp_dir("locked");
        let hub = hub_with_coins(&dir, 2_000_000);
        let locks: crate::BetLocks =
            Arc::new(std::sync::Mutex::new(std::collections::HashMap::from([(
                "game".to_string(),
                std::collections::HashMap::from([(1_u64, 1_500_000_i64)]),
            )])));
        let config = Arc::new(RwLock::new(
            serde_json::from_str::<BotConfig>(include_str!("../config.example.json")).unwrap(),
        ));
        hub.connect(config, locks, Arc::new(ServerActivity::default()));
        // 쓸 수 있는 코인은 50만: 설립(100만+수수료)은 안 되고, 시장가 매수는 50만어치까지만.
        let refused = hub
            .transact(
                1,
                |_, rules| Ok(Need::Exactly(StockMarket::founding_cost(1_000_000, rules))),
                |market, _, rules, now| {
                    market.found_company(
                        1,
                        "Alpha",
                        "잠금회사",
                        Sector::Game,
                        1_000_000,
                        now,
                        rules,
                    )
                },
            )
            .await;
        assert!(refused.unwrap_err().contains("배팅"));
        let result = hub
            .order(1, "Alpha", "100070", Side::Buy, 1_000, None)
            .await
            .unwrap();
        assert!(result.notional + result.fee <= 500_000);
        assert!(coins(&hub, 1).await >= 1_500_000);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_ledger_left_in_the_market_file_is_applied_once_after_a_restart() {
        let dir = temp_dir("recover");
        {
            let hub = hub_with_coins(&dir, 0);
            let mut market = hub.market.write().await;
            market.transfer(1, "Alpha", 7_000, "테스트 지급");
            market.to_treasury(1_000, "테스트 수수료");
            let snapshot = market.clone();
            drop(market);
            // 시장 파일만 저장되고 통계 파일은 저장되기 전에 멈춘 상황.
            assert!(hub.write_market(snapshot).await);
        }
        let stats_file = StatsFile::default();
        let hub = StockHub::load(
            dir.join("stocks.json"),
            Arc::new(RwLock::new(stats_file)),
            Arc::new(dir.join("stats.json")),
        )
        .unwrap();
        hub.recover().await;
        assert_eq!(coins(&hub, 1).await, 7_000);
        hub.recover().await;
        assert_eq!(coins(&hub, 1).await, 7_000, "두 번 반영하지 않는다");
        assert!(hub.market.read().await.journal.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn server_activity_reaches_the_market() {
        let dir = temp_dir("activity");
        let hub = hub_with_coins(&dir, 0);
        let activity = Arc::new(ServerActivity::default());
        let config = Arc::new(RwLock::new(
            serde_json::from_str::<BotConfig>(include_str!("../config.example.json")).unwrap(),
        ));
        hub.connect(
            config,
            Arc::new(std::sync::Mutex::new(Default::default())),
            activity.clone(),
        );
        activity.mafia_games.fetch_add(3, Ordering::Relaxed);
        activity.casino_hands.fetch_add(10, Ordering::Relaxed);
        hub.tick().await;
        let recorded = hub.market.read().await.activity;
        assert_eq!((recorded.mafia_games, recorded.casino_hands), (3, 10));
        hub.tick().await;
        assert_eq!(
            hub.market.read().await.activity.mafia_games,
            3,
            "한 번만 넘긴다"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

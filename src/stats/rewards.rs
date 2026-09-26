// stats/rewards.rs — 코인 벌이: 마피아 참여 보상, 일일 미션, 업적, 연속 출석 보너스.
// 보상 코인은 새로 발행한다. 부계정으로 찍어 내지 못하게 참여 보상은 하루 판 수로, 미션은 하루
// 세 개로, 업적은 단계마다 한 번씩으로 총량을 묶는다. 금액은 모두 운영 설정이고(0이면 끔), 발행한
// 누적은 `RewardTotals`에 남긴다.

use super::{PlayerStats, StatsFile, ensure_player_stats, player_won_game, rating_team_key};
use crate::game::MafiaGame;
use crate::model::Winner;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DEFAULT_REWARD_GAME_COINS: i64 = 1_000;
pub const DEFAULT_REWARD_WIN_COINS: i64 = 1_000;
pub const DEFAULT_REWARD_DAILY_GAMES: i64 = 5;
pub const DEFAULT_MISSION_COINS: i64 = 2_000;
pub const DEFAULT_MISSION_BONUS_COINS: i64 = 3_000;
pub const DEFAULT_ACHIEVEMENT_REWARD_PCT: i64 = 100;
pub const DEFAULT_STREAK_WEEK_COINS: i64 = 10_000;
pub const DEFAULT_STREAK_MONTH_COINS: i64 = 30_000;

/// 한 시간 (KST 날짜 계산용).
const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 24 * HOUR_MS;
const KST_OFFSET_MS: i64 = 9 * HOUR_MS;
/// 미션 보너스를 받았다는 표시.
const BONUS_ID: &str = "bonus";
/// 업적 표 버전. 2부터 보상을 올리고 단계를 늘렸다 (1에서 받은 업적은 차액을 한 번 더 준다).
const ACHIEVEMENT_VERSION: u32 = 2;

/// 보상 규칙 (운영 설정으로 만든다).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RewardRules {
    /// 마피아 판을 끝까지 하면 받는 코인.
    pub game_coins: i64,
    /// 이긴 쪽이 더 받는 코인.
    pub win_coins: i64,
    /// 참여 보상을 받는 하루 판 수.
    pub daily_games: i64,
    /// 일일 미션 하나의 보상.
    pub mission_coins: i64,
    /// 일일 미션 세 개를 다 끝낸 보너스.
    pub mission_bonus: i64,
    /// 업적 보상 배율 (%, 0이면 업적 보상 끔).
    pub achievement_pct: i64,
    /// 연속 출석 7일마다 보너스.
    pub streak_week: i64,
    /// 연속 출석 30일마다 보너스.
    pub streak_month: i64,
}

impl Default for RewardRules {
    fn default() -> Self {
        Self {
            game_coins: DEFAULT_REWARD_GAME_COINS,
            win_coins: DEFAULT_REWARD_WIN_COINS,
            daily_games: DEFAULT_REWARD_DAILY_GAMES,
            mission_coins: DEFAULT_MISSION_COINS,
            mission_bonus: DEFAULT_MISSION_BONUS_COINS,
            achievement_pct: DEFAULT_ACHIEVEMENT_REWARD_PCT,
            streak_week: DEFAULT_STREAK_WEEK_COINS,
            streak_month: DEFAULT_STREAK_MONTH_COINS,
        }
    }
}

/// 사람마다의 보상 기록.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewardRecord {
    /// 일일 기록의 날짜 (한국 시간 YYYY-MM-DD). 날짜가 바뀌면 아래 일일 값을 새로 센다.
    #[serde(default)]
    pub day: String,
    #[serde(default)]
    pub day_games: i64,
    #[serde(default)]
    pub day_wins: i64,
    #[serde(default)]
    pub day_hands: i64,
    /// 오늘 참여 보상을 받은 판 수.
    #[serde(default)]
    pub day_rewarded_games: i64,
    /// 오늘 받은 미션 (미션 id, 보너스는 "bonus").
    #[serde(default)]
    pub claimed: Vec<String>,
    /// 누적 카지노 판 수·잭팟 당첨 수 (업적용).
    #[serde(default)]
    pub casino_hands: i64,
    #[serde(default)]
    pub jackpots: i64,
    /// 팀별 승리 수 ("citizen", "mafia", "cult", "joker"). 업적 개편 뒤부터 센다.
    #[serde(default)]
    pub team_wins: BTreeMap<String, i64>,
    /// 받은 업적 id ("games-100" 처럼 트랙과 목표).
    #[serde(default)]
    pub achievements: Vec<String>,
    /// 업적 표 버전 (차액 지급을 한 번만 하려고).
    #[serde(default)]
    pub achievement_version: u32,
    /// 연속 출석 일수와 마지막으로 센 날, 최장 연속 출석, 누적 출석 일수.
    #[serde(default)]
    pub attendance_streak: i64,
    #[serde(default)]
    pub streak_day: String,
    #[serde(default)]
    pub best_attendance_streak: i64,
    #[serde(default)]
    pub attendance_days: i64,
    /// 일일 미션 세 개를 다 끝낸 날 수.
    #[serde(default)]
    pub mission_days: i64,
    /// 보상으로 받은 코인 누적.
    #[serde(default)]
    pub earned: i64,
}

impl RewardRecord {
    fn roll(&mut self, today: &str) {
        if self.day != today {
            self.day = today.to_string();
            self.day_games = 0;
            self.day_wins = 0;
            self.day_hands = 0;
            self.day_rewarded_games = 0;
            self.claimed.clear();
        }
    }
}

/// 보상으로 새로 발행한 코인 누적 (운영자 확인용).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewardTotals {
    #[serde(default)]
    pub participation: i64,
    #[serde(default)]
    pub missions: i64,
    #[serde(default)]
    pub achievements: i64,
    #[serde(default)]
    pub streak: i64,
}

impl RewardTotals {
    pub fn total(&self) -> i64 {
        self.participation
            .saturating_add(self.missions)
            .saturating_add(self.achievements)
            .saturating_add(self.streak)
    }
}

fn pay(entry: &mut PlayerStats, amount: i64) {
    entry.coins = entry.coins.saturating_add(amount);
    entry.rewards.earned = entry.rewards.earned.saturating_add(amount);
}

/// "12,345" (단위 없이).
fn group(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    if value < 0 {
        format!("-{grouped}")
    } else {
        grouped
    }
}

/// 한국 시간 자정 (unix ms). 오늘 주식 체결을 셀 때 쓴다.
pub fn kst_day_start_ms(now_ms: i64) -> i64 {
    (now_ms + KST_OFFSET_MS).div_euclid(DAY_MS) * DAY_MS - KST_OFFSET_MS
}

/// 한국 시간 어제 날짜 (YYYY-MM-DD). 연속 출석을 셀 때 쓴다.
pub fn kst_yesterday() -> String {
    let kst = chrono::FixedOffset::east_opt(9 * 3600).expect("KST offset");
    (chrono::Utc::now() - chrono::Duration::days(1))
        .with_timezone(&kst)
        .format("%Y-%m-%d")
        .to_string()
}

// ------------------------------------------------------------ 마피아 참여 보상

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameReward {
    pub user_id: u64,
    pub name: String,
    pub won: bool,
    /// 받은 코인 (오늘 한도를 넘었으면 0).
    pub amount: i64,
    pub balance: i64,
}

/// 판이 끝났을 때: 참가자마다 오늘의 판 수·승수와 팀별 승리를 세고(미션·업적용), 하루 한도 안에서
/// 참여 보상을 준다.
pub fn grant_game_rewards(
    stats: &mut StatsFile,
    game: &MafiaGame,
    winner: Winner,
    rules: &RewardRules,
    today: &str,
) -> Vec<GameReward> {
    let mut rewards = Vec::new();
    let mut total = 0_i64;
    for player in &game.players {
        let won = player_won_game(game, player, winner);
        let team = rating_team_key(game, player);
        let entry = ensure_player_stats(stats, player.user_id, &player.name);
        entry.rewards.roll(today);
        entry.rewards.day_games += 1;
        if won {
            entry.rewards.day_wins += 1;
            *entry.rewards.team_wins.entry(team.to_string()).or_default() += 1;
        }
        let mut amount = 0;
        if rules.game_coins > 0 && entry.rewards.day_rewarded_games < rules.daily_games {
            amount = rules.game_coins + if won { rules.win_coins.max(0) } else { 0 };
            entry.rewards.day_rewarded_games += 1;
            pay(entry, amount);
            total = total.saturating_add(amount);
        }
        rewards.push(GameReward {
            user_id: player.user_id,
            name: player.name.clone(),
            won,
            amount,
            balance: entry.coins,
        });
    }
    stats.reward_totals.participation = stats.reward_totals.participation.saturating_add(total);
    rewards
}

/// 카지노 한 판 (일일 미션·업적용).
pub fn record_casino_hand(stats: &mut StatsFile, user_id: u64, name: &str, today: &str) {
    let entry = ensure_player_stats(stats, user_id, name);
    entry.rewards.roll(today);
    entry.rewards.day_hands += 1;
    entry.rewards.casino_hands += 1;
}

/// 카지노 잭팟 당첨 (업적용).
pub fn record_jackpot(stats: &mut StatsFile, user_id: u64, name: &str) {
    ensure_player_stats(stats, user_id, name).rewards.jackpots += 1;
}

// ------------------------------------------------------------ 일일 미션

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissionKind {
    MafiaGames,
    MafiaWins,
    CasinoHands,
    StockTrades,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mission {
    pub id: String,
    pub category: &'static str,
    pub title: String,
    pub kind: MissionKind,
    pub target: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissionStatus {
    pub mission: Mission,
    pub progress: i64,
    pub done: bool,
    pub claimed: bool,
}

/// 사람·날짜마다 정해지는 수 (FNV-1a: 봇을 다시 켜거나 버전이 바뀌어도 같은 미션).
fn pick(user_id: u64, day: &str, category: &str, count: usize) -> usize {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in user_id
        .to_le_bytes()
        .iter()
        .chain(day.as_bytes())
        .chain(category.as_bytes())
    {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    (hash % count.max(1) as u64) as usize
}

fn mission(category: &'static str, kind: MissionKind, target: i64) -> Mission {
    let (key, title) = match kind {
        MissionKind::MafiaGames => ("mafia-games", format!("마피아 {target}판 끝까지 하기")),
        MissionKind::MafiaWins => ("mafia-wins", format!("마피아에서 {target}번 이기기")),
        MissionKind::CasinoHands => ("casino-hands", format!("카지노 {target}판 하기")),
        MissionKind::StockTrades => ("stock-trades", format!("주식 {target}번 체결하기")),
    };
    Mission {
        id: format!("{key}-{target}"),
        category,
        title,
        kind,
        target,
    }
}

/// 오늘의 미션 세 개 (마피아·카지노·주식에서 하나씩, 날마다 바뀐다).
pub fn daily_missions(user_id: u64, day: &str) -> Vec<Mission> {
    let mafia = [
        (MissionKind::MafiaGames, 1),
        (MissionKind::MafiaGames, 2),
        (MissionKind::MafiaWins, 1),
    ];
    let casino = [
        (MissionKind::CasinoHands, 10),
        (MissionKind::CasinoHands, 20),
    ];
    let stocks = [(MissionKind::StockTrades, 1), (MissionKind::StockTrades, 3)];
    let (kind, target) = mafia[pick(user_id, day, "mafia", mafia.len())];
    let first = mission("마피아", kind, target);
    let (kind, target) = casino[pick(user_id, day, "casino", casino.len())];
    let second = mission("카지노", kind, target);
    let (kind, target) = stocks[pick(user_id, day, "stocks", stocks.len())];
    let third = mission("주식", kind, target);
    vec![first, second, third]
}

/// 오늘의 미션 진행. 주식 체결 수는 호출자가 시장에서 센다.
pub fn mission_status(
    entry: Option<&PlayerStats>,
    user_id: u64,
    today: &str,
    stock_trades_today: i64,
) -> Vec<MissionStatus> {
    let record = entry
        .map(|entry| &entry.rewards)
        .filter(|record| record.day == today);
    daily_missions(user_id, today)
        .into_iter()
        .map(|mission| {
            let progress = match mission.kind {
                MissionKind::MafiaGames => record.map_or(0, |record| record.day_games),
                MissionKind::MafiaWins => record.map_or(0, |record| record.day_wins),
                MissionKind::CasinoHands => record.map_or(0, |record| record.day_hands),
                MissionKind::StockTrades => stock_trades_today,
            };
            let claimed = record.is_some_and(|record| record.claimed.contains(&mission.id));
            MissionStatus {
                done: progress >= mission.target,
                progress,
                claimed,
                mission,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Claim {
    /// (이름, 코인)
    pub paid: Vec<(String, i64)>,
    pub total: i64,
    pub balance: i64,
}

/// 끝낸 미션의 보상을 받는다 (받은 것은 건너뛴다). 세 개를 다 끝내면 보너스도 준다.
/// 돌려주는 진행 상황은 받은 뒤의 것이다.
pub fn claim_missions(
    stats: &mut StatsFile,
    user_id: u64,
    name: &str,
    today: &str,
    stock_trades_today: i64,
    rules: &RewardRules,
) -> (Vec<MissionStatus>, Claim) {
    let mut claim = Claim::default();
    if rules.mission_coins <= 0 && rules.mission_bonus <= 0 {
        let entry = ensure_player_stats(stats, user_id, name);
        claim.balance = entry.coins;
        return (
            mission_status(Some(entry), user_id, today, stock_trades_today),
            claim,
        );
    }
    let entry = ensure_player_stats(stats, user_id, name);
    entry.rewards.roll(today);
    let statuses = mission_status(Some(entry), user_id, today, stock_trades_today);
    for status in &statuses {
        if status.done && !status.claimed && rules.mission_coins > 0 {
            entry.rewards.claimed.push(status.mission.id.clone());
            pay(entry, rules.mission_coins);
            claim
                .paid
                .push((status.mission.title.clone(), rules.mission_coins));
            claim.total += rules.mission_coins;
        }
    }
    let all_done = statuses.iter().all(|status| status.done);
    if all_done && !entry.rewards.claimed.iter().any(|id| id == BONUS_ID) {
        entry.rewards.claimed.push(BONUS_ID.to_string());
        entry.rewards.mission_days += 1;
        if rules.mission_bonus > 0 {
            pay(entry, rules.mission_bonus);
            claim
                .paid
                .push(("세 미션 모두 달성 보너스".to_string(), rules.mission_bonus));
            claim.total += rules.mission_bonus;
        }
    }
    claim.balance = entry.coins;
    let statuses = mission_status(Some(entry), user_id, today, stock_trades_today);
    stats.reward_totals.missions = stats.reward_totals.missions.saturating_add(claim.total);
    (statuses, claim)
}

/// 오늘 미션 보너스를 받았는지.
pub fn mission_bonus_claimed(entry: Option<&PlayerStats>, today: &str) -> bool {
    entry.is_some_and(|entry| {
        entry.rewards.day == today && entry.rewards.claimed.iter().any(|id| id == BONUS_ID)
    })
}

// ------------------------------------------------------------ 업적

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    Games,
    Wins,
    BestWinStreak,
    StarPlayer,
    RolesPlayed,
    /// 팀별 승리 ("citizen", "mafia", "cult", "joker").
    TeamWins(&'static str),
    PlayHours,
    RatingPeak,
    CasinoHands,
    Jackpots,
    StockTrades,
    StockProfit,
    CompaniesFounded,
    CompaniesListed,
    AttendanceDays,
    AttendanceStreak,
    MissionDays,
}

/// 업적 트랙: 같은 기록의 목표를 단계별로 이어서 준다 (id는 "트랙-목표").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AchievementTrack {
    pub key: &'static str,
    pub title: &'static str,
    /// 목표를 적는 틀 (`{}` 자리에 목표 수).
    pub goal: &'static str,
    pub metric: Metric,
    /// (목표, 기본 보상). 운영 설정의 배율을 곱한다.
    pub tiers: &'static [(i64, i64)],
}

const fn track(
    key: &'static str,
    title: &'static str,
    goal: &'static str,
    metric: Metric,
    tiers: &'static [(i64, i64)],
) -> AchievementTrack {
    AchievementTrack {
        key,
        title,
        goal,
        metric,
        tiers,
    }
}

pub const ACHIEVEMENT_TRACKS: &[AchievementTrack] = &[
    track(
        "games",
        "마피아 판 수",
        "마피아 {}판",
        Metric::Games,
        &[
            (1, 5_000),
            (10, 15_000),
            (30, 30_000),
            (50, 50_000),
            (100, 100_000),
            (200, 180_000),
            (300, 250_000),
            (500, 400_000),
            (1_000, 800_000),
        ],
    ),
    track(
        "wins",
        "승리",
        "{}승",
        Metric::Wins,
        &[
            (1, 10_000),
            (10, 30_000),
            (30, 60_000),
            (50, 100_000),
            (100, 200_000),
            (200, 350_000),
            (300, 500_000),
            (500, 800_000),
        ],
    ),
    track(
        "streak",
        "최고 연승",
        "{}연승",
        Metric::BestWinStreak,
        &[(3, 20_000), (5, 50_000), (7, 100_000), (10, 250_000)],
    ),
    track(
        "star",
        "스타플레이어",
        "스타플레이어 {}번",
        Metric::StarPlayer,
        &[
            (1, 15_000),
            (5, 50_000),
            (10, 100_000),
            (30, 250_000),
            (50, 400_000),
        ],
    ),
    track(
        "roles",
        "해 본 직업",
        "직업 {}가지",
        Metric::RolesPlayed,
        &[
            (5, 20_000),
            (10, 50_000),
            (15, 100_000),
            (20, 200_000),
            (25, 300_000),
        ],
    ),
    track(
        "citizen-wins",
        "시민팀 승리",
        "시민팀으로 {}승",
        Metric::TeamWins("citizen"),
        &[(10, 30_000), (50, 120_000), (100, 250_000)],
    ),
    track(
        "mafia-wins",
        "마피아팀 승리",
        "마피아팀으로 {}승",
        Metric::TeamWins("mafia"),
        &[(5, 30_000), (20, 100_000), (50, 250_000)],
    ),
    track(
        "cult-wins",
        "교주팀 승리",
        "교주팀으로 {}승",
        Metric::TeamWins("cult"),
        &[(3, 50_000), (10, 150_000)],
    ),
    track(
        "joker-wins",
        "조커 승리",
        "조커로 {}승",
        Metric::TeamWins("joker"),
        &[(1, 50_000), (5, 200_000)],
    ),
    track(
        "hours",
        "플레이 시간",
        "마피아 {}시간",
        Metric::PlayHours,
        &[(10, 30_000), (50, 100_000), (100, 200_000), (300, 500_000)],
    ),
    track(
        "rating",
        "최고 레이팅",
        "레이팅 {}",
        Metric::RatingPeak,
        &[
            (1_100, 30_000),
            (1_200, 80_000),
            (1_300, 150_000),
            (1_500, 300_000),
        ],
    ),
    track(
        "casino",
        "카지노 판 수",
        "카지노 {}판",
        Metric::CasinoHands,
        &[
            (1, 5_000),
            (100, 20_000),
            (500, 50_000),
            (1_000, 100_000),
            (5_000, 300_000),
            (10_000, 500_000),
        ],
    ),
    track(
        "jackpot",
        "카지노 잭팟",
        "잭팟 {}번",
        Metric::Jackpots,
        &[(1, 50_000), (3, 150_000), (10, 400_000)],
    ),
    track(
        "stock",
        "주식 체결",
        "주식 {}번 체결",
        Metric::StockTrades,
        &[
            (1, 10_000),
            (10, 30_000),
            (100, 100_000),
            (500, 250_000),
            (1_000, 400_000),
        ],
    ),
    track(
        "stock-profit",
        "주식 실현 수익",
        "실현 수익 {}원",
        Metric::StockProfit,
        &[
            (100_000, 30_000),
            (1_000_000, 100_000),
            (10_000_000, 300_000),
        ],
    ),
    track(
        "company",
        "회사 설립",
        "회사 {}개 세우기",
        Metric::CompaniesFounded,
        &[(1, 30_000)],
    ),
    track(
        "listed",
        "회사 상장",
        "회사 {}개 상장",
        Metric::CompaniesListed,
        &[(1, 80_000)],
    ),
    track(
        "attend",
        "출석",
        "출석 {}일",
        Metric::AttendanceDays,
        &[(7, 20_000), (30, 60_000), (100, 200_000), (365, 600_000)],
    ),
    track(
        "attend-streak",
        "최장 연속 출석",
        "{}일 연속 출석",
        Metric::AttendanceStreak,
        &[(7, 30_000), (30, 100_000), (100, 300_000)],
    ),
    track(
        "mission-days",
        "미션 올클리어",
        "미션 세 개 모두 {}일",
        Metric::MissionDays,
        &[(1, 10_000), (10, 50_000), (30, 120_000), (100, 400_000)],
    ),
];

/// 업적 표 1(처음 내놓은 것)의 보상. 표 2로 올린 차액을 한 번 지급할 때 쓴다.
const ACHIEVEMENT_V1_REWARDS: &[(&str, i64)] = &[
    ("games-1", 2_000),
    ("games-10", 5_000),
    ("games-50", 15_000),
    ("games-100", 30_000),
    ("games-300", 60_000),
    ("wins-1", 3_000),
    ("wins-10", 8_000),
    ("wins-50", 25_000),
    ("wins-100", 50_000),
    ("streak-3", 5_000),
    ("streak-5", 15_000),
    ("star-1", 5_000),
    ("star-10", 20_000),
    ("roles-10", 10_000),
    ("casino-1", 1_000),
    ("casino-100", 5_000),
    ("casino-1000", 20_000),
    ("jackpot-1", 10_000),
    ("stock-1", 2_000),
    ("company-1", 5_000),
    ("listed-1", 10_000),
];

/// 업적 판단에 쓰는 주식 시장 쪽 값 (호출자가 시장에서 센다).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StockProgress {
    /// 누적 체결 수.
    pub trades: i64,
    /// 실현 손익 누적.
    pub realized: i64,
    /// 세운 회사 수 (해산·상장폐지 포함).
    pub founded: i64,
    /// 상장까지 간 회사 수.
    pub listed: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AchievementStatus {
    pub id: String,
    pub track: &'static AchievementTrack,
    /// "마피아 100판"
    pub title: String,
    pub target: i64,
    pub progress: i64,
    pub done: bool,
    pub claimed: bool,
    /// 배율을 곱한 보상.
    pub reward: i64,
}

fn metric_value(entry: Option<&PlayerStats>, stock: StockProgress, metric: Metric) -> i64 {
    let Some(entry) = entry else {
        return match metric {
            Metric::StockTrades => stock.trades,
            Metric::StockProfit => stock.realized.max(0),
            Metric::CompaniesFounded => stock.founded,
            Metric::CompaniesListed => stock.listed,
            _ => 0,
        };
    };
    match metric {
        Metric::Games => entry.games,
        Metric::Wins => entry.wins,
        Metric::BestWinStreak => entry.best_win_streak,
        Metric::StarPlayer => entry.star_player_count,
        Metric::RolesPlayed => entry.roles.values().filter(|count| **count > 0).count() as i64,
        Metric::TeamWins(team) => entry.rewards.team_wins.get(team).copied().unwrap_or(0),
        Metric::PlayHours => entry.play_seconds / 3_600,
        Metric::RatingPeak => entry.rating_peak,
        Metric::CasinoHands => entry.rewards.casino_hands,
        Metric::Jackpots => entry.rewards.jackpots,
        Metric::StockTrades => stock.trades,
        Metric::StockProfit => stock.realized.max(0),
        Metric::CompaniesFounded => stock.founded,
        Metric::CompaniesListed => stock.listed,
        Metric::AttendanceDays => entry.rewards.attendance_days,
        Metric::AttendanceStreak => entry.rewards.best_attendance_streak,
        Metric::MissionDays => entry.rewards.mission_days,
    }
}

fn scaled(reward: i64, rules: &RewardRules) -> i64 {
    reward.saturating_mul(rules.achievement_pct.max(0)) / 100
}

/// 모든 업적의 진행 (트랙 순서, 트랙 안에서는 목표 순서).
pub fn achievement_status(
    entry: Option<&PlayerStats>,
    stock: StockProgress,
    rules: &RewardRules,
) -> Vec<AchievementStatus> {
    let mut out = Vec::new();
    for track in ACHIEVEMENT_TRACKS {
        let progress = metric_value(entry, stock, track.metric);
        for (target, reward) in track.tiers {
            let id = format!("{}-{target}", track.key);
            let claimed = entry.is_some_and(|entry| entry.rewards.achievements.contains(&id));
            out.push(AchievementStatus {
                title: track.goal.replace("{}", &group(*target)),
                track,
                target: *target,
                progress,
                done: progress >= *target,
                claimed,
                reward: scaled(*reward, rules),
                id,
            });
        }
    }
    out
}

/// 새로 이룬 업적의 보상을 받는다 (업적 보상이 꺼져 있으면 받지 않고 그대로 둔다). 업적 표 1에서
/// 이미 받은 업적은 표 2로 올린 차액을 한 번 더 준다.
pub fn claim_achievements(
    stats: &mut StatsFile,
    user_id: u64,
    name: &str,
    stock: StockProgress,
    rules: &RewardRules,
) -> (Vec<AchievementStatus>, Claim) {
    let mut claim = Claim::default();
    let entry = ensure_player_stats(stats, user_id, name);
    if rules.achievement_pct > 0 {
        let statuses = achievement_status(Some(entry), stock, rules);
        if entry.rewards.achievement_version < ACHIEVEMENT_VERSION {
            for status in statuses.iter().filter(|status| status.claimed) {
                let Some((_, old)) = ACHIEVEMENT_V1_REWARDS
                    .iter()
                    .find(|(id, _)| *id == status.id)
                else {
                    continue;
                };
                let extra = status.reward - scaled(*old, rules);
                if extra > 0 {
                    pay(entry, extra);
                    claim
                        .paid
                        .push((format!("{} 보상 인상분", status.title), extra));
                    claim.total += extra;
                }
            }
            entry.rewards.achievement_version = ACHIEVEMENT_VERSION;
        }
        for status in &statuses {
            if status.done && !status.claimed {
                entry.rewards.achievements.push(status.id.clone());
                pay(entry, status.reward);
                claim.paid.push((status.title.clone(), status.reward));
                claim.total += status.reward;
            }
        }
    }
    claim.balance = entry.coins;
    let statuses = achievement_status(Some(entry), stock, rules);
    stats.reward_totals.achievements = stats.reward_totals.achievements.saturating_add(claim.total);
    (statuses, claim)
}

// ------------------------------------------------------------ 연속 출석

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreakBonus {
    pub streak: i64,
    /// 이번에 받은 보너스 (7일·30일째가 아니면 0).
    pub bonus: i64,
    pub balance: i64,
}

/// 출석한 뒤 부른다: 연속 출석 일수와 누적 출석 일수를 세고 7일째·30일째마다 보너스를 준다. 같은 날
/// 두 번 세지 않는다. `previous_attendance`는 이번 출석 전의 마지막 출석 날짜 (연속 기록이 없던
/// 사람을 이어 주려고 쓴다).
pub fn apply_attendance_streak(
    stats: &mut StatsFile,
    user_id: u64,
    name: &str,
    today: &str,
    yesterday: &str,
    previous_attendance: &str,
    rules: &RewardRules,
) -> StreakBonus {
    let entry = ensure_player_stats(stats, user_id, name);
    let record = &mut entry.rewards;
    if record.streak_day == today {
        return StreakBonus {
            streak: record.attendance_streak,
            bonus: 0,
            balance: entry.coins,
        };
    }
    let continued = record.streak_day == yesterday
        || (record.streak_day.is_empty() && previous_attendance == yesterday);
    record.attendance_streak = if continued {
        record.attendance_streak.max(1) + 1
    } else {
        1
    };
    record.streak_day = today.to_string();
    record.attendance_days += 1;
    record.best_attendance_streak = record.best_attendance_streak.max(record.attendance_streak);
    let streak = record.attendance_streak;
    let mut bonus = 0;
    if streak % 7 == 0 {
        bonus += rules.streak_week.max(0);
    }
    if streak % 30 == 0 {
        bonus += rules.streak_month.max(0);
    }
    if bonus > 0 {
        pay(entry, bonus);
    }
    let balance = entry.coins;
    stats.reward_totals.streak = stats.reward_totals.streak.saturating_add(bonus);
    StreakBonus {
        streak,
        bonus,
        balance,
    }
}

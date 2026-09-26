// stats/rewards.rs — 코인 벌이: 마피아 참여 보상, 일일 미션, 업적, 연속 출석 보너스.
// 보상 코인은 새로 발행한다. 부계정으로 찍어 내지 못하게 참여 보상은 하루 판 수로, 미션은 하루
// 세 개로, 업적은 한 번씩으로 총량을 묶는다. 금액은 모두 운영 설정이고(0이면 끔), 발행한 누적은
// `RewardTotals`에 남긴다.

use super::{PlayerStats, StatsFile, ensure_player_stats, player_won_game};
use crate::game::MafiaGame;
use crate::model::Winner;
use serde::{Deserialize, Serialize};

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
    /// 받은 업적 id.
    #[serde(default)]
    pub achievements: Vec<String>,
    /// 연속 출석 일수와 마지막으로 센 날.
    #[serde(default)]
    pub attendance_streak: i64,
    #[serde(default)]
    pub streak_day: String,
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

/// 판이 끝났을 때: 참가자마다 오늘의 판 수·승수를 세고(미션용), 하루 한도 안에서 참여 보상을 준다.
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
        let entry = ensure_player_stats(stats, player.user_id, &player.name);
        entry.rewards.roll(today);
        entry.rewards.day_games += 1;
        if won {
            entry.rewards.day_wins += 1;
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
    if all_done && rules.mission_bonus > 0 && !entry.rewards.claimed.iter().any(|id| id == BONUS_ID)
    {
        entry.rewards.claimed.push(BONUS_ID.to_string());
        pay(entry, rules.mission_bonus);
        claim
            .paid
            .push(("세 미션 모두 달성 보너스".to_string(), rules.mission_bonus));
        claim.total += rules.mission_bonus;
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
    CasinoHands,
    Jackpots,
    StockTrades,
    CompaniesFounded,
    CompaniesListed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Achievement {
    pub id: &'static str,
    pub title: &'static str,
    pub metric: Metric,
    pub target: i64,
    /// 기본 보상 (운영 설정의 배율을 곱한다).
    pub reward: i64,
}

const fn achievement(
    id: &'static str,
    title: &'static str,
    metric: Metric,
    target: i64,
    reward: i64,
) -> Achievement {
    Achievement {
        id,
        title,
        metric,
        target,
        reward,
    }
}

pub const ACHIEVEMENTS: &[Achievement] = &[
    achievement("games-1", "첫 판", Metric::Games, 1, 2_000),
    achievement("games-10", "마피아 10판", Metric::Games, 10, 5_000),
    achievement("games-50", "마피아 50판", Metric::Games, 50, 15_000),
    achievement("games-100", "마피아 100판", Metric::Games, 100, 30_000),
    achievement("games-300", "마피아 300판", Metric::Games, 300, 60_000),
    achievement("wins-1", "첫 승리", Metric::Wins, 1, 3_000),
    achievement("wins-10", "10승", Metric::Wins, 10, 8_000),
    achievement("wins-50", "50승", Metric::Wins, 50, 25_000),
    achievement("wins-100", "100승", Metric::Wins, 100, 50_000),
    achievement("streak-3", "3연승", Metric::BestWinStreak, 3, 5_000),
    achievement("streak-5", "5연승", Metric::BestWinStreak, 5, 15_000),
    achievement("star-1", "첫 스타플레이어", Metric::StarPlayer, 1, 5_000),
    achievement(
        "star-10",
        "스타플레이어 10번",
        Metric::StarPlayer,
        10,
        20_000,
    ),
    achievement(
        "roles-10",
        "직업 10가지 해 보기",
        Metric::RolesPlayed,
        10,
        10_000,
    ),
    achievement("casino-1", "카지노 첫 판", Metric::CasinoHands, 1, 1_000),
    achievement(
        "casino-100",
        "카지노 100판",
        Metric::CasinoHands,
        100,
        5_000,
    ),
    achievement(
        "casino-1000",
        "카지노 1,000판",
        Metric::CasinoHands,
        1_000,
        20_000,
    ),
    achievement("jackpot-1", "잭팟 당첨", Metric::Jackpots, 1, 10_000),
    achievement("stock-1", "첫 주식 거래", Metric::StockTrades, 1, 2_000),
    achievement(
        "company-1",
        "회사 세우기",
        Metric::CompaniesFounded,
        1,
        5_000,
    ),
    achievement(
        "listed-1",
        "회사 상장시키기",
        Metric::CompaniesListed,
        1,
        10_000,
    ),
];

/// 업적 판단에 쓰는 주식 시장 쪽 값 (호출자가 시장에서 센다).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StockProgress {
    /// 체결 기록 수 (최근 기록 기준, 0이면 거래한 적 없음).
    pub trades: i64,
    /// 세운 회사 수 (해산·상장폐지 포함).
    pub founded: i64,
    /// 상장까지 간 회사 수.
    pub listed: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AchievementStatus {
    pub achievement: Achievement,
    pub progress: i64,
    pub done: bool,
    pub claimed: bool,
    /// 배율을 곱한 보상.
    pub reward: i64,
}

fn metric_value(entry: Option<&PlayerStats>, stock: StockProgress, metric: Metric) -> i64 {
    match metric {
        Metric::Games => entry.map_or(0, |entry| entry.games),
        Metric::Wins => entry.map_or(0, |entry| entry.wins),
        Metric::BestWinStreak => entry.map_or(0, |entry| entry.best_win_streak),
        Metric::StarPlayer => entry.map_or(0, |entry| entry.star_player_count),
        Metric::RolesPlayed => entry.map_or(0, |entry| {
            entry.roles.values().filter(|count| **count > 0).count() as i64
        }),
        Metric::CasinoHands => entry.map_or(0, |entry| entry.rewards.casino_hands),
        Metric::Jackpots => entry.map_or(0, |entry| entry.rewards.jackpots),
        Metric::StockTrades => stock.trades,
        Metric::CompaniesFounded => stock.founded,
        Metric::CompaniesListed => stock.listed,
    }
}

pub fn achievement_status(
    entry: Option<&PlayerStats>,
    stock: StockProgress,
    rules: &RewardRules,
) -> Vec<AchievementStatus> {
    ACHIEVEMENTS
        .iter()
        .map(|achievement| {
            let progress = metric_value(entry, stock, achievement.metric);
            AchievementStatus {
                achievement: *achievement,
                progress,
                done: progress >= achievement.target,
                claimed: entry.is_some_and(|entry| {
                    entry
                        .rewards
                        .achievements
                        .iter()
                        .any(|id| id == achievement.id)
                }),
                reward: achievement
                    .reward
                    .saturating_mul(rules.achievement_pct.max(0))
                    / 100,
            }
        })
        .collect()
}

/// 새로 이룬 업적의 보상을 받는다 (업적 보상이 꺼져 있으면 받지 않고 그대로 둔다).
pub fn claim_achievements(
    stats: &mut StatsFile,
    user_id: u64,
    name: &str,
    stock: StockProgress,
    rules: &RewardRules,
) -> (Vec<AchievementStatus>, Claim) {
    let mut claim = Claim::default();
    let entry = ensure_player_stats(stats, user_id, name);
    let statuses = achievement_status(Some(entry), stock, rules);
    if rules.achievement_pct > 0 {
        for status in &statuses {
            if status.done && !status.claimed {
                entry
                    .rewards
                    .achievements
                    .push(status.achievement.id.to_string());
                pay(entry, status.reward);
                claim
                    .paid
                    .push((status.achievement.title.to_string(), status.reward));
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

/// 출석한 뒤 부른다: 연속 출석 일수를 세고 7일째·30일째마다 보너스를 준다. 같은 날 두 번 세지 않는다.
/// `previous_attendance`는 이번 출석 전의 마지막 출석 날짜 (연속 기록이 없던 사람을 이어 주려고 쓴다).
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
    if record.streak_day != today {
        let continued = record.streak_day == yesterday
            || (record.streak_day.is_empty() && previous_attendance == yesterday);
        record.attendance_streak = if continued {
            record.attendance_streak.max(1) + 1
        } else {
            1
        };
        record.streak_day = today.to_string();
    } else {
        return StreakBonus {
            streak: record.attendance_streak,
            bonus: 0,
            balance: entry.coins,
        };
    }
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

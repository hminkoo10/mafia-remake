// stats/coins.rs — 코인: 출석, 배팅 정산, 스타플레이어 상금, 내신 쿠폰 교환

use super::{
    INITIAL_RATING, PlayerStats, StatsFile, ensure_player_stats, player_won_game, rating_team_key,
};
use crate::game::MafiaGame;
use crate::model::{Player, Role, Winner};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 출석 1회당 기본 지급 코인(원). 설정(attendance_coins)으로 바꿀 수 있다.
pub const DEFAULT_ATTENDANCE_COINS: i64 = 10_000;
/// 스타플레이어 상금 기본값(원). 동표면 인원수로 나눈다.
pub const DEFAULT_STAR_PLAYER_COINS: i64 = 1_000;
/// 내신 쿠폰 1포인트당 코인(원) 기본값.
pub const DEFAULT_COUPON_COINS_PER_POINT: i64 = 10_000;
const COUPON_HISTORY_LIMIT: usize = 20;

/// 발급받은 쿠폰 기록. 코드는 발급 응답을 그대로 보관한다.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CouponRecord {
    pub issued_at: String,
    pub points: i64,
    pub codes: Vec<String>,
}

/// 한국 시간 기준 오늘 날짜(YYYY-MM-DD). 출석은 이 날짜가 바뀌면 다시 할 수 있다.
pub fn kst_today() -> String {
    let kst = chrono::FixedOffset::east_opt(9 * 3600).expect("KST offset");
    chrono::Utc::now()
        .with_timezone(&kst)
        .format("%Y-%m-%d")
        .to_string()
}

/// "12,345원" 표기.
pub fn coin_text(amount: i64) -> String {
    let digits = amount.unsigned_abs().to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    if amount < 0 {
        format!("-{grouped}원")
    } else {
        format!("{grouped}원")
    }
}

/// "+1,400원" / "-700원" 표기.
pub fn signed_coin_text(delta: i64) -> String {
    if delta >= 0 {
        format!("+{}", coin_text(delta))
    } else {
        coin_text(delta)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttendanceOutcome {
    Claimed { amount: i64, balance: i64 },
    AlreadyClaimed { balance: i64 },
}

/// 출석: 하루(한국 시간) 한 번 코인을 지급한다.
pub fn claim_attendance(
    stats: &mut StatsFile,
    user_id: u64,
    name: &str,
    amount: i64,
    today: &str,
) -> AttendanceOutcome {
    let entry = ensure_player_stats(stats, user_id, name);
    if entry.last_attendance_date == today {
        return AttendanceOutcome::AlreadyClaimed {
            balance: entry.coins,
        };
    }
    let amount = amount.max(0);
    entry.last_attendance_date = today.to_string();
    entry.coins = entry.coins.saturating_add(amount);
    AttendanceOutcome::Claimed {
        amount,
        balance: entry.coins,
    }
}

/// 배팅액 설정. 음수나 보유 코인 초과는 거부한다. 성공하면 보유 코인을 돌려준다.
pub fn set_bet_amount(
    stats: &mut StatsFile,
    user_id: u64,
    name: &str,
    amount: i64,
) -> Result<i64, String> {
    if amount < 0 {
        return Err("배팅액은 0원 이상이어야 합니다.".to_string());
    }
    let entry = ensure_player_stats(stats, user_id, name);
    if amount > entry.coins {
        return Err(format!(
            "보유 코인({})보다 많이 배팅할 수 없습니다.",
            coin_text(entry.coins)
        ));
    }
    entry.bet_amount = amount;
    Ok(entry.coins)
}

/// 판 시작 시 실제로 걸리는 배팅액: 설정값을 보유 코인 안으로 자른다.
pub fn effective_bet(entry: Option<&PlayerStats>) -> i64 {
    entry.map_or(0, |entry| entry.bet_amount.clamp(0, entry.coins.max(0)))
}

#[derive(Debug, Clone, PartialEq)]
pub struct BetSettlement {
    pub user_id: u64,
    pub name: String,
    pub bet: i64,
    pub won: bool,
    /// 승리면 획득 배율, 패배면 차감 배율.
    pub multiplier: f64,
    pub delta: i64,
    pub balance: i64,
    pub factors: Vec<String>,
}

/// 배팅 정산. 판 시작 시 잡아 둔 배팅액을 승패와 배율에 따라 지급·차감한다.
/// 배율은 이 판을 기록하기 전의 전적을 기준으로 하므로, 전적 기록보다 먼저 부른다.
pub fn settle_bets(
    stats: &mut StatsFile,
    game: &MafiaGame,
    initial_roles: &HashMap<u64, Role>,
    winner: Winner,
    bets: &HashMap<u64, i64>,
) -> Vec<BetSettlement> {
    let mut players = game.players.clone();
    players.sort_by_key(|player| player.name.to_lowercase());
    let mut settlements = Vec::new();
    for player in &players {
        let bet = bets.get(&player.user_id).copied().unwrap_or(0).max(0);
        if bet == 0 {
            continue;
        }
        let role = initial_roles
            .get(&player.user_id)
            .copied()
            .unwrap_or(player.role);
        let won = player_won_game(game, player, winner);
        let (win_multiplier, factors) = bet_win_multiplier(stats, game, player, role);
        let (multiplier, delta) = if won {
            (win_multiplier, (bet as f64 * win_multiplier).round() as i64)
        } else {
            let loss_multiplier = (1.0 / win_multiplier).clamp(0.5, 1.0);
            (
                loss_multiplier,
                -((bet as f64 * loss_multiplier).round() as i64),
            )
        };
        let entry = ensure_player_stats(stats, player.user_id, &player.name);
        entry.coins = entry.coins.saturating_add(delta).max(0);
        settlements.push(BetSettlement {
            user_id: player.user_id,
            name: player.name.clone(),
            bet,
            won,
            multiplier,
            delta,
            balance: entry.coins,
            factors,
        });
    }
    settlements
}

/// 승리 시 배당 배율과 그 근거.
/// 팀(시민 1.0 / 마피아 1.4 / 교주 1.6 / 조커 2.0) × 직업 난이도(그 직업의 전체
/// 승률이 낮을수록 높게, 0.7~1.5) × 실력 보정(본인 승률·레이팅이 높을수록 낮게,
/// 0.7~1.3). 전체는 0.5~4.0으로 자른다. 패배 차감 배율은 이 값의 역수(0.5~1.0).
pub fn bet_win_multiplier(
    stats: &StatsFile,
    game: &MafiaGame,
    player: &Player,
    role: Role,
) -> (f64, Vec<String>) {
    let (team_name, team_factor) = match rating_team_key(game, player) {
        "mafia" => ("마피아팀", 1.4),
        "cult" => ("교주팀", 1.6),
        "joker" => ("조커", 2.0),
        _ => ("시민팀", 1.0),
    };
    let role_factor = stats
        .role_outcomes
        .get(role.value())
        .map_or(1.0, |outcome| {
            // 사전 2승 2패를 섞어 표본이 적을 때 극단값을 막는다.
            let rate = (outcome.wins as f64 + 2.0) / (outcome.games as f64 + 4.0);
            (0.5 / rate).clamp(0.7, 1.5)
        });
    let skill_factor = stats
        .users
        .get(&player.user_id.to_string())
        .map_or(1.0, |entry| {
            let win_rate = (entry.wins as f64 + 2.0) / (entry.games as f64 + 4.0);
            let win_rate_factor = (1.0 - (win_rate - 0.5) * 1.2).clamp(0.7, 1.3);
            let rating_factor =
                (1.0 - (entry.rating - INITIAL_RATING) as f64 / 1000.0 * 0.5).clamp(0.75, 1.25);
            (win_rate_factor * rating_factor).clamp(0.7, 1.3)
        });
    let multiplier = (team_factor * role_factor * skill_factor).clamp(0.5, 4.0);
    let factors = vec![
        format!("{team_name} ×{team_factor:.1}"),
        format!("직업 난이도 ×{role_factor:.2}"),
        format!("실력 보정 ×{skill_factor:.2}"),
    ];
    (multiplier, factors)
}

/// 최다 득표자 목록 (동표 포함, id 순). 표가 없으면 빈 벡터.
pub fn tally_star_votes(votes: &HashMap<u64, u64>) -> Vec<(u64, usize)> {
    let mut counts = HashMap::<u64, usize>::new();
    for target in votes.values() {
        *counts.entry(*target).or_default() += 1;
    }
    let Some(best) = counts.values().copied().max() else {
        return Vec::new();
    };
    let mut winners = counts
        .into_iter()
        .filter(|(_, count)| *count == best)
        .collect::<Vec<_>>();
    winners.sort_unstable();
    winners
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarAward {
    pub user_id: u64,
    pub name: String,
    pub votes: usize,
    pub prize: i64,
    pub balance: i64,
}

/// 스타플레이어 상금 지급. 동표면 상금을 인원수로 나눈다(나머지 버림).
pub fn award_star_players(
    stats: &mut StatsFile,
    winners: &[(u64, String, usize)],
    prize_total: i64,
) -> Vec<StarAward> {
    if winners.is_empty() {
        return Vec::new();
    }
    let prize = prize_total.max(0) / winners.len() as i64;
    winners
        .iter()
        .map(|(user_id, name, votes)| {
            let entry = ensure_player_stats(stats, *user_id, name);
            entry.coins = entry.coins.saturating_add(prize);
            entry.star_player_count += 1;
            StarAward {
                user_id: *user_id,
                name: name.clone(),
                votes: *votes,
                prize,
                balance: entry.coins,
            }
        })
        .collect()
}

/// 코인을 미리 차감한다 (쿠폰 발급 전 예약). 부족하면 거부한다. 성공하면 남은 코인.
pub fn reserve_coins(
    stats: &mut StatsFile,
    user_id: u64,
    name: &str,
    cost: i64,
) -> Result<i64, String> {
    let entry = ensure_player_stats(stats, user_id, name);
    if cost <= 0 {
        return Err("차감할 금액이 올바르지 않습니다.".to_string());
    }
    if entry.coins < cost {
        return Err(format!(
            "보유 코인이 부족합니다. 필요 {} / 보유 {}",
            coin_text(cost),
            coin_text(entry.coins)
        ));
    }
    entry.coins -= cost;
    Ok(entry.coins)
}

/// 예약했던 코인을 돌려준다 (쿠폰 발급 실패 시).
pub fn refund_coins(stats: &mut StatsFile, user_id: u64, name: &str, amount: i64) -> i64 {
    let entry = ensure_player_stats(stats, user_id, name);
    entry.coins = entry.coins.saturating_add(amount.max(0));
    entry.coins
}

/// 발급된 쿠폰을 기록한다 (최근 20건 보관).
pub fn record_coupon(
    stats: &mut StatsFile,
    user_id: u64,
    name: &str,
    points: i64,
    codes: Vec<String>,
    issued_at: &str,
) {
    let entry = ensure_player_stats(stats, user_id, name);
    entry.coupon_points_exchanged = entry.coupon_points_exchanged.saturating_add(points);
    entry.coupons.push(CouponRecord {
        issued_at: issued_at.to_string(),
        points,
        codes,
    });
    let overflow = entry.coupons.len().saturating_sub(COUPON_HISTORY_LIMIT);
    if overflow > 0 {
        entry.coupons.drain(..overflow);
    }
}

/// 관리자 코인 조정 결과 (조정 전/후 보유액).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoinAdjustment {
    pub before: i64,
    pub after: i64,
}

/// 관리자 지급/차감. 차감은 0원 아래로 내려가지 않는다 (보유액까지만 차감).
pub fn adjust_coins(stats: &mut StatsFile, user_id: u64, name: &str, delta: i64) -> CoinAdjustment {
    let entry = ensure_player_stats(stats, user_id, name);
    let before = entry.coins;
    entry.coins = entry.coins.saturating_add(delta).max(0);
    CoinAdjustment {
        before,
        after: entry.coins,
    }
}

/// 관리자 보유 코인 설정 (0원 이상).
pub fn set_coins(
    stats: &mut StatsFile,
    user_id: u64,
    name: &str,
    amount: i64,
) -> Result<CoinAdjustment, String> {
    if amount < 0 {
        return Err("코인은 0원 이상으로만 설정할 수 있습니다.".to_string());
    }
    let entry = ensure_player_stats(stats, user_id, name);
    let before = entry.coins;
    entry.coins = amount;
    Ok(CoinAdjustment {
        before,
        after: amount,
    })
}

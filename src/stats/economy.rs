// stats/economy.rs — 코인 순환: 복지 금고와 잭팟 풀, 일간 롤링 환급, 주간 손실 환급, 저잔고 구조금.
// 금고는 카지노 하우스 수익·홀덤 레이크·선물 수수료·마피아 배팅 패배분 일부로 채워지고,
// 환급·구조금·스타플레이어 상금·블랙잭 하우스 손실로 나간다. 비율과 한도는 모두 운영 설정이다.

use super::{StatsFile, coin_text, ensure_player_stats};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 비율은 만분율로 적는다 (10,000 = 100%, 250 = 2.5%).
pub const BP_SCALE: i64 = 10_000;

pub const DEFAULT_HOLDEM_RAKE_BP: i64 = 250;
pub const DEFAULT_HOLDEM_RAKE_CAP: i64 = 500;
pub const DEFAULT_GIFT_FEE_BP: i64 = 300;
pub const DEFAULT_BET_LOSS_TREASURY_BP: i64 = 5_000;
pub const DEFAULT_JACKPOT_SHARE_BP: i64 = 2_000;
pub const DEFAULT_JACKPOT_PAYOUT_BP: i64 = 5_000;
pub const DEFAULT_JACKPOT_LOSER_BP: i64 = 5_000;
pub const DEFAULT_JACKPOT_WINNER_BP: i64 = 2_500;
pub const DEFAULT_WEEKLY_CASHBACK_BP: i64 = 1_000;
pub const DEFAULT_WEEKLY_CASHBACK_CAP: i64 = 20_000;
/// 블랙잭 하우스 우위(약 0.5%)보다 낮아야 롤링만 노리고 돌리는 판이 이득이 되지 않는다.
pub const DEFAULT_DAILY_ROLLING_BP: i64 = 20;
pub const DEFAULT_DAILY_ROLLING_CAP: i64 = 5_000;
pub const DEFAULT_RELIEF_THRESHOLD: i64 = 2_000;
pub const DEFAULT_RELIEF_AMOUNT: i64 = 5_000;
pub const DEFAULT_RELIEF_GIFT_LOCK_HOURS: i64 = 24;

/// 운영자가 정하는 비율(만분율)과 한도(원). config.json에서 만든다 (`BotConfig::economy_rules`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EconomyRules {
    /// 홀덤 레이크: 플롭을 연 핸드의 팟에서 떼는 비율과 핸드당 상한(칩).
    pub holdem_rake_bp: i64,
    pub holdem_rake_cap: i64,
    /// 코인 선물 수수료 (받는 사람이 그만큼 덜 받는다).
    pub gift_fee_bp: i64,
    /// 마피아 배팅 패배로 잃은 코인 중 금고로 가는 비율 (나머지는 사라진다).
    pub bet_loss_treasury_bp: i64,
    /// 금고로 들어오는 코인 중 잭팟 풀로 떼어 두는 비율.
    pub jackpot_share_bp: i64,
    /// 잭팟이 터지면 풀에서 지급하는 비율 (나머지는 다음 잭팟 씨앗).
    pub jackpot_payout_bp: i64,
    /// 배드비트 잭팟: 진 사람 몫, 이긴 사람 몫 (나머지는 같은 핸드에 참가한 사람들이 나눈다).
    pub jackpot_loser_bp: i64,
    pub jackpot_winner_bp: i64,
    /// 주간 손실 환급: 지난주 하우스 게임(블랙잭·마피아 배팅) 순손실에 대한 비율과 1인 상한.
    pub weekly_cashback_bp: i64,
    pub weekly_cashback_cap: i64,
    /// 일간 롤링 환급: 어제 카지노 베팅 총액(롤링)에 대한 비율과 1인 상한.
    pub daily_rolling_bp: i64,
    pub daily_rolling_cap: i64,
    /// 구조금: 보유 코인+테이블 칩이 기준 미만이면 하루 한 번, 받은 뒤 선물 금지 시간.
    pub relief_threshold: i64,
    pub relief_amount: i64,
    pub relief_gift_lock_hours: i64,
}

impl Default for EconomyRules {
    fn default() -> Self {
        Self {
            holdem_rake_bp: DEFAULT_HOLDEM_RAKE_BP,
            holdem_rake_cap: DEFAULT_HOLDEM_RAKE_CAP,
            gift_fee_bp: DEFAULT_GIFT_FEE_BP,
            bet_loss_treasury_bp: DEFAULT_BET_LOSS_TREASURY_BP,
            jackpot_share_bp: DEFAULT_JACKPOT_SHARE_BP,
            jackpot_payout_bp: DEFAULT_JACKPOT_PAYOUT_BP,
            jackpot_loser_bp: DEFAULT_JACKPOT_LOSER_BP,
            jackpot_winner_bp: DEFAULT_JACKPOT_WINNER_BP,
            weekly_cashback_bp: DEFAULT_WEEKLY_CASHBACK_BP,
            weekly_cashback_cap: DEFAULT_WEEKLY_CASHBACK_CAP,
            daily_rolling_bp: DEFAULT_DAILY_ROLLING_BP,
            daily_rolling_cap: DEFAULT_DAILY_ROLLING_CAP,
            relief_threshold: DEFAULT_RELIEF_THRESHOLD,
            relief_amount: DEFAULT_RELIEF_AMOUNT,
            relief_gift_lock_hours: DEFAULT_RELIEF_GIFT_LOCK_HOURS,
        }
    }
}

/// `amount`의 `bp`만분율 (내림). 음수·0이면 0, 비율은 100%까지만 본다.
pub fn bp_of(amount: i64, bp: i64) -> i64 {
    if amount <= 0 || bp <= 0 {
        return 0;
    }
    let raw = i128::from(amount) * i128::from(bp.min(BP_SCALE)) / i128::from(BP_SCALE);
    i64::try_from(raw).unwrap_or(i64::MAX)
}

/// "2.5%" 꼴 비율 표기.
pub fn bp_text(bp: i64) -> String {
    let bp = bp.clamp(0, BP_SCALE);
    let whole = bp / 100;
    let fraction = bp % 100;
    if fraction == 0 {
        format!("{whole}%")
    } else if fraction % 10 == 0 {
        format!("{whole}.{}%", fraction / 10)
    } else {
        format!("{whole}.{fraction:02}%")
    }
}

/// 복지 금고와 잭팟 풀 (stats.json, 모든 서버 공용).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Treasury {
    /// 복지 금고: 환급·구조금·스타플레이어 상금을 주고 블랙잭 하우스 손실을 메운다.
    #[serde(default)]
    pub balance: i64,
    /// 잭팟 풀.
    #[serde(default)]
    pub jackpot: i64,
    /// 누적 유입 (잭팟 적립 포함).
    #[serde(default)]
    pub inflow_total: i64,
    /// 누적 지출 (잭팟 지급 포함).
    #[serde(default)]
    pub outflow_total: i64,
    /// 금고가 모자라 새로 발행해 준 코인 누적 (구조금 부족분).
    #[serde(default)]
    pub minted_total: i64,
}

/// 유저별 환급 기록 (stats.json의 유저 항목).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EconomyRecord {
    /// 날짜(한국 시간 YYYY-MM-DD)별 카지노 롤링. 지난 날짜는 환급하고 지운다.
    #[serde(default)]
    pub rolling: BTreeMap<String, i64>,
    /// 주(한국 시간 ISO 주, 2026-W39)별 하우스 게임 순손익. 지난 주는 환급하고 지운다.
    #[serde(default)]
    pub house_net: BTreeMap<String, i64>,
    #[serde(default)]
    pub last_rolling: Option<EconomyPayout>,
    #[serde(default)]
    pub last_cashback: Option<EconomyPayout>,
    /// 마지막으로 구조금을 받은 날짜 (한국 시간).
    #[serde(default)]
    pub relief_date: String,
    /// 이 시각(유닉스 ms)까지 코인을 선물할 수 없다 (구조금을 다른 계정으로 옮기지 못하게).
    #[serde(default)]
    pub gift_locked_until: i64,
}

/// 지급된 환급 한 건.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EconomyPayout {
    /// 환급 대상 기간 (날짜 또는 주).
    pub period: String,
    /// 실제로 받은 코인.
    pub amount: i64,
    /// 금고가 넉넉했다면 받았을 코인 (모자라면 비율대로 줄어든다).
    #[serde(default)]
    pub claimed: i64,
}

/// 한국 시간 기준 이번 주 (ISO 주, "2026-W39"). 월요일 0시에 바뀐다.
pub fn kst_week() -> String {
    let kst = chrono::FixedOffset::east_opt(9 * 3600).expect("KST offset");
    week_key(chrono::Utc::now().with_timezone(&kst).date_naive())
}

/// 날짜가 속한 ISO 주 키. 문자열 순서가 시간 순서와 같다.
pub fn week_key(date: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    let week = date.iso_week();
    format!("{}-W{:02}", week.year(), week.week())
}

/// 금고에 넣는다. 그중 잭팟 몫은 잭팟 풀로 간다. 잭팟으로 간 코인을 돌려준다.
pub fn treasury_deposit(stats: &mut StatsFile, amount: i64, rules: &EconomyRules) -> i64 {
    if amount <= 0 {
        return 0;
    }
    let jackpot = bp_of(amount, rules.jackpot_share_bp);
    let treasury = &mut stats.treasury;
    treasury.jackpot = treasury.jackpot.saturating_add(jackpot);
    treasury.balance = treasury.balance.saturating_add(amount - jackpot);
    treasury.inflow_total = treasury.inflow_total.saturating_add(amount);
    jackpot
}

/// 금고에서 꺼낸다 (잔액까지만). 실제로 꺼낸 코인을 돌려준다.
pub fn treasury_take(stats: &mut StatsFile, want: i64) -> i64 {
    let treasury = &mut stats.treasury;
    let paid = want.clamp(0, treasury.balance.max(0));
    treasury.balance -= paid;
    treasury.outflow_total = treasury.outflow_total.saturating_add(paid);
    paid
}

/// 카지노 한 판의 하우스 손익을 금고에 반영한다. 번 것은 넣고, 잃은 것은 금고에서 메운다
/// (금고가 비면 그 이상은 메우지 않는다: 플레이어 칩은 이미 테이블에서 지급됐다).
pub fn apply_house_result(stats: &mut StatsFile, house_delta: i64, rules: &EconomyRules) {
    if house_delta > 0 {
        treasury_deposit(stats, house_delta, rules);
    } else if house_delta < 0 {
        treasury_take(stats, house_delta.saturating_neg());
    }
}

/// 카지노 롤링(베팅 총액)을 오늘 날짜에 더한다.
pub fn record_rolling(stats: &mut StatsFile, user_id: u64, name: &str, amount: i64, day: &str) {
    if amount <= 0 {
        return;
    }
    let entry = ensure_player_stats(stats, user_id, name);
    let slot = entry.economy.rolling.entry(day.to_string()).or_default();
    *slot = slot.saturating_add(amount);
}

/// 하우스 게임(블랙잭·마피아 배팅) 순손익을 이번 주에 더한다.
pub fn record_house_net(stats: &mut StatsFile, user_id: u64, name: &str, delta: i64, week: &str) {
    if delta == 0 {
        return;
    }
    let entry = ensure_player_stats(stats, user_id, name);
    let slot = entry.economy.house_net.entry(week.to_string()).or_default();
    *slot = slot.saturating_add(delta);
}

/// 마피아 배팅 정산 뒤: 주간 순손익을 기록하고, 잃은 코인의 일부를 금고로 보낸다.
pub fn record_bet_economy(
    stats: &mut StatsFile,
    settlements: &[super::BetSettlement],
    rules: &EconomyRules,
    week: &str,
) {
    for settlement in settlements {
        record_house_net(
            stats,
            settlement.user_id,
            &settlement.name,
            settlement.delta,
            week,
        );
        if settlement.delta < 0 {
            let lost = settlement.delta.saturating_neg();
            treasury_deposit(stats, bp_of(lost, rules.bet_loss_treasury_bp), rules);
        }
    }
}

/// 환급 정산 결과 (운영 로그용).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EconomyPayoutSummary {
    pub rolling_users: usize,
    pub rolling_paid: i64,
    pub cashback_users: usize,
    pub cashback_paid: i64,
    /// 금고가 넉넉했다면 줬을 총액.
    pub claimed: i64,
    /// 금고가 모자라 비율대로 줄여 줬다.
    pub shortfall: bool,
    /// 지난 기간 기록을 지웠다 (저장이 필요하다).
    pub changed: bool,
}

struct PendingClaim {
    user: String,
    rolling: i64,
    rolling_period: Option<String>,
    cashback: i64,
    cashback_period: Option<String>,
}

/// 지난 날짜의 롤링 환급과 지난주의 손실 환급을 한꺼번에 준다. 기간마다 한 번만 주고, 준 기록은
/// 지운다 (몇 번을 불러도 같은 기간을 두 번 주지 않는다). 금고가 모자라면 모두 같은 비율로 줄인다.
pub fn run_economy_payouts(
    stats: &mut StatsFile,
    rules: &EconomyRules,
    today: &str,
    this_week: &str,
) -> EconomyPayoutSummary {
    let mut summary = EconomyPayoutSummary::default();
    let mut keys = stats.users.keys().cloned().collect::<Vec<_>>();
    keys.sort_unstable();
    let mut claims = Vec::new();
    for key in keys {
        let Some(entry) = stats.users.get_mut(&key) else {
            continue;
        };
        let economy = &mut entry.economy;
        let past_days = economy
            .rolling
            .range(..today.to_string())
            .map(|(day, amount)| (day.clone(), *amount))
            .collect::<Vec<_>>();
        let past_weeks = economy
            .house_net
            .range(..this_week.to_string())
            .map(|(week, net)| (week.clone(), *net))
            .collect::<Vec<_>>();
        if past_days.is_empty() && past_weeks.is_empty() {
            continue;
        }
        summary.changed = true;
        let mut claim = PendingClaim {
            user: key.clone(),
            rolling: 0,
            rolling_period: None,
            cashback: 0,
            cashback_period: None,
        };
        for (day, amount) in past_days {
            economy.rolling.remove(&day);
            let rebate = bp_of(amount, rules.daily_rolling_bp).min(rules.daily_rolling_cap.max(0));
            claim.rolling = claim.rolling.saturating_add(rebate);
            claim.rolling_period = Some(day);
        }
        for (week, net) in past_weeks {
            economy.house_net.remove(&week);
            let rebate = bp_of(net.saturating_neg(), rules.weekly_cashback_bp)
                .min(rules.weekly_cashback_cap.max(0));
            claim.cashback = claim.cashback.saturating_add(rebate);
            claim.cashback_period = Some(week);
        }
        if claim.rolling > 0 || claim.cashback > 0 {
            claims.push(claim);
        }
    }
    let total = claims.iter().fold(0_i64, |sum, claim| {
        sum.saturating_add(claim.rolling)
            .saturating_add(claim.cashback)
    });
    summary.claimed = total;
    if total == 0 {
        return summary;
    }
    let available = stats.treasury.balance.max(0);
    summary.shortfall = total > available;
    let scale = |amount: i64| -> i64 {
        if total <= available {
            amount
        } else {
            let scaled = i128::from(amount) * i128::from(available) / i128::from(total);
            i64::try_from(scaled).unwrap_or(0)
        }
    };
    for claim in claims {
        let rolling = scale(claim.rolling);
        let cashback = scale(claim.cashback);
        let paid = treasury_take(stats, rolling.saturating_add(cashback));
        // 비율대로 줄인 합은 잔액을 넘지 않으므로 paid == rolling + cashback 이다.
        let Some(entry) = stats.users.get_mut(&claim.user) else {
            continue;
        };
        entry.coins = entry.coins.saturating_add(paid);
        if let Some(period) = claim.rolling_period.filter(|_| claim.rolling > 0) {
            entry.economy.last_rolling = Some(EconomyPayout {
                period,
                amount: rolling,
                claimed: claim.rolling,
            });
            summary.rolling_users += 1;
            summary.rolling_paid += rolling;
        }
        if let Some(period) = claim.cashback_period.filter(|_| claim.cashback > 0) {
            entry.economy.last_cashback = Some(EconomyPayout {
                period,
                amount: cashback,
                claimed: claim.cashback,
            });
            summary.cashback_users += 1;
            summary.cashback_paid += cashback;
        }
    }
    summary
}

/// 구조금 지급 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReliefPaid {
    pub amount: i64,
    /// 그중 금고에서 나간 코인 (나머지는 새로 발행했다).
    pub from_treasury: i64,
    pub balance: i64,
    /// 이 시각(유닉스 ms)까지 선물할 수 없다.
    pub gift_locked_until: i64,
}

/// 구조금: 보유 코인과 테이블 칩을 합쳐 기준 미만이면 하루(한국 시간) 한 번 준다. 금고에서 먼저
/// 꺼내고, 모자라면 새로 발행한다 (잃은 사람이 다시 시작할 수 있어야 하므로 금고가 비어도 준다).
pub fn claim_relief(
    stats: &mut StatsFile,
    user_id: u64,
    name: &str,
    chips_on_table: i64,
    rules: &EconomyRules,
    today: &str,
    now_ms: i64,
) -> Result<ReliefPaid, String> {
    if rules.relief_amount <= 0 {
        return Err("지금은 구조금을 주지 않습니다.".to_string());
    }
    let (coins, claimed_today) = stats
        .users
        .get(&user_id.to_string())
        .map_or((0, false), |entry| {
            (entry.coins, entry.economy.relief_date == today)
        });
    if claimed_today {
        return Err(
            "오늘은 이미 구조금을 받았습니다. 내일(한국 시간 0시 이후) 다시 받을 수 있어요."
                .to_string(),
        );
    }
    let total = coins.saturating_add(chips_on_table.max(0));
    if total >= rules.relief_threshold {
        return Err(format!(
            "보유 코인 {}(테이블 칩 포함)이 기준 {} 이상이라 받을 수 없습니다.",
            coin_text(total),
            coin_text(rules.relief_threshold)
        ));
    }
    let from_treasury = treasury_take(stats, rules.relief_amount);
    let minted = rules.relief_amount - from_treasury;
    stats.treasury.minted_total = stats.treasury.minted_total.saturating_add(minted);
    let lock_ms = rules.relief_gift_lock_hours.clamp(0, 24 * 365) * 3_600_000;
    let entry = ensure_player_stats(stats, user_id, name);
    entry.coins = entry.coins.saturating_add(rules.relief_amount);
    entry.economy.relief_date = today.to_string();
    entry.economy.gift_locked_until = now_ms.saturating_add(lock_ms);
    Ok(ReliefPaid {
        amount: rules.relief_amount,
        from_treasury,
        balance: entry.coins,
        gift_locked_until: entry.economy.gift_locked_until,
    })
}

/// 잭팟 풀에서 `bp`만분율을 꺼낸다 (실제로 꺼낸 코인).
pub fn jackpot_take(stats: &mut StatsFile, bp: i64) -> i64 {
    let treasury = &mut stats.treasury;
    let paid = bp_of(treasury.jackpot, bp).clamp(0, treasury.jackpot.max(0));
    treasury.jackpot -= paid;
    treasury.outflow_total = treasury.outflow_total.saturating_add(paid);
    paid
}

/// 잭팟 지급 한 건.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JackpotShare {
    pub user_id: u64,
    pub name: String,
    pub amount: i64,
}

/// 잭팟을 나눠 준다. 풀의 `jackpot_payout_bp`만큼을 꺼내, 이긴 사람이 있으면(배드비트) 진 사람
/// `jackpot_loser_bp`, 이긴 사람 `jackpot_winner_bp`, 나머지는 같은 핸드 참가자가 나눈다. 이긴 사람이
/// 없으면(수티드 트립스) 부른 사람이 모두 가진다. 참가자가 없으면 그 몫도 부른 사람에게 간다.
/// 나누고 남는 우수리는 풀로 돌려놓는다.
pub fn pay_jackpot(
    stats: &mut StatsFile,
    hitters: &[(u64, String)],
    winners: &[(u64, String)],
    others: &[(u64, String)],
    rules: &EconomyRules,
) -> Vec<JackpotShare> {
    if hitters.is_empty() {
        return Vec::new();
    }
    let pot = jackpot_take(stats, rules.jackpot_payout_bp);
    if pot <= 0 {
        return Vec::new();
    }
    let (hitter_bp, winner_bp) = if winners.is_empty() {
        (BP_SCALE, 0)
    } else {
        (rules.jackpot_loser_bp, rules.jackpot_winner_bp)
    };
    let mut hitter_part = bp_of(pot, hitter_bp);
    let winner_part = bp_of(pot, winner_bp).min(pot - hitter_part);
    let mut others_part = pot - hitter_part - winner_part;
    if others.is_empty() {
        hitter_part += others_part;
        others_part = 0;
    }
    let mut shares: Vec<JackpotShare> = Vec::new();
    let mut paid = 0_i64;
    for (group, part) in [
        (hitters, hitter_part),
        (winners, winner_part),
        (others, others_part),
    ] {
        if group.is_empty() || part <= 0 {
            continue;
        }
        let each = part / group.len() as i64;
        if each <= 0 {
            continue;
        }
        for (user_id, name) in group {
            let entry = ensure_player_stats(stats, *user_id, name);
            entry.coins = entry.coins.saturating_add(each);
            paid += each;
            match shares.iter_mut().find(|share| share.user_id == *user_id) {
                Some(share) => share.amount += each,
                None => shares.push(JackpotShare {
                    user_id: *user_id,
                    name: name.clone(),
                    amount: each,
                }),
            }
        }
    }
    // 나누고 남은 우수리는 풀로 돌려놓는다 (지출에서도 뺀다).
    let leftover = pot - paid;
    stats.treasury.jackpot = stats.treasury.jackpot.saturating_add(leftover);
    stats.treasury.outflow_total = stats.treasury.outflow_total.saturating_sub(leftover);
    shares
}

/// 관리자: 금고나 잭팟 풀에 코인을 새로 넣거나(발행) 빼서 없앤다. 바뀐 뒤 잔액을 돌려준다.
pub fn admin_adjust_treasury(stats: &mut StatsFile, jackpot: bool, delta: i64) -> i64 {
    let treasury = &mut stats.treasury;
    let slot = if jackpot {
        &mut treasury.jackpot
    } else {
        &mut treasury.balance
    };
    *slot = slot.saturating_add(delta).max(0);
    *slot
}

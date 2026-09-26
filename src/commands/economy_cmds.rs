// commands/economy_cmds.rs — 코인 순환: /금고(현황), /구조금, /금고관리(관리자), 환급 정산 작업

use super::*;

#[poise::command(
    slash_command,
    rename = "금고",
    description_localized("ko", "복지 금고·잭팟과 내 롤링·손실 환급, 구조금 조건을 봅니다.")
)]
pub async fn treasury_info(ctx: Context<'_>) -> Result<(), Error> {
    let rules = ctx.data().config.read().await.economy_rules();
    let user_id = ctx.author().id.get();
    let today = stats::kst_today();
    let week = stats::kst_week();
    let message = {
        let stats_read = ctx.data().stats.read().await;
        let treasury = stats_read.treasury.clone();
        let economy = stats_read
            .users
            .get(&user_id.to_string())
            .map(|entry| entry.economy.clone())
            .unwrap_or_default();
        treasury_text(&treasury, &economy, &rules, &today, &week)
    };
    reply_embed(ctx, message, "복지 금고", serenity::Colour::GOLD, true).await?;
    Ok(())
}

/// `/금고` 본문.
pub(crate) fn treasury_text(
    treasury: &stats::Treasury,
    economy: &stats::EconomyRecord,
    rules: &stats::EconomyRules,
    today: &str,
    week: &str,
) -> String {
    let rolling_today = economy.rolling.get(today).copied().unwrap_or(0);
    let rolling_rebate =
        stats::bp_of(rolling_today, rules.daily_rolling_bp).min(rules.daily_rolling_cap);
    let week_net = economy.house_net.get(week).copied().unwrap_or(0);
    let cashback = stats::bp_of(week_net.saturating_neg(), rules.weekly_cashback_bp)
        .min(rules.weekly_cashback_cap);
    let mut lines = vec![
        format!(
            "**복지 금고** {} · **잭팟** {}",
            stats::coin_text(treasury.balance),
            stats::coin_text(treasury.jackpot)
        ),
        format!(
            "금고는 블랙잭 하우스 수익·홀덤 레이크({}, 핸드당 최대 {})·코인 선물 수수료({})·마피아 배팅으로 잃은 코인의 {}로 채워지고, 들어온 코인의 {}는 잭팟에 쌓입니다. 환급·구조금·스타플레이어 상금은 금고에서 나갑니다.",
            stats::bp_text(rules.holdem_rake_bp),
            stats::coin_text(rules.holdem_rake_cap),
            stats::bp_text(rules.gift_fee_bp),
            stats::bp_text(rules.bet_loss_treasury_bp),
            stats::bp_text(rules.jackpot_share_bp),
        ),
        String::new(),
        "**내 환급**".to_string(),
        format!(
            "- 오늘 카지노 롤링 {} → 내일 0시 롤링 환급 예상 **{}** (롤링의 {}, 하루 최대 {})",
            stats::coin_text(rolling_today),
            stats::coin_text(rolling_rebate),
            stats::bp_text(rules.daily_rolling_bp),
            stats::coin_text(rules.daily_rolling_cap)
        ),
        format!(
            "- 이번 주 하우스 게임(블랙잭·마피아 배팅) 손익 {} → 다음 주 월요일 손실 환급 예상 **{}** (손실의 {}, 최대 {})",
            stats::signed_coin_text(week_net),
            stats::coin_text(cashback),
            stats::bp_text(rules.weekly_cashback_bp),
            stats::coin_text(rules.weekly_cashback_cap)
        ),
    ];
    let mut last = Vec::new();
    if let Some(payout) = &economy.last_rolling {
        last.push(payout_text("롤링", payout));
    }
    if let Some(payout) = &economy.last_cashback {
        last.push(payout_text("손실", payout));
    }
    if !last.is_empty() {
        lines.push(format!("- 지난 환급: {}", last.join(" · ")));
    }
    lines.push(
        "홀덤 롤링은 레이크를 뗀 핸드만 셉니다. 금고가 모자라면 모두 같은 비율로 줄어듭니다."
            .to_string(),
    );
    lines.push(String::new());
    lines.push(format!(
        "**구조금** 보유 코인·테이블 칩·주식 평가액을 합쳐 {} 미만이면 하루 한 번 {}을 받을 수 있습니다 (`/구조금`). 받은 뒤 {}시간 동안은 코인을 선물할 수 없습니다.",
        stats::coin_text(rules.relief_threshold),
        stats::coin_text(rules.relief_amount),
        rules.relief_gift_lock_hours
    ));
    lines.push(format!(
        "**잭팟** 홀덤에서 포카드 이상으로 지거나(배드비트) 블랙잭 21+3 수티드 트립스가 나오면 잭팟의 {}를 나눠 드립니다. 배드비트는 진 사람 {}, 이긴 사람 {}, 나머지는 같은 핸드 참가자에게 갑니다.",
        stats::bp_text(rules.jackpot_payout_bp),
        stats::bp_text(rules.jackpot_loser_bp),
        stats::bp_text(rules.jackpot_winner_bp)
    ));
    lines.join("\n")
}

fn payout_text(label: &str, payout: &stats::EconomyPayout) -> String {
    if payout.claimed > payout.amount {
        format!(
            "{label} {} +{} (금고 부족으로 {} 중)",
            payout.period,
            stats::coin_text(payout.amount),
            stats::coin_text(payout.claimed)
        )
    } else {
        format!(
            "{label} {} +{}",
            payout.period,
            stats::coin_text(payout.amount)
        )
    }
}

#[poise::command(
    slash_command,
    rename = "구조금",
    description_localized(
        "ko",
        "코인을 거의 다 잃었을 때 하루 한 번 다시 시작할 코인을 받습니다."
    )
)]
pub async fn relief_command(ctx: Context<'_>) -> Result<(), Error> {
    const TITLE: &str = "구조금";
    let rules = ctx.data().config.read().await.economy_rules();
    let user = ctx.author();
    let user_id = user.id.get();
    // 테이블 칩과 주식(평가액·주문 증거금)도 재산으로 센다 (코인을 옮겨 두고 받지 못하게).
    let chips = ctx
        .data()
        .casino
        .chips_on_table(user_id)
        .await
        .saturating_add(ctx.data().stocks.portfolio_value(user_id).await);
    let today = stats::kst_today();
    let now_ms = chrono::Utc::now().timestamp_millis();
    let applied = {
        let mut stats_file = ctx.data().stats.write().await;
        stats::claim_relief(
            &mut stats_file,
            user_id,
            &user.name,
            chips,
            &rules,
            &today,
            now_ms,
        )
        .map(|paid| (paid, stats_file.clone()))
    };
    let (paid, snapshot) = match applied {
        Ok(applied) => applied,
        Err(message) => {
            reply_embed(ctx, message, TITLE, serenity::Colour::RED, true).await?;
            return Ok(());
        }
    };
    save_stats_snapshot(ctx.data(), snapshot).await;
    eprintln!(
        "relief: user={user_id} amount={} from_treasury={} balance={}",
        paid.amount, paid.from_treasury, paid.balance
    );
    reply_embed(
        ctx,
        format!(
            "구조금 **{}**을 받았습니다.\n보유 코인: **{}**\n{}까지는 코인을 선물할 수 없습니다.",
            stats::coin_text(paid.amount),
            stats::coin_text(paid.balance),
            stats::kst_time_text(paid.gift_locked_until)
        ),
        TITLE,
        serenity::Colour::DARK_GREEN,
        true,
    )
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum TreasuryAdminAction {
    #[name = "넣기"]
    Deposit,
    #[name = "빼기"]
    Withdraw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum TreasuryTarget {
    #[name = "금고"]
    Treasury,
    #[name = "잭팟"]
    Jackpot,
}

/// 관리자: 금고·잭팟에 코인을 새로 넣거나(처음 씨앗 등) 빼서 없앤다. 서버 로그에 남긴다.
#[poise::command(
    slash_command,
    rename = "금고관리",
    description_localized("ko", "관리자: 복지 금고나 잭팟에 코인을 넣거나 뺍니다.")
)]
pub async fn manage_treasury(
    ctx: Context<'_>,
    #[description = "동작"] 동작: TreasuryAdminAction,
    #[description = "대상"] 대상: TreasuryTarget,
    #[description = "금액(원)"]
    #[min = 1]
    금액: i64,
) -> Result<(), Error> {
    const TITLE: &str = "금고 관리";
    if !require_manager(ctx).await? {
        return Ok(());
    }
    if 금액 <= 0 {
        reply_embed(
            ctx,
            "금액은 1원 이상이어야 합니다.",
            TITLE,
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    }
    let jackpot = 대상 == TreasuryTarget::Jackpot;
    let delta = match 동작 {
        TreasuryAdminAction::Deposit => 금액,
        TreasuryAdminAction::Withdraw => -금액,
    };
    let (before, after, snapshot) = {
        let mut stats_file = ctx.data().stats.write().await;
        let before = if jackpot {
            stats_file.treasury.jackpot
        } else {
            stats_file.treasury.balance
        };
        let after = stats::admin_adjust_treasury(&mut stats_file, jackpot, delta);
        (before, after, stats_file.clone())
    };
    save_stats_snapshot(ctx.data(), snapshot).await;
    let target = if jackpot { "잭팟" } else { "복지 금고" };
    let text = format!(
        "{target}: {} → **{}** ({})",
        stats::coin_text(before),
        stats::coin_text(after),
        stats::signed_coin_text(after - before)
    );
    let log_channel_id = ctx.data().config.read().await.log_channel_id;
    send_admin_log(
        ctx.http(),
        log_channel_id,
        TITLE,
        format!("{} 님이 {text}", ctx.author().name),
    )
    .await;
    reply_embed(ctx, text, TITLE, serenity::Colour::DARK_GREEN, false).await?;
    Ok(())
}

/// 1분마다 지난 날짜의 롤링 환급과 지난주의 손실 환급을 준다 (기간마다 한 번). 봇이 꺼져 있던
/// 동안 밀린 기간도 켜지자마자 준다.
pub async fn run_economy_ticker(ctx: serenity::Context, data: Data) {
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        let rules = data.config.read().await.economy_rules();
        let today = stats::kst_today();
        let week = stats::kst_week();
        let (summary, snapshot) = {
            let mut stats_file = data.stats.write().await;
            let summary = stats::run_economy_payouts(&mut stats_file, &rules, &today, &week);
            let snapshot = summary.changed.then(|| stats_file.clone());
            (summary, snapshot)
        };
        let Some(snapshot) = snapshot else {
            continue;
        };
        save_stats_snapshot(&data, snapshot).await;
        if summary.claimed == 0 {
            continue;
        }
        let paid = summary.rolling_paid + summary.cashback_paid;
        let mut text = format!(
            "롤링 환급 {}명 {} · 손실 환급 {}명 {} (합계 {})",
            summary.rolling_users,
            stats::coin_text(summary.rolling_paid),
            summary.cashback_users,
            stats::coin_text(summary.cashback_paid),
            stats::coin_text(paid)
        );
        if summary.shortfall {
            text.push_str(&format!(
                "\n금고가 모자라 {} 중 {}만 비율대로 나눠 줬습니다.",
                stats::coin_text(summary.claimed),
                stats::coin_text(paid)
            ));
        }
        eprintln!("economy payouts: {text}");
        let log_channel_id = data.config.read().await.log_channel_id;
        send_admin_log(&ctx.http, log_channel_id, "코인 환급", text).await;
    }
}

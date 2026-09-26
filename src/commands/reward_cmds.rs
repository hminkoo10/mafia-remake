// commands/reward_cmds.rs — 코인 벌이: /미션(오늘의 미션 진행·보상 받기), /업적(업적 목록·보상 받기).
// 보상은 명령을 쓰면 끝낸 것만 바로 받는다 (받은 것은 다시 주지 않는다).

use super::*;
use crate::stock_hub::now_ms;

/// 주식 시장에서 세는 값: 오늘 체결 수와 업적용 거래·설립·상장 수.
async fn stock_progress(data: &Data, user_id: u64) -> (i64, stats::StockProgress) {
    let market = data.stocks.market.read().await;
    let day_start = market.clock(stats::kst_day_start_ms(now_ms()));
    let account = market.accounts.get(&user_id);
    let today = account.map_or(0, |account| {
        account
            .fills
            .iter()
            .filter(|fill| fill.at >= day_start)
            .count() as i64
    });
    let founded = market
        .companies
        .values()
        .filter(|company| company.founder() == Some(user_id));
    let progress = stats::StockProgress {
        // 누적 체결 수를 세기 전에 거래한 사람은 남아 있는 체결 기록 수부터 시작한다.
        trades: account.map_or(0, |account| account.trades.max(account.fills.len() as i64)),
        realized: account.map_or(0, |account| account.realized),
        founded: founded.clone().count() as i64,
        listed: founded.filter(|company| company.listed_at > 0).count() as i64,
    };
    (today, progress)
}

fn claim_text(claim: &stats::Claim) -> Option<String> {
    (claim.total > 0).then(|| {
        let items = claim
            .paid
            .iter()
            .map(|(title, amount)| format!("{title} +{}", stats::coin_text(*amount)))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "🎁 **{} 받았습니다** ({items})\n보유 코인: **{}**",
            stats::coin_text(claim.total),
            stats::coin_text(claim.balance)
        )
    })
}

#[poise::command(
    slash_command,
    rename = "미션",
    description_localized("ko", "오늘의 미션 진행을 보고, 끝낸 미션의 보상을 받습니다.")
)]
pub async fn missions(ctx: Context<'_>) -> Result<(), Error> {
    let user = ctx.author();
    let user_id = user.id.get();
    let rules = ctx.data().config.read().await.reward_rules();
    let today = stats::kst_today();
    let (trades_today, _) = stock_progress(ctx.data(), user_id).await;
    let (statuses, claim, bonus_claimed, rewarded_games, snapshot) = {
        let mut stats_file = ctx.data().stats.write().await;
        let (statuses, claim) = stats::claim_missions(
            &mut stats_file,
            user_id,
            &user.name,
            &today,
            trades_today,
            &rules,
        );
        let entry = stats_file.users.get(&user_id.to_string());
        let bonus_claimed = stats::mission_bonus_claimed(entry, &today);
        let rewarded_games = entry
            .filter(|entry| entry.rewards.day == today)
            .map_or(0, |entry| entry.rewards.day_rewarded_games);
        let snapshot = (claim.total > 0).then(|| stats_file.clone());
        (statuses, claim, bonus_claimed, rewarded_games, snapshot)
    };
    if let Some(snapshot) = snapshot {
        save_stats_snapshot(ctx.data(), snapshot).await;
        let items = claim
            .paid
            .iter()
            .map(|(title, amount)| format!("{title} +{}", stats::coin_text(*amount)))
            .collect::<Vec<_>>()
            .join(", ");
        ctx.data().audit.push(
            crate::audit_log::COINS,
            format!(
                "🎯 {} 미션 보상 {} ({items}, 보유 코인 {})",
                user.name,
                stats::coin_text(claim.total),
                stats::coin_text(claim.balance)
            ),
        );
    }
    let mut lines = Vec::new();
    if rules.mission_coins <= 0 && rules.mission_bonus <= 0 {
        lines.push("지금은 일일 미션 보상이 꺼져 있습니다.".to_string());
    }
    for status in &statuses {
        let icon = if status.claimed {
            "✅"
        } else if status.done {
            "🎁"
        } else {
            "⬜"
        };
        lines.push(format!(
            "{icon} **{}** {} — {}/{}{}",
            status.mission.category,
            status.mission.title,
            status.progress.min(status.mission.target),
            status.mission.target,
            if status.claimed {
                " (받음)".to_string()
            } else {
                format!(" (+{})", stats::coin_text(rules.mission_coins))
            }
        ));
    }
    lines.push(if bonus_claimed {
        "✅ 세 미션 모두 달성 보너스 (받음)".to_string()
    } else {
        format!(
            "⭐ 세 미션을 모두 끝내면 보너스 **+{}**",
            stats::coin_text(rules.mission_bonus)
        )
    });
    if let Some(text) = claim_text(&claim) {
        lines.push(String::new());
        lines.push(text);
    }
    lines.push(String::new());
    if rules.game_coins > 0 {
        lines.push(format!(
            "🎮 마피아 참여 보상: 판을 끝까지 하면 {}, 이기면 +{} (하루 {}판까지, 오늘 {}판 받음)",
            stats::coin_text(rules.game_coins),
            stats::coin_text(rules.win_coins),
            rules.daily_games,
            rewarded_games.min(rules.daily_games)
        ));
    }
    lines.push(
        "미션은 한국 시간 자정에 바뀌고, 끝낸 미션은 `/미션`을 쓰면 바로 받습니다. 업적은 `/업적`에서 봅니다."
            .to_string(),
    );
    reply_embed(
        ctx,
        lines.join("\n"),
        "오늘의 미션",
        serenity::Colour::DARK_GREEN,
        true,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "업적",
    description_localized("ko", "업적 목록을 보고, 새로 이룬 업적의 보상을 받습니다.")
)]
pub async fn achievements(ctx: Context<'_>) -> Result<(), Error> {
    let user = ctx.author();
    let user_id = user.id.get();
    let rules = ctx.data().config.read().await.reward_rules();
    let (_, stock) = stock_progress(ctx.data(), user_id).await;
    let (statuses, claim, earned, snapshot) = {
        let mut stats_file = ctx.data().stats.write().await;
        let (statuses, claim) =
            stats::claim_achievements(&mut stats_file, user_id, &user.name, stock, &rules);
        let earned = stats_file
            .users
            .get(&user_id.to_string())
            .map_or(0, |entry| entry.rewards.earned);
        let snapshot = (claim.total > 0).then(|| stats_file.clone());
        (statuses, claim, earned, snapshot)
    };
    if let Some(snapshot) = snapshot {
        save_stats_snapshot(ctx.data(), snapshot).await;
        let items = claim
            .paid
            .iter()
            .map(|(title, amount)| format!("{title} +{}", stats::coin_text(*amount)))
            .collect::<Vec<_>>()
            .join(", ");
        ctx.data().audit.push(
            crate::audit_log::COINS,
            format!(
                "🏆 {} 업적 보상 {} ({items}, 보유 코인 {})",
                user.name,
                stats::coin_text(claim.total),
                stats::coin_text(claim.balance)
            ),
        );
    }
    let done = statuses.iter().filter(|status| status.claimed).count();
    let mut lines = vec![format!(
        "업적 **{done}/{}**단계 · 코인 벌기로 받은 누적 **{}**",
        statuses.len(),
        stats::coin_text(earned)
    )];
    if rules.achievement_pct <= 0 {
        lines.push(
            "지금은 업적 보상이 꺼져 있습니다 (이룬 업적은 켜지면 받을 수 있습니다).".to_string(),
        );
    }
    if let Some(text) = claim_text(&claim) {
        lines.push(text);
    }
    lines.push(String::new());
    // 트랙마다 한 줄: 받은 단계와 다음 목표·보상.
    for track in stats::ACHIEVEMENT_TRACKS {
        let tiers = statuses
            .iter()
            .filter(|status| std::ptr::eq(status.track, track))
            .collect::<Vec<_>>();
        let claimed = tiers.iter().filter(|status| status.claimed).count();
        match tiers.iter().find(|status| !status.claimed) {
            Some(next) => {
                let icon = if next.done { "🎁" } else { "⬜" };
                lines.push(format!(
                    "{icon} **{}** {claimed}/{}단계 · 다음 {} ({}/{}) **+{}**",
                    track.title,
                    tiers.len(),
                    next.title,
                    count_text(next.progress.min(next.target)),
                    count_text(next.target),
                    stats::coin_text(next.reward)
                ));
            }
            None => lines.push(format!(
                "🏅 **{}** 모든 단계 달성 ({}/{})",
                track.title,
                tiers.len(),
                tiers.len()
            )),
        }
    }
    lines.push(String::new());
    lines.push(
        "업적은 단계마다 한 번씩 받고, `/업적`을 쓰면 새로 이룬 단계의 보상을 바로 받습니다. 팀별 승리와 출석·미션 일수는 업적 개편 뒤부터 셉니다."
            .to_string(),
    );
    reply_embed(ctx, lines.join("\n"), "업적", serenity::Colour::GOLD, true).await?;
    Ok(())
}

/// "12,345" (단위 없이).
fn count_text(value: i64) -> String {
    stats::coin_text(value).trim_end_matches('원').to_string()
}

/// 판 결과 뒤에 붙이는 참여 보상 안내 (보상이 없으면 None).
pub(crate) fn game_reward_text(
    rewards: &[stats::GameReward],
    rules: &stats::RewardRules,
) -> Option<String> {
    if rules.game_coins <= 0 || rewards.is_empty() {
        return None;
    }
    let items = rewards
        .iter()
        .map(|reward| {
            if reward.amount > 0 {
                format!("{} +{}", reward.name, stats::coin_text(reward.amount))
            } else {
                format!("{} (오늘 한도)", reward.name)
            }
        })
        .collect::<Vec<_>>()
        .join(" · ");
    Some(format!(
        "판을 끝까지 한 사람 {}, 이긴 쪽 +{} (하루 {}판까지)\n{items}",
        stats::coin_text(rules.game_coins),
        stats::coin_text(rules.win_coins),
        rules.daily_games
    ))
}

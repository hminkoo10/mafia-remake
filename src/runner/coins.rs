// runner/coins.rs — 배팅 공개·정산 안내와 스타플레이어 투표

use super::*;

const STAR_VOTE_SECONDS: u64 = 10;

/// 익명 게임에서도 실명을 쓴다 (배팅은 별명과 무관한 개인 설정이고, 참가자 명단은
/// 모집 때 이미 공개돼 있다).
fn real_player_name(running: &RunningGame, player: &Player) -> String {
    running
        .anonymous_original_names
        .get(&player.user_id)
        .cloned()
        .unwrap_or_else(|| player.name.clone())
}

/// 게임 시작 직후 참가자 전원의 배팅액을 공개한다.
pub async fn announce_bets(ctx: &serenity::Context, running: &Arc<RwLock<RunningGame>>) {
    let mut lines = {
        let running_read = running.read().await;
        running_read
            .game
            .players
            .iter()
            .map(|player| {
                let name = real_player_name(&running_read, player);
                let bet = running_read.bets.get(&player.user_id).copied().unwrap_or(0);
                let line = if bet > 0 {
                    format!("{name}: **{}**", stats::coin_text(bet))
                } else {
                    format!("{name}: 배팅 없음")
                };
                (name.to_lowercase(), line)
            })
            .collect::<Vec<_>>()
    };
    lines.sort();
    let body = lines
        .into_iter()
        .map(|(_, line)| line)
        .collect::<Vec<_>>()
        .join("\n");
    if let Err(error) = send_game_embed(
        ctx,
        running,
        format!("이번 판 배팅액입니다. 승패와 배율에 따라 게임이 끝난 뒤 정산됩니다.\n\n{body}"),
        "배팅 현황",
        serenity::Colour::GOLD,
        vec![],
        false,
        true,
    )
    .await
    {
        eprintln!("failed to announce bets: {error:?}");
    }
}

pub fn star_vote_components(
    guild_id: serenity::GuildId,
    candidates: &[(u64, String)],
    disabled: bool,
) -> Vec<serenity::CreateActionRow> {
    let options = candidates
        .iter()
        .take(25)
        .map(|(user_id, name)| {
            serenity::CreateSelectMenuOption::new(
                name.chars().take(100).collect::<String>(),
                user_id.to_string(),
            )
        })
        .collect::<Vec<_>>();
    let select = serenity::CreateSelectMenu::new(
        format!("starvote:{}", guild_id.get()),
        serenity::CreateSelectMenuKind::String { options },
    )
    .placeholder("스타플레이어를 선택하세요 (본인 제외)")
    .min_values(1)
    .max_values(1)
    .disabled(disabled);
    vec![serenity::CreateActionRow::SelectMenu(select)]
}

/// 판이 끝난 뒤 알릴 코인 벌이 (참여 보상, 이 판으로 오늘의 마피아 미션을 끝낸 사람).
#[derive(Debug, Default)]
pub struct GameRewardNotice {
    pub rewards: Vec<stats::GameReward>,
    pub rules: stats::RewardRules,
    pub mission_done: Vec<String>,
}

/// 게임 결과 발표 뒤: 배팅 정산 안내 → 참여 보상 → 스타플레이어 투표(10초) → 상금 지급.
pub async fn announce_coin_results(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
    settlements: &[stats::BetSettlement],
    rewards: &GameRewardNotice,
) {
    if !settlements.is_empty() {
        let lines = settlements
            .iter()
            .map(|item| {
                format!(
                    "{}: **{}** (배팅 {} × {:.2} — {}) → 보유 {}",
                    item.name,
                    stats::signed_coin_text(item.delta),
                    stats::coin_text(item.bet),
                    item.multiplier,
                    item.factors.join(" / "),
                    stats::coin_text(item.balance)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        if let Err(error) = send_game_embed(
            ctx,
            running,
            lines,
            "배팅 정산",
            serenity::Colour::GOLD,
            vec![],
            true,
            true,
        )
        .await
        {
            eprintln!("failed to announce bet settlements: {error:?}");
        }
    }
    if let Some(mut text) = crate::commands::game_reward_text(&rewards.rewards, &rewards.rules) {
        if !rewards.mission_done.is_empty() {
            text.push_str(&format!(
                "\n\n🎯 오늘의 마피아 미션 달성: {} — `/미션`으로 보상을 받으세요.",
                rewards.mission_done.join(", ")
            ));
        }
        if let Err(error) = send_game_embed(
            ctx,
            running,
            text,
            "참여 보상",
            serenity::Colour::DARK_GREEN,
            vec![],
            true,
            true,
        )
        .await
        {
            eprintln!("failed to announce game rewards: {error:?}");
        }
    }
    run_star_player_vote(ctx, data, running).await;
}

async fn run_star_player_vote(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) {
    let prize_total = data.config.read().await.star_player_coins;
    let (guild_id, candidates, notify) = {
        let mut running_write = running.write().await;
        let mut candidates = running_write
            .game
            .players
            .iter()
            .map(|player| {
                (
                    player.user_id,
                    game_result_display_name(&running_write, player),
                )
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|(_, name)| name.to_lowercase());
        running_write.star_votes.clear();
        running_write.star_vote_open = candidates.len() >= 2;
        (
            running_write.guild_id,
            candidates,
            running_write.star_vote_notify.clone(),
        )
    };
    if candidates.len() < 2 {
        return;
    }
    let vote_message = send_game_embed(
        ctx,
        running,
        format!(
            "이번 판 최고의 활약을 보여준 플레이어를 {STAR_VOTE_SECONDS}초 동안 골라 주세요 (참가자만, 본인 제외).\n가장 많은 표를 받은 사람에게 **{}**을 복지 금고에서 드립니다 (금고가 모자라면 남은 만큼). 동표면 나눠 드립니다.",
            stats::coin_text(prize_total)
        ),
        "스타플레이어 투표",
        serenity::Colour::GOLD,
        star_vote_components(guild_id, &candidates, false),
        true,
        true,
    )
    .await;
    let deadline = Instant::now() + Duration::from_secs(STAR_VOTE_SECONDS);
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                break;
            }
            _ = notify.notified() => {
                let running_read = running.read().await;
                let all_voted = running_read
                    .game
                    .players
                    .iter()
                    .all(|player| running_read.star_votes.contains_key(&player.user_id));
                if all_voted {
                    break;
                }
            }
        }
    }
    let votes = {
        let mut running_write = running.write().await;
        running_write.star_vote_open = false;
        running_write.star_votes.clone()
    };
    if let Ok(mut message) = vote_message {
        let _ = message
            .edit(
                &ctx.http,
                serenity::EditMessage::new().components(star_vote_components(
                    guild_id,
                    &candidates,
                    true,
                )),
            )
            .await;
    }
    let winners = {
        let running_read = running.read().await;
        stats::tally_star_votes(&votes)
            .into_iter()
            .filter_map(|(user_id, count)| {
                let player = running_read.game.get_player(user_id)?;
                Some((user_id, real_player_name(&running_read, player), count))
            })
            .collect::<Vec<_>>()
    };
    if winners.is_empty() {
        let _ = send_game_embed(
            ctx,
            running,
            "투표가 없어 이번 판 스타플레이어는 없습니다.",
            "스타플레이어",
            serenity::Colour::GOLD,
            vec![],
            true,
            true,
        )
        .await;
        return;
    }
    let (awards, snapshot) = {
        let mut stats_file = data.stats.write().await;
        let awards = stats::award_star_players(&mut stats_file, &winners, prize_total);
        (awards, stats_file.clone())
    };
    let stats_path = data.stats_path.clone();
    match tokio::task::spawn_blocking(move || stats::save_stats(&*stats_path, &snapshot)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("failed to save stats after star vote: {error:?}"),
        Err(error) => eprintln!("failed to join stats save task after star vote: {error:?}"),
    }
    {
        let mut running_write = running.write().await;
        let winner_ids = awards.iter().map(|award| award.user_id).collect::<Vec<_>>();
        running_write.record_replay_event(
            "star_player",
            None,
            &winner_ids,
            serde_json::json!({
                "votes": votes.len(),
                "prize_each": awards.first().map_or(0, |award| award.prize),
            }),
        );
    }
    let display_names = candidates.into_iter().collect::<HashMap<_, _>>();
    let mut lines = awards
        .iter()
        .map(|award| {
            format!(
                "🌟 **{}** ({}표) {} → 보유 {}",
                display_names
                    .get(&award.user_id)
                    .cloned()
                    .unwrap_or_else(|| award.name.clone()),
                award.votes,
                stats::signed_coin_text(award.prize),
                stats::coin_text(award.balance)
            )
        })
        .collect::<Vec<_>>();
    let paid_total = awards.iter().map(|award| award.prize).sum::<i64>();
    if prize_total > 0 && paid_total == 0 {
        lines.push("복지 금고가 비어 있어 이번 상금은 드리지 못했습니다.".to_string());
    } else if awards.len() > 1 {
        lines.push(format!(
            "동표로 {}명이 {}을 나눠 받았습니다.",
            awards.len(),
            stats::coin_text(paid_total)
        ));
    }
    let _ = send_game_embed(
        ctx,
        running,
        lines.join("\n"),
        "스타플레이어",
        serenity::Colour::GOLD,
        vec![],
        true,
        true,
    )
    .await;
}

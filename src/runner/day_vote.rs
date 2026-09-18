// runner/day_vote.rs — 낮·투표·찬반투표 진행

use super::*;

pub async fn run_day(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) -> Result<()> {
    let config = data.config.read().await.clone();
    let (
        guild_id,
        day_notify,
        discussion_seconds,
        hackers,
        vigilantes,
        psychologists,
        hypnotists,
        mercenary_contracts,
    ) = {
        let mut running_write = running.write().await;
        running_write.game.phase = Phase::Day;
        running_write.phase_deadline =
            Some(Instant::now() + Duration::from_secs(config.discussion_seconds));
        running_write.day_chat_open = true;
        running_write.final_defense_user_id = None;
        running_write.day_skip_voter_ids.clear();
        running_write.day_skip_confirmed = false;
        running_write.day_extension_voter_ids.clear();
        running_write.day_extension_active = false;
        running_write.day_extension_confirmed = false;
        let mercenary_contracts = running_write.game.receive_mercenary_contracts();
        running_write.record_replay_event(
            "phase_started",
            None,
            &[],
            serde_json::json!({
                "phase": "day",
                "duration_seconds": config.discussion_seconds,
                "mercenary_contract_count": mercenary_contracts.len(),
            }),
        );
        (
            running_write.guild_id,
            running_write.day_notify.clone(),
            config.discussion_seconds,
            running_write.game.hacker_day_actors(),
            running_write.game.vigilante_day_actors(),
            running_write.game.psychologist_day_actors(),
            running_write.game.hypnotist_day_actors(),
            mercenary_contracts,
        )
    };
    unlock_pending_dead_chats(ctx, data, running).await;
    upsert_game_status(ctx, running).await;
    // 밤 동안의 [확성] 개인 허용을 원상 복구한 뒤 낮 채팅을 연다.
    restore_member_game_channel_chat(ctx, running).await;
    // 마녀 저주(개구리) 차단을 낮 채팅이 열리기 전에 다시 확실히 건다.
    // 위 복구가 저주 차단과 같은 멤버 오버라이트를 되돌릴 수 있고, 차단이
    // 빠지면 개구리의 메시지가 잠깐 보였다가 지워지는 흐름이 된다.
    let frogs = {
        let running_read = running.read().await;
        running_read
            .game
            .players
            .iter()
            .filter(|player| running_read.game.is_frog(player))
            .cloned()
            .collect::<Vec<_>>()
    };
    for player in &frogs {
        deny_frog_game_channel_chat(ctx, running, player).await;
    }
    set_game_channel_chat(ctx, data, running, true).await;
    set_channel_slowmode(ctx, running, config.chat_slowmode_seconds).await;
    sync_private_role_chat_permissions(ctx, data, running).await;
    sync_lover_chat_access(ctx, data, running).await;
    sync_cult_team_channel_access(ctx, data, running).await;
    sync_madam_seduction_permissions(ctx, running).await;
    sync_shaman_chat_access(ctx, data, running).await;
    unlock_pending_dead_chats(ctx, data, running).await;
    for (mercenary, client) in &mercenary_contracts {
        let _ = send_player_secret(
            ctx,
            running,
            mercenary,
            mercenary_contract_received_message(),
            vec![],
        )
        .await;
        let _ = send_player_secret(
            ctx,
            running,
            client,
            format!(
                "[의뢰] 당신은 용병에게 의뢰했습니다. 용병은 **{}** 님입니다.",
                mercenary.name
            ),
            vec![],
        )
        .await;
    }
    let discussion_time = duration_text(discussion_seconds);
    let public_status = running.read().await.game.public_status();
    let mut day_message = send_game_embed(
        ctx,
        running,
        format!(
            "{}일차 낮입니다. {discussion_time} 동안 자유롭게 토론하세요.\n생존자 과반이 `바로 투표`를 누르면 토론과 연장을 끝내고 바로 지목 투표로 넘어갑니다.\n시간이 지나면 {DAY_EXTENSION_VOTE_SECONDS}초 동안 1분 연장 투표가 열립니다. 생존자 과반수가 연장을 누르면 1분 연장되고, 연장은 낮마다 1번만 가능합니다. 과반수가 모이지 않으면 바로 투표로 넘어갑니다.\n{public_status}",
            running.read().await.game.day_number
        ),
        "낮 토론",
        serenity::Colour::GOLD,
        day_skip_components(guild_id, false, false),
        false,
        true,
    )
    .await?;
    let mut failed_hackers = Vec::new();
    for actor in hackers {
        if !send_day_single_select(ctx, running, &actor, "hacker", "해킹 대상을 선택하세요").await
        {
            failed_hackers.push(actor.name);
        }
    }
    if !failed_hackers.is_empty() {
        let channel_id = running.read().await.channel_id;
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            format!(
                "해커 낮 행동 DM을 보낼 수 없는 참가자: {}",
                failed_hackers.join(", ")
            ),
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
        )
        .await;
    }
    let mut failed_vigilantes = Vec::new();
    for actor in vigilantes {
        if !send_day_single_select(
            ctx,
            running,
            &actor,
            "vigilante",
            "숙청 조사 대상을 선택하세요",
        )
        .await
        {
            failed_vigilantes.push(actor.name);
        }
    }
    if !failed_vigilantes.is_empty() {
        let channel_id = running.read().await.channel_id;
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            format!(
                "자경단원 낮 행동 DM을 보낼 수 없는 참가자: {}",
                failed_vigilantes.join(", ")
            ),
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
        )
        .await;
    }
    let mut failed_psychologists = Vec::new();
    for actor in psychologists {
        if !send_day_multi_select(
            ctx,
            running,
            &actor,
            "psychologist",
            "관찰할 두 명을 선택하세요",
            2,
        )
        .await
        {
            failed_psychologists.push(actor.name);
        }
    }
    if !failed_psychologists.is_empty() {
        let channel_id = running.read().await.channel_id;
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            format!(
                "심리학자 낮 행동 선택지를 보낼 수 없는 참가자: {}",
                failed_psychologists.join(", ")
            ),
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
        )
        .await;
    }
    let mut failed_hypnotists = Vec::new();
    for actor in hypnotists {
        if !send_day_button_action(
            ctx,
            running,
            &actor,
            "hypnotist",
            "최면을 해제하려면 버튼을 누르세요.",
            "최면 해제",
        )
        .await
        {
            failed_hypnotists.push(actor.name);
        }
    }
    if !failed_hypnotists.is_empty() {
        let channel_id = running.read().await.channel_id;
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            format!(
                "최면술사 낮 행동 버튼을 보낼 수 없는 참가자: {}",
                failed_hypnotists.join(", ")
            ),
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
        )
        .await;
    }
    let mut extension_used = false;
    let mut current_discussion_seconds = discussion_seconds;
    let mut discussion_deadline = Instant::now() + Duration::from_secs(current_discussion_seconds);
    loop {
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(discussion_deadline)) => {
                    break;
                }
                _ = day_notify.notified() => {
                    let running_read = running.read().await;
                    if running_read.game.phase == Phase::Ended || running_read.day_skip_confirmed {
                        break;
                    }
                }
            }
        }
        {
            let running_read = running.read().await;
            if running_read.game.phase == Phase::Ended || running_read.day_skip_confirmed {
                let _ = day_message
                    .edit(
                        &ctx.http,
                        serenity::EditMessage::new()
                            .components(day_skip_components(guild_id, true, true)),
                    )
                    .await;
                return Ok(());
            }
        }
        if extension_used {
            send_game_embed(
                ctx,
                running,
                "연장된 토론 시간이 종료되었습니다.\n토론 연장은 낮마다 1번만 가능하므로 바로 지목 투표로 넘어갑니다.",
                "낮 토론 종료",
                serenity::Colour::GOLD,
                vec![],
                false,
                true,
            )
            .await?;
            let _ = day_message
                .edit(
                    &ctx.http,
                    serenity::EditMessage::new()
                        .components(day_skip_components(guild_id, true, false)),
                )
                .await;
            return Ok(());
        }

        let (alive_count, required_votes) = {
            let mut running_write = running.write().await;
            let alive_count = running_write.game.alive_players().len();
            running_write.day_extension_voter_ids.clear();
            running_write.day_extension_active = true;
            running_write.day_extension_confirmed = false;
            running_write.phase_deadline =
                Some(Instant::now() + Duration::from_secs(DAY_EXTENSION_VOTE_SECONDS));
            (alive_count, majority_required(alive_count))
        };
        let mut extension_message = send_game_embed(
            ctx,
            running,
            format!(
                "{} 토론 시간이 지났습니다.\n{DAY_EXTENSION_VOTE_SECONDS}초 안에 생존자 과반수({required_votes}/{alive_count}명)가 `1분 연장`을 누르면 낮 토론을 1분 연장합니다.\n과반수가 모이지 않으면 바로 투표로 넘어갑니다.",
                duration_text(current_discussion_seconds)
            ),
            "낮 토론 연장 투표",
            serenity::Colour::GOLD,
            day_extension_components(guild_id, false, false),
            false,
            true,
        )
        .await?;
        let extension_deadline = Instant::now() + Duration::from_secs(DAY_EXTENSION_VOTE_SECONDS);
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(extension_deadline)) => {
                    break;
                }
                _ = day_notify.notified() => {
                    let running_read = running.read().await;
                    if running_read.game.phase == Phase::Ended
                        || running_read.day_skip_confirmed
                        || running_read.day_extension_confirmed
                    {
                        break;
                    }
                }
            }
        }
        let (skip_confirmed, extension_confirmed, extension_votes, phase_ended) = {
            let mut running_write = running.write().await;
            running_write.day_extension_active = false;
            (
                running_write.day_skip_confirmed,
                running_write.day_extension_confirmed,
                running_write.day_extension_voter_ids.len(),
                running_write.game.phase == Phase::Ended,
            )
        };
        if skip_confirmed {
            let _ = extension_message
                .edit(
                    &ctx.http,
                    serenity::EditMessage::new()
                        .embed(make_embed(
                            "생존자 과반수가 바로 투표를 선택해 연장 투표를 종료합니다.\n바로 지목 투표로 넘어갑니다.",
                            "바로 투표",
                            serenity::Colour::DARK_GREEN,
                        ))
                        .components(day_extension_components(guild_id, true, false)),
                )
                .await;
            let _ = day_message
                .edit(
                    &ctx.http,
                    serenity::EditMessage::new()
                        .components(day_skip_components(guild_id, true, true)),
                )
                .await;
            return Ok(());
        }
        if phase_ended {
            return Ok(());
        }
        if extension_confirmed {
            extension_used = true;
            current_discussion_seconds = DISCUSSION_EXTENSION_SECONDS;
            discussion_deadline =
                Instant::now() + Duration::from_secs(DISCUSSION_EXTENSION_SECONDS);
            running.write().await.phase_deadline = Some(discussion_deadline);
            continue;
        }
        let _ = extension_message
            .edit(
                &ctx.http,
                serenity::EditMessage::new()
                    .embed(make_embed(
                        format!(
                            "{DAY_EXTENSION_VOTE_SECONDS}초 동안 1분 연장 투표가 과반수에 도달하지 못했습니다. ({extension_votes}/{required_votes}명)\n바로 투표로 넘어갑니다."
                        ),
                        "낮 토론 종료",
                        serenity::Colour::GOLD,
                    ))
                    .components(day_extension_components(guild_id, true, false)),
            )
            .await;
        let _ = day_message
            .edit(
                &ctx.http,
                serenity::EditMessage::new().components(day_skip_components(guild_id, true, false)),
            )
            .await;
        return Ok(());
    }
}

pub async fn send_day_single_select(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    actor: &Player,
    kind: &str,
    placeholder: &str,
) -> bool {
    send_day_multi_select(ctx, running, actor, kind, placeholder, 1).await
}

pub fn day_action_secret_text(kind: &str) -> &'static str {
    match kind {
        "hacker" => {
            "해커 낮 행동을 선택하세요.\n해킹은 1회용입니다. 선택한 대상의 직업은 밤이 시작될 때 비밀 메시지로 전달됩니다.\n해킹 사용 후 자신에게 쓰이는 능력은 해킹 대상에게 우회됩니다."
        }
        "vigilante" => {
            "자경단원 낮 행동을 선택하세요.\n숙청 조사는 1회용입니다. 밤이 시작될 때 대상이 마피아팀인지 비밀 메시지로 전달됩니다.\n숙청 처형은 조사와 별개로 밤에 한 번 시도할 수 있고, 마피아팀이 아니어도 기회가 소진됩니다."
        }
        "psychologist" => {
            "심리학자 낮 행동을 선택하세요.\n자신을 제외한 생존자 2명을 선택하면 두 사람이 같은 팀인지 즉시 확인합니다."
        }
        "hypnotist" => {
            "최면에 걸린 플레이어들을 모두 깨웁니다.\n시민팀이면 시민팀으로만 보이고, 시민팀이 아니면 직업을 확인합니다.\n최면을 해제하면 다음 밤에는 최면을 걸 수 없습니다."
        }
        _ => "낮 능력을 선택하세요.",
    }
}

pub async fn send_day_multi_select(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    actor: &Player,
    kind: &str,
    placeholder: &str,
    count: u8,
) -> bool {
    let (guild_id, mut targets) = {
        let running_read = running.read().await;
        (
            running_read.guild_id,
            running_read
                .game
                .players
                .iter()
                .filter(|player| player.alive && player.user_id != actor.user_id)
                .cloned()
                .collect::<Vec<_>>(),
        )
    };
    targets.sort_by_key(|player| player.name.to_lowercase());
    let options = targets
        .iter()
        .take(25)
        .map(|target| {
            serenity::CreateSelectMenuOption::new(
                target.name.chars().take(100).collect::<String>(),
                target.user_id.to_string(),
            )
        })
        .collect::<Vec<_>>();
    let select = serenity::CreateSelectMenu::new(
        format!("{kind}:{}:{}", guild_id.get(), actor.user_id),
        serenity::CreateSelectMenuKind::String { options },
    )
    .placeholder(placeholder)
    .min_values(count)
    .max_values(count);
    send_player_secret(
        ctx,
        running,
        actor,
        day_action_secret_text(kind),
        vec![serenity::CreateActionRow::SelectMenu(select)],
    )
    .await
}

pub async fn send_day_button_action(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    actor: &Player,
    kind: &str,
    text: &str,
    label: &str,
) -> bool {
    let guild_id = running.read().await.guild_id;
    send_player_secret(
        ctx,
        running,
        actor,
        format!("{}\n\n{}", day_action_secret_text(kind), text),
        vec![serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new(format!("{kind}:{}:{}", guild_id.get(), actor.user_id))
                .label(label)
                .style(serenity::ButtonStyle::Primary),
        ])],
    )
    .await
}

pub async fn run_vote(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) -> Result<()> {
    let config = data.config.read().await.clone();
    let escaped_executions;
    let (guild_id, vote_notify, seconds, alive) = {
        let mut running_write = running.write().await;
        escaped_executions = running_write.game.start_vote()?;
        running_write.phase_deadline =
            Some(Instant::now() + Duration::from_secs(config.vote_seconds));
        running_write.day_chat_open = false;
        running_write.final_defense_user_id = None;
        running_write.record_replay_event(
            "phase_started",
            None,
            &[],
            serde_json::json!({
                "phase": "vote",
                "escaped_executed_user_ids": escaped_executions.iter().map(|player| player.user_id).collect::<Vec<_>>(),
                "duration_seconds": config.vote_seconds,
            }),
        );
        (
            running_write.guild_id,
            running_write.vote_notify.clone(),
            config.vote_seconds,
            running_write
                .game
                .alive_players()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
        )
    };
    // [도주] 전날 처형을 피해 도주한 플레이어는 투표 시작과 함께 사망한다.
    if !escaped_executions.is_empty() {
        apply_death_side_effects(ctx, data, running, &escaped_executions).await;
        let lines = escaped_executions
            .iter()
            .map(|player| format!("[전날 도주했던 {}님이 처형당했습니다.]", player.name))
            .collect::<Vec<_>>()
            .join("\n");
        send_game_embed(
            ctx,
            running,
            lines,
            "도주자 처형",
            serenity::Colour::RED,
            vec![],
            true,
            true,
        )
        .await?;
        // 이 사망으로 승패가 갈렸으면 투표를 진행하지 않는다 (루프의 승자 발표가 처리).
        if running.read().await.game.winner().is_some() {
            return Ok(());
        }
    }
    upsert_game_status(ctx, running).await;
    set_game_channel_chat(ctx, data, running, false).await;
    let mut options = alive
        .iter()
        .take(24)
        .map(|target| {
            serenity::CreateSelectMenuOption::new(
                target.name.chars().take(100).collect::<String>(),
                target.user_id.to_string(),
            )
        })
        .collect::<Vec<_>>();
    options.push(serenity::CreateSelectMenuOption::new("스킵", "skip"));
    let select = serenity::CreateSelectMenu::new(
        format!("vote:{}", guild_id.get()),
        serenity::CreateSelectMenuKind::String { options },
    )
    .placeholder("처형할 대상 또는 스킵을 선택하세요")
    .min_values(1)
    .max_values(1);
    send_game_embed(
        ctx,
        running,
        format!(
            "지목 투표를 시작합니다. {seconds}초 안에 최후변론에 세울 사람을 선택하세요.\n투표 중에는 게임 채널 채팅이 비활성화됩니다.\n생존자가 모두 투표하면 남은 시간을 기다리지 않고 바로 정산합니다."
        ),
        "지목 투표 시작",
        serenity::Colour::GOLD,
        vec![serenity::CreateActionRow::SelectMenu(select)],
        false,
        true,
    )
    .await?;
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(seconds)) => {}
        _ = vote_notify.notified() => {}
    }
    if running.read().await.game.phase == Phase::Ended {
        return Ok(());
    }
    let vote_result = {
        let mut running_write = running.write().await;
        let result = running_write.game.resolve_nomination_vote()?;
        let target_ids = result
            .executed
            .as_ref()
            .map(|player| vec![player.user_id])
            .unwrap_or_default();
        let vote_counts = running_write.replay_vote_counts(&result.vote_counts);
        let weighted_vote_counts = running_write.replay_vote_counts(&result.weighted_vote_counts);
        let thief_steal = running_write.replay_text_results(&result.thief_steal_results);
        running_write.record_replay_event(
            "nomination_vote_resolved",
            None,
            &target_ids,
            serde_json::json!({
                "executed_user_id": result.executed.as_ref().map(|player| player.user_id),
                "tied": result.tied,
                "skipped": result.skipped,
                "vote_counts": vote_counts,
                "weighted_vote_counts": weighted_vote_counts,
                "madam_seduced_user_ids": result.madam_seduced.iter().map(|player| player.user_id).collect::<Vec<_>>(),
                "madam_newly_contacted_user_ids": result.madam_newly_contacted.iter().map(|player| player.user_id).collect::<Vec<_>>(),
                "blocked_voter_user_ids": result.blocked_voters.iter().map(|player| player.user_id).collect::<Vec<_>>(),
                "thief_steal": thief_steal,
            }),
        );
        result
    };
    handle_madam_seduction_result(ctx, data, running, &vote_result).await;
    deliver_thief_steal_results(ctx, data, running, &vote_result).await;
    sync_cult_team_channel_access(ctx, data, running).await;
    sync_lover_chat_access(ctx, data, running).await;
    let vote_summary = {
        let running_read = running.read().await;
        anonymous_vote_summary(&running_read.game, &vote_result)
    };
    if vote_result.executed.is_none() {
        let message = if vote_result.tied {
            "투표가 동률이라 최후변론 대상이 없습니다."
        } else if vote_result.skipped {
            "스킵이 최다 득표하여 최후변론 대상이 없습니다."
        } else {
            "투표가 없어 최후변론 대상이 없습니다."
        };
        send_game_embed(
            ctx,
            running,
            format!("{message}\n\n익명 투표 집계\n{vote_summary}"),
            "지목 투표 결과",
            serenity::Colour::GOLD,
            vec![],
            false,
            true,
        )
        .await?;
        return Ok(());
    }
    let nominee = vote_result.executed.unwrap();
    let terrorist_targets = {
        let mut running_write = running.write().await;
        running_write.final_defense_user_id = Some(nominee.user_id);
        running_write.phase_deadline = Some(Instant::now() + Duration::from_secs(20));
        running_write
            .game
            .begin_terrorist_final_defense(nominee.user_id)
    };
    sync_anonymous_general_chat_permissions(ctx, running).await;
    set_channel_slowmode(ctx, running, 0).await;
    // 마담에게 유혹당한 대상자도 자신의 최후변론은 할 수 있다 (개구리만 예외).
    if !running.read().await.game.is_frog(&nominee) {
        set_member_game_channel_chat(ctx, running, &nominee, true).await;
    }
    if !terrorist_targets.is_empty()
        && !send_player_secret(
            ctx,
            running,
            &nominee,
            "최후의 반론 중 습격할 한 명을 선택하세요.\n투표로 처형되면, 선택한 대상이 마피아 또는 접선을 완료한 마피아 보조직업일 때 함께 사망합니다.",
            terrorist_final_defense_components(guild_id, nominee.user_id, &terrorist_targets),
        )
        .await
    {
        eprintln!(
            "failed to send terrorist final defense target selection: {}",
            nominee.user_id
        );
    }
    send_game_embed(
        ctx,
        running,
        format!(
            "지목 투표 결과, {} 님이 최후변론 대상이 되었습니다.\n\n익명 투표 집계\n{vote_summary}",
            nominee.name
        ),
        "지목 투표 결과",
        serenity::Colour::GOLD,
        vec![],
        false,
        true,
    )
    .await?;
    send_game_embed(
        ctx,
        running,
        format!(
            "{} 님의 최후변론 시간입니다. 20초 동안 지목된 사람만 말할 수 있습니다.\n이 시간 동안 슬로우모드는 해제됩니다.",
            nominee.name
        ),
        "최후변론",
        serenity::Colour::GOLD,
        vec![],
        false,
        true,
    )
    .await?;
    tokio::time::sleep(Duration::from_secs(20)).await;
    if running.read().await.game.phase == Phase::Ended {
        return Ok(());
    }
    {
        let mut running_write = running.write().await;
        running_write.game.start_confirmation_vote()?;
        running_write.phase_deadline =
            Some(Instant::now() + Duration::from_secs(CONFIRM_VOTE_SECONDS));
        running_write.final_defense_user_id = None;
        running_write.record_replay_event(
            "phase_started",
            None,
            &[nominee.user_id],
            serde_json::json!({
                "phase": "confirm_vote",
                "duration_seconds": CONFIRM_VOTE_SECONDS,
                "nominee_user_id": nominee.user_id,
            }),
        );
    }
    restore_member_game_channel_chat(ctx, running).await;
    upsert_game_status(ctx, running).await;
    set_game_channel_chat(ctx, data, running, false).await;
    let confirm_notify = running.read().await.confirm_notify.clone();
    send_game_embed(
        ctx,
        running,
        format!(
            "{} 님 처형 여부를 찬반투표합니다. {CONFIRM_VOTE_SECONDS}초 안에 선택하세요.\n실제 투표 수 기준 과반수 이상이 찬성하면 처형합니다.",
            nominee.name
        ),
        "찬반투표",
        serenity::Colour::GOLD,
        vec![serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new(format!("confirm:{}:1", guild_id.get()))
                .label("찬성")
                .style(serenity::ButtonStyle::Success),
            serenity::CreateButton::new(format!("confirm:{}:0", guild_id.get()))
                .label("반대")
                .style(serenity::ButtonStyle::Danger),
        ])],
        false,
        true,
    )
    .await?;
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(CONFIRM_VOTE_SECONDS)) => {}
        _ = confirm_notify.notified() => {}
    }
    if running.read().await.game.phase == Phase::Ended {
        return Ok(());
    }
    let confirm_context = {
        let running_read = running.read().await;
        confirmation_vote_context(&running_read.game)
    };
    let confirm_result = {
        let mut running_write = running.write().await;
        let result = running_write
            .game
            .resolve_confirmation_vote(nominee.user_id)?;
        let mut target_ids = result
            .executed
            .as_ref()
            .map(|player| vec![player.user_id])
            .unwrap_or_default();
        target_ids.extend(result.extra_killed.iter().map(|player| player.user_id));
        let vote_counts = running_write.replay_confirm_vote_counts(&result.vote_counts);
        let weighted_vote_counts =
            running_write.replay_confirm_vote_counts(&result.weighted_vote_counts);
        running_write.record_replay_event(
            "confirmation_vote_resolved",
            None,
            &target_ids,
            serde_json::json!({
                "nominee_user_id": nominee.user_id,
                "executed_user_id": result.executed.as_ref().map(|player| player.user_id),
                "escaped_user_id": result.escaped.as_ref().map(|player| player.user_id),
                "extra_killed_user_ids": result.extra_killed.iter().map(|player| player.user_id).collect::<Vec<_>>(),
                "approved": result.approved,
                "tied": result.tied,
                "blocked_by_politician": result.blocked_by_politician,
                "vote_counts": vote_counts,
                "weighted_vote_counts": weighted_vote_counts,
                "judge_user_id": result.judge.as_ref().map(|player| player.user_id),
                "judge_choice": result.judge_choice,
                "decided_by_judge": result.decided_by_judge,
            }),
        );
        result
    };
    set_channel_slowmode(ctx, running, config.chat_slowmode_seconds).await;
    let summary_section = confirmation_vote_summary_section(
        &confirm_result,
        confirm_context,
        config.show_confirmation_vote_counts,
    );
    let judge_notice = if confirm_result.decided_by_judge {
        if let Some(judge) = &confirm_result.judge {
            let judge_choice = match confirm_result.judge_choice {
                None => "미투표(처형 없음)",
                Some(true) => "찬성",
                Some(false) => "반대",
            };
            format!(
                "\n\n[판사 {}님이 투표 결과를 정했습니다]\n판사의 선택: {judge_choice}",
                judge.name
            )
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let mut dead_players = Vec::new();
    if let Some(executed) = &confirm_result.executed {
        dead_players.push(executed.clone());
    }
    dead_players.extend(confirm_result.extra_killed.iter().cloned());
    apply_death_side_effects(ctx, data, running, &dead_players).await;
    sync_cult_team_channel_access(ctx, data, running).await;
    sync_lover_chat_access(ctx, data, running).await;
    upsert_game_status(ctx, running).await;
    let (message, color, include_dead) = if let Some(escaped) = &confirm_result.escaped {
        (
            format!(
                "[{}님이 도주했습니다!]
찬반투표로 처형이 결정되었지만 {}님은 처형장을 탈출했습니다. 다음날 투표가 시작될 때 처형됩니다.{judge_notice}{summary_section}",
                escaped.name, escaped.name
            ),
            serenity::Colour::ORANGE,
            false,
        )
    } else if confirm_result.blocked_by_politician {
        (
            format!(
                "찬반투표 결과, {} 님은 **정치인** 입니다.\n[정치인은 투표로 죽지 않습니다]\n\n{} 님은 처형되지 않고 밤으로 넘어갑니다.{judge_notice}{summary_section}",
                nominee.name, nominee.name
            ),
            serenity::Colour::ORANGE,
            false,
        )
    } else if let Some(executed) = &confirm_result.executed {
        let killed_lines = {
            let running_read = running.read().await;
            dead_players
                .iter()
                .map(|killed| {
                    format!(
                        "- {}: {}",
                        killed.name,
                        death_role_text(&running_read, killed)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let mut result_message = format!("찬반투표 결과, {} 님이 처형되었습니다.", executed.name);
        if !confirm_result.extra_killed.is_empty() {
            if executed.role == Role::Terrorist {
                for target in &confirm_result.extra_killed {
                    result_message.push('\n');
                    result_message.push_str(&terrorist_execution_message(executed, target));
                }
            } else {
                result_message.push_str(
                    "\n처형 대상이 지목하고 있던 시민팀이 아닌 대상도 함께 사망했습니다.",
                );
            }
        }
        (
            format!("{result_message}\n\n사망자\n{killed_lines}{judge_notice}{summary_section}"),
            serenity::Colour::GOLD,
            true,
        )
    } else if confirm_result.tied {
        (
            format!("찬반투표가 동률이라 처형하지 않습니다.{judge_notice}{summary_section}"),
            serenity::Colour::GOLD,
            false,
        )
    } else {
        let reject_message = confirmation_rejection_message(&confirm_result, confirm_context);
        (
            format!("{reject_message}{judge_notice}{summary_section}"),
            serenity::Colour::GOLD,
            false,
        )
    };
    send_game_embed(
        ctx,
        running,
        message,
        "찬반투표 결과",
        color,
        vec![],
        include_dead,
        true,
    )
    .await?;
    Ok(())
}

/// 도벽 결과는 투표가 끝난 뒤에야 도둑에게 전달된다. 마피아 직업을 훔쳐 접선한
/// 도둑에게는 마피아 채널 접근도 함께 열어준다.
pub(crate) async fn deliver_thief_steal_results(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
    vote_result: &VoteResult,
) {
    for (thief_id, message) in &vote_result.thief_steal_results {
        let player = running.read().await.game.get_player(*thief_id).cloned();
        let Some(player) = player.filter(|player| player.alive) else {
            continue;
        };
        if !send_player_secret(ctx, running, &player, message.clone(), vec![]).await {
            eprintln!("failed to deliver thief steal result: user_id={thief_id}");
        }
    }
    for thief in &vote_result.thief_newly_contacted {
        grant_private_role_member_access(ctx, data, running, Role::Mafia, thief).await;
    }
}

pub(crate) fn terrorist_execution_message(terrorist: &Player, target: &Player) -> String {
    format!(
        "[테러리스트 {}님이 {}님을 습격하였습니다.]",
        terrorist.name, target.name
    )
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ConfirmationVoteContext {
    pub(crate) eligible_voters: usize,
    pub(crate) submitted_voters: usize,
}

pub(crate) fn confirmation_vote_context(game: &MafiaGame) -> ConfirmationVoteContext {
    let alive_ids = game
        .alive_players()
        .into_iter()
        .map(|player| player.user_id)
        .collect::<HashSet<_>>();
    let submitted_voters = game
        .confirm_votes
        .keys()
        .filter(|user_id| alive_ids.contains(user_id))
        .count();
    ConfirmationVoteContext {
        eligible_voters: alive_ids.len(),
        submitted_voters,
    }
}

pub(crate) fn confirmation_vote_summary(
    confirm_result: &ConfirmVoteResult,
    context: ConfirmationVoteContext,
) -> String {
    let yes = confirm_result.vote_counts.get(&true).copied().unwrap_or(0);
    let no = confirm_result.vote_counts.get(&false).copied().unwrap_or(0);
    let submitted_vote_count = yes + no;
    let required_yes = confirmation_required_yes(confirm_result);
    let weighted_vote_count = confirmation_weighted_vote_count(confirm_result);
    let abstained = context
        .eligible_voters
        .saturating_sub(context.submitted_voters);
    if weighted_vote_count == submitted_vote_count {
        format!(
            "찬성 {yes}표 / 반대 {no}표 / 미투표 {abstained}명\n처형 기준: 찬성 {required_yes}표 이상 (투표수 {submitted_vote_count}표 기준)"
        )
    } else {
        format!(
            "찬성 {yes}표 / 반대 {no}표 / 미투표 {abstained}명\n처형 기준: 찬성 처리값 {required_yes} 이상 (처리 투표수 {weighted_vote_count} 기준)"
        )
    }
}

pub(crate) fn confirmation_vote_summary_section(
    confirm_result: &ConfirmVoteResult,
    context: ConfirmationVoteContext,
    show_counts: bool,
) -> String {
    if show_counts {
        format!(
            "\n\n찬반투표 집계\n{}",
            confirmation_vote_summary(confirm_result, context)
        )
    } else {
        String::new()
    }
}

pub(crate) fn confirmation_weighted_counts(
    confirm_result: &ConfirmVoteResult,
) -> &HashMap<bool, i32> {
    if confirm_result.weighted_vote_counts.is_empty() {
        &confirm_result.vote_counts
    } else {
        &confirm_result.weighted_vote_counts
    }
}

pub(crate) fn confirmation_weighted_vote_count(confirm_result: &ConfirmVoteResult) -> i32 {
    let counts = confirmation_weighted_counts(confirm_result);
    counts.values().copied().sum()
}

pub(crate) fn confirmation_required_yes(confirm_result: &ConfirmVoteResult) -> i32 {
    let counts = confirmation_weighted_counts(confirm_result);
    let yes = counts.get(&true).copied().unwrap_or(0);
    let no = counts.get(&false).copied().unwrap_or(0);
    let submitted_vote_count = yes + no;
    if submitted_vote_count <= 0 {
        1
    } else {
        submitted_vote_count / 2 + 1
    }
}

pub(crate) fn confirmation_rejection_message(
    confirm_result: &ConfirmVoteResult,
    _context: ConfirmationVoteContext,
) -> String {
    if confirm_result.decided_by_judge {
        return "판사의 선택으로 처형하지 않습니다.".to_string();
    }
    let counts = confirmation_weighted_counts(confirm_result);
    let yes = counts.get(&true).copied().unwrap_or(0);
    let no = counts.get(&false).copied().unwrap_or(0);
    if yes == no {
        "찬성과 반대가 같아 처형하지 않습니다.".to_string()
    } else if yes > no {
        let required_yes = confirmation_required_yes(confirm_result);
        format!(
            "찬성이 더 많지만 투표수 기준 과반수에 도달하지 못해 처형하지 않습니다. (찬성 {yes}/{required_yes}표)"
        )
    } else {
        "반대가 많아 처형하지 않습니다.".to_string()
    }
}

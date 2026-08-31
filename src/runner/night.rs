// runner/night.rs — 밤 진행·밤 행동 DM·아침 공지

use super::*;

pub(crate) fn remaining_night_wait(deadline: Instant, now: Instant) -> Duration {
    deadline.saturating_duration_since(now)
}

pub(crate) async fn wait_for_night_deadline_or_action(deadline: Instant, notify: &Notify) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(remaining_night_wait(deadline, Instant::now())) => true,
        _ = notify.notified() => false,
    }
}

pub(crate) struct TimedNightEvents {
    guild_id: serenity::GuildId,
    cursed_players: Vec<Player>,
    witch_contacts: Vec<u64>,
    cult_bells: u32,
    revived_players: Vec<Player>,
}

impl TimedNightEvents {
    fn is_empty(&self) -> bool {
        self.cursed_players.is_empty()
            && self.witch_contacts.is_empty()
            && self.cult_bells == 0
            && self.revived_players.is_empty()
    }
}

pub(crate) async fn take_timed_night_events(
    running: &Arc<RwLock<RunningGame>>,
) -> Option<TimedNightEvents> {
    let mut running_write = running.write().await;
    if running_write.game.phase != Phase::Night {
        return None;
    }
    let (cursed_players, witch_contacts) = running_write.game.apply_witch_curses(&HashSet::new());
    let events = TimedNightEvents {
        guild_id: running_write.guild_id,
        cursed_players,
        witch_contacts,
        cult_bells: running_write.game.consume_cult_bells(),
        revived_players: running_write.game.revive_pending_scientists(),
    };
    (!events.is_empty()).then_some(events)
}

pub(crate) async fn apply_timed_night_event_side_effects(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
    events: TimedNightEvents,
) -> Result<()> {
    let TimedNightEvents {
        guild_id,
        cursed_players,
        witch_contacts,
        cult_bells,
        revived_players,
    } = events;

    for player in &cursed_players {
        deny_frog_game_channel_chat(ctx, running, player).await;
        disable_private_role_channels_for_player(ctx, running, player).await;
        let _ = send_player_secret(
            ctx,
            running,
            player,
            "마녀의 저주에 걸렸습니다. 다음 밤까지 개구리가 되어 모든 게임 채팅에서 발언할 수 없습니다.",
            vec![],
        )
        .await;
    }
    for user_id in &witch_contacts {
        let player = running.read().await.game.get_player(*user_id).cloned();
        if let Some(player) = player {
            grant_private_role_member_access(ctx, data, running, Role::Mafia, &player).await;
            let _ = send_player_secret(
                ctx,
                running,
                &player,
                "저주 대상이 마피아라 마피아와 접선했습니다.",
                vec![],
            )
            .await;
        }
    }
    if cult_bells > 0 {
        send_game_embed(
            ctx,
            running,
            std::iter::repeat_n("교주의 종소리가 울렸습니다.", cult_bells as usize)
                .collect::<Vec<_>>()
                .join("\n"),
            "교주 포교",
            serenity::Colour::ORANGE,
            vec![],
            false,
            true,
        )
        .await?;
    }
    if !revived_players.is_empty() {
        let config = data.config.read().await.clone();
        let roles = channel_role_ids(ctx, guild_id, &config, data.bot_user_id).await?;
        for player in &revived_players {
            restore_revived_player_roles(ctx, running, roles, player).await;
            // [분석] 부활한 과학자에게 공격자 정보를 전달한다.
            let notice = running
                .write()
                .await
                .game
                .take_analysis_notice(player.user_id);
            if let Some(notice) = notice {
                let _ = send_player_secret(ctx, running, player, notice, vec![]).await;
            }
        }
        send_game_embed(
            ctx,
            running,
            revived_players
                .iter()
                .map(|player| format!("[과학자 {}님이 부활했습니다.]", player.name))
                .collect::<Vec<_>>()
                .join("\n"),
            "과학자 부활",
            serenity::Colour::DARK_GREEN,
            vec![],
            false,
            true,
        )
        .await?;
    }
    sync_cult_team_channel_access(ctx, data, running).await;
    sync_lover_chat_access(ctx, data, running).await;
    sync_shaman_chat_access(ctx, data, running).await;
    sync_anonymous_general_chat_permissions(ctx, running).await;
    Ok(())
}

pub async fn trigger_timed_night_events(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) -> Result<()> {
    let Some(events) = take_timed_night_events(running).await else {
        return Ok(());
    };
    let counts = (
        events.cursed_players.len(),
        events.witch_contacts.len(),
        events.cult_bells,
        events.revived_players.len(),
    );
    let guild_id = events.guild_id;
    let ctx = ctx.clone();
    let data = data.clone();
    let running = running.clone();
    tokio::spawn(async move {
        let started_at = Instant::now();
        if let Err(error) =
            apply_timed_night_event_side_effects(&ctx, &data, &running, events).await
        {
            eprintln!(
                "timed night event side effects failed: guild_id={} cursed={} witch_contacts={} cult_bells={} revived={} error={error:?}",
                guild_id.get(),
                counts.0,
                counts.1,
                counts.2,
                counts.3,
            );
            return;
        }
        let elapsed = started_at.elapsed();
        if elapsed >= Duration::from_secs(2) {
            eprintln!(
                "slow timed night event side effects: guild_id={} elapsed_ms={} cursed={} witch_contacts={} cult_bells={} revived={}",
                guild_id.get(),
                elapsed.as_millis(),
                counts.0,
                counts.1,
                counts.2,
                counts.3,
            );
        }
    });
    Ok(())
}

pub async fn run_night(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) -> Result<()> {
    let phase_started_at = Instant::now();
    let (
        actors,
        restored_frogs,
        hacker_results,
        vigilante_results,
        godfather_contacts,
        seconds,
        deadline,
        notify,
    ) = {
        let config = data.config.read().await.clone();
        let mut running_write = running.write().await;
        let deadline = phase_started_at + Duration::from_secs(config.night_seconds);
        running_write.game.phase = Phase::Night;
        running_write.phase_deadline = Some(deadline);
        running_write.day_chat_open = false;
        running_write.final_defense_user_id = None;
        running_write.night_timed_events_due = config.night_seconds <= 10;
        running_write.contractor_contract_drafts.clear();
        running_write.activity_night_results.clear();
        running_write.record_replay_event(
            "phase_started",
            None,
            &[],
            serde_json::json!({
                "phase": "night",
                "duration_seconds": config.night_seconds,
            }),
        );
        let restored_frogs = running_write.game.restore_frogs();
        let hacker_results = running_write.game.consume_hacker_results();
        let vigilante_results = running_write.game.consume_vigilante_results();
        let godfather_contacts = running_write.game.ensure_godfather_auto_contact();
        let actors = running_write.game.night_action_actors();
        (
            actors,
            restored_frogs,
            hacker_results,
            vigilante_results,
            godfather_contacts,
            config.night_seconds,
            deadline,
            running_write.night_notify.clone(),
        )
    };
    let (guild_id, day_number) = {
        let running_read = running.read().await;
        (running_read.guild_id, running_read.game.day_number)
    };
    eprintln!(
        "night phase started: guild_id={} day={} duration_seconds={} actors={}",
        guild_id.get(),
        day_number,
        seconds,
        actors.len(),
    );
    upsert_game_status(ctx, running).await;
    set_game_channel_chat(ctx, data, running, false).await;
    // [확성] 보유자는 밤에도 전체 채팅이 열린다 (익명 게임은 릴레이 판정이 처리).
    let loudspeakers = {
        let running_read = running.read().await;
        running_read
            .game
            .players
            .iter()
            .filter(|player| running_read.game.is_loudspeaker_active(player))
            .cloned()
            .collect::<Vec<_>>()
    };
    let anonymous_enabled = running.read().await.anonymous_enabled;
    for holder in loudspeakers {
        set_member_game_channel_chat(ctx, running, &holder, true).await;
        // 사용법 안내 겸 권한이 열렸다는 확인. 익명 게임은 쓰는 위치가 다르다.
        let where_to = if anonymous_enabled {
            "개인 익명 채팅 채널"
        } else {
            "게임 채널"
        };
        let _ = send_player_secret(
            ctx,
            running,
            &holder,
            format!(
                "[확성] 이번 밤 {where_to}에 메시지를 보낼 수 있습니다. 인당 게임 중 1회이며, 다른 확성 보유자가 먼저 보내면 이번 밤에는 쓸 수 없습니다."
            ),
            vec![],
        )
        .await;
    }
    unlock_pending_dead_chats(ctx, data, running).await;
    sync_private_role_chat_permissions(ctx, data, running).await;
    sync_lover_chat_access(ctx, data, running).await;
    sync_cult_team_channel_access(ctx, data, running).await;
    sync_scientist_mafia_permissions(ctx, data, running).await;
    sync_madam_seduction_permissions(ctx, running).await;
    sync_shaman_chat_access(ctx, data, running).await;
    for player in &restored_frogs {
        restore_frog_game_channel_permission(ctx, running, player).await;
        restore_private_role_channels_for_player(ctx, data, running, player).await;
    }
    for (user_id, message) in hacker_results.into_iter().chain(vigilante_results) {
        let player = running.read().await.game.get_player(user_id).cloned();
        if let Some(player) = player {
            let _ = send_player_secret(ctx, running, &player, message, vec![]).await;
        }
    }
    for user_id in godfather_contacts {
        let player = running.read().await.game.get_player(user_id).cloned();
        if let Some(player) = player {
            grant_private_role_member_access(ctx, data, running, Role::Mafia, &player).await;
            let _ = send_player_secret(
                ctx,
                running,
                &player,
                "세 번째 밤이 되어 마피아 팀과 자동 접선했습니다. 이제 마피아 비밀방을 볼 수 있고 밤마다 확정 처치 대상을 지목합니다.",
                vec![],
            )
            .await;
        }
    }
    send_game_embed(
        ctx,
        running,
        format!(
            "밤이 되었습니다. {seconds}초 동안 게임 채널 채팅이 비활성화됩니다.\n밤 행동이 있는 역할은 본인 익명 채널 또는 DM에서 선택합니다.\n변경 가능한 밤 행동은 밤이 끝나기 전 다시 선택하면 대상을 바꿀 수 있습니다."
        ),
        "밤",
        serenity::Colour::GOLD,
        vec![],
        false,
        true,
    )
    .await?;
    let police_can_act = actors.iter().any(|actor| actor.role == Role::Police);
    let mut failed_actions = Vec::new();
    for actor in actors {
        if let Err(error) = send_night_action_dm(ctx, running, &actor).await {
            eprintln!(
                "secret delivery failed: stage=night_action guild_id={} user_id={} player={:?} role={} {}",
                running.read().await.guild_id.get(),
                actor.user_id,
                actor.name,
                actor.role.value(),
                error.log_detail(),
            );
            failed_actions.push(format!("{} ({})", actor.name, error.public_reason()));
        }
    }
    if !failed_actions.is_empty() {
        send_game_embed(
            ctx,
            running,
            format!(
                "밤 행동 선택지를 보낼 수 없는 참가자와 원인:\n{}\n\n서버 콘솔에는 Discord 원문 오류와 채널/사용자 ID를 기록했습니다.",
                failed_actions.join("\n")
            ),
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
            false,
            true,
        )
        .await?;
    }
    // [유언] 보유자에게 매 밤 작성 버튼을 보낸다.
    let will_holders = {
        let running_read = running.read().await;
        running_read
            .game
            .players
            .iter()
            .filter(|player| {
                player.alive
                    && running_read.game.has_tier_ability(
                        player.user_id,
                        mafia_remake::model::TierAbility::LastWill,
                    )
                    && !running_read.game.is_frog(player)
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    for holder in will_holders {
        let has_will = running
            .read()
            .await
            .game
            .last_wills
            .contains_key(&holder.user_id);
        let prompt = if has_will {
            "이전에 작성한 유언이 있습니다. 다시 작성하면 덮어씁니다.\n밤에 사망하면 아침에 유언이 모두에게 공개됩니다."
        } else {
            "[유언] 밤 동안 유언을 작성할 수 있습니다.\n밤에 사망하면 아침에 유언이 모두에게 공개됩니다."
        };
        let _ = send_player_secret(
            ctx,
            running,
            &holder,
            prompt,
            vec![serenity::CreateActionRow::Buttons(vec![
                serenity::CreateButton::new(format!(
                    "lastwill:{}:{}",
                    guild_id.get(),
                    holder.user_id
                ))
                .label("유언 작성")
                .style(serenity::ButtonStyle::Secondary),
            ])],
        )
        .await;
    }
    let has_changeable_mafia_action = { running.write().await.game.has_changeable_mafia_action() };
    if has_changeable_mafia_action {
        upsert_private_role_status_message(ctx, running, Role::Mafia).await;
    }
    if seconds <= 10 {
        {
            let mut running_write = running.write().await;
            running_write.night_timed_events_due = true;
        }
        trigger_timed_night_events(ctx, data, running).await?;
        wait_for_night_deadline_or_action(deadline, &notify).await;
    } else {
        let warning_deadline = deadline - Duration::from_secs(10);
        let reached_ten_seconds =
            wait_for_night_deadline_or_action(warning_deadline, &notify).await;
        if running.read().await.game.phase == Phase::Ended {
            return Ok(());
        }
        {
            let mut running_write = running.write().await;
            running_write.night_timed_events_due = true;
        }
        if reached_ten_seconds {
            let warning_ctx = ctx.clone();
            let warning_running = running.clone();
            tokio::spawn(async move {
                if let Err(error) = send_game_embed(
                    &warning_ctx,
                    &warning_running,
                    "밤 시간이 10초 남았습니다. 아직 행동하지 않았다면 지금 선택하세요.",
                    "밤 10초 전",
                    serenity::Colour::GOLD,
                    vec![],
                    false,
                    true,
                )
                .await
                {
                    eprintln!("failed to send ten-second night warning: {error:?}");
                }
            });
            trigger_timed_night_events(ctx, data, running).await?;
            wait_for_night_deadline_or_action(deadline, &notify).await;
        } else {
            trigger_timed_night_events(ctx, data, running).await?;
        }
    }
    if running.read().await.game.phase == Phase::Ended {
        return Ok(());
    }
    {
        let mut running_write = running.write().await;
        running_write.night_timed_events_due = true;
    }
    trigger_timed_night_events(ctx, data, running).await?;
    eprintln!(
        "night resolution starting: guild_id={} day={} elapsed_ms={}",
        guild_id.get(),
        day_number,
        phase_started_at.elapsed().as_millis(),
    );
    let result = {
        let mut running_write = running.write().await;
        running_write.game.resolve_night()?
    };
    eprintln!(
        "night resolved: guild_id={} day={} elapsed_ms={} killed={}",
        guild_id.get(),
        day_number,
        phase_started_at.elapsed().as_millis(),
        result.killed_players.len(),
    );
    {
        let mut running_write = running.write().await;
        let killed_ids = result
            .killed_players
            .iter()
            .map(|player| player.user_id)
            .collect::<Vec<_>>();
        let private_results = serde_json::json!({
            "detective": running_write.replay_text_results(&result.detective_results),
            "inspector": running_write.replay_text_results(&result.inspector_results),
            "inspector_target_notices": running_write.replay_text_results(&result.inspector_target_notices),
            "civil_servant": running_write.replay_text_results(&result.civil_servant_results),
            "paparazzi": running_write.replay_text_results(&result.paparazzi_results),
            "fraudster": running_write.replay_text_results(&result.fraudster_results),
            "soldier_watch": running_write.replay_text_results(&result.soldier_watch_results),
            "tier_ability": running_write.replay_text_results(&result.tier_ability_results),
            "published_wills": result.published_wills.iter().map(|(name, will)| serde_json::json!({"name": name, "will": will})).collect::<Vec<_>>(),
            "spy": running_write.replay_text_results(&result.spy_results),
            "contractor": running_write.replay_text_results(&result.contractor_results),
            "witch": running_write.replay_text_results(&result.witch_results),
            "godfather": running_write.replay_text_results(&result.godfather_results),
            "shaman": running_write.replay_text_results(&result.shaman_results),
            "priest": running_write.replay_text_results(&result.priest_results),
            "agent": running_write.replay_text_results(&result.agent_results),
            "thief_police": running_write.replay_text_results(&result.thief_police_results),
            "reporter": running_write.replay_text_results(&result.reporter_results),
            "vigilante": running_write.replay_text_results(&result.vigilante_results),
            "mercenary": running_write.replay_text_results(&result.mercenary_results),
            "nurse": running_write.replay_text_results(&result.nurse_results),
            "gangster": running_write.replay_text_results(&result.gangster_results),
            "cult": running_write.replay_text_results(&result.cult_results),
            "fanatic": running_write.replay_text_results(&result.fanatic_results),
        });
        let details = serde_json::json!({
            "mafia_target_user_id": result.mafia_target.as_ref().map(|player| player.user_id),
            "protected_user_id": result.protected.as_ref().map(|player| player.user_id),
            "police_target_user_id": result.police_target.as_ref().map(|player| player.user_id),
            "police_target_is_mafia": result.police_target_is_mafia,
            "killed_user_ids": killed_ids.clone(),
            "contractor_kill_user_ids": result.contractor_kills.iter().map(|player| player.user_id).collect::<Vec<_>>(),
            "vigilante_kill_user_ids": result.vigilante_kills.iter().map(|player| player.user_id).collect::<Vec<_>>(),
            "mercenary_kill_user_ids": result.mercenary_kills.iter().map(|player| player.user_id).collect::<Vec<_>>(),
            "priest_revive_user_ids": result.priest_revives.iter().map(|player| player.user_id).collect::<Vec<_>>(),
            "shaman_purification_user_ids": result.shaman_purifications.clone(),
            "contacts": {
                "spy": result.spy_contacts.clone(),
                "contractor": result.contractor_contacts.clone(),
                "fraudster": result.fraudster_contacts.clone(),
                "witch": result.witch_contacts.clone(),
                "godfather": result.godfather_contacts.clone(),
                "nurse": result.nurse_contacts.clone(),
                "fanatic_inherits": result.fanatic_inherits.clone(),
            },
            "private_results": private_results,
            "cult_bells": result.cult_bells,
        });
        running_write.record_replay_event("night_resolved", None, &killed_ids, details);
    }
    // Activity 프론트엔드용 밤 행동 결과 저장
    {
        let mut running_write = running.write().await;
        for map in [
            &result.detective_results,
            &result.inspector_results,
            &result.inspector_target_notices,
            &result.civil_servant_results,
            &result.paparazzi_results,
            &result.fraudster_results,
            &result.soldier_watch_results,
            &result.tier_ability_results,
            &result.spy_results,
            &result.contractor_results,
            &result.witch_results,
            &result.godfather_results,
            &result.shaman_results,
            &result.priest_results,
            &result.agent_results,
            &result.reporter_results,
            &result.vigilante_results,
            &result.mercenary_results,
            &result.nurse_results,
            &result.gangster_results,
            &result.cult_results,
            &result.fanatic_results,
            &result.hacker_results,
            &result.thief_police_results,
        ] {
            for (user_id, text) in map {
                running_write
                    .activity_night_results
                    .insert(*user_id, text.clone());
            }
        }
        // 경찰 조사 결과
        if let Some(target) = &result.police_target {
            let result_text = if result.police_target_is_mafia.unwrap_or(false) {
                "마피아"
            } else {
                "시민"
            };
            let msg = format!("조사 결과: {} 님은 {}.", target.name, result_text);
            let police_ids: Vec<u64> = running_write
                .game
                .alive_players()
                .iter()
                .filter(|p| p.role == Role::Police)
                .map(|p| p.user_id)
                .collect();
            for id in police_ids {
                running_write.activity_night_results.insert(id, msg.clone());
            }
        }
    }
    let doctor_saved = result
        .mafia_target
        .as_ref()
        .zip(result.protected.as_ref())
        .is_some_and(|(mafia_target, protected)| mafia_target.user_id == protected.user_id)
        && result.mafia_target.as_ref().is_none_or(|mafia_target| {
            !result
                .killed_players
                .iter()
                .any(|player| player.user_id == mafia_target.user_id)
        })
        && result.lover_sacrifices.is_empty();
    apply_death_side_effects(ctx, data, running, &result.killed_players).await;
    if result.killed_players.is_empty() {
        // [은폐] 조용한 밤: 치료로 살아났다는 문구 대신 아무 일도 없던 것처럼 보인다.
        if doctor_saved && !result.quiet_night {
            if let Some(saved_player) = &result.protected {
                send_game_embed(
                    ctx,
                    running,
                    format!(
                        "아침이 밝았습니다. **{}**님이 의사의 치료로 살아났습니다.",
                        saved_player.name
                    ),
                    "밤 결과",
                    serenity::Colour::DARK_GREEN,
                    vec![],
                    true,
                    true,
                )
                .await?;
            }
        } else {
            send_game_embed(
                ctx,
                running,
                "아침이 밝았습니다. 아무도 사망하지 않았습니다.",
                "밤 결과",
                serenity::Colour::GOLD,
                vec![],
                true,
                true,
            )
            .await?;
        }
    } else {
        let mut lines = Vec::new();
        {
            let running_read = running.read().await;
            for killed in &result.killed_players {
                if result
                    .mercenary_kills
                    .iter()
                    .any(|player| player.user_id == killed.user_id)
                {
                    lines.push(format!(
                        "- [{}님이 살해당했습니다.] {}",
                        killed.name,
                        death_role_text(&running_read, killed)
                    ));
                } else if result
                    .contractor_kills
                    .iter()
                    .any(|player| player.user_id == killed.user_id)
                {
                    lines.push(format!(
                        "- {} 님이 청부업자에게 정체를 들켜 암살 당했습니다. {}",
                        killed.name,
                        death_role_text(&running_read, killed)
                    ));
                } else if result
                    .vigilante_kills
                    .iter()
                    .any(|player| player.user_id == killed.user_id)
                {
                    lines.push(format!(
                        "- {} 님이 자경단원에게 숙청당했습니다. {}",
                        killed.name,
                        death_role_text(&running_read, killed)
                    ));
                } else {
                    lines.push(format!(
                        "- {}: {}",
                        killed.name,
                        death_role_text(&running_read, killed)
                    ));
                }
            }
        }
        let mut message = format!(
            "아침이 밝았습니다. 밤 사이 사망자가 발생했습니다.\n{}",
            lines.join("\n")
        );
        if !result.lover_sacrifices.is_empty() {
            let lover_lines = result
                .lover_sacrifices
                .iter()
                .map(|(savior, saved)| {
                    format!(
                        "- {}님이 연인 {}님을 살리고 대신 마피아에게 살해 당했습니다!",
                        savior.name, saved.name
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            message.push_str("\n\n연인 희생\n");
            message.push_str(&lover_lines);
        }
        if !result.published_wills.is_empty() {
            let will_lines = result
                .published_wills
                .iter()
                .map(|(name, will)| format!("- {}님의 유언: {}", name, will))
                .collect::<Vec<_>>()
                .join(
                    "
",
                );
            message.push_str(
                "

[유언 공개]
",
            );
            message.push_str(&will_lines);
        }
        if !result.terrorist_retaliations.is_empty() {
            let retaliation_lines = result
                .terrorist_retaliations
                .iter()
                .map(|(terrorist, target)| {
                    format!(
                        "- {} 님이 지목 중이던 {} 님도 함께 사망했습니다.",
                        terrorist.name, target.name
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            message.push_str("\n\n지목 반격\n");
            message.push_str(&retaliation_lines);
        }
        send_game_embed(
            ctx,
            running,
            message,
            "밤 결과",
            serenity::Colour::GOLD,
            vec![],
            true,
            true,
        )
        .await?;
    }
    if !result.killed_players.is_empty()
        && doctor_saved
        && !result.quiet_night
        && let Some(saved_player) = &result.protected
    {
        send_game_embed(
            ctx,
            running,
            format!("**{}**님이 의사의 치료로 살아났습니다.", saved_player.name),
            "의사 치료",
            serenity::Colour::DARK_GREEN,
            vec![],
            true,
            true,
        )
        .await?;
    }
    if !result.soldier_blocks.is_empty() {
        send_game_embed(
            ctx,
            running,
            result
                .soldier_blocks
                .iter()
                .map(|soldier| {
                    format!(
                        "군인 **{}**님이 마피아의 공격을 버텨냈습니다!",
                        soldier.name
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
            "군인 방탄",
            serenity::Colour::ORANGE,
            vec![],
            true,
            true,
        )
        .await?;
    }
    if !result.night_raid_reveals.is_empty() {
        send_game_embed(
            ctx,
            running,
            result
                .night_raid_reveals
                .iter()
                .map(|player| format!("[야습] {}님은 의사였습니다!", player.name))
                .collect::<Vec<_>>()
                .join("\n"),
            "야습",
            serenity::Colour::RED,
            vec![],
            true,
            true,
        )
        .await?;
    }
    if !result.priest_revives.is_empty() {
        send_game_embed(
            ctx,
            running,
            result
                .priest_revives
                .iter()
                .map(|player| format!("[{}님이 부활하셨습니다]", player.name))
                .collect::<Vec<_>>()
                .join("\n"),
            "성직자 소생",
            serenity::Colour::DARK_GREEN,
            vec![],
            true,
            true,
        )
        .await?;
    }
    if !result.reporter_results.is_empty() {
        send_game_embed(
            ctx,
            running,
            result
                .reporter_results
                .values()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n"),
            "기자 특종",
            serenity::Colour::DARK_GREEN,
            vec![],
            true,
            true,
        )
        .await?;
    }
    if result.cult_bells > 0 {
        send_game_embed(
            ctx,
            running,
            std::iter::repeat_n("교주의 종소리가 울렸습니다.", result.cult_bells as usize)
                .collect::<Vec<_>>()
                .join("\n"),
            "교주 포교",
            serenity::Colour::ORANGE,
            vec![],
            true,
            true,
        )
        .await?;
    }
    send_private_result_maps(ctx, running, &result).await;
    apply_purification_side_effects(ctx, data, running, &result.shaman_purifications).await;
    if !result.priest_revives.is_empty() {
        let config = data.config.read().await.clone();
        let guild_id = running.read().await.guild_id;
        if let Ok(roles) = channel_role_ids(ctx, guild_id, &config, data.bot_user_id).await {
            for player in &result.priest_revives {
                restore_revived_player_roles(ctx, running, roles, player).await;
            }
        }
    }
    for user_id in result
        .spy_contacts
        .iter()
        .chain(&result.contractor_contacts)
        .chain(&result.fraudster_contacts)
        .chain(&result.witch_contacts)
        .chain(&result.tier_ability_contacts)
    {
        let player = running.read().await.game.get_player(*user_id).cloned();
        if let Some(player) = player.filter(|player| player.alive) {
            grant_private_role_member_access(ctx, data, running, Role::Mafia, &player).await;
        }
    }
    for user_id in &result.nurse_contacts {
        let player = running.read().await.game.get_player(*user_id).cloned();
        if let Some(player) = player.filter(|player| player.alive) {
            grant_private_role_member_access(ctx, data, running, Role::Doctor, &player).await;
        }
    }
    for (user_id, inherited_role) in &result.graverobber_results {
        let player = running.read().await.game.get_player(*user_id).cloned();
        if let Some(player) = player {
            if PRIVATE_CHAT_ROLES.contains(inherited_role) {
                grant_private_role_member_access(ctx, data, running, *inherited_role, &player)
                    .await;
            }
            let _ = send_player_secret(
                ctx,
                running,
                &player,
                format!(
                    "도굴꾼 능력으로 **{}** 직업을 이어받았습니다.",
                    inherited_role.value()
                ),
                vec![],
            )
            .await;
        }
    }
    for user_id in &result.fanatic_inherits {
        let player = running.read().await.game.get_player(*user_id).cloned();
        if let Some(player) = player {
            let _ = send_player_secret(
                ctx,
                running,
                &player,
                "교주가 사망해 광신도가 교주의 능력을 물려받았습니다.",
                vec![],
            )
            .await;
        }
    }
    sync_cult_team_channel_access(ctx, data, running).await;
    sync_lover_chat_access(ctx, data, running).await;
    announce_police_result(ctx, running, &result).await;
    let config = data.config.read().await.clone();
    announce_public_police_status(ctx, running, &config, police_can_act, &result).await?;
    announce_morning_mafia_count(ctx, running, &config).await?;
    upsert_game_status(ctx, running).await;
    Ok(())
}

pub async fn send_night_action_dm(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    actor: &Player,
) -> std::result::Result<SecretDeliveryRoute, SecretDeliveryFailure> {
    let (guild_id, role, can_change, targets, contractor_draft) = {
        let running_read = running.read().await;
        let role = effective_night_role(&running_read.game, actor);
        let targets = if role == Role::Contractor {
            running_read.game.contractor_contract_targets(actor)
        } else {
            night_targets(&running_read.game, actor)
        };
        let contractor_draft = if role == Role::Contractor {
            running_read
                .contractor_contract_drafts
                .get(&actor.user_id)
                .cloned()
                .unwrap_or_default()
        } else {
            ContractorContractDraft::default()
        };
        (
            running_read.guild_id,
            role,
            running_read.game.night_action_can_be_changed(actor),
            targets,
            contractor_draft,
        )
    };
    if targets.is_empty() && role != Role::Reporter {
        return Ok(SecretDeliveryRoute::NotRequired);
    };
    if role == Role::Contractor {
        return send_player_secret_detailed(
            ctx,
            running,
            actor,
            contractor_contract_prompt(&targets, &contractor_draft),
            contractor_contract_components(guild_id, actor.user_id, &targets, &contractor_draft),
        )
        .await;
    }
    // 공무원은 플레이어가 아니라 직업을 고르므로 전용 셀렉트를 쓴다.
    if role == Role::CivilServant {
        return send_player_secret_detailed(
            ctx,
            running,
            actor,
            "공무원 조회할 직업을 선택하세요\n밤이 끝날 때 그 직업을 가진 생존자를 알려드립니다.\n**조회는 밤마다 한 번뿐이며, 제출 후에는 바꿀 수 없습니다.** 이번 게임에 없는 직업을 골라도 조회는 소모됩니다.",
            civil_servant_query_components(guild_id, actor.user_id),
        )
        .await;
    }
    let mut prompt = if can_change {
        format!(
            "{} 밤 행동을 선택하세요\n밤이 끝나기 전 다시 선택하면 대상을 변경할 수 있습니다.",
            role.value()
        )
    } else {
        format!("{} 밤 행동을 선택하세요", role.value())
    };
    if let Some(notice) = night_action_notice(role) {
        prompt.push_str("\n\n");
        prompt.push_str(notice);
    }
    send_player_secret_detailed(
        ctx,
        running,
        actor,
        prompt,
        night_action_components(guild_id, actor.user_id, role, &targets),
    )
    .await
}

/// 공무원 조회용 직업 셀렉트. 경찰 계열과 시민을 제외한 시민팀 직업 전체를
/// 보여준다(이번 게임에 없는 직업도 포함 — 헛조회도 규칙의 일부).
pub fn civil_servant_query_components(
    guild_id: serenity::GuildId,
    actor_id: u64,
) -> Vec<serenity::CreateActionRow> {
    let options = mafia_remake::model::CIVIL_SERVANT_QUERY_ROLES
        .iter()
        .take(25)
        .map(|role| serenity::CreateSelectMenuOption::new(role.value(), role.value()))
        .collect::<Vec<_>>();
    vec![serenity::CreateActionRow::SelectMenu(
        serenity::CreateSelectMenu::new(
            format!("civilquery:{}:{}", guild_id.get(), actor_id),
            serenity::CreateSelectMenuKind::String { options },
        )
        .placeholder("조회할 직업을 선택하세요 (밤마다 1회, 변경 불가)")
        .min_values(1)
        .max_values(1),
    )]
}

/// 사용 제한이 있는 밤 능력은 선택 화면에서 그 사실을 알린다.
pub fn night_action_notice(role: Role) -> Option<&'static str> {
    match role {
        Role::Inspector => Some(
            "**이 수사는 1회용입니다.** 게임 중 한 번만 사용할 수 있고, 결과는 제출 즉시 나오며 대상을 바꿀 수 없습니다.",
        ),
        Role::Priest => Some("**이 소생은 1회용입니다.** 게임 중 한 번만 사용할 수 있습니다."),
        Role::Police => {
            Some("**조사 대상은 제출 즉시 결과가 나오며, 이번 밤에는 다시 바꿀 수 없습니다.**")
        }
        _ => None,
    }
}

pub fn night_action_components(
    guild_id: serenity::GuildId,
    actor_id: u64,
    role: Role,
    targets: &[Player],
) -> Vec<serenity::CreateActionRow> {
    let mut options = targets
        .iter()
        .take(if role == Role::Reporter { 24 } else { 25 })
        .map(|target| {
            serenity::CreateSelectMenuOption::new(
                target.name.chars().take(100).collect::<String>(),
                target.user_id.to_string(),
            )
        })
        .collect::<Vec<_>>();
    if role == Role::Reporter {
        options.push(serenity::CreateSelectMenuOption::new("사용 안함", "skip"));
    }
    let select = serenity::CreateSelectMenu::new(
        format!("night:{}:{}:{}", guild_id.get(), actor_id, role.value()),
        serenity::CreateSelectMenuKind::String { options },
    )
    .placeholder(night_placeholder(role))
    .min_values(1)
    .max_values(1);
    vec![serenity::CreateActionRow::SelectMenu(select)]
}

pub fn terrorist_final_defense_components(
    guild_id: serenity::GuildId,
    actor_id: u64,
    targets: &[Player],
) -> Vec<serenity::CreateActionRow> {
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
        format!("terrorist_defense:{}:{}", guild_id.get(), actor_id),
        serenity::CreateSelectMenuKind::String { options },
    )
    .placeholder("습격할 대상을 선택하세요")
    .min_values(1)
    .max_values(1);
    vec![serenity::CreateActionRow::SelectMenu(select)]
}

pub fn contractor_contract_components(
    guild_id: serenity::GuildId,
    actor_id: u64,
    targets: &[Player],
    draft: &ContractorContractDraft,
) -> Vec<serenity::CreateActionRow> {
    let target_row = |slot: usize| {
        let other_target_id = draft.target_ids[1 - slot];
        let target_options = targets
            .iter()
            .filter(|target| Some(target.user_id) != other_target_id)
            .take(25)
            .map(|target| {
                serenity::CreateSelectMenuOption::new(
                    target.name.chars().take(100).collect::<String>(),
                    target.user_id.to_string(),
                )
            })
            .collect::<Vec<_>>();
        let placeholder = draft.target_ids[slot]
            .and_then(|target_id| {
                targets
                    .iter()
                    .find(|target| target.user_id == target_id)
                    .map(|target| format!("{}번 대상: {}", slot + 1, target.name))
            })
            .unwrap_or_else(|| format!("{}번 청부 대상 선택", slot + 1));
        serenity::CreateActionRow::SelectMenu(
            serenity::CreateSelectMenu::new(
                format!("contractor_target:{}:{}:{}", guild_id.get(), actor_id, slot),
                serenity::CreateSelectMenuKind::String {
                    options: target_options,
                },
            )
            .placeholder(placeholder)
            .min_values(1)
            .max_values(1),
        )
    };
    // 추측 가능한 직업이 25개 이하라 팀 구분 없이 한 목록으로 보여준다.
    let role_row = |slot: usize| {
        let role_options = contractor_guessable_roles()
            .take(25)
            .map(|role| serenity::CreateSelectMenuOption::new(role.value(), role.value()))
            .collect::<Vec<_>>();
        let placeholder = match draft.guessed_roles[slot] {
            Some(role) => format!("{}번 대상 직업: {}", slot + 1, role.value()),
            None => format!("{}번 대상 직업 선택", slot + 1),
        };
        serenity::CreateActionRow::SelectMenu(
            serenity::CreateSelectMenu::new(
                format!("contractor_role:{}:{}:{}", guild_id.get(), actor_id, slot),
                serenity::CreateSelectMenuKind::String {
                    options: role_options,
                },
            )
            .placeholder(placeholder)
            .min_values(1)
            .max_values(1),
        )
    };
    let submit_row = serenity::CreateActionRow::Buttons(vec![
        serenity::CreateButton::new(format!("contractor_submit:{}:{}", guild_id.get(), actor_id))
            .label("청부 확정")
            .style(serenity::ButtonStyle::Success),
    ]);

    vec![
        target_row(0),
        role_row(0),
        target_row(1),
        role_row(1),
        submit_row,
    ]
}

pub fn contractor_contract_prompt(targets: &[Player], draft: &ContractorContractDraft) -> String {
    let target_line = |slot: usize| {
        let target_name = draft.target_ids[slot]
            .and_then(|target_id| {
                targets
                    .iter()
                    .find(|target| target.user_id == target_id)
                    .map(|target| target.name.as_str())
            })
            .unwrap_or("미선택");
        let role_name = draft.guessed_roles[slot]
            .map(Role::value)
            .unwrap_or("직업 미선택");
        format!("{}번 대상: {} -> {}", slot + 1, target_name, role_name)
    };
    format!(
        "두 명과 각 직업을 추측합니다. 대상과 직업은 어느 쪽을 먼저 골라도 됩니다.\n둘 중 한 명이라도 마피아를 정확히 맞히면 접선합니다. 직업이 공개된 사람은 대상에서 제외되고, 경찰 계열 직업은 추측할 수 없습니다.\n\n{}\n{}\n\n밤이 끝나기 전 다시 확정하면 청부 대상을 변경할 수 있습니다.",
        target_line(0),
        target_line(1),
    )
}

pub fn night_placeholder(role: Role) -> &'static str {
    match role {
        Role::Mafia => "공격할 대상을 선택하세요",
        Role::Doctor => "보호할 대상을 선택하세요",
        Role::Nurse => "처방/치료 대상을 선택하세요",
        Role::Police => "조사할 대상을 선택하세요 (밤마다 1회, 변경 불가)",
        Role::Inspector => "수사할 대상을 선택하세요 (1회용, 변경 불가)",
        Role::CivilServant => "조회할 직업을 선택하세요",
        Role::Vigilante => "숙청할 대상을 선택하세요",
        Role::Hypnotist => "최면을 걸 대상을 선택하세요",
        Role::Mercenary => "처형할 대상을 선택하세요",
        Role::Reporter => "특종 대상 또는 사용 안함을 선택하세요",
        Role::Detective => "추적할 대상을 선택하세요",
        Role::Shaman => "성불할 사망자를 선택하세요",
        Role::Priest => "소생할 사망자를 선택하세요",
        Role::Spy => "첩보할 대상을 선택하세요",
        Role::Witch => "저주할 대상을 선택하세요",
        Role::Godfather => "확정 처치할 대상을 선택하세요",
        Role::Terrorist => "지목할 대상을 선택하세요",
        Role::Gangster => "공갈할 대상을 선택하세요",
        Role::Thief => "도벽으로 훔친 능력의 대상을 선택하세요",
        Role::CultLeader => "포교할 대상을 선택하세요",
        Role::Fanatic => "추종할 대상을 선택하세요",
        _ => "대상을 선택하세요",
    }
}

pub fn effective_night_role(game: &MafiaGame, actor: &Player) -> Role {
    if actor.role == Role::Thief {
        game.thief_night_role(actor).unwrap_or(actor.role)
    } else {
        actor.role
    }
}

pub fn night_targets(game: &MafiaGame, actor: &Player) -> Vec<Player> {
    let role = effective_night_role(game, actor);
    let mut alive = game
        .alive_players()
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    alive.sort_by_key(|player| player.name.to_lowercase());
    let mut targets = match role {
        Role::Mafia => alive
            .into_iter()
            .filter(|player| game.can_mafia_attack(player, Some(actor.user_id)))
            .collect(),
        Role::Doctor => alive,
        Role::Nurse => {
            if game.nurse_contacted.contains(&actor.user_id) {
                if game.alive_role_count(Role::Doctor) == 0 {
                    alive
                } else {
                    Vec::new()
                }
            } else {
                alive
                    .into_iter()
                    .filter(|player| player.user_id != actor.user_id)
                    .collect()
            }
        }
        Role::Shaman | Role::Priest => game
            .unpurified_dead_players()
            .into_iter()
            .cloned()
            .collect(),
        // [조문] 훔친 능력이 없는 도둑의 밤 대상은 성불 전 사망자다.
        Role::Thief => game
            .unpurified_dead_players()
            .into_iter()
            .cloned()
            .collect(),
        Role::CultLeader => alive
            .into_iter()
            .filter(|player| player.user_id != actor.user_id && !game.is_cult_team(player))
            .collect(),
        Role::Vigilante => game.vigilante_execution_targets(actor),
        Role::Contractor => game.contractor_contract_targets(actor),
        _ => alive
            .into_iter()
            .filter(|player| player.user_id != actor.user_id)
            .collect(),
    };
    targets.sort_by_key(|player| player.name.to_lowercase());
    targets
}

pub async fn send_private_result_maps(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    result: &NightResult,
) {
    let mut maps = vec![
        result.detective_results.clone(),
        result.inspector_target_notices.clone(),
        result.civil_servant_results.clone(),
        result.paparazzi_results.clone(),
        result.fraudster_results.clone(),
        result.soldier_watch_results.clone(),
        result.tier_ability_results.clone(),
        result.spy_results.clone(),
        result.contractor_results.clone(),
        result.witch_results.clone(),
        result.godfather_results.clone(),
        result.shaman_results.clone(),
        result.priest_results.clone(),
        result.agent_results.clone(),
        result.thief_police_results.clone(),
        result.reporter_results.clone(),
        result.vigilante_results.clone(),
        result.mercenary_results.clone(),
        result.nurse_results.clone(),
        result.gangster_results.clone(),
        result.cult_results.clone(),
        result.fanatic_results.clone(),
    ];
    maps.push(result.hacker_results.clone());
    for map in maps {
        for (user_id, text) in map {
            let player = running.read().await.game.get_player(user_id).cloned();
            if let Some(player) = player {
                let _ = send_player_secret(ctx, running, &player, text, vec![]).await;
            }
        }
    }
    let _ = running;
}

pub async fn announce_police_result(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    result: &NightResult,
) {
    let (police_players, message) = {
        let running_read = running.read().await;
        if running_read.game.police_result_announced {
            return;
        }
        let police_players = running_read
            .game
            .alive_players()
            .into_iter()
            .filter(|player| player.role == Role::Police)
            // 재안내는 그 밤 조사를 제출한 경찰에게만 간다. 도굴로 밤 중에
            // 경찰이 된 플레이어가 죽은 경찰의 결과를 물려받으면 안 된다.
            .filter(|player| {
                result.police_actor_ids.is_empty()
                    || result.police_actor_ids.contains(&player.user_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        if police_players.is_empty() {
            return;
        }
        let message = if let Some(target) = &result.police_target {
            let result_text = if result.police_target_is_mafia.unwrap_or(false) {
                "마피아입니다"
            } else {
                "마피아가 아닙니다"
            };
            format!("조사 결과: {} 님은 **{}**.", target.name, result_text)
        } else {
            "이번 밤 경찰 조사가 없었습니다.".to_string()
        };
        (police_players, message)
    };
    {
        let mut running_write = running.write().await;
        running_write.game.mark_police_result_announced();
    }
    for player in police_players {
        let _ = send_player_secret(ctx, running, &player, message.clone(), vec![]).await;
    }
}

pub async fn announce_public_police_status(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    config: &config::BotConfig,
    police_can_act: bool,
    result: &NightResult,
) -> Result<()> {
    if !config.reveal_public_police_status || !police_can_act {
        return Ok(());
    }
    let (message, color) = if result.police_target.is_none() {
        (
            "이번 밤 경찰 조사가 진행되지 않았습니다.",
            serenity::Colour::ORANGE,
        )
    } else if result.police_target_is_mafia.unwrap_or(false) {
        (
            "경찰이 마피아를 발견했습니다. 자세한 조사 결과는 경찰 비공개 채널로 전달됩니다.",
            serenity::Colour::DARK_GREEN,
        )
    } else {
        (
            "경찰이 마피아를 발견하지 못했습니다. 자세한 조사 결과는 경찰 비공개 채널로 전달됩니다.",
            serenity::Colour::ORANGE,
        )
    };
    send_game_embed(
        ctx,
        running,
        message,
        "경찰 조사 결과 공개",
        color,
        vec![],
        true,
        true,
    )
    .await?;
    Ok(())
}

pub async fn announce_morning_mafia_count(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    config: &config::BotConfig,
) -> Result<()> {
    if !config.reveal_morning_mafia_count {
        return Ok(());
    }
    let mafia_count = {
        let running_read = running.read().await;
        running_read
            .game
            .alive_players()
            .into_iter()
            .filter(|player| running_read.game.is_known_mafia_team(player))
            .count()
    };
    send_game_embed(
        ctx,
        running,
        format!("현재 생존 마피아: **{mafia_count}명**"),
        "아침 마피아 현황",
        serenity::Colour::GOLD,
        vec![],
        true,
        true,
    )
    .await?;
    Ok(())
}

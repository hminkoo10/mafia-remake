// commands/interactions.rs — 버튼·셀렉트·모달 인터랙션 핸들러 (밤 행동, 투표, 참가 등)

use super::*;

pub async fn handle_component(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
) -> Result<()> {
    let custom_id = component.data.custom_id.as_str();
    let parts = custom_id.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        ["join", guild] => handle_join(ctx, data, component, parse_guild(guild)?).await?,
        ["spectate", guild] => handle_spectate(ctx, data, component, parse_guild(guild)?).await?,
        ["startnow", guild] => {
            handle_recruitment_finish(ctx, data, component, parse_guild(guild)?, false).await?
        }
        ["cancelrec", guild] => {
            handle_recruitment_finish(ctx, data, component, parse_guild(guild)?, true).await?
        }
        ["autostart", guild] => {
            handle_auto_start_open(ctx, data, component, parse_guild(guild)?).await?
        }
        ["lastwill", guild, user] => {
            handle_last_will_open(ctx, data, component, parse_guild(guild)?, user.parse()?).await?
        }
        ["night", guild, actor_id, _role] => {
            handle_night_action(ctx, data, component, parse_guild(guild)?, actor_id.parse()?)
                .await?
        }
        ["civilquery", guild, actor_id] => {
            handle_civil_servant_query(ctx, data, component, parse_guild(guild)?, actor_id.parse()?)
                .await?
        }
        ["terrorist_defense", guild, actor_id] => {
            handle_terrorist_final_defense_target(
                ctx,
                data,
                component,
                parse_guild(guild)?,
                actor_id.parse()?,
            )
            .await?
        }
        ["contractor_target", guild, actor_id, slot] => {
            handle_contractor_target(
                ctx,
                data,
                component,
                parse_guild(guild)?,
                actor_id.parse()?,
                slot.parse()?,
            )
            .await?
        }
        ["contractor_role", guild, actor_id, slot] => {
            handle_contractor_role(
                ctx,
                data,
                component,
                parse_guild(guild)?,
                actor_id.parse()?,
                slot.parse()?,
            )
            .await?
        }
        ["contractor_group", guild, actor_id, group] => {
            handle_contractor_group(
                ctx,
                data,
                component,
                parse_guild(guild)?,
                actor_id.parse()?,
                group,
            )
            .await?
        }
        ["contractor_submit", guild, actor_id] => {
            handle_contractor_submit(ctx, data, component, parse_guild(guild)?, actor_id.parse()?)
                .await?
        }
        ["vote", guild] => handle_day_vote(ctx, data, component, parse_guild(guild)?).await?,
        ["confirm", guild, approve] => {
            handle_confirm_vote(ctx, data, component, parse_guild(guild)?, *approve == "1").await?
        }
        ["skipday", guild] => handle_skip_day(ctx, data, component, parse_guild(guild)?).await?,
        ["extendday", guild] => {
            handle_day_extension(ctx, data, component, parse_guild(guild)?).await?
        }
        ["hacker", guild, actor_id] => {
            handle_hacker(ctx, data, component, parse_guild(guild)?, actor_id.parse()?).await?
        }
        ["vigilante", guild, actor_id] => {
            handle_vigilante(ctx, data, component, parse_guild(guild)?, actor_id.parse()?).await?
        }
        ["psychologist", guild, actor_id] => {
            handle_psychologist(ctx, data, component, parse_guild(guild)?, actor_id.parse()?)
                .await?
        }
        ["hypnotist", guild, actor_id] => {
            handle_hypnotist(ctx, data, component, parse_guild(guild)?, actor_id.parse()?).await?
        }
        ["thief", guild, actor_id] => {
            handle_thief(ctx, component, parse_guild(guild)?, actor_id.parse()?).await?
        }
        _ => ack_component(ctx, component).await,
    }
    Ok(())
}

pub async fn handle_modal(
    ctx: &serenity::Context,
    data: &Data,
    modal: &serenity::ModalInteraction,
) -> Result<()> {
    let custom_id = modal.data.custom_id.as_str();
    let parts = custom_id.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        ["autostart", guild] => {
            handle_auto_start_submit(ctx, data, modal, parse_guild(guild)?).await?;
        }
        ["lastwill", guild, user] => {
            handle_last_will_submit(ctx, data, modal, parse_guild(guild)?, user.parse()?).await?;
        }
        _ => {}
    }
    Ok(())
}

pub fn modal_value(modal: &serenity::ModalInteraction, custom_id: &str) -> Option<String> {
    modal
        .data
        .components
        .iter()
        .flat_map(|row| row.components.iter())
        .find_map(|component| match component {
            serenity::ActionRowComponent::InputText(input) if input.custom_id == custom_id => {
                input.value.clone()
            }
            _ => None,
        })
}

pub async fn send_modal_private(
    ctx: &serenity::Context,
    modal: &serenity::ModalInteraction,
    message: impl Into<String>,
    color: serenity::Colour,
) -> serenity::Result<()> {
    modal
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::Message(
                serenity::CreateInteractionResponseMessage::new()
                    .embed(make_embed(message, "마피아 게임", color))
                    .ephemeral(true),
            ),
        )
        .await
}

pub fn parse_guild(value: &str) -> Result<serenity::GuildId> {
    Ok(serenity::GuildId::new(value.parse()?))
}

pub fn selected_values(component: &serenity::ComponentInteraction) -> Vec<String> {
    match &component.data.kind {
        serenity::ComponentInteractionDataKind::StringSelect { values } => values.clone(),
        _ => Vec::new(),
    }
}

/// 실시간 추적 알림을 즉시 DM으로 전달한다 (밤 행동 제출 직후 호출).
pub(crate) async fn deliver_detective_live_notices(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
) {
    let notices = running.write().await.game.take_detective_live_notices();
    for (detective_id, text) in notices {
        let player = running.read().await.game.get_player(detective_id).cloned();
        if let Some(player) = player {
            let _ = send_player_secret(ctx, running, &player, text, vec![]).await;
        }
    }
}

pub(crate) fn contractor_live_view(
    running: &mut RunningGame,
    actor_id: u64,
) -> Result<(Vec<Player>, ContractorContractDraft)> {
    let Some(actor) = running.game.get_player(actor_id).cloned() else {
        bail!("청부업자 정보를 찾을 수 없습니다.");
    };
    if !running.game.contractor_can_use_contract(actor_id) {
        bail!("청부를 사용할 수 있는 상태가 아닙니다.");
    }

    let targets = running.game.contractor_contract_targets(&actor);
    let draft = running
        .contractor_contract_drafts
        .entry(actor_id)
        .or_default();
    for slot in 0..2 {
        if draft.guessed_roles[slot].is_some_and(|role| !is_contractor_guess_role(role)) {
            draft.guessed_roles[slot] = None;
        }
        if draft.target_ids[slot]
            .is_some_and(|target_id| !targets.iter().any(|target| target.user_id == target_id))
        {
            draft.target_ids[slot] = None;
            draft.guessed_roles[slot] = None;
        }
    }
    if draft.target_ids[0].is_some() && draft.target_ids[0] == draft.target_ids[1] {
        draft.target_ids[1] = None;
        draft.guessed_roles[1] = None;
    }
    Ok((targets, draft.clone()))
}

pub(crate) fn set_contractor_draft_target(
    draft: &mut ContractorContractDraft,
    slot: usize,
    target_id: u64,
) -> Result<()> {
    if slot >= 2 {
        bail!("잘못된 청부 선택입니다.");
    }
    // 반대 슬롯과 같은 대상을 고르면(메시지 갱신 전의 잔상 옵션으로 가능)
    // 최근 선택이 이기고 반대 슬롯을 비운다. 여기서 에러를 내면 응답 없이
    // 끝나 "확정이 안 되는" 막다른 상태가 된다.
    if draft.target_ids[1 - slot] == Some(target_id) {
        draft.target_ids[1 - slot] = None;
        draft.guessed_roles[1 - slot] = None;
    }
    // 대상을 바꿔도 이미 고른 직업 추측은 유지한다 (다시 고르면 덮어쓴다).
    draft.target_ids[slot] = Some(target_id);
    Ok(())
}

pub(crate) fn contractor_draft_submission(
    draft: &ContractorContractDraft,
) -> Option<(u64, u64, Role, Role)> {
    Some((
        draft.target_ids[0]?,
        draft.target_ids[1]?,
        draft.guessed_roles[0]?,
        draft.guessed_roles[1]?,
    ))
}

pub(crate) async fn update_contractor_message(
    ctx: &serenity::Context,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
    targets: &[Player],
    draft: &ContractorContractDraft,
) -> Result<()> {
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::UpdateMessage(
                serenity::CreateInteractionResponseMessage::new()
                    .embed(make_embed(
                        contractor_contract_prompt(targets, draft),
                        "청부업자 밤 행동",
                        serenity::Colour::DARK_GREEN,
                    ))
                    .components(contractor_contract_components(
                        guild_id, actor_id, targets, draft,
                    )),
            ),
        )
        .await?;
    Ok(())
}

pub async fn handle_contractor_target(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
    slot: usize,
) -> Result<()> {
    if component.user.id.get() != actor_id {
        send_component_private(ctx, component, "본인에게 온 선택지만 사용할 수 있습니다.").await?;
        return Ok(());
    }
    if slot >= 2 {
        send_component_private(ctx, component, "잘못된 청부 선택입니다.").await?;
        return Ok(());
    }
    let Some(target_id) = selected_values(component)
        .first()
        .and_then(|value| value.parse().ok())
    else {
        send_component_private(ctx, component, "청부 대상을 선택해야 합니다.").await?;
        return Ok(());
    };
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };

    let view = {
        let mut running_write = running.write().await;
        match contractor_live_view(&mut running_write, actor_id) {
            Ok((targets, _)) => {
                if !targets.iter().any(|target| target.user_id == target_id) {
                    Err(anyhow::anyhow!("현재 선택할 수 없는 청부 대상입니다."))
                } else {
                    let draft = running_write
                        .contractor_contract_drafts
                        .get_mut(&actor_id)
                        .expect("청부 초안이 생성되어야 합니다.");
                    // 에러를 `?`로 흘리면 인터랙션 응답 없이 끝나 "상호작용
                    // 실패"만 뜬다. 반드시 사용자에게 보이는 경로로 처리한다.
                    match set_contractor_draft_target(draft, slot, target_id) {
                        Ok(()) => Ok((targets, draft.clone())),
                        Err(error) => Err(error),
                    }
                }
            }
            Err(error) => Err(error),
        }
    };
    let (targets, draft) = match view {
        Ok(view) => view,
        Err(error) => {
            send_component_private(ctx, component, error.to_string()).await?;
            return Ok(());
        }
    };
    update_contractor_message(ctx, component, guild_id, actor_id, &targets, &draft).await?;
    Ok(())
}

pub async fn handle_contractor_role(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
    slot: usize,
) -> Result<()> {
    if component.user.id.get() != actor_id {
        send_component_private(ctx, component, "본인에게 온 선택지만 사용할 수 있습니다.").await?;
        return Ok(());
    }
    if slot >= 2 {
        send_component_private(ctx, component, "잘못된 청부 선택입니다.").await?;
        return Ok(());
    }
    let Some(role) = selected_values(component)
        .first()
        .and_then(|value| find_role_by_name(value))
    else {
        send_component_private(ctx, component, "청부 대상 직업을 선택해야 합니다.").await?;
        return Ok(());
    };
    if !is_contractor_guess_role(role) {
        send_component_private(ctx, component, "청부로 추측할 수 없는 직업입니다.").await?;
        return Ok(());
    }
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let view = {
        let mut running_write = running.write().await;
        match contractor_live_view(&mut running_write, actor_id) {
            Ok((targets, _)) => {
                let draft = running_write
                    .contractor_contract_drafts
                    .get_mut(&actor_id)
                    .expect("청부 초안이 생성되어야 합니다.");
                // 대상보다 직업을 먼저 골라도 된다.
                draft.guessed_roles[slot] = Some(role);
                Ok((targets, draft.clone()))
            }
            Err(error) => Err(error),
        }
    };
    let (targets, draft) = match view {
        Ok(view) => view,
        Err(error) => {
            send_component_private(ctx, component, error.to_string()).await?;
            return Ok(());
        }
    };
    update_contractor_message(ctx, component, guild_id, actor_id, &targets, &draft).await?;
    Ok(())
}

pub async fn handle_contractor_group(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
    group_value: &str,
) -> Result<()> {
    if component.user.id.get() != actor_id {
        send_component_private(ctx, component, "본인에게 온 선택지만 사용할 수 있습니다.").await?;
        return Ok(());
    }
    // 직업 목록이 하나로 합쳐져 그룹 전환은 더 이상 없다. 옛 메시지의 버튼을
    // 눌러도 죽지 않도록 화면만 현재 상태로 새로 그린다.
    let _ = group_value;
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let view = {
        let mut running_write = running.write().await;
        contractor_live_view(&mut running_write, actor_id)
    };
    let (targets, draft) = match view {
        Ok(view) => view,
        Err(error) => {
            send_component_private(ctx, component, error.to_string()).await?;
            return Ok(());
        }
    };
    update_contractor_message(ctx, component, guild_id, actor_id, &targets, &draft).await?;
    Ok(())
}

pub async fn handle_contractor_submit(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
) -> Result<()> {
    if component.user.id.get() != actor_id {
        send_component_private(ctx, component, "본인에게 온 선택지만 사용할 수 있습니다.").await?;
        return Ok(());
    }
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let (message, done, newly_contacted_mafia, targets) = {
        let mut running_write = running.write().await;
        let was_known_mafia_team = running_write
            .game
            .get_player(actor_id)
            .is_some_and(|actor| running_write.game.is_known_mafia_team(actor));
        let Some(draft) = running_write
            .contractor_contract_drafts
            .get(&actor_id)
            .cloned()
        else {
            send_component_private(
                ctx,
                component,
                "청부 대상 2명과 각 대상의 직업을 모두 선택하세요.",
            )
            .await?;
            return Ok(());
        };
        let Some((first_target_id, second_target_id, first_role, second_role)) =
            contractor_draft_submission(&draft)
        else {
            send_component_private(
                ctx,
                component,
                "청부 대상 2명과 각 대상의 직업을 모두 선택하세요.",
            )
            .await?;
            return Ok(());
        };
        let message = match running_write.game.submit_contractor_contract(
            actor_id,
            first_target_id,
            first_role,
            second_target_id,
            second_role,
        ) {
            Ok(message) => message,
            Err(error) => {
                send_component_private(ctx, component, error.to_string()).await?;
                return Ok(());
            }
        };
        running_write.record_replay_event(
            "contractor_contract",
            Some(actor_id),
            &[first_target_id, second_target_id],
            serde_json::json!({
                "guesses": [
                    {"target_user_id": first_target_id, "role": first_role.value(), "role_key": format!("{:?}", first_role)},
                    {"target_user_id": second_target_id, "role": second_role.value(), "role_key": format!("{:?}", second_role)}
                ],
                "message": message.clone(),
            }),
        );
        running_write.contractor_contract_drafts.remove(&actor_id);
        let targets = running_write
            .game
            .get_player(actor_id)
            .map(|actor| running_write.game.contractor_contract_targets(actor))
            .unwrap_or_default();
        let newly_contacted_mafia = running_write
            .game
            .get_player(actor_id)
            .filter(|actor| {
                actor.alive
                    && !was_known_mafia_team
                    && running_write.game.is_known_mafia_team(actor)
            })
            .cloned();
        let done = running_write.game.should_finish_night_early();
        (message, done, newly_contacted_mafia, targets)
    };
    if let Some(player) = &newly_contacted_mafia {
        grant_private_role_member_access(ctx, data, &running, Role::Mafia, player).await;
    }
    deliver_detective_live_notices(ctx, &running).await;
    if done {
        running.read().await.night_notify.notify_waiters();
    }
    let reset_draft = ContractorContractDraft::default();
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::UpdateMessage(
                serenity::CreateInteractionResponseMessage::new()
                    .embed(make_embed(
                        format!(
                            "{message}\n\n{}",
                            contractor_contract_prompt(&targets, &reset_draft)
                        ),
                        "밤 행동 완료",
                        serenity::Colour::DARK_GREEN,
                    ))
                    .components(contractor_contract_components(
                        guild_id,
                        actor_id,
                        &targets,
                        &reset_draft,
                    )),
            ),
        )
        .await?;
    if running.read().await.night_timed_events_due {
        trigger_timed_night_events(ctx, data, &running).await?;
    }
    Ok(())
}

pub async fn handle_skip_day(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
) -> Result<()> {
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let user_id = component.user.id.get();
    let outcome = {
        let mut running_write = running.write().await;
        if running_write.game.phase != Phase::Day {
            return send_component_private(ctx, component, "지금 진행 중인 낮 토론이 없습니다.")
                .await
                .map_err(Into::into);
        }
        let alive_ids = running_write
            .game
            .alive_players()
            .into_iter()
            .map(|player| player.user_id)
            .collect::<HashSet<_>>();
        if !alive_ids.contains(&user_id) {
            return send_component_private(
                ctx,
                component,
                "생존 중인 참가자만 바로 투표를 선택할 수 있습니다.",
            )
            .await
            .map_err(Into::into);
        }
        let required_votes = majority_required(alive_ids.len());
        if running_write.day_skip_voter_ids.contains(&user_id) {
            return send_component_private(
                ctx,
                component,
                format!(
                    "이미 바로 투표에 동의했습니다. 현재 {}/{}명",
                    running_write.day_skip_voter_ids.len(),
                    required_votes
                ),
            )
            .await
            .map_err(Into::into);
        }
        running_write.day_skip_voter_ids.insert(user_id);
        let vote_count = running_write.day_skip_voter_ids.len();
        running_write.record_replay_event(
            "day_skip_vote",
            Some(user_id),
            &[],
            serde_json::json!({
                "vote_count": vote_count,
                "required_votes": required_votes,
                "alive_count": alive_ids.len(),
                "confirmed": vote_count >= required_votes,
            }),
        );
        if vote_count < required_votes {
            return send_component_private(
                ctx,
                component,
                format!("바로 투표에 동의했습니다. 현재 {vote_count}/{required_votes}명"),
            )
            .await
            .map_err(Into::into);
        }
        running_write.day_skip_confirmed = true;
        running_write.day_extension_active = false;
        (
            vote_count,
            alive_ids.len(),
            running_write.day_notify.clone(),
            running_write.guild_id,
        )
    };
    let (vote_count, alive_count, notify, guild_id) = outcome;
    notify.notify_waiters();
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::UpdateMessage(
                serenity::CreateInteractionResponseMessage::new()
                    .embed(make_embed(
                        format!(
                            "생존자 과반수가 바로 투표를 선택했습니다. ({vote_count}/{alive_count}명)\n토론을 끝내고 바로 지목 투표로 넘어갑니다."
                        ),
                        "바로 투표",
                        serenity::Colour::DARK_GREEN,
                    ))
                    .components(day_skip_components(guild_id, true, true)),
            ),
        )
        .await?;
    if running.read().await.night_timed_events_due {
        trigger_timed_night_events(ctx, data, &running).await?;
    }
    Ok(())
}

pub async fn handle_day_extension(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
) -> Result<()> {
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let user_id = component.user.id.get();
    let outcome = {
        let mut running_write = running.write().await;
        if !running_write.day_extension_active {
            return send_component_private(ctx, component, "연장 투표가 종료되었습니다.")
                .await
                .map_err(Into::into);
        }
        if running_write.game.phase != Phase::Day {
            return send_component_private(ctx, component, "지금 진행 중인 낮 토론이 없습니다.")
                .await
                .map_err(Into::into);
        }
        let alive_ids = running_write
            .game
            .alive_players()
            .into_iter()
            .map(|player| player.user_id)
            .collect::<HashSet<_>>();
        if !alive_ids.contains(&user_id) {
            return send_component_private(
                ctx,
                component,
                "생존 중인 참가자만 연장 투표를 할 수 있습니다.",
            )
            .await
            .map_err(Into::into);
        }
        let required_votes = majority_required(alive_ids.len());
        if running_write.day_extension_voter_ids.contains(&user_id) {
            return send_component_private(
                ctx,
                component,
                format!(
                    "이미 1분 연장에 투표했습니다. 현재 {}/{}명",
                    running_write.day_extension_voter_ids.len(),
                    required_votes
                ),
            )
            .await
            .map_err(Into::into);
        }
        running_write.day_extension_voter_ids.insert(user_id);
        let vote_count = running_write.day_extension_voter_ids.len();
        running_write.record_replay_event(
            "day_extension_vote",
            Some(user_id),
            &[],
            serde_json::json!({
                "vote_count": vote_count,
                "required_votes": required_votes,
                "alive_count": alive_ids.len(),
                "confirmed": vote_count >= required_votes,
            }),
        );
        if vote_count < required_votes {
            return send_component_private(
                ctx,
                component,
                format!("1분 연장에 투표했습니다. 현재 {vote_count}/{required_votes}명"),
            )
            .await
            .map_err(Into::into);
        }
        running_write.day_extension_confirmed = true;
        running_write.day_extension_active = false;
        (
            vote_count,
            alive_ids.len(),
            running_write.day_notify.clone(),
            running_write.guild_id,
        )
    };
    let (vote_count, alive_count, notify, guild_id) = outcome;
    notify.notify_waiters();
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::UpdateMessage(
                serenity::CreateInteractionResponseMessage::new()
                    .embed(make_embed(
                        format!(
                            "생존자 과반수가 1분 연장을 선택했습니다. ({vote_count}/{alive_count}명)\n낮 토론을 1분 연장합니다."
                        ),
                        "낮 토론 연장",
                        serenity::Colour::DARK_GREEN,
                    ))
                    .components(day_extension_components(guild_id, true, true)),
            ),
        )
        .await?;
    if running.read().await.night_timed_events_due {
        trigger_timed_night_events(ctx, data, &running).await?;
    }
    Ok(())
}

pub async fn handle_join(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
) -> Result<()> {
    let Some(recruitment) = data.recruitments.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "참가자 모집이 종료되었습니다.").await?;
        return Ok(());
    };
    let mut rec = recruitment.write().await;
    if !rec.accepting {
        send_component_private(ctx, component, "참가자 모집이 종료되었습니다.").await?;
        return Ok(());
    }
    let user_id = component.user.id.get();
    let config_snapshot = data.config.read().await;
    if is_blacklisted(&config_snapshot, user_id) {
        send_component_private(
            ctx,
            component,
            "블랙리스트에 등록된 유저는 참가할 수 없습니다.",
        )
        .await?;
        return Ok(());
    }
    drop(config_snapshot);
    if rec.joined_ids.contains(&user_id) {
        send_component_private(ctx, component, "이미 참가했습니다.").await?;
        return Ok(());
    }
    if rec.spectator_ids.contains(&user_id) {
        send_component_private(ctx, component, "이미 관전자로 등록되어 있습니다.").await?;
        return Ok(());
    }
    if rec.joined_ids.len() >= rec.max_players {
        send_component_private(
            ctx,
            component,
            format!(
                "최대 참가 인원 {}명에 도달해 더 이상 참가할 수 없습니다.",
                rec.max_players
            ),
        )
        .await?;
        return Ok(());
    }
    if let Some(member) = component.member.clone() {
        if !member.roles.contains(&rec.participant_role_id) {
            let role_id = rec.participant_role_id;
            let _ = crate::http_pool::with_fallback(ctx, |http| {
                let member = member.clone();
                async move { member.add_role(&http, role_id).await }
            })
            .await;
        }
        rec.joined_names.insert(user_id, display_name(&member));
    } else {
        rec.joined_names
            .insert(user_id, component.user.name.clone());
    }
    rec.joined_ids.insert(user_id);
    // 자동시작 인원에 도달하면 남은 모집 시간을 기다리지 않고 바로 시작한다.
    let auto_started = auto_start_reached(&rec);
    if auto_started {
        rec.accepting = false;
        rec.done.notify_waiters();
    }
    let updated = rec.clone();
    drop(rec);
    let reply = if auto_started {
        "참가 완료! 자동시작 인원이 모여 바로 시작합니다."
    } else {
        "참가 완료!"
    };
    send_component_private(ctx, component, reply).await?;
    update_recruitment_message(
        ctx,
        data,
        component,
        guild_id,
        &updated,
        RECRUITMENT_STATUS_OPEN,
        auto_started,
    )
    .await;
    Ok(())
}

pub async fn handle_spectate(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
) -> Result<()> {
    let Some(recruitment) = data.recruitments.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "참가자 모집이 종료되었습니다.").await?;
        return Ok(());
    };
    let mut rec = recruitment.write().await;
    if !rec.accepting {
        send_component_private(ctx, component, "참가자 모집이 종료되었습니다.").await?;
        return Ok(());
    }
    let user_id = component.user.id.get();
    if rec.joined_ids.contains(&user_id) {
        send_component_private(ctx, component, "이미 참가자로 등록되어 있습니다.").await?;
        return Ok(());
    }
    if rec.spectator_ids.contains(&user_id) {
        send_component_private(ctx, component, "이미 관전자로 등록되어 있습니다.").await?;
        return Ok(());
    }
    rec.spectator_ids.insert(user_id);
    if let Some(member) = component.member.clone() {
        rec.spectator_names.insert(user_id, display_name(&member));
        if let Some(role_id) = rec.spectator_role_id {
            if !member.roles.contains(&role_id) {
                let _ = crate::http_pool::with_fallback(ctx, |http| {
                    let member = member.clone();
                    async move { member.add_role(&http, role_id).await }
                })
                .await;
            }
        }
    } else {
        rec.spectator_names
            .insert(user_id, component.user.name.clone());
    }
    let updated = rec.clone();
    drop(rec);
    send_component_private(ctx, component, "관전 등록 완료!").await?;
    update_recruitment_message(
        ctx,
        data,
        component,
        guild_id,
        &updated,
        RECRUITMENT_STATUS_OPEN,
        false,
    )
    .await;
    Ok(())
}

/// `자동시작` 버튼: 주최자에게 인원 입력 모달을 띄운다.
pub async fn handle_auto_start_open(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
) -> Result<()> {
    let Some(recruitment) = data.recruitments.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "참가자 모집이 종료되었습니다.").await?;
        return Ok(());
    };
    let (is_host, accepting, modal) = {
        let rec = recruitment.read().await;
        (
            component.user.id == rec.host_user_id,
            rec.accepting,
            auto_start_modal(guild_id, &rec),
        )
    };
    if !is_host {
        send_component_private(ctx, component, "게임을 모집한 주최자만 사용할 수 있습니다.")
            .await?;
        return Ok(());
    }
    if !accepting {
        send_component_private(ctx, component, "참가자 모집이 종료되었습니다.").await?;
        return Ok(());
    }
    component
        .create_response(ctx, serenity::CreateInteractionResponse::Modal(modal))
        .await?;
    Ok(())
}

/// 자동시작 인원 모달 제출. 이미 그 인원이 모여 있으면 즉시 모집을 끝낸다.
pub async fn handle_auto_start_submit(
    ctx: &serenity::Context,
    data: &Data,
    modal: &serenity::ModalInteraction,
    guild_id: serenity::GuildId,
) -> Result<()> {
    let Some(recruitment) = data.recruitments.get(&guild_id).map(|entry| entry.clone()) else {
        send_modal_private(
            ctx,
            modal,
            "참가자 모집이 종료되었습니다.",
            serenity::Colour::RED,
        )
        .await?;
        return Ok(());
    };
    let raw = modal_value(modal, "auto_start_players").unwrap_or_default();
    let mut rec = recruitment.write().await;
    if modal.user.id != rec.host_user_id {
        drop(rec);
        send_modal_private(
            ctx,
            modal,
            "게임을 모집한 주최자만 사용할 수 있습니다.",
            serenity::Colour::RED,
        )
        .await?;
        return Ok(());
    }
    if !rec.accepting {
        drop(rec);
        send_modal_private(
            ctx,
            modal,
            "참가자 모집이 종료되었습니다.",
            serenity::Colour::RED,
        )
        .await?;
        return Ok(());
    }
    let Ok(count) = raw.trim().parse::<usize>() else {
        let (minimum, maximum) = (rec.minimum_players, rec.max_players);
        drop(rec);
        send_modal_private(
            ctx,
            modal,
            format!("인원은 {minimum}~{maximum} 사이의 숫자로 입력하세요."),
            serenity::Colour::RED,
        )
        .await?;
        return Ok(());
    };
    if count < rec.minimum_players || count > rec.max_players {
        let (minimum, maximum) = (rec.minimum_players, rec.max_players);
        drop(rec);
        send_modal_private(
            ctx,
            modal,
            format!(
                "자동시작 인원은 최소 시작 인원 {minimum}명 이상, 최대 참가 인원 {maximum}명 이하여야 합니다."
            ),
            serenity::Colour::RED,
        )
        .await?;
        return Ok(());
    }
    rec.auto_start_players = Some(count);
    let reached = auto_start_reached(&rec);
    if reached {
        rec.accepting = false;
        rec.done.notify_waiters();
    }
    let updated = rec.clone();
    drop(rec);
    let message = if reached {
        format!(
            "이미 {}명이 모여 있어 바로 시작합니다.",
            updated.joined_ids.len()
        )
    } else {
        format!("참가자가 {count}명이 되면 즉시 시작합니다.")
    };
    send_modal_private(ctx, modal, message, serenity::Colour::DARK_GREEN).await?;
    if let Some(recruitment_message) = modal.message.as_ref() {
        update_recruitment_message_at(
            ctx,
            data,
            recruitment_message.channel_id,
            recruitment_message.id,
            guild_id,
            &updated,
            RECRUITMENT_STATUS_OPEN,
            reached,
        )
        .await;
    }
    Ok(())
}

pub async fn handle_recruitment_finish(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    cancelled: bool,
) -> Result<()> {
    let Some(recruitment) = data.recruitments.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "참가자 모집이 이미 종료되었습니다.").await?;
        return Ok(());
    };
    let mut rec = recruitment.write().await;
    if component.user.id != rec.host_user_id {
        send_component_private(ctx, component, "게임을 모집한 주최자만 사용할 수 있습니다.")
            .await?;
        return Ok(());
    }
    if !cancelled && rec.joined_ids.len() < rec.minimum_players {
        send_component_private(
            ctx,
            component,
            format!(
                "아직 시작할 수 없습니다. 최소 {}명이 필요합니다. 현재 {}명입니다.",
                rec.minimum_players,
                rec.joined_ids.len()
            ),
        )
        .await?;
        return Ok(());
    }
    rec.cancelled = cancelled;
    rec.accepting = false;
    let updated = rec.clone();
    rec.done.notify_waiters();
    drop(rec);
    if cancelled {
        ack_component(ctx, component).await;
        update_recruitment_message(
            ctx,
            data,
            component,
            guild_id,
            &updated,
            RECRUITMENT_STATUS_CANCELLED,
            true,
        )
        .await;
    } else {
        ack_component(ctx, component).await;
    }
    Ok(())
}

/// 공무원 조회 제출. 제출 즉시 확정되며 같은 밤에는 바꿀 수 없다.
pub async fn handle_civil_servant_query(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
) -> Result<()> {
    if component.user.id.get() != actor_id {
        send_component_private(ctx, component, "본인에게 온 선택지만 사용할 수 있습니다.").await?;
        return Ok(());
    }
    let Some(role) = selected_values(component)
        .first()
        .and_then(|value| find_role_by_name(value))
    else {
        send_component_private(ctx, component, "조회할 직업을 선택해야 합니다.").await?;
        return Ok(());
    };
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let (message, done) = {
        let mut running_write = running.write().await;
        let message = match running_write
            .game
            .submit_civil_servant_query(actor_id, role)
        {
            Ok(message) => message,
            Err(error) => {
                send_component_private(ctx, component, error.to_string()).await?;
                return Ok(());
            }
        };
        running_write.record_replay_event(
            "night_action",
            Some(actor_id),
            &[],
            serde_json::json!({
                "choice": "role_query",
                "effective_role": Role::CivilServant.value(),
                "effective_role_key": format!("{:?}", Role::CivilServant),
                "queried_role": role.value(),
                "queried_role_key": format!("{:?}", role),
                "message": message.clone(),
            }),
        );
        (message, running_write.game.should_finish_night_early())
    };
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::UpdateMessage(
                serenity::CreateInteractionResponseMessage::new()
                    .embed(make_embed(
                        format!("{message}\n결과는 밤이 끝날 때 전달됩니다."),
                        "공무원 조회",
                        serenity::Colour::DARK_GREEN,
                    ))
                    .components(vec![]),
            ),
        )
        .await?;
    if done {
        running.read().await.night_notify.notify_one();
    }
    Ok(())
}

/// [유언] 버튼: 작성 모달을 띄운다.
pub async fn handle_last_will_open(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    user_id: u64,
) -> Result<()> {
    if component.user.id.get() != user_id {
        send_component_private(ctx, component, "본인에게 온 선택지만 사용할 수 있습니다.").await?;
        return Ok(());
    }
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let current_will = {
        let running_read = running.read().await;
        running_read.game.last_wills.get(&user_id).cloned()
    };
    let mut input = serenity::CreateInputText::new(
        serenity::InputTextStyle::Paragraph,
        "유언 (최대 300자)",
        "last_will_text",
    )
    .placeholder("밤에 사망하면 아침에 모두에게 공개됩니다.")
    .min_length(1)
    .max_length(300)
    .required(true);
    if let Some(will) = current_will.filter(|will| !will.is_empty()) {
        input = input.value(will);
    }
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::Modal(
                serenity::CreateModal::new(
                    format!("lastwill:{}:{}", guild_id.get(), user_id),
                    "유언 작성",
                )
                .components(vec![serenity::CreateActionRow::InputText(input)]),
            ),
        )
        .await?;
    Ok(())
}

pub async fn handle_last_will_submit(
    ctx: &serenity::Context,
    data: &Data,
    modal: &serenity::ModalInteraction,
    guild_id: serenity::GuildId,
    user_id: u64,
) -> Result<()> {
    if modal.user.id.get() != user_id {
        send_modal_private(
            ctx,
            modal,
            "본인에게 온 선택지만 사용할 수 있습니다.",
            serenity::Colour::RED,
        )
        .await?;
        return Ok(());
    }
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_modal_private(
            ctx,
            modal,
            "진행 중인 게임이 없습니다.",
            serenity::Colour::RED,
        )
        .await?;
        return Ok(());
    };
    let text = modal_value(modal, "last_will_text").unwrap_or_default();
    let result = {
        let mut running_write = running.write().await;
        running_write.game.submit_last_will(user_id, &text)
    };
    match result {
        Ok(message) => {
            send_modal_private(ctx, modal, message, serenity::Colour::DARK_GREEN).await?;
        }
        Err(error) => {
            send_modal_private(ctx, modal, error.to_string(), serenity::Colour::RED).await?;
        }
    }
    Ok(())
}

pub async fn handle_night_action(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
) -> Result<()> {
    if component.user.id.get() != actor_id {
        send_component_private(ctx, component, "본인에게 온 선택지만 사용할 수 있습니다.").await?;
        return Ok(());
    }
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let values = selected_values(component);
    let target_id = values.first().and_then(|value| {
        if value == "skip" {
            None
        } else {
            value.parse().ok()
        }
    });
    let (
        message,
        done,
        mafia_action_view,
        changeable_action_view,
        spy_bonus_targets,
        newly_contacted_mafia,
        cult_bells,
        purified_target,
    ) = {
        let mut running_write = running.write().await;
        let was_known_mafia_team = running_write
            .game
            .get_player(actor_id)
            .is_some_and(|actor| running_write.game.is_known_mafia_team(actor));
        let message = match running_write.game.submit_night_action(actor_id, target_id) {
            Ok(message) => message,
            Err(error) => {
                send_component_private(ctx, component, error.to_string()).await?;
                return Ok(());
            }
        };
        // 경찰은 대상을 고른 즉시 조사 결과를 본다. 밤이 끝날 때 나오는 결과는
        // 같은 내용의 재안내다.
        let message = match running_write.game.police_result_for_actor(actor_id) {
            Some(result) => format!("{message}\n{result}"),
            None => message,
        };
        let cult_bells = running_write.game.consume_cult_bells();
        let actor = running_write.game.get_player(actor_id).cloned();
        let effective_role = actor
            .as_ref()
            .map(|actor| effective_night_role(&running_write.game, actor));
        let target_ids = target_id.into_iter().collect::<Vec<_>>();
        running_write.record_replay_event(
            "night_action",
            Some(actor_id),
            &target_ids,
            serde_json::json!({
                "choice": if target_ids.is_empty() { "skip" } else { "player" },
                "effective_role": effective_role.map(|role| role.value()),
                "effective_role_key": effective_role.map(|role| format!("{:?}", role)),
                "message": message.clone(),
            }),
        );
        let newly_contacted_mafia = actor
            .as_ref()
            .filter(|actor| {
                actor.alive
                    && !was_known_mafia_team
                    && running_write.game.is_known_mafia_team(actor)
            })
            .cloned();
        let mafia_action_view = actor.as_ref().and_then(|actor| {
            let role = effective_night_role(&running_write.game, actor);
            if actor.role == Role::Mafia || (actor.role == Role::Thief && role == Role::Mafia) {
                Some((
                    night_targets(&running_write.game, actor),
                    mafia_night_target_status_text(&running_write),
                ))
            } else {
                None
            }
        });
        let changeable_action_view = actor.as_ref().and_then(|actor| {
            if !running_write.game.night_action_can_be_changed(actor) {
                return None;
            }
            let role = effective_night_role(&running_write.game, actor);
            if role == Role::Mafia {
                return None;
            }
            Some((role, night_targets(&running_write.game, actor)))
        });
        let spy_bonus_targets = actor.as_ref().and_then(|actor| {
            if actor.role == Role::Spy && running_write.game.spy_can_use_bonus_action(actor_id) {
                Some(night_targets(&running_write.game, actor))
            } else {
                None
            }
        });
        // [성불] 결과가 즉시 나오므로 사망자 채널 접근도 곧바로 정리한다.
        let purified_target = actor
            .as_ref()
            .filter(|actor| effective_night_role(&running_write.game, actor) == Role::Shaman)
            .and_then(|_| target_id);
        let done = running_write.game.should_finish_night_early();
        (
            message,
            done,
            mafia_action_view,
            changeable_action_view,
            spy_bonus_targets,
            newly_contacted_mafia,
            cult_bells,
            purified_target,
        )
    };
    if let Some(player) = &newly_contacted_mafia {
        grant_private_role_member_access(ctx, data, &running, Role::Mafia, player).await;
    }
    if let Some(purified_id) = purified_target {
        apply_purification_side_effects(ctx, data, &running, &[purified_id]).await;
    }
    deliver_detective_live_notices(ctx, &running).await;
    let response_message = message;
    if let Some((targets, status_text)) = mafia_action_view {
        component
            .create_response(
                ctx,
                serenity::CreateInteractionResponse::UpdateMessage(
                    serenity::CreateInteractionResponseMessage::new()
                        .embed(make_embed(
                            format!("{response_message}\n\n{status_text}"),
                            "마피아 처치 선택",
                            serenity::Colour::DARK_GREEN,
                        ))
                        .components(night_action_components(
                            guild_id,
                            actor_id,
                            Role::Mafia,
                            &targets,
                        )),
                ),
            )
            .await?;
        upsert_private_role_status_message(ctx, &running, Role::Mafia).await;
        if running.read().await.night_timed_events_due {
            trigger_timed_night_events(ctx, data, &running).await?;
        }
        return Ok(());
    }
    if let Some(targets) = spy_bonus_targets {
        component
            .create_response(
                ctx,
                serenity::CreateInteractionResponse::UpdateMessage(
                    serenity::CreateInteractionResponseMessage::new()
                        .embed(make_embed(
                            format!(
                                "{response_message}\n\n추가 첩보를 한 번 더 사용할 수 있습니다."
                            ),
                            "접선 성공",
                            serenity::Colour::DARK_GREEN,
                        ))
                        .components(night_action_components(
                            guild_id,
                            actor_id,
                            Role::Spy,
                            &targets,
                        )),
                ),
            )
            .await?;
        if running.read().await.night_timed_events_due {
            trigger_timed_night_events(ctx, data, &running).await?;
        }
        return Ok(());
    }
    if let Some((role, targets)) = changeable_action_view {
        component
            .create_response(
                ctx,
                serenity::CreateInteractionResponse::UpdateMessage(
                    serenity::CreateInteractionResponseMessage::new()
                        .embed(make_embed(
                            format!(
                                "{response_message}\n\n밤이 끝나기 전 다시 선택하면 대상을 변경할 수 있습니다."
                            ),
                            "밤 행동 완료",
                            serenity::Colour::DARK_GREEN,
                        ))
                        .components(night_action_components(guild_id, actor_id, role, &targets)),
                ),
            )
            .await?;
        if running.read().await.night_timed_events_due {
            trigger_timed_night_events(ctx, data, &running).await?;
        }
        return Ok(());
    }
    if done {
        running.read().await.night_notify.notify_waiters();
    }
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::UpdateMessage(
                serenity::CreateInteractionResponseMessage::new()
                    .embed(make_embed(
                        response_message,
                        "밤 행동 완료",
                        serenity::Colour::DARK_GREEN,
                    ))
                    .components(vec![]),
            ),
        )
        .await?;
    if running.read().await.night_timed_events_due {
        trigger_timed_night_events(ctx, data, &running).await?;
    }
    if cult_bells > 0 {
        send_game_embed(
            ctx,
            &running,
            std::iter::repeat_n("교주의 종소리가 울렸습니다.", cult_bells as usize)
                .collect::<Vec<_>>()
                .join("\n"),
            "교주 포교",
            serenity::Colour::ORANGE,
            vec![],
            true,
            true,
        )
        .await?;
        sync_cult_team_channel_access(ctx, data, &running).await;
    }
    Ok(())
}

pub async fn handle_terrorist_final_defense_target(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
) -> Result<()> {
    if component.user.id.get() != actor_id {
        send_component_private(ctx, component, "본인에게 온 선택지만 사용할 수 있습니다.").await?;
        return Ok(());
    }
    let Some(target_id) = selected_values(component)
        .first()
        .and_then(|value| value.parse::<u64>().ok())
    else {
        send_component_private(ctx, component, "대상을 선택해야 합니다.").await?;
        return Ok(());
    };
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let selection_result = {
        let mut running_write = running.write().await;
        if running_write.final_defense_user_id != Some(actor_id) {
            Err("현재 최후의 반론 대상이 아닙니다.".to_string())
        } else {
            running_write
                .game
                .submit_terrorist_final_defense_target(actor_id, target_id)
                .map(|message| {
                    running_write.record_replay_event(
                        "terrorist_final_defense_target",
                        Some(actor_id),
                        &[target_id],
                        serde_json::json!({
                            "message": message.clone(),
                        }),
                    );
                    message
                })
                .map_err(|error| error.to_string())
        }
    };
    let message = match selection_result {
        Ok(message) => message,
        Err(error) => {
            send_component_private(ctx, component, error).await?;
            return Ok(());
        }
    };
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::UpdateMessage(
                serenity::CreateInteractionResponseMessage::new()
                    .embed(make_embed(
                        message,
                        "테러리스트 습격 대상 선택",
                        serenity::Colour::DARK_GREEN,
                    ))
                    .components(vec![]),
            ),
        )
        .await?;
    Ok(())
}

pub async fn handle_day_vote(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
) -> Result<()> {
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let values = selected_values(component);
    let target_id = values.first().and_then(|value| {
        if value == "skip" {
            None
        } else {
            value.parse().ok()
        }
    });
    let voter_id = component.user.id.get();
    let (message, done, newly_contacted_mafia) = {
        let mut running_write = running.write().await;
        let was_known_mafia_team = running_write
            .game
            .get_player(voter_id)
            .is_some_and(|voter| running_write.game.is_known_mafia_team(voter));
        let message = match running_write.game.submit_day_vote(voter_id, target_id) {
            Ok(message) => message,
            Err(error) => {
                send_component_private(ctx, component, error.to_string()).await?;
                return Ok(());
            }
        };
        let target_ids = target_id.into_iter().collect::<Vec<_>>();
        running_write.record_replay_event(
            "day_vote",
            Some(voter_id),
            &target_ids,
            serde_json::json!({
                "choice": if target_ids.is_empty() { "skip" } else { "player" },
                "message": message.clone(),
            }),
        );
        let newly_contacted_mafia = running_write
            .game
            .get_player(voter_id)
            .filter(|voter| {
                voter.alive
                    && !was_known_mafia_team
                    && running_write.game.is_known_mafia_team(voter)
            })
            .cloned();
        (
            message,
            running_write.game.all_day_votes_submitted(),
            newly_contacted_mafia,
        )
    };
    if let Some(player) = &newly_contacted_mafia {
        grant_private_role_member_access(ctx, data, &running, Role::Mafia, player).await;
    }
    if done {
        running.read().await.vote_notify.notify_waiters();
    }
    send_component_private(ctx, component, message).await?;
    Ok(())
}

pub async fn handle_confirm_vote(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    approve: bool,
) -> Result<()> {
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let (message, done) = {
        let mut running_write = running.write().await;
        let message = match running_write
            .game
            .submit_confirmation_vote(component.user.id.get(), approve)
        {
            Ok(message) => message,
            Err(error) => {
                send_component_private(ctx, component, error.to_string()).await?;
                return Ok(());
            }
        };
        running_write.record_replay_event(
            "confirmation_vote",
            Some(component.user.id.get()),
            &[],
            serde_json::json!({
                "approve": approve,
                "choice": if approve { "approve" } else { "reject" },
                "message": message.clone(),
            }),
        );
        (message, running_write.game.all_confirm_votes_submitted())
    };
    if done {
        running.read().await.confirm_notify.notify_waiters();
    }
    send_component_private(ctx, component, message).await?;
    Ok(())
}

pub async fn handle_hacker(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
) -> Result<()> {
    let value = selected_values(component)
        .first()
        .and_then(|v| v.parse().ok());
    handle_day_action(
        ctx,
        data,
        component,
        guild_id,
        actor_id,
        value,
        "hacker_action",
        "해킹 완료",
        |game, actor, target| game.submit_hacker_action(actor, target),
        |_, _, message| format!("{message}\n밤이 시작될 때 대상의 직업을 확인합니다."),
    )
    .await
}

pub async fn handle_vigilante(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
) -> Result<()> {
    let value = selected_values(component)
        .first()
        .and_then(|v| v.parse().ok());
    handle_day_action(
        ctx,
        data,
        component,
        guild_id,
        actor_id,
        value,
        "vigilante_investigation",
        "숙청 조사 완료",
        |game, actor, target| game.submit_vigilante_investigation(actor, target),
        |_game, _actor, message| message,
    )
    .await
}

pub async fn handle_thief(
    ctx: &serenity::Context,
    component: &serenity::ComponentInteraction,
    _guild_id: serenity::GuildId,
    _actor_id: u64,
) -> Result<()> {
    send_component_private(
        ctx,
        component,
        "도벽은 별도 선택이 아니라 마지막 지목 투표 대상에게 자동으로 적용되고, 결과는 투표가 끝난 뒤 전달됩니다.",
    )
    .await?;
    Ok(())
}

pub async fn handle_psychologist(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
) -> Result<()> {
    if component.user.id.get() != actor_id {
        send_component_private(ctx, component, "본인에게 온 선택지만 사용할 수 있습니다.").await?;
        return Ok(());
    }
    let values = selected_values(component);
    if values.len() < 2 {
        send_component_private(ctx, component, "서로 다른 두 명을 선택해야 합니다.").await?;
        return Ok(());
    }
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let (Some(first), Some(second)) = (
        values.first().and_then(|value| value.parse().ok()),
        values.get(1).and_then(|value| value.parse().ok()),
    ) else {
        ack_component(ctx, component).await;
        return Ok(());
    };
    let message = {
        let mut running_write = running.write().await;
        let message = match running_write
            .game
            .submit_psychologist_observation(actor_id, first, second)
        {
            Ok(message) => message,
            Err(error) => {
                send_component_private(ctx, component, error.to_string()).await?;
                return Ok(());
            }
        };
        running_write.record_replay_event(
            "psychologist_observation",
            Some(actor_id),
            &[first, second],
            serde_json::json!({
                "message": message.clone(),
            }),
        );
        message
    };
    ack_component(ctx, component).await;
    component
        .channel_id
        .edit_message(
            &ctx.http,
            component.message.id,
            serenity::EditMessage::new()
                .embed(make_embed(
                    message,
                    "심리학자 관찰 완료",
                    serenity::Colour::DARK_GREEN,
                ))
                .components(vec![]),
        )
        .await?;
    Ok(())
}

pub async fn handle_hypnotist(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
) -> Result<()> {
    if component.user.id.get() != actor_id {
        send_component_private(ctx, component, "본인에게 온 선택지만 사용할 수 있습니다.").await?;
        return Ok(());
    }
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let message = {
        let mut running_write = running.write().await;
        let mut target_ids = running_write
            .game
            .hypnotized_targets
            .get(&actor_id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect::<Vec<_>>();
        target_ids.sort_unstable();
        match running_write.game.submit_hypnotist_wake(actor_id) {
            Ok(message) => {
                running_write.record_replay_event(
                    "hypnotist_wake",
                    Some(actor_id),
                    &target_ids,
                    serde_json::json!({
                        "message": message.clone(),
                    }),
                );
                message
            }
            Err(error) => {
                send_component_private(ctx, component, error.to_string()).await?;
                return Ok(());
            }
        }
    };
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::UpdateMessage(
                serenity::CreateInteractionResponseMessage::new()
                    .embed(make_embed(
                        message,
                        "최면 해제 완료",
                        serenity::Colour::DARK_GREEN,
                    ))
                    .components(vec![]),
            ),
        )
        .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_day_action<F, G>(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
    target_id: Option<u64>,
    replay_kind: &'static str,
    title: &'static str,
    apply: F,
    finish_message: G,
) -> Result<()>
where
    F: FnOnce(&mut MafiaGame, u64, u64) -> Result<String>,
    G: FnOnce(&mut MafiaGame, u64, String) -> String,
{
    if component.user.id.get() != actor_id {
        send_component_private(ctx, component, "본인에게 온 선택지만 사용할 수 있습니다.").await?;
        return Ok(());
    }
    let Some(target_id) = target_id else {
        send_component_private(ctx, component, "대상을 선택해야 합니다.").await?;
        return Ok(());
    };
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let (message, newly_contacted_mafia) = {
        let mut running_write = running.write().await;
        let was_known_mafia_team = running_write
            .game
            .get_player(actor_id)
            .is_some_and(|actor| running_write.game.is_known_mafia_team(actor));
        let message = match apply(&mut running_write.game, actor_id, target_id) {
            Ok(message) => message,
            Err(error) => {
                send_component_private(ctx, component, error.to_string()).await?;
                return Ok(());
            }
        };
        let message = finish_message(&mut running_write.game, actor_id, message);
        running_write.record_replay_event(
            replay_kind,
            Some(actor_id),
            &[target_id],
            serde_json::json!({
                "message": message.clone(),
            }),
        );
        let newly_contacted_mafia = running_write
            .game
            .get_player(actor_id)
            .filter(|actor| {
                actor.alive
                    && !was_known_mafia_team
                    && running_write.game.is_known_mafia_team(actor)
            })
            .cloned();
        (message, newly_contacted_mafia)
    };
    if let Some(player) = &newly_contacted_mafia {
        grant_private_role_member_access(ctx, data, &running, Role::Mafia, player).await;
    }
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::UpdateMessage(
                serenity::CreateInteractionResponseMessage::new()
                    .embed(make_embed(message, title, serenity::Colour::DARK_GREEN))
                    .components(vec![]),
            ),
        )
        .await?;
    Ok(())
}

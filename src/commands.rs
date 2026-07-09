// 역할: poise 슬래시 명령어, 컴포넌트 핸들러, 메시지 이벤트 처리,
//        익명 메시지 릴레이, 통계/리더보드, 역할 정보 조회

#![allow(unused_imports, clippy::too_many_arguments, clippy::collapsible_if)]

use super::web_settings;
use super::{
    AnonymousNameMode, Context, ContractorContractDraft, Data, Error, GAME_NOTIFICATION_ROLE,
    LeaderboardMetric, MAX_GAME_PLAYERS, RECRUITMENT_SECONDS, Recruitment, RunningGame,
    SPECTATOR_ROLE,
};
use crate::channel::*;
use crate::embed::*;
use crate::runner::{
    contractor_contract_components, contractor_role_modal, effective_night_role, game_loop,
    night_action_components, night_targets, role_message, role_short_guide,
    trigger_timed_night_events,
};
use ab_glyph::{
    Font, FontArc, GlyphId, OutlinedGlyph, PxScale, Rect as GlyphRect, ScaleFont, point,
};
use anyhow::{Context as AnyhowContext, Result, bail};
use dashmap::DashMap;
use image::{ImageFormat, Rgb, RgbImage};
use mafia_remake::config;
use mafia_remake::game::{GameCounts, MafiaGame, majority_required};
use mafia_remake::model::{
    CITIZEN_SPECIAL_ROLES, CONTRACTOR_GUESS_ROLES, MAFIA_SPECIAL_ROLES, NEUTRAL_SPECIAL_ROLES,
    NightResult, PUBLIC_CITIZEN_SPECIAL_ROLES, PUBLIC_CULT_SPECIAL_ROLES,
    PUBLIC_MAFIA_SPECIAL_ROLES, PUBLIC_NEUTRAL_SPECIAL_ROLES, Phase, Player, Role, VoteResult,
    Winner,
};
use mafia_remake::stats;
use poise::serenity_prelude as serenity;
use poise::serenity_prelude::Mentionable;
use rand::seq::{IndexedRandom, SliceRandom};
use secrecy::ExposeSecret;
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Notify, RwLock};

async fn defer_best_effort(ctx: Context<'_>, command_name: &str) -> bool {
    match ctx.defer().await {
        Ok(()) => true,
        Err(error) => {
            eprintln!("failed to defer {command_name}: {error:?}");
            false
        }
    }
}

async fn reply_embed_with_channel_fallback(
    ctx: Context<'_>,
    message: impl Into<String>,
    title: &str,
    color: serenity::Colour,
    ephemeral: bool,
) -> Result<(), Error> {
    let message = message.into();
    if let Err(error) = reply_embed(ctx, message.clone(), title, color, ephemeral).await {
        eprintln!("failed to send interaction reply '{title}': {error:?}");
        send_channel_embed(ctx.http(), ctx.channel_id(), message, title, color, vec![]).await?;
    }
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아시작",
    description_localized("ko", "저장된 설정대로 마피아 게임 참가자를 모집하고 시작합니다.")
)]
pub async fn start_game(ctx: Context<'_>) -> Result<(), Error> {
    let Some(guild_id) = ctx.guild_id() else {
        reply_embed(
            ctx,
            "서버 채널에서만 사용할 수 있습니다.",
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let channel_id = ctx.channel_id();
    if ctx.data().games.contains_key(&guild_id) {
        reply_embed(
            ctx,
            "이미 진행 중인 게임이 있습니다.",
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    }
    if ctx.data().recruitments.contains_key(&guild_id) {
        reply_embed(
            ctx,
            "이미 참가자를 모집 중입니다.",
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    }
    let config_snapshot = ctx.data().config.read().await.clone();
    if !config_snapshot.game_enabled {
        reply_embed(
            ctx,
            "마피아 게임이 비활성화되어 있습니다.",
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    }
    let Some(participant_role) = role_by_name(
        ctx.serenity_context(),
        guild_id,
        &config_snapshot.participant_role,
    )
    .await?
    else {
        reply_embed(
            ctx,
            format!(
                "'{}' 역할을 찾을 수 없습니다.",
                config_snapshot.participant_role
            ),
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };

    let role_history = {
        let stats_read = ctx.data().stats.read().await;
        stats::role_appearance_counts(&stats_read)
    };
    let special_roles = choose_special_roles_balanced(&config_snapshot, &role_history)?;
    let mut role_counts =
        selected_role_counts_balanced(&config_snapshot, &special_roles, &role_history)?;
    let minimum_players = minimum_player_count(&role_counts);
    let max_players = effective_max_player_count(&config_snapshot);
    if max_players < minimum_players {
        reply_embed(
            ctx,
            format!("현재 설정의 최소 시작 인원은 {minimum_players}명이라 최대 인원 {max_players}명으로 시작할 수 없습니다."),
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    }
    let done = Arc::new(Notify::new());
    let recruitment = Arc::new(RwLock::new(Recruitment {
        host_user_id: ctx.author().id,
        participant_role_id: participant_role.id,
        role_counts: role_counts.clone(),
        special_roles: special_roles.clone(),
        max_players,
        minimum_players,
        joined_ids: HashSet::new(),
        joined_names: HashMap::new(),
        spectator_ids: HashSet::new(),
        spectator_names: HashMap::new(),
        accepting: true,
        cancelled: false,
        done: done.clone(),
    }));
    ctx.data()
        .recruitments
        .insert(guild_id, recruitment.clone());

    let mention = role_by_name(ctx.serenity_context(), guild_id, GAME_NOTIFICATION_ROLE)
        .await?
        .map(|role| role.mention().to_string());
    let rec = recruitment.read().await;
    let mut reply = poise::CreateReply::default()
        .embed(recruitment_embed(&rec, &config_snapshot, "모집 중입니다."))
        .components(recruitment_components(guild_id, false));
    if let Some(mention) = mention {
        reply = reply.content(mention);
    }
    drop(rec);
    ctx.send(reply).await?;

    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(RECRUITMENT_SECONDS)) => {}
        _ = done.notified() => {}
    }

    let mut rec = recruitment.write().await;
    rec.accepting = false;
    let cancelled = rec.cancelled || rec.joined_ids.len() < rec.minimum_players;
    rec.cancelled = cancelled;
    let player_data = rec
        .joined_ids
        .iter()
        .map(|id| {
            (
                *id,
                rec.joined_names
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| id.to_string()),
            )
        })
        .collect::<Vec<_>>();
    if cancelled {
        ctx.data().recruitments.remove(&guild_id);
        reply_embed(
            ctx,
            "참가자 모집이 취소되었습니다.",
            "참가자 모집 취소",
            serenity::Colour::RED,
            false,
        )
        .await?;
        return Ok(());
    }
    let mut game_special_roles = expand_special_roles(&rec.special_roles);
    if config_snapshot.enable_cult_team {
        game_special_roles.extend([Role::CultLeader, Role::Fanatic]);
        *role_counts.entry(Role::CultLeader).or_default() += 1;
        *role_counts.entry(Role::Fanatic).or_default() += 1;
    }
    let participant_user_ids = rec.joined_ids.clone();
    let spectator_user_ids = rec.spectator_ids.clone();
    drop(rec);
    ctx.data().recruitments.remove(&guild_id);

    let game = MafiaGame::new_with_counts(
        player_data,
        GameCounts {
            mafia_count: *role_counts.get(&Role::Mafia).unwrap_or(&0),
            doctor_count: *role_counts.get(&Role::Doctor).unwrap_or(&0),
            police_count: *role_counts.get(&Role::Police).unwrap_or(&0),
            agent_count: *role_counts.get(&Role::Agent).unwrap_or(&0),
            vigilante_count: *role_counts.get(&Role::Vigilante).unwrap_or(&0),
            joker_count: 0,
            special_roles: game_special_roles,
        },
    )?;
    let initial_roles = game.players.iter().map(|p| (p.user_id, p.role)).collect();
    let mut running_game = RunningGame {
        guild_id,
        channel_id,
        participant_user_ids,
        spectator_user_ids,
        reveal_death_roles: config_snapshot.reveal_death_roles,
        anonymous_enabled: config_snapshot.anonymous_mode,
        game,
        started_at: Instant::now(),
        activity_game_key: uuid::Uuid::new_v4().to_string(),
        phase_deadline: None,
        initial_roles,
        memos: HashMap::new(),
        game_status_message_id: None,
        game_status_text: None,
        anonymous_aliases: HashMap::new(),
        anonymous_original_names: HashMap::new(),
        anonymous_input_channel_ids: HashMap::new(),
        anonymous_input_channel_owners: HashMap::new(),
        anonymous_dead_input_channel_ids: HashMap::new(),
        anonymous_dead_input_channel_owners: HashMap::new(),
        dead_chat_unlocked_ids: HashSet::new(),
        pending_dead_chat_user_ids: HashSet::new(),
        anonymous_shaman_input_channel_ids: HashMap::new(),
        anonymous_shaman_input_channel_owners: HashMap::new(),
        anonymous_role_input_channel_ids: HashMap::new(),
        anonymous_role_input_channels: HashMap::new(),
        anonymous_role_input_status_message_ids: HashMap::new(),
        anonymous_role_status_texts: HashMap::new(),
        anonymous_channel_topics: HashMap::new(),
        anonymous_webhook_urls: HashMap::new(),
        original_game_channel_overwrites: HashMap::new(),
        game_channel_overwrites: HashMap::new(),
        member_channel_overwrites: HashMap::new(),
        original_slowmode_delays: HashMap::new(),
        private_channel_ids: HashMap::new(),
        private_role_status_message_ids: HashMap::new(),
        private_role_status_texts: HashMap::new(),
        memo_channel_ids: HashMap::new(),
        shaman_channel_id: None,
        shaman_status_message_id: None,
        shaman_status_text: None,
        frog_channel_id: None,
        frog_game_channel_overwrites: HashMap::new(),
        madam_seduction_channel_overwrites: HashMap::new(),
        day_chat_open: false,
        final_defense_user_id: None,
        day_skip_voter_ids: HashSet::new(),
        day_skip_confirmed: false,
        day_extension_voter_ids: HashSet::new(),
        day_extension_active: false,
        day_extension_confirmed: false,
        night_timed_events_due: false,
        contractor_contract_drafts: HashMap::new(),
        activity_night_results: HashMap::new(),
        replay_events: Vec::new(),
        next_replay_sequence: 1,
        night_notify: Arc::new(Notify::new()),
        vote_notify: Arc::new(Notify::new()),
        confirm_notify: Arc::new(Notify::new()),
        day_notify: Arc::new(Notify::new()),
        stats_recorded: false,
    };
    running_game.record_replay_event(
        "game_started",
        None,
        &[],
        serde_json::json!({
            "participant_count": running_game.game.players.len(),
            "spectator_count": running_game.spectator_user_ids.len(),
        }),
    );
    let running = Arc::new(RwLock::new(running_game));
    ctx.data().games.insert(guild_id, running.clone());
    let data = ctx.data().clone();
    let serenity_ctx = ctx.serenity_context().clone();
    tokio::spawn(async move {
        if let Err(error) = game_loop(serenity_ctx, data, running).await {
            eprintln!("Rust game loop error: {error:?}");
        }
    });

    let running = ctx.data().games.get(&guild_id).unwrap();
    let game = &running.read().await.game;
    reply_embed(
        ctx,
        format!(
            "게임을 시작합니다. 참가자 {}명에게 역할을 DM으로 보냅니다.\n{}",
            game.players.len(),
            public_role_count_text(game)
        ),
        "게임 시작",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

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
        ["night", guild, actor_id, _role] => {
            handle_night_action(ctx, data, component, parse_guild(guild)?, actor_id.parse()?)
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
        ["contractor_roles", guild, actor_id] => {
            handle_contractor_role_modal_open(
                ctx,
                data,
                component,
                parse_guild(guild)?,
                actor_id.parse()?,
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
        ["contractor_roles", guild, actor_id] => {
            handle_contractor_role_modal_submit(
                ctx,
                data,
                modal,
                parse_guild(guild)?,
                actor_id.parse()?,
            )
            .await?
        }
        _ => {}
    }
    Ok(())
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

pub fn parse_contractor_role_input(input: &str) -> Option<Role> {
    find_role_by_name(input).filter(|role| CONTRACTOR_GUESS_ROLES.contains(role))
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
    running
        .write()
        .await
        .contractor_contract_drafts
        .entry(actor_id)
        .or_default()
        .target_ids[slot] = Some(target_id);
    ack_component(ctx, component).await;
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
    if !CONTRACTOR_GUESS_ROLES.contains(&role) {
        send_component_private(ctx, component, "청부로 추측할 수 없는 직업입니다.").await?;
        return Ok(());
    }
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    running
        .write()
        .await
        .contractor_contract_drafts
        .entry(actor_id)
        .or_default()
        .guessed_roles[slot] = Some(role);
    ack_component(ctx, component).await;
    Ok(())
}

pub async fn handle_contractor_role_modal_open(
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
    let has_targets = {
        let running_read = running.read().await;
        running_read
            .contractor_contract_drafts
            .get(&actor_id)
            .is_some_and(|draft| draft.target_ids.iter().all(Option::is_some))
    };
    if !has_targets {
        send_component_private(ctx, component, "청부 대상 2명을 먼저 선택하세요.").await?;
        return Ok(());
    }
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::Modal(contractor_role_modal(guild_id, actor_id)),
        )
        .await?;
    Ok(())
}

pub async fn handle_contractor_role_modal_submit(
    ctx: &serenity::Context,
    data: &Data,
    modal: &serenity::ModalInteraction,
    guild_id: serenity::GuildId,
    actor_id: u64,
) -> Result<()> {
    if modal.user.id.get() != actor_id {
        send_modal_private(
            ctx,
            modal,
            "본인에게 온 선택지만 사용할 수 있습니다.",
            serenity::Colour::RED,
        )
        .await?;
        return Ok(());
    }
    let role_hint = "직업 이름을 정확히 입력하세요. 예: 마피아, 의사, 시민";
    let Some(first_role) = modal_value(modal, "first_role")
        .as_deref()
        .and_then(parse_contractor_role_input)
    else {
        send_modal_private(ctx, modal, role_hint, serenity::Colour::RED).await?;
        return Ok(());
    };
    let Some(second_role) = modal_value(modal, "second_role")
        .as_deref()
        .and_then(parse_contractor_role_input)
    else {
        send_modal_private(ctx, modal, role_hint, serenity::Colour::RED).await?;
        return Ok(());
    };
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
    let draft = {
        let running_read = running.read().await;
        running_read
            .contractor_contract_drafts
            .get(&actor_id)
            .cloned()
    };
    let Some(draft) = draft else {
        send_modal_private(
            ctx,
            modal,
            "청부 대상 2명을 먼저 선택하세요.",
            serenity::Colour::RED,
        )
        .await?;
        return Ok(());
    };
    let (Some(first_target_id), Some(second_target_id)) =
        (draft.target_ids[0], draft.target_ids[1])
    else {
        send_modal_private(
            ctx,
            modal,
            "청부 대상 2명을 먼저 선택하세요.",
            serenity::Colour::RED,
        )
        .await?;
        return Ok(());
    };
    let (message, done, newly_contacted_mafia) = {
        let mut running_write = running.write().await;
        let was_known_mafia_team = running_write
            .game
            .get_player(actor_id)
            .is_some_and(|actor| running_write.game.is_known_mafia_team(actor));
        let message = match running_write.game.submit_contractor_contract(
            actor_id,
            first_target_id,
            first_role,
            second_target_id,
            second_role,
        ) {
            Ok(message) => message,
            Err(error) => {
                drop(running_write);
                send_modal_private(ctx, modal, error.to_string(), serenity::Colour::RED).await?;
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
        (message, done, newly_contacted_mafia)
    };
    send_modal_private(
        ctx,
        modal,
        format!("{message}\n\n밤이 끝나기 전 다시 선택하면 청부 대상을 변경할 수 있습니다."),
        serenity::Colour::DARK_GREEN,
    )
    .await?;
    if let Some(player) = &newly_contacted_mafia {
        grant_private_role_member_access(ctx, data, &running, Role::Mafia, player).await;
    }
    if done {
        running.read().await.night_notify.notify_waiters();
    }
    if running.read().await.night_timed_events_due {
        trigger_timed_night_events(ctx, data, &running).await?;
    }
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
        let (Some(first_target_id), Some(second_target_id), Some(first_role), Some(second_role)) = (
            draft.target_ids[0],
            draft.target_ids[1],
            draft.guessed_roles[0],
            draft.guessed_roles[1],
        ) else {
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
    if done {
        running.read().await.night_notify.notify_waiters();
    }
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::UpdateMessage(
                serenity::CreateInteractionResponseMessage::new()
                    .embed(make_embed(
                        format!(
                            "{message}\n\n밤이 끝나기 전 다시 선택하면 청부 대상을 변경할 수 있습니다."
                        ),
                        "밤 행동 완료",
                        serenity::Colour::DARK_GREEN,
                    ))
                    .components(contractor_contract_components(
                        guild_id, actor_id, &targets,
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
        let _ = member.add_role(ctx, rec.participant_role_id).await;
        rec.joined_names.insert(user_id, display_name(&member));
    } else {
        rec.joined_names
            .insert(user_id, component.user.name.clone());
    }
    rec.joined_ids.insert(user_id);
    let updated = rec.clone();
    drop(rec);
    send_component_private(ctx, component, "참가 완료!").await?;
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
        if let Some(role) = role_by_name(ctx, guild_id, SPECTATOR_ROLE).await? {
            let _ = member.add_role(ctx, role.id).await;
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
        let spy_bonus_targets = actor.and_then(|actor| {
            if actor.role == Role::Spy && running_write.game.spy_can_use_bonus_action(actor_id) {
                Some(night_targets(&running_write.game, &actor))
            } else {
                None
            }
        });
        let done = running_write.game.should_finish_night_early();
        (
            message,
            done,
            mafia_action_view,
            changeable_action_view,
            spy_bonus_targets,
            newly_contacted_mafia,
            cult_bells,
        )
    };
    if let Some(player) = &newly_contacted_mafia {
        grant_private_role_member_access(ctx, data, &running, Role::Mafia, player).await;
    }
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
    let (message, done) = {
        let mut running_write = running.write().await;
        let message = match running_write
            .game
            .submit_day_vote(component.user.id.get(), target_id)
        {
            Ok(message) => message,
            Err(error) => {
                send_component_private(ctx, component, error.to_string()).await?;
                return Ok(());
            }
        };
        let target_ids = target_id.into_iter().collect::<Vec<_>>();
        running_write.record_replay_event(
            "day_vote",
            Some(component.user.id.get()),
            &target_ids,
            serde_json::json!({
                "choice": if target_ids.is_empty() { "skip" } else { "player" },
                "message": message.clone(),
            }),
        );
        (message, running_write.game.all_day_votes_submitted())
    };
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
        |game, actor, message| {
            let investigation = game
                .consume_vigilante_results()
                .remove(&actor)
                .unwrap_or_else(|| "조사 결과를 확인하지 못했습니다.".to_string());
            format!("{message}\n\n{investigation}")
        },
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
        "도벽은 별도 선택이 아니라 지목 투표한 대상에게 자동으로 적용됩니다.",
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

async fn halt_running_game(running: &Arc<RwLock<RunningGame>>) -> String {
    let (roles, notifies) = {
        let mut running_write = running.write().await;
        running_write.game.phase = Phase::Ended;
        (
            running_write.game.reveal_roles(),
            [
                running_write.night_notify.clone(),
                running_write.vote_notify.clone(),
                running_write.confirm_notify.clone(),
                running_write.day_notify.clone(),
            ],
        )
    };
    for notify in notifies {
        notify.notify_waiters();
    }
    roles
}

#[poise::command(
    slash_command,
    rename = "마피아중지",
    description_localized("ko", "진행 중인 마피아 게임을 중지합니다.")
)]
pub async fn stop_game(ctx: Context<'_>) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    if let Some((_id, running)) = ctx.data().games.remove(&guild_id) {
        let roles = halt_running_game(&running).await;
        if let Err(error) = send_game_embed(
            ctx.serenity_context(),
            &running,
            format!("관리자가 게임을 중지했습니다.\n\n최종 역할\n{roles}"),
            "게임 중지",
            serenity::Colour::RED,
            vec![],
            true,
            true,
        )
        .await
        {
            eprintln!("failed to announce stopped game: {error:?}");
        }
        cleanup_game(ctx.serenity_context(), ctx.data(), &running).await;
        reply_embed(
            ctx,
            "게임을 중지했습니다.",
            "게임 중지",
            serenity::Colour::DARK_GREEN,
            false,
        )
        .await?;
    } else {
        reply_embed(
            ctx,
            "진행 중인 게임이 없습니다.",
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
    }
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아정리",
    description_localized("ko", "비정상 종료된 마피아 게임 채널과 역할을 강제로 정리합니다.")
)]
pub async fn cleanup_stuck_game(ctx: Context<'_>) -> Result<(), Error> {
    let deferred = defer_best_effort(ctx, "마피아정리").await;
    if !require_manager(ctx).await? {
        return Ok(());
    }
    if !deferred {
        let _ = send_channel_embed(
            ctx.http(),
            ctx.channel_id(),
            "마피아 정리를 시작했습니다.",
            "마피아 정리",
            serenity::Colour::GOLD,
            vec![],
        )
        .await;
    }
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let cleaned_running_game = if let Some((_id, running)) = ctx.data().games.remove(&guild_id) {
        halt_running_game(&running).await;
        cleanup_game(ctx.serenity_context(), ctx.data(), &running).await;
        true
    } else {
        false
    };
    let summary = cleanup_orphaned_game_artifacts(
        ctx.serenity_context(),
        ctx.data(),
        guild_id,
        !cleaned_running_game,
    )
    .await;
    reply_embed_with_channel_fallback(
        ctx,
        format!(
            "남아 있던 게임 채널, 역할, 권한, 슬로우모드를 정리했습니다.\n추가 삭제 채널: {}개\n역할 제거: {}개",
            summary.channels_deleted, summary.role_removals
        ),
        "마피아 정리 완료",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

pub async fn show_public_status_impl(ctx: Context<'_>) -> Result<(), Error> {
    let Some(guild_id) = ctx.guild_id() else {
        reply_embed(
            ctx,
            "서버에서만 사용할 수 있습니다.",
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let Some(running) = ctx.data().games.get(&guild_id).map(|entry| entry.clone()) else {
        reply_embed(
            ctx,
            "진행 중인 게임이 없습니다.",
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let (text, ephemeral) = {
        let running_read = running.read().await;
        (
            command_status_text(&running_read, ctx.author().id.get()),
            running_read.anonymous_enabled
                && running_read
                    .game
                    .get_player(ctx.author().id.get())
                    .is_some(),
        )
    };
    reply_embed(ctx, text, "게임 현황", serenity::Colour::GOLD, ephemeral).await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "상태",
    description_localized("ko", "현재 마피아 게임 생존자와 사망자를 확인합니다.")
)]
pub async fn show_public_status(ctx: Context<'_>) -> Result<(), Error> {
    show_public_status_impl(ctx).await
}

#[poise::command(
    slash_command,
    rename = "마피아상태",
    description_localized("ko", "진행 중인 마피아 게임 상태를 확인합니다.")
)]
pub async fn show_manager_status(ctx: Context<'_>) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let Some(guild_id) = ctx.guild_id() else {
        reply_embed(
            ctx,
            "서버에서만 사용할 수 있습니다.",
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let Some(running) = ctx.data().games.get(&guild_id).map(|entry| entry.clone()) else {
        reply_embed(
            ctx,
            "진행 중인 게임이 없습니다.",
            "마피아 게임",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let text = running.read().await.game.public_status();
    reply_embed(ctx, text, "게임 상태", serenity::Colour::GOLD, true).await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "메모",
    description_localized("ko", "개인 메모 채널에 참가자별 메모를 저장하거나 조회합니다.")
)]
pub async fn memo(
    ctx: Context<'_>,
    #[description = "메모 대상 참가자"] 참가자: serenity::User,
    #[description = "저장할 메모 내용. 비워두면 조회합니다."] 메모내용: Option<String>,
) -> Result<(), Error> {
    let Some(guild_id) = ctx.guild_id() else {
        reply_embed(
            ctx,
            "서버에서만 사용할 수 있습니다.",
            "메모",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let Some(running) = ctx.data().games.get(&guild_id).map(|entry| entry.clone()) else {
        reply_embed(
            ctx,
            "진행 중인 게임이 없습니다.",
            "메모",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let author_id = ctx.author().id.get();
    let (author, target, channel_id) = {
        let running_read = running.read().await;
        let Some(author) = running_read.game.get_player(author_id).cloned() else {
            reply_embed(
                ctx,
                "현재 게임 참가자만 메모를 사용할 수 있습니다.",
                "메모",
                serenity::Colour::RED,
                true,
            )
            .await?;
            return Ok(());
        };
        let Some(target) = running_read.game.get_player(참가자.id.get()).cloned() else {
            reply_embed(
                ctx,
                "메모 대상은 현재 게임 참가자여야 합니다.",
                "메모",
                serenity::Colour::RED,
                true,
            )
            .await?;
            return Ok(());
        };
        (author, target, running_read.channel_id)
    };

    let config = ctx.data().config.read().await.clone();
    let roles = channel_role_ids(
        ctx.serenity_context(),
        guild_id,
        &config,
        ctx.data().bot_user_id,
    )
    .await?;
    let category = source_category(ctx.serenity_context(), channel_id).await;
    let Some(memo_channel_id) =
        ensure_memo_channel(ctx.serenity_context(), &running, &author, roles, category).await
    else {
        reply_embed(
            ctx,
            "개인 메모 채널을 만들 수 없습니다.",
            "메모",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };

    let content = 메모내용.unwrap_or_default().trim().to_string();
    if !content.is_empty() {
        let (memo_number, target_name) = {
            let mut running_write = running.write().await;
            let target_name = running_write
                .game
                .get_player(target.user_id)
                .map(|target| status_display_name(&running_write, target))
                .unwrap_or_else(|| target.name.clone());
            let memos = running_write
                .memos
                .entry(author_id)
                .or_default()
                .entry(target.user_id)
                .or_default();
            memos.push(content.clone());
            (memos.len(), target_name)
        };
        let _ = send_channel_embed(
            ctx.http(),
            memo_channel_id,
            format!("대상: {target_name}\n{memo_number}. {content}"),
            "메모 등록",
            serenity::Colour::DARK_GREEN,
            vec![],
        )
        .await;
        reply_embed(
            ctx,
            format!("{target_name} 님에 대한 메모를 저장했습니다."),
            "메모 등록",
            serenity::Colour::DARK_GREEN,
            true,
        )
        .await?;
    } else {
        let chunks = {
            let running_read = running.read().await;
            let target_name = running_read
                .game
                .get_player(target.user_id)
                .map(|target| status_display_name(&running_read, target))
                .unwrap_or_else(|| target.name.clone());
            let memos = running_read
                .memos
                .get(&author_id)
                .and_then(|target_memos| target_memos.get(&target.user_id))
                .cloned()
                .unwrap_or_default();
            let header = format!("{target_name} 님에 대한 메모");
            if memos.is_empty() {
                vec![format!("{header}\n저장된 메모가 없습니다.")]
            } else {
                let mut chunks = Vec::new();
                let mut current = header.clone();
                for (index, memo) in memos.iter().enumerate() {
                    let line = format!("{}. {memo}", index + 1);
                    if current.len() + line.len() + 1 > 3500 {
                        chunks.push(current);
                        current = format!("{header} (계속)\n{line}");
                    } else {
                        current.push('\n');
                        current.push_str(&line);
                    }
                }
                chunks.push(current);
                chunks
            }
        };
        for chunk in chunks {
            ctx.send(
                poise::CreateReply::default()
                    .embed(make_embed(chunk, "메모 조회", serenity::Colour::GOLD))
                    .ephemeral(true),
            )
            .await?;
        }
    }
    Ok(())
}

pub fn personal_stats_text(
    stats_file: &stats::StatsFile,
    user_id: u64,
    fallback_name: &str,
) -> String {
    let Some(entry) = stats_file.users.get(&user_id.to_string()) else {
        return "아직 기록된 게임 전적이 없습니다.".to_string();
    };
    let name = if entry.name.is_empty() {
        fallback_name
    } else {
        &entry.name
    };
    format!(
        "{name}님의 전적\n전체 게임: **{}판**\n승리/패배: **{}승 {}패**\n승률: **{}**\n연승: **{}연승** (최고 {}연승)\n마피아팀 플레이: **{}회**\n게임시간: **{}**\n레이팅: **{}점** / **{}랭크** (최고 {}점, 반영 {}판)\n\n역할별 플레이\n{}",
        entry.games,
        entry.wins,
        entry.losses,
        stats::win_rate_text(entry.wins, entry.games),
        entry.win_streak,
        entry.best_win_streak,
        entry.mafia_team_games,
        stats::play_duration_text(entry.play_seconds),
        entry.rating,
        stats::rating_rank(entry.rating),
        entry.rating_peak,
        entry.rating_games,
        stats::role_stats_text(entry)
    )
}

#[poise::command(
    slash_command,
    rename = "내정보",
    description_localized("ko", "내 마피아 게임 전적을 확인합니다.")
)]
pub async fn show_my_info(ctx: Context<'_>) -> Result<(), Error> {
    let stats_file = ctx.data().stats.read().await;
    let user = ctx.author();
    let text = personal_stats_text(&stats_file, user.id.get(), &user.name);
    reply_embed(ctx, text, "내정보", serenity::Colour::GOLD, true).await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "레이팅로그",
    description_localized("ko", "내 최근 레이팅 변화 기록을 확인합니다.")
)]
pub async fn rating_log(ctx: Context<'_>) -> Result<(), Error> {
    let stats_file = ctx.data().stats.read().await;
    let user = ctx.author();
    let text = stats::rating_log_text(&stats_file, user.id.get(), &user.name, 10);
    reply_embed(ctx, text, "레이팅 로그", serenity::Colour::GOLD, true).await?;
    Ok(())
}

pub fn image_color(hex: &str) -> Rgb<u8> {
    let value = hex.trim_start_matches('#');
    let red = u8::from_str_radix(&value[0..2], 16).unwrap_or(255);
    let green = u8::from_str_radix(&value[2..4], 16).unwrap_or(255);
    let blue = u8::from_str_radix(&value[4..6], 16).unwrap_or(255);
    Rgb([red, green, blue])
}

pub fn fill_rect(image: &mut RgbImage, x: i32, y: i32, width: u32, height: u32, color: Rgb<u8>) {
    let left = x.max(0) as u32;
    let top = y.max(0) as u32;
    let right = (x as i64 + width as i64)
        .clamp(0, image.width() as i64)
        .max(left as i64) as u32;
    let bottom = (y as i64 + height as i64)
        .clamp(0, image.height() as i64)
        .max(top as i64) as u32;

    for pixel_y in top..bottom {
        for pixel_x in left..right {
            image.put_pixel(pixel_x, pixel_y, color);
        }
    }
}

pub fn fill_horizontal_line(image: &mut RgbImage, x0: i32, x1: i32, y: i32, color: Rgb<u8>) {
    if y < 0 || y >= image.height() as i32 {
        return;
    }
    let left = x0.min(x1).max(0) as u32;
    let right = x0.max(x1).min(image.width() as i32 - 1);
    if right < 0 || left > right as u32 {
        return;
    }
    for pixel_x in left..=right as u32 {
        image.put_pixel(pixel_x, y as u32, color);
    }
}

pub fn fill_circle(image: &mut RgbImage, center: (i32, i32), radius: i32, color: Rgb<u8>) {
    let mut x = 0;
    let mut y = radius;
    let mut p = 1 - radius;
    let (x0, y0) = center;

    while x <= y {
        fill_horizontal_line(image, x0 - x, x0 + x, y0 + y, color);
        fill_horizontal_line(image, x0 - y, x0 + y, y0 + x, color);
        fill_horizontal_line(image, x0 - x, x0 + x, y0 - y, color);
        fill_horizontal_line(image, x0 - y, x0 + y, y0 - x, color);

        x += 1;
        if p < 0 {
            p += 2 * x + 1;
        } else {
            y -= 1;
            p += 2 * (x - y) + 1;
        }
    }
}

pub fn blend_channel(left: u8, right: u8, left_weight: f32, right_weight: f32) -> u8 {
    let value = left as f32 * left_weight + right as f32 * right_weight;
    if value < u8::MAX as f32 {
        if value > u8::MIN as f32 {
            value as u8
        } else {
            u8::MIN
        }
    } else {
        u8::MAX
    }
}

pub fn blend_rgb(left: Rgb<u8>, right: Rgb<u8>, left_weight: f32, right_weight: f32) -> Rgb<u8> {
    Rgb([
        blend_channel(left[0], right[0], left_weight, right_weight),
        blend_channel(left[1], right[1], left_weight, right_weight),
        blend_channel(left[2], right[2], left_weight, right_weight),
    ])
}

pub fn layout_lb_glyphs(
    scale: PxScale,
    font: &impl Font,
    text: &str,
    mut visit: impl FnMut(OutlinedGlyph, GlyphRect),
) {
    let font = font.as_scaled(scale);
    let mut last: Option<GlyphId> = None;
    let mut width = 0.0;

    for character in text.chars() {
        let glyph_id = font.glyph_id(character);
        let glyph = glyph_id.with_scale_and_position(scale, point(width, font.ascent()));
        width += font.h_advance(glyph_id);
        if let Some(outlined) = font.outline_glyph(glyph) {
            if let Some(last) = last {
                width += font.kern(glyph_id, last);
            }
            last = Some(glyph_id);
            let bounds = outlined.px_bounds();
            visit(outlined, bounds);
        }
    }
}

pub fn draw_lb_text(
    image: &mut RgbImage,
    font: &FontArc,
    size: f32,
    x: i32,
    y: i32,
    text: impl AsRef<str>,
    color: Rgb<u8>,
) {
    let image_width = image.width() as i32;
    let image_height = image.height() as i32;

    layout_lb_glyphs(PxScale::from(size), font, text.as_ref(), |glyph, bounds| {
        glyph.draw(|glyph_x, glyph_y, value| {
            let image_x = glyph_x as i32 + x + bounds.min.x.round() as i32;
            let image_y = glyph_y as i32 + y + bounds.min.y.round() as i32;
            let value = value.clamp(0.0, 1.0);

            if (0..image_width).contains(&image_x) && (0..image_height).contains(&image_y) {
                let pixel = *image.get_pixel(image_x as u32, image_y as u32);
                image.put_pixel(
                    image_x as u32,
                    image_y as u32,
                    blend_rgb(pixel, color, 1.0 - value, value),
                );
            }
        });
    });
}

pub fn truncate_for_board(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut text = value
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    text.push_str("...");
    text
}

pub fn leaderboard_metric_column(metric: &str) -> &'static str {
    match metric {
        "winrate" => "winrate",
        "streak" => "streak",
        "games" => "games",
        "mafia" => "mafia",
        "playtime" => "time",
        "rating" => "rating",
        _ => "record",
    }
}

pub fn render_leaderboard_image(stats_file: &stats::StatsFile, metric: &str) -> Option<Vec<u8>> {
    let entries = stats::leaderboard_entries(stats_file, metric, 10);
    if entries.is_empty() {
        return None;
    }

    const IMAGE_WIDTH: u32 = 1280;
    const TOP_PADDING: i32 = 40;
    const SIDE_PADDING: i32 = 48;
    const HEADER_HEIGHT: i32 = 150;
    const ROW_HEIGHT: i32 = 78;
    const BOTTOM_PADDING: i32 = 44;

    let height =
        (TOP_PADDING + HEADER_HEIGHT + ROW_HEIGHT * entries.len() as i32 + BOTTOM_PADDING) as u32;
    let mut image = RgbImage::from_pixel(IMAGE_WIDTH, height, image_color("#111318"));
    let font = FontArc::try_from_slice(include_bytes!("../MalangmalangR.ttf")).ok()?;

    let text = image_color("#f5f7fb");
    let muted = image_color("#aeb6c8");
    let accent = image_color("#ffd166");
    let panel = image_color("#1d2028");
    let row_dark = image_color("#242832");
    let row_light = image_color("#292e3a");

    draw_lb_text(
        &mut image,
        &font,
        44.0,
        SIDE_PADDING,
        TOP_PADDING,
        "마피아 리더보드",
        text,
    );
    draw_lb_text(
        &mut image,
        &font,
        24.0,
        SIDE_PADDING,
        TOP_PADDING + 58,
        "게임 종료 후 기록된 전적 기준",
        muted,
    );
    fill_rect(
        &mut image,
        IMAGE_WIDTH as i32 - SIDE_PADDING - 230,
        TOP_PADDING + 10,
        210,
        38,
        image_color("#374151"),
    );
    draw_lb_text(
        &mut image,
        &font,
        24.0,
        IMAGE_WIDTH as i32 - SIDE_PADDING - 214,
        TOP_PADDING + 16,
        format!("기준: {}", stats::leaderboard_metric_name(metric)),
        text,
    );

    let panel_top = TOP_PADDING + 116;
    let panel_bottom = height as i32 - BOTTOM_PADDING + 8;
    fill_rect(
        &mut image,
        SIDE_PADDING,
        panel_top,
        IMAGE_WIDTH - (SIDE_PADDING as u32 * 2),
        (panel_bottom - panel_top) as u32,
        panel,
    );

    let columns = HashMap::from([
        ("rank", SIDE_PADDING + 32),
        ("name", SIDE_PADDING + 110),
        ("record", SIDE_PADDING + 390),
        ("games", SIDE_PADDING + 535),
        ("winrate", SIDE_PADDING + 635),
        ("streak", SIDE_PADDING + 745),
        ("mafia", SIDE_PADDING + 850),
        ("time", SIDE_PADDING + 955),
        ("rating", SIDE_PADDING + 1085),
    ]);
    let selected_column = leaderboard_metric_column(metric);
    let header_y = panel_top + 24;
    for (key, label) in [
        ("rank", "#"),
        ("name", "이름"),
        ("record", "승패"),
        ("games", "판수"),
        ("winrate", "승률"),
        ("streak", "연승"),
        ("mafia", "마피아"),
        ("time", "시간"),
        ("rating", "랭크/레이팅"),
    ] {
        draw_lb_text(
            &mut image,
            &font,
            21.0,
            columns[key],
            header_y,
            label,
            if key == selected_column {
                accent
            } else {
                muted
            },
        );
    }

    let row_start_y = panel_top + 62;
    for (index, (_user_id, entry)) in entries.iter().enumerate() {
        let rank = index + 1;
        let y = row_start_y + index as i32 * ROW_HEIGHT;
        let row_fill = if rank % 2 == 1 { row_dark } else { row_light };
        fill_rect(
            &mut image,
            SIDE_PADDING + 18,
            y,
            IMAGE_WIDTH - ((SIDE_PADDING + 18) as u32 * 2),
            (ROW_HEIGHT - 10) as u32,
            row_fill,
        );
        let medal = match rank {
            1 => image_color("#f6c945"),
            2 => image_color("#c4ccd8"),
            3 => image_color("#c58b5b"),
            _ => image_color("#3b4252"),
        };
        fill_circle(&mut image, (columns["rank"] + 17, y + 36), 20, medal);
        draw_lb_text(
            &mut image,
            &font,
            24.0,
            columns["rank"] + if rank < 10 { 9 } else { 3 },
            y + 22,
            rank.to_string(),
            if rank <= 3 {
                image_color("#111318")
            } else {
                text
            },
        );

        let name = if entry.name.is_empty() {
            "알 수 없음".to_string()
        } else {
            truncate_for_board(&entry.name, 13)
        };
        let values = [
            ("name", name),
            ("record", format!("{}승 {}패", entry.wins, entry.losses)),
            ("games", format!("{}판", entry.games)),
            ("winrate", stats::win_rate_text(entry.wins, entry.games)),
            ("streak", format!("{}연승", entry.win_streak)),
            ("mafia", format!("{}회", entry.mafia_team_games)),
            ("time", stats::play_duration_text(entry.play_seconds)),
            (
                "rating",
                format!("{} {}점", stats::rating_rank(entry.rating), entry.rating),
            ),
        ];
        for (key, value) in values {
            draw_lb_text(
                &mut image,
                &font,
                if key == "name" { 27.0 } else { 23.0 },
                columns[key],
                y + if key == "name" { 18 } else { 21 },
                value,
                if key == selected_column { accent } else { text },
            );
        }
    }
    draw_lb_text(
        &mut image,
        &font,
        18.0,
        SIDE_PADDING + 18,
        height as i32 - 30,
        "마피아 게임 진행 메시지",
        muted,
    );

    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .ok()?;
    Some(bytes.into_inner())
}

#[poise::command(
    slash_command,
    rename = "리더보드",
    description_localized("ko", "마피아 게임 전적 순위를 확인합니다.")
)]
pub async fn show_leaderboard(
    ctx: Context<'_>,
    #[description = "정렬 기준"] 기준: Option<LeaderboardMetric>,
) -> Result<(), Error> {
    let metric = 기준.map_or("wins", LeaderboardMetric::value);
    let stats_file = Arc::new(ctx.data().stats.read().await.clone());
    let image_stats = Arc::clone(&stats_file);
    if let Some(image) =
        tokio::task::spawn_blocking(move || render_leaderboard_image(image_stats.as_ref(), metric))
            .await?
    {
        ctx.send(
            poise::CreateReply::default().attachment(serenity::CreateAttachment::bytes(
                image,
                format!("mafia_leaderboard_{metric}.png"),
            )),
        )
        .await?;
        return Ok(());
    }
    let text = stats::leaderboard_text(stats_file.as_ref(), metric);
    reply_embed(ctx, text, "리더보드", serenity::Colour::GOLD, false).await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "리더보드초기화",
    description_localized("ko", "마피아 게임 전적과 리더보드를 초기화합니다.")
)]
pub async fn reset_leaderboard(ctx: Context<'_>) -> Result<(), Error> {
    let deferred = defer_best_effort(ctx, "리더보드초기화").await;
    if !require_manager(ctx).await? {
        return Ok(());
    }
    if !deferred {
        let _ = send_channel_embed(
            ctx.http(),
            ctx.channel_id(),
            "리더보드 초기화를 시작했습니다.",
            "리더보드",
            serenity::Colour::GOLD,
            vec![],
        )
        .await;
    }
    let stats_snapshot = {
        let mut stats_file = ctx.data().stats.write().await;
        *stats_file = stats::StatsFile::default();
        stats_file.clone()
    };
    let stats_path = ctx.data().stats_path.clone();
    tokio::task::spawn_blocking(move || stats::save_stats(&*stats_path, &stats_snapshot)).await??;
    reply_embed_with_channel_fallback(
        ctx,
        "리더보드와 개인 전적을 초기화했습니다.",
        "리더보드",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아설정",
    description_localized("ko", "마피아 게임 기본 설정을 변경합니다.")
)]
#[allow(clippy::too_many_arguments)]
pub async fn configure_game(
    ctx: Context<'_>,
    #[description = "마피아 수"] mafia: Option<u32>,
    #[description = "의사 수"] doctor: Option<u32>,
    #[description = "경찰 수"] police: Option<u32>,
    #[description = "시민 특수룰 수"] citizen_special: Option<u32>,
    #[description = "마피아 특수룰 수"] mafia_special: Option<u32>,
    #[description = "중립 특수룰 수"] neutral_special: Option<u32>,
    #[description = "낮 채팅 슬로우모드 초. 기본 3초"] slowmode: Option<u64>,
    #[description = "사망 시 직업 공개 여부"] death_role_reveal: Option<bool>,
    #[description = "낮에 경찰 조사 성공 여부 공개 여부"] police_status_reveal: Option<bool>,
    #[description = "아침 생존 마피아 수 공개 여부"] mafia_count_reveal: Option<bool>,
    #[description = "사립탐정 활성화 여부"] detective: Option<bool>,
    #[description = "형사 활성화 여부"] inspector: Option<bool>,
    #[description = "영매 활성화 여부"] shaman: Option<bool>,
    #[description = "도굴꾼 활성화 여부"] graverobber: Option<bool>,
    #[description = "스파이 활성화 여부"] spy: Option<bool>,
    #[description = "청부업자 활성화 여부"] contractor: Option<bool>,
    #[description = "마녀 활성화 여부"] witch: Option<bool>,
    #[description = "과학자 활성화 여부"] scientist: Option<bool>,
    #[description = "대부 활성화 여부"] godfather: Option<bool>,
    #[description = "조커 활성화 여부"] joker: Option<bool>,
    #[description = "정치인 활성화 여부"] politician: Option<bool>,
    #[description = "판사 활성화 여부"] judge: Option<bool>,
    #[description = "기자 활성화 여부"] reporter: Option<bool>,
    #[description = "해커 활성화 여부"] hacker: Option<bool>,
    #[description = "테러리스트 활성화 여부"] terrorist: Option<bool>,
    #[description = "군인 활성화 여부"] soldier: Option<bool>,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    let previous = config_write.clone();
    if let Some(value) = mafia {
        if value < 1 {
            reply_embed(
                ctx,
                "마피아는 최소 1명이어야 합니다.",
                "설정 오류",
                serenity::Colour::RED,
                true,
            )
            .await?;
            return Ok(());
        }
        config_write.default_mafia_count = value;
    }
    if let Some(value) = doctor {
        config_write.default_doctor_count = value;
    }
    if let Some(value) = police {
        config_write.default_police_count = value;
    }
    if let Some(value) = citizen_special {
        config_write.citizen_special_count = value;
    }
    if let Some(value) = mafia_special {
        config_write.mafia_special_count = value;
    }
    if let Some(value) = neutral_special {
        config_write.neutral_special_count = value;
    }
    if let Some(value) = slowmode {
        config_write.chat_slowmode_seconds = value;
    }
    if let Some(value) = death_role_reveal {
        config_write.reveal_death_roles = value;
    }
    if let Some(value) = police_status_reveal {
        config_write.reveal_public_police_status = value;
    }
    if let Some(value) = mafia_count_reveal {
        config_write.reveal_morning_mafia_count = value;
    }
    if let Some(value) = detective {
        config_write.enable_detective = value;
    }
    if let Some(value) = inspector {
        config_write.enable_inspector = value;
    }
    if let Some(value) = shaman {
        config_write.enable_shaman = value;
    }
    if let Some(value) = graverobber {
        config_write.enable_graverobber = value;
    }
    if let Some(value) = spy {
        config_write.enable_spy = value;
    }
    if let Some(value) = contractor {
        config_write.enable_contractor = value;
    }
    if let Some(value) = witch {
        config_write.enable_witch = value;
    }
    if let Some(value) = scientist {
        config_write.enable_scientist = value;
    }
    if let Some(value) = godfather {
        config_write.enable_godfather = value;
    }
    if let Some(value) = joker {
        config_write.enable_joker = value;
    }
    if let Some(value) = politician {
        config_write.enable_politician = value;
    }
    if let Some(value) = judge {
        config_write.enable_judge = value;
    }
    if let Some(value) = reporter {
        config_write.enable_reporter = value;
    }
    if let Some(value) = hacker {
        config_write.enable_hacker = value;
    }
    if let Some(value) = terrorist {
        config_write.enable_terrorist = value;
    }
    if let Some(value) = soldier {
        config_write.enable_soldier = value;
    }
    let validation = choose_special_roles(&config_write)
        .and_then(|special_roles| selected_role_counts(&config_write, &special_roles))
        .map(|role_counts| {
            let minimum_players = minimum_player_count(&role_counts);
            let max_players = effective_max_player_count(&config_write);
            (minimum_players, max_players)
        });
    match validation {
        Ok((minimum_players, max_players)) if max_players < minimum_players => {
            *config_write = previous;
            reply_embed(
                ctx,
                format!("현재 설정의 최소 시작 인원은 {minimum_players}명이라 최대 인원 {max_players}명으로 시작할 수 없습니다."),
                "설정 오류",
                serenity::Colour::RED,
                true,
            )
            .await?;
            return Ok(());
        }
        Err(error) => {
            *config_write = previous;
            reply_embed(
                ctx,
                error.to_string(),
                "설정 오류",
                serenity::Colour::RED,
                true,
            )
            .await?;
            return Ok(());
        }
        _ => {}
    }
    config::save_config(&*ctx.data().config_path, &config_write)?;
    let text = current_settings_text(&config_write, "마피아 설정을 저장했습니다.");
    drop(config_write);
    reply_embed(
        ctx,
        text,
        "마피아 설정",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아인원설정",
    description_localized("ko", "마피아 게임 모집 최대 인원을 설정합니다.")
)]
pub async fn configure_player_limit(
    ctx: Context<'_>,
    #[description = "최대 참가 인원. 0은 제한 없음(봇 최대 24명)"] max_players: u32,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    if max_players as usize > MAX_GAME_PLAYERS {
        reply_embed(
            ctx,
            format!("최대 인원은 {MAX_GAME_PLAYERS}명 이하로 설정해야 합니다."),
            "설정 오류",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    config_write.max_player_count = max_players;
    config::save_config(&*ctx.data().config_path, &config_write)?;
    let text = current_settings_text(&config_write, "마피아 인원 설정을 저장했습니다.");
    drop(config_write);
    reply_embed(
        ctx,
        text,
        "마피아 설정",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아익명설정",
    description_localized("ko", "마피아 게임 익명 채팅 사용 여부를 설정합니다.")
)]
pub async fn configure_anonymous_mode(
    ctx: Context<'_>,
    #[description = "익명 채팅 사용 여부"] enabled: bool,
    #[description = "익명 이름을 동물로 할지 숫자로 할지 선택합니다."] 이름방식: Option<
        AnonymousNameMode,
    >,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    config_write.anonymous_mode = enabled;
    if let Some(name_mode) = 이름방식 {
        config_write.anonymous_name_mode = name_mode.value().to_string();
    }
    config::save_config(&*ctx.data().config_path, &config_write)?;
    let text = current_settings_text(&config_write, "마피아 익명 설정을 저장했습니다.");
    drop(config_write);
    reply_embed(
        ctx,
        text,
        "마피아 설정",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아웹설정",
    description_localized(
        "ko",
        "브라우저에서 게임 설정을 편집할 수 있는 1회용 링크를 발급합니다. (관리자 전용)"
    )
)]
pub async fn web_configure_game(ctx: Context<'_>) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let Some(guild_id) = ctx.guild_id() else {
        reply_embed(
            ctx,
            "서버에서만 사용할 수 있습니다.",
            "웹 설정",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let user = ctx.author();
    let token = web_settings::issue_session(
        &ctx.data().web_sessions,
        guild_id.get(),
        user.id.get(),
        user.name.clone(),
    );
    let url = format!(
        "{}{}/{}",
        ctx.data().web_base_url.trim_end_matches('/'),
        web_settings::settings_path(),
        token
    );
    let minutes = web_settings::session_ttl_minutes();
    reply_embed(
        ctx,
        format!(
            "아래 링크에서 마피아 게임 설정을 편집할 수 있습니다.\n{url}\n\n⚠️ 이 링크는 **{}** 님만 사용할 수 있고, **{minutes}분 동안 1회**만 유효합니다. 다른 사람과 공유하지 마세요.",
            user.name
        ),
        "웹 설정 링크 발급",
        serenity::Colour::DARK_GREEN,
        true,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아추가설정",
    description_localized("ko", "추가 역할 묶음을 설정합니다.")
)]
#[allow(clippy::too_many_arguments)]
pub async fn configure_extra_roles(
    ctx: Context<'_>,
    nurse: Option<bool>,
    lover: Option<bool>,
    priest: Option<bool>,
    madam: Option<bool>,
    gangster: Option<bool>,
    prophet: Option<bool>,
    psychologist: Option<bool>,
    hypnotist: Option<bool>,
    mercenary: Option<bool>,
    thief: Option<bool>,
    cult_team: Option<bool>,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    if let Some(v) = nurse {
        config_write.enable_nurse = v;
    }
    if let Some(v) = lover {
        config_write.enable_lover = v;
    }
    if let Some(v) = priest {
        config_write.enable_priest = v;
    }
    if let Some(v) = madam {
        config_write.enable_madam = v;
    }
    if let Some(v) = gangster {
        config_write.enable_gangster = v;
    }
    if let Some(v) = prophet {
        config_write.enable_prophet = v;
    }
    if let Some(v) = psychologist {
        config_write.enable_psychologist = v;
    }
    if let Some(v) = hypnotist {
        config_write.enable_hypnotist = v;
    }
    if let Some(v) = mercenary {
        config_write.enable_mercenary = v;
    }
    if let Some(v) = thief {
        config_write.enable_thief = v;
    }
    if let Some(v) = cult_team {
        config_write.enable_cult_team = v;
    }
    config::save_config(&*ctx.data().config_path, &config_write)?;
    let text = current_settings_text(&config_write, "마피아 추가 설정을 저장했습니다.");
    drop(config_write);
    reply_embed(
        ctx,
        text,
        "마피아 설정",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아수사설정",
    description_localized("ko", "수사직 후보를 설정합니다.")
)]
pub async fn configure_investigation_role(
    ctx: Context<'_>,
    agent: Option<bool>,
    vigilante: Option<bool>,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    if let Some(v) = agent {
        config_write.use_agent = v;
    }
    if let Some(v) = vigilante {
        config_write.use_vigilante = v;
    }
    config::save_config(&*ctx.data().config_path, &config_write)?;
    let text = current_settings_text(&config_write, "마피아 수사 설정을 저장했습니다.");
    drop(config_write);
    reply_embed(
        ctx,
        text,
        "마피아 설정",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아비활성화",
    description_localized("ko", "마피아 게임 시작을 비활성화합니다.")
)]
pub async fn disable_mafia_game(ctx: Context<'_>) -> Result<(), Error> {
    set_game_enabled(ctx, false).await
}

#[poise::command(
    slash_command,
    rename = "마피아활성화",
    description_localized("ko", "마피아 게임 시작을 활성화합니다.")
)]
pub async fn enable_mafia_game(ctx: Context<'_>) -> Result<(), Error> {
    set_game_enabled(ctx, true).await
}

pub async fn set_game_enabled(ctx: Context<'_>, enabled: bool) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    config_write.game_enabled = enabled;
    config::save_config(&*ctx.data().config_path, &config_write)?;
    drop(config_write);
    reply_embed(
        ctx,
        if enabled {
            "마피아 게임을 활성화했습니다. 이제 새 게임을 시작할 수 있습니다."
        } else {
            "마피아 게임을 비활성화했습니다. 새 게임을 시작할 수 없습니다."
        },
        "마피아 게임",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "블랙리스트추가",
    description_localized("ko", "마피아 게임 참가 블랙리스트에 유저를 추가합니다.")
)]
pub async fn add_to_blacklist(
    ctx: Context<'_>,
    #[description = "블랙리스트에 추가할 유저"] 유저: serenity::User,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    let id = 유저.id.get();
    let changed = !config_write.blacklist_user_ids.contains(&id);
    if changed {
        config_write.blacklist_user_ids.push(id);
        config_write.blacklist_user_ids.sort_unstable();
    }
    config::save_config(&*ctx.data().config_path, &config_write)?;
    drop(config_write);
    reply_embed(
        ctx,
        if changed {
            format!(
                "{} 님을 블랙리스트에 추가했습니다. 이제 게임에 참가할 수 없습니다.",
                유저.name
            )
        } else {
            format!("{} 님은 이미 블랙리스트에 있습니다.", 유저.name)
        },
        "블랙리스트",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "블랙리스트해제",
    description_localized("ko", "마피아 게임 참가 블랙리스트에서 유저를 제거합니다.")
)]
pub async fn remove_from_blacklist(
    ctx: Context<'_>,
    #[description = "블랙리스트에서 해제할 유저"] 유저: serenity::User,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let mut config_write = ctx.data().config.write().await;
    let id = 유저.id.get();
    let before = config_write.blacklist_user_ids.len();
    config_write
        .blacklist_user_ids
        .retain(|user_id| *user_id != id);
    let changed = config_write.blacklist_user_ids.len() != before;
    config::save_config(&*ctx.data().config_path, &config_write)?;
    drop(config_write);
    reply_embed(
        ctx,
        if changed {
            format!(
                "{} 님을 블랙리스트에서 해제했습니다. 이제 게임에 참가할 수 있습니다.",
                유저.name
            )
        } else {
            format!("{} 님은 블랙리스트에 없습니다.", 유저.name)
        },
        "블랙리스트",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "블랙리스트목록",
    description_localized("ko", "마피아 게임 참가 블랙리스트 목록을 확인합니다.")
)]
pub async fn show_blacklist(ctx: Context<'_>) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let config_read = ctx.data().config.read().await;
    let text = if config_read.blacklist_user_ids.is_empty() {
        "블랙리스트가 비어 있습니다.".to_string()
    } else {
        config_read
            .blacklist_user_ids
            .iter()
            .take(50)
            .enumerate()
            .map(|(i, id)| format!("{}. `{id}`", i + 1))
            .collect::<Vec<_>>()
            .join("\n")
    };
    drop(config_read);
    reply_embed(ctx, text, "블랙리스트", serenity::Colour::GOLD, true).await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "직업정보",
    description_localized("ko", "특정 직업의 설명을 확인합니다.")
)]
pub async fn show_role_info(
    ctx: Context<'_>,
    #[description = "설명을 볼 직업 이름"] 직업명: String,
) -> Result<(), Error> {
    let role = find_role_by_name(&직업명);
    if let Some(role) = role {
        reply_embed(
            ctx,
            format!("{}\n{}", role.value(), role_short_guide(role)),
            "직업정보",
            serenity::Colour::DARK_GREEN,
            false,
        )
        .await?;
    } else {
        reply_embed(
            ctx,
            "직업을 찾을 수 없습니다. 정확한 직업명을 입력하세요.",
            "직업정보",
            serenity::Colour::RED,
            true,
        )
        .await?;
    }
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "역할설명",
    description_localized("ko", "마피아 게임 전체 역할 설명을 공지용 임베드로 보냅니다.")
)]
pub async fn show_role_descriptions(ctx: Context<'_>) -> Result<(), Error> {
    let mut lines = Vec::new();
    for role in [
        Role::Mafia,
        Role::Police,
        Role::Agent,
        Role::Vigilante,
        Role::Inspector,
        Role::Doctor,
        Role::Nurse,
        Role::Gangster,
        Role::Prophet,
        Role::Psychologist,
        Role::Hypnotist,
        Role::Mercenary,
        Role::Detective,
        Role::Shaman,
        Role::Priest,
        Role::Graverobber,
        Role::Politician,
        Role::Judge,
        Role::Reporter,
        Role::Hacker,
        Role::Terrorist,
        Role::Lover,
        Role::Soldier,
        Role::Spy,
        Role::Contractor,
        Role::Thief,
        Role::Witch,
        Role::Scientist,
        Role::Madam,
        Role::Godfather,
        Role::CultLeader,
        Role::Fanatic,
        Role::Joker,
        Role::Citizen,
    ] {
        lines.push(format!("**{}** - {}", role.value(), role_short_guide(role)));
    }
    reply_embed(
        ctx,
        lines.join("\n"),
        "역할 설명",
        serenity::Colour::GOLD,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "마피아능력",
    description_localized("ko", "배정받은 역할과 능력 설명을 다시 확인합니다.")
)]
pub async fn show_abilities(ctx: Context<'_>) -> Result<(), Error> {
    let Some(guild_id) = ctx.guild_id() else {
        reply_embed(
            ctx,
            "서버에서만 사용할 수 있습니다.",
            "능력 설명",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let Some(running) = ctx.data().games.get(&guild_id).map(|entry| entry.clone()) else {
        reply_embed(
            ctx,
            "진행 중인 게임이 없습니다.",
            "능력 설명",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let running_read = running.read().await;
    let Some(player) = running_read.game.get_player(ctx.author().id.get()) else {
        reply_embed(
            ctx,
            "현재 게임 참가자만 능력 설명을 확인할 수 있습니다.",
            "능력 설명",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    reply_embed(
        ctx,
        role_message(&running_read.game, player),
        "능력 설명",
        serenity::Colour::GOLD,
        true,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "용어정보",
    description_localized("ko", "마피아 게임 용어 하나를 확인합니다.")
)]
pub async fn show_term_info(
    ctx: Context<'_>,
    #[description = "설명을 볼 용어"] 용어: String,
) -> Result<(), Error> {
    let Some(term) = find_term_by_name(&용어) else {
        reply_embed(
            ctx,
            "용어를 찾을 수 없습니다. 정확한 용어를 입력하세요.",
            "용어정보",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    reply_embed(
        ctx,
        format!("분류: {}\n\n{}", term.category, term_field_value(&term)),
        &format!("용어정보 - {}", term.names[0]),
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "용어설명",
    description_localized("ko", "마피아 게임 용어 설명을 공지용 임베드로 보냅니다.")
)]
pub async fn show_term_descriptions(ctx: Context<'_>) -> Result<(), Error> {
    for (index, (title, body)) in term_guide_pages().into_iter().enumerate() {
        if index == 0 {
            reply_embed(ctx, body, &title, serenity::Colour::GOLD, false).await?;
        } else {
            send_channel_embed(
                &ctx.serenity_context().http,
                ctx.channel_id(),
                body,
                &title,
                serenity::Colour::GOLD,
                vec![],
            )
            .await?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct TermEntry {
    category: String,
    names: Vec<String>,
    meaning: String,
    example: String,
}

pub fn find_term_by_name(name: &str) -> Option<TermEntry> {
    let query = name.trim().to_lowercase();
    if query.is_empty() {
        return None;
    }
    let terms = mafia_term_entries();
    for term in &terms {
        if term.names.iter().any(|alias| alias.to_lowercase() == query) {
            return Some(term.clone());
        }
    }
    let matches = terms
        .into_iter()
        .filter(|term| {
            term.names
                .iter()
                .any(|alias| alias.to_lowercase().contains(&query))
        })
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        matches.into_iter().next()
    } else {
        None
    }
}

pub fn term_field_value(term: &TermEntry) -> String {
    let mut lines = vec![term.meaning.clone()];
    if term.names.len() > 1 {
        lines.push(format!("같은 말: {}", term.names[1..].join(", ")));
    }
    if !term.example.is_empty() {
        lines.push(format!("예시: {}", term.example));
    }
    lines.join("\n")
}

pub fn term_guide_pages() -> Vec<(String, String)> {
    let mut pages = Vec::new();
    let mut grouped: Vec<(String, Vec<TermEntry>)> = Vec::new();
    for term in mafia_term_entries() {
        if let Some((_category, terms)) = grouped
            .iter_mut()
            .find(|(category, _terms)| *category == term.category)
        {
            terms.push(term);
        } else {
            grouped.push((term.category.clone(), vec![term]));
        }
    }
    for (category, terms) in grouped {
        let mut body =
            "마피아42 용어 문서를 참고해 이 봇 진행에 맞게 짧게 정리한 용어집입니다.".to_string();
        let mut page_index = 1;
        for term in terms {
            let entry = format!("\n\n**{}**\n{}", term.names[0], term_field_value(&term));
            if body.len() + entry.len() > 3600 {
                let title = if page_index == 1 {
                    format!("용어 설명 - {category}")
                } else {
                    format!("용어 설명 - {category} {page_index}")
                };
                pages.push((title, body));
                page_index += 1;
                body = "마피아42 용어 문서를 참고해 이 봇 진행에 맞게 짧게 정리한 용어집입니다."
                    .to_string();
            }
            body.push_str(&entry);
        }
        let title = if page_index == 1 {
            format!("용어 설명 - {category}")
        } else {
            format!("용어 설명 - {category} {page_index}")
        };
        pages.push((title, body));
    }
    pages
}

pub fn mafia_term_entries() -> Vec<TermEntry> {
    macro_rules! t {
        ($cat:expr, [$($name:expr),+], $meaning:expr, $example:expr) => {
            TermEntry {
                category: $cat.to_string(),
                names: vec![$($name.to_string()),+],
                meaning: $meaning.to_string(),
                example: $example.to_string(),
            }
        };
    }
    vec![
        t!(
            "기본",
            ["n픽", "픽"],
            "플레이어 위치나 번호를 부르는 말입니다.",
            "3픽 조사 = 3번 플레이어를 조사"
        ),
        t!(
            "기본",
            ["직공", "ㅈㄱ"],
            "자기 직업을 공개한다는 뜻입니다.",
            "경찰 직공 ㄱ"
        ),
        t!(
            "기본",
            ["조결"],
            "경찰, 사립탐정 등 조사 역할의 결과입니다.",
            "2픽 노맢 조결"
        ),
        t!(
            "기본",
            ["퍼블"],
            "첫 번째 밤에 마피아 공격으로 죽은 사람입니다.",
            "퍼블이 경찰이면 퍼경"
        ),
        t!(
            "기본",
            ["퍼경", "경퍼"],
            "첫날 밤에 수사직이 죽은 상황입니다.",
            "경찰이 안 나오면 퍼경 가능성 체크"
        ),
        t!(
            "기본",
            ["연퍼"],
            "이전 판에 이어 또 첫날 죽은 사람을 가리킵니다.",
            "방마다 매너 기준이 다를 수 있음"
        ),
        t!(
            "기본",
            ["아봉"],
            "말을 거의 하지 않는 상태입니다. 잠수와 달리 투표나 능력은 할 수 있습니다.",
            ""
        ),
        t!(
            "기본",
            ["홀직", "홀경", "홀의"],
            "그 직업을 주장하는 사람이 한 명뿐인 상황입니다.",
            "홀경 = 경찰 주장 1명"
        ),
        t!(
            "기본",
            ["맞직", "맞경", "맞의"],
            "같은 직업을 주장하는 사람이 둘 이상인 상황입니다.",
            "맞경이면 둘 중 하나가 거짓일 가능성 큼"
        ),
        t!(
            "기본",
            ["쓰리직", "쓰리경", "쓰리의"],
            "같은 직업 주장자가 세 명인 상황입니다.",
            "쓰리경이면 보조나 마피아가 섞였을 가능성 큼"
        ),
        t!(
            "기본",
            ["늦직", "눈치직"],
            "다른 사람이 직업을 밝힌 뒤 늦게 같은 직업으로 나온 사람입니다.",
            "늦경은 의심을 받기 쉬움"
        ),
        t!(
            "기본",
            ["진직", "진경", "진의"],
            "맞직 중 진짜 직업인 사람입니다.",
            "진경을 살려야 함"
        ),
        t!(
            "기본",
            ["짭직", "구라직", "짭경", "구라경"],
            "맞직 중 가짜 직업인 사람입니다.",
            "짭의가 달림"
        ),
        t!(
            "기본",
            ["확직", "확경", "확의"],
            "직업이나 시민성이 거의 확정된 사람입니다.",
            "확직이 오더를 잡음"
        ),
        t!(
            "기본",
            ["반확"],
            "완전 확정은 아니지만 시민 가능성이 높은 사람입니다.",
            "홀경이 반확으로 오더"
        ),
        t!(
            "기본",
            ["무직", "백수"],
            "능력을 쓸 수 없거나 쓸 일이 사라진 직업 상태입니다.",
            "도굴 실패 도굴꾼은 사실상 무직"
        ),
        t!(
            "진영/직업",
            ["시팀"],
            "시민팀입니다.",
            "시팀은 마피아 제거가 목표"
        ),
        t!(
            "진영/직업",
            ["맢팀", "마피아팀"],
            "마피아팀입니다.",
            "접선한 보조도 맢팀으로 봄"
        ),
        t!(
            "진영/직업",
            ["교팀", "교주팀"],
            "교주, 광신도, 포교된 사람을 포함한 교주팀입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["중직"],
            "중요 직업입니다. 보통 수사직과 의사를 말합니다.",
            "경찰/요원/자경단원, 의사"
        ),
        t!(
            "진영/직업",
            ["특직", "특"],
            "중직을 제외한 시민팀 특수 직업입니다.",
            "기자, 영매, 군인 등"
        ),
        t!(
            "진영/직업",
            ["보조"],
            "마피아팀 특수 직업입니다.",
            "스파이, 마녀, 청부업자 등"
        ),
        t!(
            "진영/직업",
            ["보광교"],
            "마피아를 제외한 악인 후보를 묶어 부르는 말입니다.",
            "보조/광신도/교주"
        ),
        t!(
            "진영/직업",
            ["맢", "ㅁ"],
            "마피아의 줄임말입니다.",
            "2맢 남음"
        ),
        t!(
            "진영/직업",
            ["맢킬"],
            "마피아의 밤 공격입니다.",
            "맢킬 대상 예측"
        ),
        t!(
            "진영/직업",
            ["홀맢"],
            "동료 마피아가 죽고 혼자 남은 마피아입니다.",
            ""
        ),
        t!("진영/직업", ["짝맢", "팀맢"], "같은 팀 마피아입니다.", ""),
        t!(
            "조사/판정",
            ["경크"],
            "경찰 조사에서 마피아라고 나온 결과입니다.",
            "3픽 경크"
        ),
        t!(
            "조사/판정",
            ["노맢"],
            "경찰 조사에서 마피아가 아니라고 나온 결과입니다.",
            "4픽 노맢"
        ),
        t!(
            "조사/판정",
            ["맞경조사", "맞조"],
            "맞경 상대를 조사하는 행동입니다. 보통 정보 가치가 낮아 의심받기 쉽습니다.",
            ""
        ),
        t!(
            "조사/판정",
            ["시조", "시체조사"],
            "그날 죽은 사람을 조사했다고 주장하는 것입니다.",
            "거짓 조결로 의심받기 쉬움"
        ),
        t!(
            "조사/판정",
            ["자조", "자기조사"],
            "자기 자신을 조사했다는 뜻입니다.",
            ""
        ),
        t!(
            "조사/판정",
            ["특경크", "특크"],
            "특직 주장자에게 마피아 판정을 내는 것입니다.",
            ""
        ),
        t!(
            "조사/판정",
            ["팀경크", "팀크"],
            "마피아팀끼리 일부러 서로를 마피아라고 몰아 신뢰를 얻으려는 전략입니다.",
            ""
        ),
        t!(
            "조사/판정",
            ["찍경크", "찍크"],
            "확실한 근거 없이 첫날 아무나 마피아라고 찍는 전략입니다.",
            ""
        ),
        t!(
            "조사/판정",
            ["팀노맢"],
            "마피아가 같은 팀에게 마피아가 아니라고 결과를 내는 전략입니다.",
            ""
        ),
        t!(
            "조사/판정",
            ["루트", "룻"],
            "사립탐정 추적을 의식해 밤 행동 대상을 규칙적으로 바꾸는 경로입니다.",
            "루트온 = 정한 루트대로 움직임"
        ),
        t!(
            "투표/진행",
            ["오더"],
            "투표나 행동 방향을 정해 지시하는 것입니다.",
            "확직 오더 따르기"
        ),
        t!(
            "투표/진행",
            ["대립"],
            "두 주장이나 두 사람이 서로 맞서는 구도입니다.",
            "맞직도 대립의 한 종류"
        ),
        t!(
            "투표/진행",
            ["교환", "x교"],
            "죽은 사람과 산 사람 중 한 명 이상이 마피아라고 보고 산 사람을 처형하는 판단입니다.",
            "경교, 의교"
        ),
        t!(
            "투표/진행",
            ["맞투"],
            "두 사람이 서로에게만 투표하게 하는 방식입니다.",
            "나머지는 스킵/무투"
        ),
        t!(
            "투표/진행",
            ["추미", "추리미스"],
            "시민팀을 잘못 의심해 처형하거나 판을 망친 판단입니다.",
            "추미 나면 사과하는 편이 좋음"
        ),
        t!(
            "투표/진행",
            ["역추리"],
            "일반적인 흐름과 반대로 판단하는 추리입니다.",
            "근거 없이 남발하면 위험"
        ),
        t!(
            "투표/진행",
            ["3:3", "3ㄷ3"],
            "생존 구도가 시민팀 3명 대 마피아팀 3명에 가까운 위험 상황입니다.",
            "보통 마피아가 매우 유리"
        ),
        t!(
            "투표/진행",
            ["n투찬", "n투반"],
            "n픽을 지목한 뒤 찬성/반대를 누르라는 짧은 오더입니다.",
            "5투찬 = 5픽 올리고 찬성"
        ),
        t!(
            "투표/진행",
            ["포커싱", "경포", "특포"],
            "특정 직업군 안에서 처형 대상을 찾자는 흐름입니다.",
            "경포 = 경찰 주장자 중 처형"
        ),
        t!(
            "전략/상황",
            ["지정힐"],
            "의사 후보들이 서로 다른 대상을 치료하게 해 진위를 가리는 방식입니다.",
            "1은 3힐, 5는 2힐"
        ),
        t!(
            "전략/상황",
            ["힐배", "킬배"],
            "의사의 치료 성공 여부로 승패가 갈리는 상황입니다.",
            ""
        ),
        t!(
            "전략/상황",
            ["홀경작"],
            "마피아가 경찰을 안 나와 홀경을 만들고 퍼블 경찰을 주장하는 전략입니다.",
            ""
        ),
        t!(
            "전략/상황",
            ["홀의작"],
            "마피아가 의사 대립을 피해서 홀의를 만들거나 역이용하는 전략입니다.",
            ""
        ),
        t!(
            "전략/상황",
            ["역홀작"],
            "자경단원 등이 숨어 있다가 경찰 사칭 마피아를 노리는 역전 전략입니다.",
            ""
        ),
        t!(
            "전략/상황",
            ["위장"],
            "자기 직업이 아닌 다른 직업처럼 행동하는 것입니다.",
            "군인 위장, 기자 위장"
        ),
        t!(
            "전략/상황",
            ["위칸"],
            "위장한 사람들이 동시에 진짜 직업을 밝히도록 카운트하는 것입니다.",
            ""
        ),
        t!(
            "전략/상황",
            ["룻칸"],
            "루트 공개를 동시에 맞추기 위해 카운트하는 것입니다.",
            ""
        ),
        t!(
            "전략/상황",
            ["노살"],
            "성직자가 특정 사망자를 살리지 말라는 의미로 쓰입니다.",
            "맞직을 살리지 말라는 오더"
        ),
        t!(
            "전략/상황",
            ["고의시조", "고시"],
            "일부러 시체 조사 결과를 내는 전략입니다. 리스크가 큽니다.",
            ""
        ),
        t!(
            "전략/상황",
            ["짜치"],
            "마피아팀이 미리 말을 맞춰 속이는 행동입니다.",
            ""
        ),
        t!(
            "전략/상황",
            ["올직공"],
            "전원이 직업을 공개하는 진행입니다.",
            "청부 위험이 없을 때 고려"
        ),
        t!(
            "기본",
            ["풍지"],
            "12인 방 등에서 직공을 뜻하는 말입니다.",
            "3풍지 = 3픽 직공"
        ),
        t!(
            "기본",
            ["방매", "ㅂㅁ"],
            "방장이나 특정 참가자를 초반에 죽이거나 조사하지 말자는 매너 룰입니다.",
            ""
        ),
        t!(
            "기본",
            ["노연퍼", "ㄴㅇㅍ"],
            "연속 퍼블 대상이 없는 상태입니다.",
            ""
        ),
        t!(
            "기본",
            ["노연퍼고정", "노연고", "ㄴㅇㅍㄱㅈ", "ㄴㅇㄱ"],
            "연퍼를 챙기지 않기로 고정한다는 뜻입니다.",
            ""
        ),
        t!(
            "기본",
            ["고퍼"],
            "특정 사람을 일부러 첫밤에 죽여달라는 고정 퍼블입니다.",
            ""
        ),
        t!(
            "기본",
            ["자투", "ㅈㅌ"],
            "자기 자신에게 투표하는 것입니다. 하루를 넘기거나 인증용으로 쓰입니다.",
            ""
        ),
        t!(
            "기본",
            ["무투", "ㅁㅌ"],
            "아무에게도 투표하지 않는 것입니다.",
            ""
        ),
        t!(
            "기본",
            ["시무", "ㅅㅁ"],
            "시간 단축 후 무투표로 넘기자는 말입니다.",
            ""
        ),
        t!(
            "기본",
            ["맢표"],
            "마피아팀이 몰래 던진 것으로 보이는 표입니다.",
            "처형 흐름과 다른 곳에 갑자기 표가 생김"
        ),
        t!(
            "기본",
            ["몰투", "몰표"],
            "한 사람에게 표가 몰리는 상황입니다.",
            ""
        ),
        t!(
            "기본",
            ["투갈", "표갈"],
            "투표가 갈려 최다 득표자가 여러 명이 되는 상황입니다.",
            ""
        ),
        t!(
            "기본",
            ["물타기"],
            "뚜렷한 근거 없이 남의 투표 흐름에 따라가는 행동입니다.",
            ""
        ),
        t!(
            "기본",
            ["잠수"],
            "말과 행동을 거의 하지 않는 상태나 그런 사람입니다.",
            ""
        ),
        t!(
            "기본",
            ["묵언수행"],
            "채팅은 하지 않지만 투표나 능력은 사용하는 상태입니다.",
            ""
        ),
        t!("기본", ["시단", "ㅅㄷ"], "낮 시간을 줄이는 행동입니다.", ""),
        t!("기본", ["시증", "ㅅㅈ"], "낮 시간을 늘리는 행동입니다.", ""),
        t!(
            "기본",
            ["칼시단"],
            "낮이 되자마자 시간을 줄이는 행동입니다.",
            ""
        ),
        t!(
            "기본",
            ["늦시단"],
            "낮 시간이 어느 정도 지난 뒤 시간을 줄이는 행동입니다.",
            ""
        ),
        t!(
            "기본",
            ["시단플"],
            "충분한 토론 없이 시간을 줄이는 플레이를 말합니다.",
            ""
        ),
        t!(
            "기본",
            ["조밤"],
            "밤에 아무도 죽지 않거나 큰 결과가 없는 조용한 밤입니다.",
            ""
        ),
        t!(
            "기본",
            ["고의조밤", "고조"],
            "마피아가 일부러 처형을 성립시키지 않아 만든 조밤입니다.",
            ""
        ),
        t!(
            "기본",
            ["교밤"],
            "교주가 포교할 수 있는 밤입니다. 보통 홀수날 밤을 말합니다.",
            ""
        ),
        t!(
            "기본",
            ["교종"],
            "교주의 포교 성공 안내, 즉 종소리 메시지를 말합니다.",
            ""
        ),
        t!(
            "기본",
            ["물총"],
            "마피아 공격이 실패하거나 계속 빗나간 상황입니다.",
            ""
        ),
        t!(
            "기본",
            ["자총"],
            "마피아가 자기 자신이나 팀을 죽이는 선택입니다.",
            ""
        ),
        t!(
            "기본",
            ["도도"],
            "도굴꾼에게 특정 직업을 넘기려는 도굴 도박입니다.",
            ""
        ),
        t!(
            "기본",
            ["밤챗"],
            "밤에만 가능한 비밀 대화입니다.",
            "마피아, 연인, 영매, 교주팀 등"
        ),
        t!(
            "진영/직업",
            ["또맢", "연맢"],
            "전판에 이어 또 마피아가 된 상황입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["은폐"],
            "원작 듀얼 능력 이름입니다. 마피아 관련 표현으로도 쓰입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["위선"],
            "원작 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["승부수", "승수"],
            "원작 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["수습"],
            "원작 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["퇴마"],
            "원작 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["무법", "무법자"],
            "원작 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["nㄱㅋ"],
            "n픽을 광클했다는 뜻입니다. 경크와 다릅니다.",
            "연인/원작 표현"
        ),
        t!("진영/직업", ["슾"], "스파이의 줄임말입니다.", ""),
        t!(
            "진영/직업",
            ["n긁슾"],
            "n픽을 조사한 스파이로 보인다는 말입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["첫접슾"],
            "첫날 밤에 마피아를 찾아 바로 접선한 스파이입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["자객"],
            "원작 스파이 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["미인계"],
            "원작 스파이 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["슾크"],
            "스파이가 교주팀 쪽 정보를 찾아 폭로하는 상황입니다.",
            "교주 모드 참고"
        ),
        t!(
            "진영/직업",
            ["n접"],
            "n픽 마피아와 접선했다는 말입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["짐인", "짐"],
            "원작 짐승인간의 줄임말입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["짐인킬"],
            "짐승인간의 처치로 사망한 상황입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["짐인판", "ㅈㅇㅍ"],
            "마피아가 짐승인간을 공격해 조밤이 난 판입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["포효"],
            "원작 짐승인간 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["야만", "야만성"],
            "원작 짐승인간 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["n먹"],
            "n픽을 이용해 접선하자는 말입니다.",
            "원작 짐승인간 표현"
        ),
        t!(
            "진영/직업",
            ["마담판"],
            "마담이 존재하거나 유혹을 받았음을 알리는 말입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["현혹"],
            "원작 마담 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["데뷔"],
            "원작 마담 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["후계", "후계자"],
            "원작 도둑 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["조문"],
            "원작 도둑 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["개굴"],
            "마녀 저주로 개구리가 된 상태입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["망마", "망각술"],
            "원작 망각술 능력을 가진 마녀를 말합니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["과자", "학자", "곽자"],
            "과학자의 줄임말입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["과그로"],
            "과학자가 일부러 어그로를 끄는 행동입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["분석", "최면"],
            "원작 과학자 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["분석투", "최면투"],
            "분석/최면 능력과 관련된 투표 표현입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["사기", "기꾼"],
            "원작 사기꾼의 줄임말입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["청부", "ㅊㅂ"],
            "청부업자의 줄임말입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["청부킬", "암살", "썰다"],
            "청부업자가 능력으로 대상을 제거하는 것입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["직감"],
            "원작 청부업자 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "조사/판정",
            ["n노맢", "nㄴㅁ"],
            "n픽이 마피아가 아니라는 조사 결과입니다.",
            ""
        ),
        t!(
            "조사/판정",
            ["n맢", "nㅁ"],
            "n픽이 마피아라는 조사 결과입니다.",
            ""
        ),
        t!(
            "조사/판정",
            ["체나조사", "ㅊㄴ조사"],
            "나이트 말 움직임처럼 대상을 골라 조사했다는 표현입니다.",
            "원작 표현"
        ),
        t!(
            "조사/판정",
            ["영장"],
            "원작 경찰 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "조사/판정",
            ["기밀"],
            "원작 경찰 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "조사/판정",
            ["도청"],
            "원작 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "조사/판정",
            ["부검"],
            "원작 경찰 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "조사/판정",
            ["랜조", "랜"],
            "기밀 능력 등으로 나온 랜덤 조사 결과입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "조사/판정",
            ["직조", "직"],
            "랜덤이 아니라 직접 고른 조사 결과입니다.",
            ""
        ),
        t!(
            "조사/판정",
            ["탐크"],
            "사립탐정 추적으로 마피아팀 단서를 잡은 상황입니다.",
            ""
        ),
        t!(
            "조사/판정",
            ["해크"],
            "해커가 악인 쪽 직업을 알아낸 상황입니다.",
            ""
        ),
        t!(
            "조사/판정",
            ["성크"],
            "성직자가 교주팀의 포교 시도를 받아 교주 정보를 얻은 상황입니다.",
            ""
        ),
        t!(
            "조사/판정",
            ["광크"],
            "광신도가 마피아를 확인한 상황입니다.",
            "교주/원작 표현"
        ),
        t!(
            "조사/판정",
            ["시체경크"],
            "죽은 사람에게 마피아 판정을 냈다는 주장입니다.",
            ""
        ),
        t!(
            "조사/판정",
            ["보조경크", "보조경"],
            "마피아가 보조직업에게 마피아 판정을 낸 상황입니다.",
            ""
        ),
        t!(
            "조사/판정",
            ["맞경노맢"],
            "맞경 중 한 명이 상대 맞경을 노맢으로 낸 상황입니다.",
            ""
        ),
        t!("진영/직업", ["자경"], "자경단원의 줄임말입니다.", ""),
        t!(
            "진영/직업",
            ["노손자경", "ㄴㅅㅈㄱ"],
            "첫날 능력을 쓰지 않은 자경단원입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["n손자경", "nㅅㅈㄱ"],
            "n픽에게 능력을 쓴 자경단원입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["n탕자경", "nㅌㅈㄱ"],
            "n픽에게 숙청/처형 능력을 쓴 자경단원입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["캔디자경"],
            "4픽에게 능력을 쓴 자경단원을 장난스럽게 부르는 말입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["자경킬"],
            "자경단원의 처형으로 사망한 상황입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["결사", "결"],
            "원작 자경단원 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!("진영/직업", ["의"], "의사의 줄임말입니다.", ""),
        t!(
            "진영/직업",
            ["자힐", "ㅈㅎ"],
            "의사가 자기 자신을 치료하는 것입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["타힐", "ㅌㅎ"],
            "의사가 다른 사람을 치료하는 것입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["센힐", "눈힐"],
            "의사가 눈치 있게 중요한 대상을 치료하는 것입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["연퍼타힐", "ㅇㅍㅌㅎ"],
            "연퍼라서 다른 사람을 치료했다는 의사 주장입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["검진타힐", "ㄱㅈㅌㅎ"],
            "검진 능력 때문에 타힐했다는 원작 의사 표현입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["n접의"],
            "n픽 간호사와 접선한 의사입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["힐룻온", "ㅎㄹㅇ"],
            "사립탐정 추적을 의식해 치료 루트를 돌렸다는 말입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["검진"],
            "원작 의사 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["박애"],
            "원작 의사 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["진정"],
            "원작 의사 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["군", "진군"],
            "군인 또는 진짜 군인을 뜻합니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["위군", "ㅇㄱ"],
            "군인인 척 위장하는 특직 표현입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["군크"],
            "군인이 보조직업 등 단서를 잡은 상황입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["군그로"],
            "군인이 방탄을 유도하려고 어그로를 끄는 행동입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["정신", "정신력"],
            "원작 군인 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["불굴", "불"],
            "원작 군인 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!("진영/직업", ["정", "정치"], "정치인의 줄임말입니다.", ""),
        t!(
            "진영/직업",
            ["정치인증", "정인"],
            "정치인의 투표 면역을 발동시켜 정치인임을 인증하는 것입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["자투정인"],
            "자기투표로 정치인증을 하려는 행동입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["독정"],
            "원작 독재 능력을 가진 정치인을 뜻합니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["영"],
            "영매 또는 원작 경찰 듀얼 능력 이름으로 쓰입니다. 문맥을 봐야 합니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["영매퍼직공", "영퍼직", "ㅇㅁㅍㅈㄱ"],
            "영매가 첫날 사망자의 직업을 묻거나 알리는 표현입니다.",
            ""
        ),
        t!("진영/직업", ["성결"], "영매 성불 결과입니다.", ""),
        t!(
            "진영/직업",
            ["칼성"],
            "밤이 되자마자 성불하는 것입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["강령"],
            "원작 영매 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!("진영/직업", ["연"], "연인의 줄임말입니다.", ""),
        t!(
            "진영/직업",
            ["연그로"],
            "연인이 일부러 다른 직업처럼 보이며 어그로를 끄는 행동입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["암호"],
            "연인끼리 밤에 정해 낮에 서로를 증명하는 말입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["원한"],
            "원작 연인 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["헌신"],
            "원작 연인 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["건", "건달"],
            "원작 건달의 줄임말입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["무협", "노협", "무협건", "ㅁㅎㄱ"],
            "건달이 협박을 하지 않았다는 말입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["첫협"],
            "첫날에 협박을 사용한 건달입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["n협"],
            "n픽을 협박했다는 말입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["갈취"],
            "원작 건달 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["길동무"],
            "원작 건달 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!("진영/직업", ["기"], "기자의 줄임말입니다.", ""),
        t!(
            "진영/직업",
            ["취실"],
            "취재 대상 사망 등으로 특종이 실패한 상황입니다.",
            ""
        ),
        t!("진영/직업", ["속보"], "기자의 특종 공개를 말합니다.", ""),
        t!(
            "진영/직업",
            ["속기"],
            "원작 속보 능력 기자를 말합니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["부고"],
            "원작 기자 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["셀카"],
            "원작 기자 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["자찍"],
            "기자가 자기 자신을 취재하는 것입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["기레기"],
            "정보 가치가 낮거나 불리한 취재를 한 기자를 비꼬는 말입니다.",
            ""
        ),
        t!("진영/직업", ["탐", "사탐"], "사립탐정의 줄임말입니다.", ""),
        t!(
            "진영/직업",
            ["n손m", "nㅅm"],
            "n픽이 m픽에게 능력을 사용했다는 탐정 결과입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["n노손", "nㄴㅅ"],
            "n픽이 능력을 사용하지 않았다는 탐정 결과입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["함정"],
            "원작 사립탐정 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["도굴무직", "도무", "ㄷㅁ"],
            "도굴꾼이 직업을 얻지 못한 상태입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["도굴OO", "도O"],
            "도굴꾼이 이어받은 직업을 알리는 표현입니다.",
            "도경 = 경찰을 도굴"
        ),
        t!(
            "진영/직업",
            ["계승"],
            "원작 도굴꾼 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["망령"],
            "원작 도굴꾼 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["테러", "ㅌㄹ"],
            "테러리스트의 줄임말입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["n손테", "nㅅㅌ"],
            "테러리스트가 n픽을 지목했다는 말입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["테펑"],
            "테러리스트 능력이 터져 함께 죽는 상황입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["유폭"],
            "원작 테러리스트 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["섬광"],
            "원작 테러리스트 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!("진영/직업", ["성직", "ㅅㅈ"], "성직자의 줄임말입니다.", ""),
        t!(
            "진영/직업",
            ["부실"],
            "부활 실패입니다.",
            "성불 대상 소생 실패 등"
        ),
        t!(
            "진영/직업",
            ["구마"],
            "원작 성직자 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["구희", "구마희생"],
            "구마/희생 능력 조합을 줄여 부르는 말입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["술사", "마술"],
            "원작 마술사의 줄임말입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["노트릭술사", "노트술", "ㄴㅌㄹㅅㅅ"],
            "아직 트릭을 걸지 않은 마술사입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["n트릭", "n트술", "nㅌㅅ"],
            "n픽에게 트릭을 걸었다는 말입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["트인"],
            "마술사 트릭 인증입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["조수"],
            "원작 마술사 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["예자", "예언"],
            "원작 예언자의 줄임말입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["도선예"],
            "도주/선각 능력 예언자를 줄여 부르는 말입니다.",
            "원작/듀얼 참고"
        ),
        t!("진영/직업", ["판"], "판사의 줄임말입니다.", ""),
        t!(
            "진영/직업",
            ["판인"],
            "판사의 투표 판정으로 정체를 인증하는 것입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["관권", "관판"],
            "원작 판사 듀얼 능력 또는 그 능력을 가진 판사입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["간호", "가노", "간", "ㄱㅎ"],
            "간호사의 줄임말입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["n노의사", "nㄴㅇㅅ"],
            "n픽은 의사가 아니라는 간호사 결과입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["n접간"],
            "n픽 의사와 접선한 간호사입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["검시", "검간"],
            "원작 간호사 듀얼 능력 또는 그 능력을 가진 간호사입니다.",
            "원작/듀얼 참고"
        ),
        t!("진영/직업", ["해", "햌"], "해커의 줄임말입니다.", ""),
        t!(
            "진영/직업",
            ["n해킹", "n햌"],
            "n픽을 해킹했다는 말입니다.",
            ""
        ),
        t!("진영/직업", ["해결"], "해킹 결과입니다.", ""),
        t!(
            "진영/직업",
            ["프록", "노프록"],
            "해커 프록시가 확인되었거나 없다는 말입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["심리", "심"],
            "원작 심리학자의 줄임말입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["nm같팀", "nm같"],
            "n픽과 m픽이 같은 팀이라는 결과입니다.",
            "원작 심리학자 참고"
        ),
        t!(
            "진영/직업",
            ["nm다팀", "nmㄷㅌ"],
            "n픽과 m픽이 다른 팀이라는 결과입니다.",
            "원작 심리학자 참고"
        ),
        t!(
            "진영/직업",
            ["프파"],
            "원작 프로파일링 능력을 가진 심리학자입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["n의뢰", "nㅇㄹ"],
            "n픽이 의뢰자라는 용병 표현입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["홀수의뢰", "짝수의뢰", "홀의뢰", "짝의뢰"],
            "의뢰자가 홀수/짝수 픽에 있다는 용병 표현입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["공무", "공"],
            "원작 공무원의 줄임말입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["○판", "노○판"],
            "특정 직업이 있거나 없는 판을 말합니다.",
            "마담판, 노교판 등"
        ),
        t!(
            "진영/직업",
            ["색출", "색공"],
            "원작 공무원 듀얼 능력 또는 그 능력을 가진 공무원입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["감사"],
            "원작 공무원 듀얼 능력 이름입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["비결"],
            "원작 비밀결사의 줄임말입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["낮비결", "밤비결"],
            "낮/밤 비밀결사를 구분하는 표현입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["파파"],
            "원작 파파라치의 줄임말입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["노이슈", "노공유", "노정보"],
            "밤에 공유받은 정보가 없다는 파파라치 표현입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["이슈옴", "공유옴"],
            "밤에 정보가 왔다는 파파라치 표현입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["n이슈", "n직공유"],
            "n픽 관련 정보를 공유받았다는 말입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["n초 이슈"],
            "몇 초에 이슈가 왔는지까지 말하는 파파라치 표현입니다.",
            "원작 역할 참고"
        ),
        t!(
            "진영/직업",
            ["눈치파파", "눈파"],
            "눈치를 보다가 나온 파파라치라는 말입니다.",
            "원작 역할 참고"
        ),
        t!("진영/직업", ["교"], "교주의 줄임말입니다.", ""),
        t!(
            "진영/직업",
            ["광접교", "접교"],
            "광신도와 접선한 교주입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["설파"],
            "원작 설파 능력을 가진 교주 또는 그로 인한 종소리입니다.",
            "원작/듀얼 참고"
        ),
        t!(
            "진영/직업",
            ["광", "광신", "신도", "팡"],
            "광신도의 줄임말입니다.",
            ""
        ),
        t!(
            "진영/직업",
            ["광접"],
            "광신도가 교주와 접선한 상태입니다.",
            ""
        ),
        t!(
            "투표/진행",
            ["어필"],
            "자신이 시민팀임을 설득하는 발언과 행동입니다.",
            ""
        ),
        t!(
            "투표/진행",
            ["판읽기"],
            "대립, 투표, 밤 결과를 보고 판 구도를 읽는 추리입니다.",
            ""
        ),
        t!(
            "투표/진행",
            ["시민티", "마피아티", "맢티"],
            "발언이나 행동에서 시민/마피아처럼 보이는 느낌입니다.",
            ""
        ),
        t!(
            "투표/진행",
            ["직멘", "직공멘트"],
            "직업을 공개할 때 쓰는 설명 문장입니다.",
            ""
        ),
        t!(
            "투표/진행",
            ["고의대립", "고대"],
            "마피아팀끼리 일부러 대립을 만드는 전략입니다.",
            ""
        ),
        t!(
            "투표/진행",
            ["팀구도"],
            "여러 명이 한 편처럼 묶여 보이는 구도입니다.",
            ""
        ),
        t!(
            "투표/진행",
            ["노확유유"],
            "퍼블에게 확성/유언/유품 같은 공개 단서가 없는 상황입니다.",
            "원작 듀얼 참고"
        ),
        t!(
            "투표/진행",
            ["특경", "특경크"],
            "특직 주장자에게 마피아 판정을 내는 것입니다.",
            ""
        ),
        t!(
            "투표/진행",
            ["이중위장"],
            "위장 직업을 다시 다른 직업으로 푸는 복합 위장입니다.",
            ""
        ),
        t!(
            "투표/진행",
            ["모밀나", "모밀N"],
            "모든 밀서는 나/N픽에게 보내라는 뜻입니다.",
            "원작 듀얼 참고"
        ),
        t!(
            "사장/원작",
            ["돌림투", "돌투"],
            "예전 메타에서 표 없는 사람을 찾기 위해 투표를 돌리던 방식입니다.",
            ""
        ),
        t!(
            "사장/원작",
            ["n초 자투"],
            "정해진 초에 동시에 자투하던 예전 보조 판별 방식입니다.",
            ""
        ),
        t!(
            "사장/원작",
            ["연크"],
            "예전 연인 능력으로 마피아를 알아냈다는 표현입니다.",
            ""
        ),
        t!(
            "사장/원작",
            ["연인퍼블", "연퍼(연인)"],
            "연인 희생 관련 첫밤 사망 표현입니다.",
            ""
        ),
        t!(
            "사장/원작",
            ["특손"],
            "특직이 직공 대신 손을 들어 존재만 알리던 예전 문화입니다.",
            ""
        ),
        t!(
            "사장/원작",
            ["투인", "퉆인"],
            "투표 순서나 투표 사실을 인증하던 예전 방식입니다.",
            ""
        ),
        t!(
            "사장/원작",
            ["픽자", "역픽자"],
            "픽 순서대로 자투하던 예전 방식입니다.",
            ""
        ),
        t!(
            "사장/원작",
            ["도둑고려", "도고", "도고시증"],
            "도둑을 의식해 채팅을 피하던 예전 메타입니다.",
            ""
        ),
        t!(
            "사장/원작",
            ["광기작"],
            "확승 상황에서 보상을 위해 일부러 게임을 끌던 예전 플레이입니다.",
            ""
        ),
        t!(
            "사장/원작",
            ["종전", "종후", "교종전", "교종후"],
            "교주 종소리 전/후를 구분하던 예전 심리학자 표현입니다.",
            ""
        ),
        t!(
            "사장/원작",
            ["지령도도"],
            "예전 지령 정보를 보고 도굴 도박을 하던 전략입니다.",
            ""
        ),
        t!(
            "플레이/매너",
            ["조결충"],
            "조사 결과만 보고 어필과 판읽기를 거의 보지 않는 사람을 비꼬는 말입니다.",
            ""
        ),
        t!(
            "플레이/매너",
            ["보험충"],
            "자기 판단 없이 남에게 책임을 넘기려는 사람을 비꼬는 말입니다.",
            ""
        ),
        t!(
            "플레이/매너",
            ["시단충"],
            "충분히 보지 않고 바로 시간을 줄이는 사람을 비꼬는 말입니다.",
            ""
        ),
        t!(
            "플레이/매너",
            ["자힐충"],
            "상황과 무관하게 자기 치료만 고집하는 의사를 비꼬는 말입니다.",
            ""
        ),
        t!(
            "플레이/매너",
            ["더티플"],
            "게임 외 요소나 비매너 협박으로 판을 흔드는 플레이입니다.",
            ""
        ),
        t!(
            "플레이/매너",
            ["톡플"],
            "외부 메신저로 정보를 공유하는 부정 플레이입니다.",
            ""
        ),
        t!(
            "플레이/매너",
            ["친플"],
            "친분을 이용해 게임 정보를 공유하거나 편을 드는 플레이입니다.",
            ""
        ),
        t!(
            "플레이/매너",
            ["투폰"],
            "여러 기기나 계정으로 같은 판에 들어오는 부정 플레이입니다.",
            ""
        ),
        t!(
            "플레이/매너",
            ["감플", "감정플"],
            "감정 때문에 게임 판단을 망치는 플레이입니다.",
            ""
        ),
        t!(
            "플레이/매너",
            ["욕플"],
            "욕설 위주로 진행하는 플레이입니다.",
            ""
        ),
        t!(
            "플레이/매너",
            ["엽플", "엽서플"],
            "엽서나 보상 등을 걸고 시민성을 주장하는 비매너 플레이입니다.",
            ""
        ),
        t!(
            "플레이/매너",
            ["아봉플"],
            "말은 안 하지만 투표와 능력은 하는 플레이입니다.",
            ""
        ),
        t!(
            "플레이/매너",
            ["찍플"],
            "근거 없이 찍어서 몰아가는 플레이입니다.",
            ""
        ),
        t!(
            "플레이/매너",
            ["룰렛플", "사다리플"],
            "추리 대신 무작위 방식으로 처형 대상을 정하는 플레이입니다.",
            ""
        ),
        t!(
            "플레이/매너",
            ["스킨플"],
            "스킨 정보를 근거로 시민/마피아를 판단하려는 플레이입니다.",
            ""
        ),
        t!(
            "플레이/매너",
            ["스킨묘사플"],
            "자기 스킨을 묘사해 직업을 인증하려는 플레이입니다.",
            ""
        ),
        t!(
            "플레이/매너",
            ["카드플", "덱플"],
            "덱이나 카드 구성을 근거로 추리하는 플레이입니다.",
            "원작 듀얼 참고"
        ),
        t!(
            "플레이/매너",
            ["이모티콘플"],
            "이모티콘 반응 속도나 종류로 시민성을 주장하는 플레이입니다.",
            ""
        ),
        t!(
            "플레이/매너",
            ["초성퀴즈", "초퀴"],
            "초성으로 정보를 숨겨 인증하려는 플레이입니다.",
            ""
        ),
        t!(
            "플레이/매너",
            ["걸기플", "~걸기플"],
            "현실 물건이나 조건을 걸고 결백을 주장하는 비매너 표현입니다.",
            ""
        ),
        t!(
            "플레이/매너",
            ["티어플"],
            "카드 티어나 숙련도를 근거로 믿어달라고 하는 플레이입니다.",
            "원작 듀얼 참고"
        ),
        t!(
            "플레이/매너",
            ["보이스플"],
            "직업별 보이스 대사를 근거로 직업을 주장하는 플레이입니다.",
            "원작 참고"
        ),
        t!(
            "플레이/매너",
            ["보석플"],
            "착용 보석을 근거로 직업을 추리하는 플레이입니다.",
            "원작 참고"
        ),
        t!(
            "플레이/매너",
            ["마명"],
            "마이너스 명성을 뜻합니다. 보통 신뢰도가 낮은 유저로 취급됩니다.",
            "원작 시스템 참고"
        ),
        t!(
            "아웃게임",
            ["자리", "ㅈㄹ"],
            "곧 들어올 사람이 있으니 자리를 비워달라는 방 밖 용어입니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["자첸"],
            "자리 체인지, 즉 자리 교체 요청입니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["○엽", "엽"],
            "엽서 아이템을 줄여 부르는 말입니다.",
            "고엽, 일엽 등"
        ),
        t!("아웃게임", ["엽교"], "엽서를 서로 교환하는 것입니다.", ""),
        t!(
            "아웃게임",
            ["무반"],
            "받은 엽서와 같은 종류를 무한 반사하겠다는 뜻입니다.",
            ""
        ),
        t!("아웃게임", ["우꽉"], "우체통이 꽉 찬 상태입니다.", ""),
        t!(
            "아웃게임",
            ["같종"],
            "같은 종류의 엽서가 이미 남아 있는 상태입니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["같쳌"],
            "같종 상태를 확인해 달라는 말입니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["같케"],
            "같종 상태를 다른 방식으로 케어해준다는 말입니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["같우대"],
            "같종, 우꽉, 대리 관련 조건을 묶어 부르는 말입니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["회재", "맞회재"],
            "엽서를 회수하고 다시 보내는 것입니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["받나"],
            "보상을 받고 방을 나가라는 뜻입니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["일괄"],
            "엽서 등을 한 번에 일괄 반사하겠다는 뜻입니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["큪", "일큪", "황큪"],
            "큐피트 아이템이나 커플 요청을 뜻합니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["획초"],
            "하루 획득량 초과 상태나 이를 노리는 방을 말합니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["접", "접선"],
            "친구 추가를 뜻하는 아웃게임 표현입니다. 인게임 접선과 문맥을 구분해야 합니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["접메", "접챗"],
            "친구끼리 할 수 있는 채팅을 말합니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["강퇴", "킥", "ㅋ"],
            "방에서 강제로 내보내는 것입니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["자킥", "ㅈㅋ"],
            "방장이 오래 시작하지 않아 자동으로 강퇴되는 상태입니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["방장잠수", "방잠"],
            "방장이 잠수한 상태입니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["경징", "경징낡", "경징권"],
            "경고/징벌류 마이너스 엽서를 묶어 부르는 말입니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["교류"],
            "엽서나 대리 목적의 최근 교류 조건입니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["명테"],
            "명성 테러입니다. 마이너스 엽서를 대량으로 보내는 행위입니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["경A징B"],
            "경고 엽서와 징벌 엽서에 각각 다른 보상을 주는 방제 표현입니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["옵패"],
            "패배 수가 승리 수보다 많은 전적 상태입니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["물컬"],
            "기본 물음표 컬렉션 상태를 말합니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["0승0패", "00"],
            "승패가 없는 관상용 또는 새 계정을 말합니다.",
            ""
        ),
        t!(
            "아웃게임",
            ["출보", "접보", "길보"],
            "출석/접속/길드 보상을 줄여 부르는 말입니다.",
            ""
        ),
    ]
}

pub fn find_role_by_name(name: &str) -> Option<Role> {
    let query = name.trim();
    [
        Role::Mafia,
        Role::Doctor,
        Role::Nurse,
        Role::Police,
        Role::Agent,
        Role::Vigilante,
        Role::Inspector,
        Role::Reporter,
        Role::Hacker,
        Role::Detective,
        Role::Shaman,
        Role::Priest,
        Role::Soldier,
        Role::Gangster,
        Role::Prophet,
        Role::Psychologist,
        Role::Hypnotist,
        Role::Mercenary,
        Role::Spy,
        Role::Contractor,
        Role::Thief,
        Role::Witch,
        Role::Scientist,
        Role::Madam,
        Role::Graverobber,
        Role::Godfather,
        Role::Joker,
        Role::Politician,
        Role::Judge,
        Role::Terrorist,
        Role::Lover,
        Role::CultLeader,
        Role::Fanatic,
        Role::Citizen,
    ]
    .into_iter()
    .find(|role| role.value() == query)
}

#[derive(Clone, Copy)]
pub(crate) enum AnonymousMessageKind {
    General { owner_id: u64 },
    Dead { owner_id: u64 },
    Shaman { owner_id: u64 },
    Role { owner_id: u64, role: Role },
}

pub fn anonymous_message_body(message: &serenity::Message) -> String {
    let mut parts = Vec::new();
    let content = message.content.trim();
    if !content.is_empty() {
        parts.push(content.to_string());
    }
    parts.extend(
        message
            .attachments
            .iter()
            .map(|attachment| attachment.url.clone()),
    );
    if parts.is_empty() {
        "(내용 없음)".to_string()
    } else {
        parts.join("\n")
    }
}

pub fn anonymous_avatar_url(author_label: &str) -> Option<String> {
    if let Some(number) = author_label
        .strip_suffix("번")
        .and_then(|value| value.parse::<usize>().ok())
    {
        let color = NUMBER_AVATAR_COLORS[(number.saturating_sub(1)) % NUMBER_AVATAR_COLORS.len()];
        return Some(format!(
            "https://dummyimage.com/128x128/{color}/ffffff.png&text={number}"
        ));
    }
    animal_emoji_code(author_label).map(|code| {
        format!("https://cdn.jsdelivr.net/gh/twitter/twemoji@14.0.2/assets/72x72/{code}.png")
    })
}

pub fn no_mentions() -> serenity::CreateAllowedMentions {
    serenity::CreateAllowedMentions::new()
        .all_users(false)
        .all_roles(false)
        .everyone(false)
        .replied_user(false)
}

pub async fn anonymous_webhook(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    channel_id: serenity::ChannelId,
) -> Option<serenity::Webhook> {
    let cached_url = running
        .read()
        .await
        .anonymous_webhook_urls
        .get(&channel_id)
        .cloned();
    if let Some(url) = cached_url
        && let Ok(webhook) = serenity::Webhook::from_url(&ctx.http, &url).await
    {
        return Some(webhook);
    }

    let webhook = channel_id
        .create_webhook(
            &ctx.http,
            serenity::CreateWebhook::new("Mafia Anonymous")
                .audit_log_reason("마피아 게임 익명 채팅 웹훅 생성"),
        )
        .await
        .ok()?;
    if let Some(url) = webhook.url.as_ref() {
        running
            .write()
            .await
            .anonymous_webhook_urls
            .insert(channel_id, url.expose_secret().to_string());
    }
    Some(webhook)
}

pub async fn send_anonymous_text(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    channel_id: serenity::ChannelId,
    author_label: &str,
    body: &str,
) {
    if let Some(webhook) = anonymous_webhook(ctx, running, channel_id).await {
        let username = author_label.chars().take(80).collect::<String>();
        let mut builder = serenity::ExecuteWebhook::new()
            .content(body)
            .username(username)
            .allowed_mentions(no_mentions());
        if let Some(avatar_url) = anonymous_avatar_url(author_label) {
            builder = builder.avatar_url(avatar_url);
        }
        if webhook.execute(&ctx.http, false, builder).await.is_ok() {
            return;
        }
    }
    let _ = channel_id
        .send_message(
            &ctx.http,
            serenity::CreateMessage::new()
                .content(format!("{author_label}: {body}"))
                .allowed_mentions(no_mentions()),
        )
        .await;
}

pub async fn send_webhook_text(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    channel_id: serenity::ChannelId,
    author_label: &str,
    avatar_url: Option<String>,
    body: &str,
) {
    if let Some(webhook) = anonymous_webhook(ctx, running, channel_id).await {
        let username = author_label.chars().take(80).collect::<String>();
        let mut builder = serenity::ExecuteWebhook::new()
            .content(body)
            .username(username)
            .allowed_mentions(no_mentions());
        if let Some(avatar_url) = avatar_url {
            builder = builder.avatar_url(avatar_url);
        }
        if webhook.execute(&ctx.http, false, builder).await.is_ok() {
            return;
        }
    }
    let _ = channel_id
        .send_message(
            &ctx.http,
            serenity::CreateMessage::new()
                .content(format!("{author_label}: {body}"))
                .allowed_mentions(no_mentions()),
        )
        .await;
}

pub fn message_author_display_name(message: &serenity::Message) -> String {
    message
        .member
        .as_ref()
        .and_then(|member| member.nick.clone())
        .or_else(|| message.author.global_name.clone())
        .unwrap_or_else(|| message.author.name.clone())
}

pub async fn send_message_author_webhook_text(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    channel_id: serenity::ChannelId,
    message: &serenity::Message,
    body: &str,
) {
    send_webhook_text(
        ctx,
        running,
        channel_id,
        &message_author_display_name(message),
        Some(message.author.face()),
        body,
    )
    .await;
}

pub fn anonymous_dead_sender_label(running: &RunningGame, sender: &Player) -> String {
    if sender.alive && sender.role == Role::Shaman {
        "익명의 목소리".to_string()
    } else if running.anonymous_enabled {
        running
            .anonymous_aliases
            .get(&sender.user_id)
            .cloned()
            .unwrap_or_else(|| "익명".to_string())
    } else {
        sender.name.clone()
    }
}

pub async fn send_dead_chat_text(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    channel_id: serenity::ChannelId,
    sender: &Player,
    body: &str,
) {
    let (anonymous_enabled, guild_id, sender_label) = {
        let running_read = running.read().await;
        (
            running_read.anonymous_enabled,
            running_read.guild_id,
            anonymous_dead_sender_label(&running_read, sender),
        )
    };
    if anonymous_enabled {
        send_anonymous_text(ctx, running, channel_id, &sender_label, body).await;
        return;
    }
    if let Ok(member) = guild_id
        .member(ctx, serenity::UserId::new(sender.user_id))
        .await
    {
        send_webhook_text(
            ctx,
            running,
            channel_id,
            &display_name(&member),
            Some(member.face()),
            body,
        )
        .await;
        return;
    }
    send_anonymous_text(ctx, running, channel_id, &sender.name, body).await;
}

pub async fn mirror_role_chat_to_dead(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
    message: &serenity::Message,
    role: Role,
    body: &str,
) {
    let Some(roles) = running_channel_roles(ctx, data, running).await else {
        return;
    };
    let (source_channel_id, viewers) = {
        let running_read = running.read().await;
        (
            running_read.channel_id,
            running_read
                .game
                .players
                .iter()
                .filter(|player| can_use_anonymous_dead_chat(&running_read, player))
                .cloned()
                .collect::<Vec<_>>(),
        )
    };
    if viewers.is_empty() {
        return;
    }
    let category = source_category(ctx, source_channel_id).await;
    let body = format!("[{}채팅] {body}", role.value());
    for viewer in viewers {
        let can_chat = {
            let running_read = running.read().await;
            running_read
                .game
                .get_player(viewer.user_id)
                .is_some_and(|player| can_use_anonymous_dead_chat(&running_read, player))
        };
        if let Some(channel_id) =
            ensure_anonymous_dead_input_channel(ctx, running, &viewer, roles, category, can_chat)
                .await
        {
            send_message_author_webhook_text(ctx, running, channel_id, message, &body).await;
        }
    }
}

pub async fn relay_anonymous_general_message(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    sender_id: u64,
    body: &str,
) {
    let (deliveries, log_channel, sender_alias) = {
        let running_read = running.read().await;
        let Some(sender) = running_read.game.get_player(sender_id) else {
            return;
        };
        let sender_alias = running_read
            .anonymous_aliases
            .get(&sender.user_id)
            .cloned()
            .unwrap_or_else(|| "익명".to_string());
        let deliveries = running_read
            .game
            .alive_players()
            .into_iter()
            .filter(|viewer| viewer.user_id != sender.user_id && !running_read.game.is_frog(viewer))
            .filter_map(|viewer| {
                running_read
                    .anonymous_input_channel_ids
                    .get(&viewer.user_id)
                    .copied()
            })
            .collect::<Vec<_>>();
        (deliveries, running_read.channel_id, sender_alias)
    };
    for channel_id in deliveries {
        send_anonymous_text(ctx, running, channel_id, &sender_alias, body).await;
    }
    send_anonymous_text(
        ctx,
        running,
        log_channel,
        "[익명 로그/일반]",
        &format!("{sender_alias} - {body}"),
    )
    .await;
}

pub async fn relay_anonymous_role_message(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    sender_id: u64,
    role: Role,
    body: &str,
) {
    let (deliveries, log_channel, sender_alias) = {
        let running_read = running.read().await;
        let Some(sender) = running_read.game.get_player(sender_id) else {
            return;
        };
        let sender_alias = running_read
            .anonymous_aliases
            .get(&sender.user_id)
            .cloned()
            .unwrap_or_else(|| "익명".to_string());
        let deliveries = anonymous_role_status_player_ids(&running_read, role)
            .into_iter()
            .filter(|viewer_id| *viewer_id != sender.user_id)
            .filter_map(|viewer_id| {
                let viewer = running_read.game.get_player(viewer_id)?;
                if !can_use_anonymous_role_chat(&running_read, viewer, role) {
                    return None;
                }
                running_read
                    .anonymous_role_input_channel_ids
                    .get(&(viewer_id, role))
                    .copied()
            })
            .collect::<Vec<_>>();
        (deliveries, running_read.channel_id, sender_alias)
    };
    for channel_id in deliveries {
        send_anonymous_text(ctx, running, channel_id, &sender_alias, body).await;
    }
    send_anonymous_text(
        ctx,
        running,
        log_channel,
        &format!("[익명 로그/{}]", role.value()),
        &format!("{sender_alias} - {body}"),
    )
    .await;
}

pub async fn relay_anonymous_dead_message(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
    sender_id: u64,
    body: &str,
) {
    let Some(roles) = running_channel_roles(ctx, data, running).await else {
        return;
    };
    let (source_channel_id, sender, viewers) = {
        let running_read = running.read().await;
        let Some(sender) = running_read.game.get_player(sender_id) else {
            return;
        };
        (
            running_read.channel_id,
            sender.clone(),
            running_read
                .game
                .players
                .iter()
                .filter(|viewer| {
                    viewer.user_id != sender.user_id
                        && can_use_anonymous_dead_chat(&running_read, viewer)
                })
                .cloned()
                .collect::<Vec<_>>(),
        )
    };
    let category = source_category(ctx, source_channel_id).await;
    for viewer in viewers {
        let can_chat = {
            let running_read = running.read().await;
            running_read
                .game
                .get_player(viewer.user_id)
                .is_some_and(|player| can_use_anonymous_dead_chat(&running_read, player))
        };
        if let Some(channel_id) =
            ensure_anonymous_dead_input_channel(ctx, running, &viewer, roles, category, can_chat)
                .await
        {
            send_dead_chat_text(ctx, running, channel_id, &sender, body).await;
        }
    }
}

pub async fn relay_anonymous_shaman_message(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    sender_id: u64,
    body: &str,
) {
    let (deliveries, log_channel, sender_label) = {
        let running_read = running.read().await;
        let Some(sender) = running_read.game.get_player(sender_id) else {
            return;
        };
        let deliveries = running_read
            .game
            .players
            .iter()
            .filter(|viewer| {
                viewer.user_id != sender.user_id
                    && ((!viewer.alive
                        && !running_read
                            .game
                            .purified_dead_ids
                            .contains(&viewer.user_id))
                        || (viewer.alive
                            && viewer.role == Role::Shaman
                            && !running_read.game.is_frog(viewer)))
            })
            .filter_map(|viewer| {
                running_read
                    .anonymous_shaman_input_channel_ids
                    .get(&viewer.user_id)
                    .copied()
            })
            .collect::<Vec<_>>();
        (
            deliveries,
            running_read.shaman_channel_id,
            anonymous_dead_sender_label(&running_read, sender),
        )
    };
    for channel_id in deliveries {
        send_anonymous_text(ctx, running, channel_id, &sender_label, body).await;
    }
    if let Some(channel_id) = log_channel {
        send_anonymous_text(
            ctx,
            running,
            channel_id,
            "[익명 로그/영매]",
            &format!("{sender_label} - {body}"),
        )
        .await;
    }
}

pub async fn handle_anonymous_message(
    ctx: &serenity::Context,
    data: &Data,
    running: Arc<RwLock<RunningGame>>,
    message: &serenity::Message,
    kind: AnonymousMessageKind,
) -> Result<()> {
    let owner_id = match kind {
        AnonymousMessageKind::General { owner_id }
        | AnonymousMessageKind::Dead { owner_id }
        | AnonymousMessageKind::Shaman { owner_id }
        | AnonymousMessageKind::Role { owner_id, .. } => owner_id,
    };
    if message.author.id.get() != owner_id {
        let _ = message.delete(&ctx.http).await;
        return Ok(());
    }

    let body = anonymous_message_body(message);
    let can_relay = {
        let running_read = running.read().await;
        let Some(player) = running_read.game.get_player(owner_id) else {
            return Ok(());
        };
        match kind {
            AnonymousMessageKind::General { .. } => {
                if running_read.game.is_madam_seduced(player) {
                    false
                } else {
                    can_use_anonymous_general_chat(&running_read, player)
                }
            }
            AnonymousMessageKind::Dead { .. } => can_use_anonymous_dead_chat(&running_read, player),
            AnonymousMessageKind::Shaman { .. } => {
                can_use_anonymous_shaman_chat(&running_read, player)
            }
            AnonymousMessageKind::Role { role, .. } => {
                if running_read.game.is_madam_seduced(player) {
                    false
                } else {
                    can_use_anonymous_role_chat(&running_read, player, role)
                }
            }
        }
    };
    if !can_relay {
        return Ok(());
    }

    match kind {
        AnonymousMessageKind::General { .. } => {
            relay_anonymous_general_message(ctx, &running, owner_id, &body).await;
        }
        AnonymousMessageKind::Dead { .. } => {
            relay_anonymous_dead_message(ctx, data, &running, owner_id, &body).await;
        }
        AnonymousMessageKind::Shaman { .. } => {
            relay_anonymous_shaman_message(ctx, &running, owner_id, &body).await;
        }
        AnonymousMessageKind::Role { role, .. } => {
            relay_anonymous_role_message(ctx, &running, owner_id, role, &body).await;
            mirror_role_chat_to_dead(ctx, data, &running, message, role, &body).await;
        }
    }
    Ok(())
}

pub async fn handle_message_event(
    ctx: &serenity::Context,
    data: &Data,
    message: &serenity::Message,
) -> Result<()> {
    if message.author.bot {
        return Ok(());
    }
    let Some(guild_id) = message.guild_id else {
        return Ok(());
    };
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        return Ok(());
    };
    let kind = {
        let running_read = running.read().await;
        if let Some(owner_id) = running_read
            .anonymous_dead_input_channel_owners
            .get(&message.channel_id)
            .copied()
        {
            Some(AnonymousMessageKind::Dead { owner_id })
        } else if let Some(owner_id) = running_read
            .anonymous_shaman_input_channel_owners
            .get(&message.channel_id)
            .copied()
        {
            Some(AnonymousMessageKind::Shaman { owner_id })
        } else if let Some(owner_id) = running_read
            .anonymous_input_channel_owners
            .get(&message.channel_id)
            .copied()
        {
            Some(AnonymousMessageKind::General { owner_id })
        } else {
            running_read
                .anonymous_role_input_channels
                .get(&message.channel_id)
                .copied()
                .map(|(owner_id, role)| AnonymousMessageKind::Role { owner_id, role })
        }
    };
    if let Some(kind) = kind {
        handle_anonymous_message(ctx, data, running, message, kind).await?;
        return Ok(());
    }

    let private_role = {
        let running_read = running.read().await;
        running_read
            .private_channel_ids
            .iter()
            .find_map(|(&role, &channel_id)| (channel_id == message.channel_id).then_some(role))
    };
    if let Some(role) = private_role {
        let player = {
            let running_read = running.read().await;
            running_read
                .game
                .get_player(message.author.id.get())
                .cloned()
        };
        if let Some(player) = player {
            if running.read().await.game.is_madam_seduced(&player) {
                let _ = message.delete(&ctx.http).await;
                set_private_role_member_access(ctx, &running, role, &player, false).await;
            } else {
                let body = anonymous_message_body(message);
                mirror_role_chat_to_dead(ctx, data, &running, message, role, &body).await;
            }
        }
        return Ok(());
    }

    let shaman_seduced = {
        let running_read = running.read().await;
        if running_read.shaman_channel_id == Some(message.channel_id) {
            running_read
                .game
                .get_player(message.author.id.get())
                .filter(|player| running_read.game.is_madam_seduced(player))
                .cloned()
        } else {
            None
        }
    };
    if let Some(player) = shaman_seduced {
        let _ = message.delete(&ctx.http).await;
        set_shaman_channel_member_access(ctx, &running, &player, true, false).await;
        return Ok(());
    }

    let frog_context = {
        let running_read = running.read().await;
        if running_read.frog_channel_id != Some(message.channel_id) {
            None
        } else {
            let player = running_read
                .game
                .get_player(message.author.id.get())
                .cloned();
            let can_croak = player.as_ref().is_some_and(|player| {
                running_read.game.is_frog(player)
                    && !running_read.game.is_madam_seduced(player)
                    && running_read.game.phase == Phase::Day
                    && running_read.day_chat_open
            });
            Some((running_read.channel_id, can_croak))
        }
    };
    if let Some((game_channel_id, can_croak)) = frog_context {
        if can_croak {
            let croak_count = message.content.chars().count().max(1);
            // Start both Discord requests together: waiting for deletion first made every croak
            // pay for two sequential API round trips before it appeared in the main channel.
            let relay = send_channel_embed(
                &ctx.http,
                game_channel_id,
                format!("개구리: {}", "개굴".repeat(croak_count)),
                "개구리 채팅",
                serenity::Colour::DARK_GREEN,
                vec![],
            );
            let delete = message.delete(&ctx.http);
            let _ = tokio::join!(relay, delete);
        } else {
            let _ = message.delete(&ctx.http).await;
        }
    }
    Ok(())
}

// 역할: poise 슬래시 명령어, 컴포넌트 핸들러, 메시지 이벤트 처리,
//        익명 메시지 릴레이, 통계/리더보드, 역할 정보 조회

#![allow(unused_imports, clippy::too_many_arguments, clippy::collapsible_if)]

use super::web_settings;
use super::{
    AnonymousNameMode, Context, ContractorContractDraft, Data, Error, GAME_NOTIFICATION_ROLE,
    LeaderboardMetric, MAX_GAME_PLAYERS, Recruitment, RunningGame, SPECTATOR_ROLE,
};
use crate::channel::*;
use crate::embed::*;
use crate::runner::{
    contractor_contract_components, contractor_contract_prompt, effective_night_role, game_loop,
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
    CITIZEN_SPECIAL_ROLES, ContractorGuessRoleGroup, MAFIA_SPECIAL_ROLES, NEUTRAL_SPECIAL_ROLES,
    NightResult, PUBLIC_CITIZEN_SPECIAL_ROLES, PUBLIC_CULT_SPECIAL_ROLES,
    PUBLIC_MAFIA_SPECIAL_ROLES, PUBLIC_NEUTRAL_SPECIAL_ROLES, Phase, Player, Role, VoteResult,
    Winner, contractor_guess_role_group, is_contractor_guess_role,
};
use mafia_remake::stats;
use poise::serenity_prelude as serenity;
use poise::serenity_prelude::Mentionable;
use rand::seq::{IndexedRandom, SliceRandom};
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Notify, RwLock};
use tokio::task::JoinSet;

mod anonymous_chat;
mod boards;
mod config_cmds;
mod guides;
mod interactions;
pub(crate) use self::anonymous_chat::*;
pub(crate) use self::boards::*;
pub(crate) use self::config_cmds::*;
pub(crate) use self::guides::*;
pub(crate) use self::interactions::*;

const ANONYMOUS_DELIVERY_CONCURRENCY: usize = 4;

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
    let spectator_role_id = role_by_name(ctx.serenity_context(), guild_id, SPECTATOR_ROLE)
        .await?
        .map(|role| role.id);

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
        spectator_role_id,
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
        auto_start_players: None,
        recruitment_seconds: config_snapshot.effective_recruitment_seconds(),
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
        _ = tokio::time::sleep(Duration::from_secs(
            config_snapshot.effective_recruitment_seconds(),
        )) => {}
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
        // 모집 중 부여한 참가자/관전자 역할을 되돌린다. 게임이 시작되지 않아
        // cleanup_game이 돌지 않으므로 여기서 정리해야 역할이 남지 않는다.
        let cancelled_recruitment = rec.clone();
        drop(rec);
        ctx.data().recruitments.remove(&guild_id);
        cleanup_recruitment_roles(ctx.serenity_context(), guild_id, &cancelled_recruitment).await;
        let cleaned = cancelled_recruitment.joined_ids.len()
            + if cancelled_recruitment.spectator_role_id.is_some() {
                cancelled_recruitment.spectator_ids.len()
            } else {
                0
            };
        let notice = if cleaned == 0 {
            "참가자 모집이 취소되었습니다.".to_string()
        } else {
            format!(
                "참가자 모집이 취소되었습니다.\n모집 중 부여한 참가자/관전자 역할 {cleaned}건을 정리했습니다."
            )
        };
        reply_embed(
            ctx,
            notice,
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

    let assignment_user_ids = player_data
        .iter()
        .map(|(user_id, _)| *user_id)
        .collect::<Vec<_>>();
    let assignment_history = {
        let stats_read = ctx.data().stats.read().await;
        stats::player_assignment_histories(&stats_read, &assignment_user_ids)
    };
    let game = MafiaGame::new_with_counts_balanced(
        player_data,
        GameCounts {
            mafia_count: *role_counts.get(&Role::Mafia).unwrap_or(&0),
            doctor_count: *role_counts.get(&Role::Doctor).unwrap_or(&0),
            police_count: *role_counts.get(&Role::Police).unwrap_or(&0),
            agent_count: *role_counts.get(&Role::Agent).unwrap_or(&0),
            vigilante_count: *role_counts.get(&Role::Vigilante).unwrap_or(&0),
            inspector_count: if config_snapshot.default_police_count > 0 {
                *role_counts.get(&Role::Inspector).unwrap_or(&0)
            } else {
                0
            },
            joker_count: if config_snapshot.enable_joker {
                config_snapshot.default_joker_count as usize
            } else {
                0
            },
            special_roles: game_special_roles,
        },
        &assignment_history,
    )?;
    let game = {
        let mut game = game;
        // 개인 티어는 실제 게임 시작 시점에만 굴린다 (생성자에서 굴리면 테스트가
        // 무작위 능력에 흔들린다).
        game.assign_tier_abilities();
        game
    };
    let initial_roles = game.players.iter().map(|p| (p.user_id, p.role)).collect();
    let stats_snapshot = {
        let mut stats_file = ctx.data().stats.write().await;
        stats::record_role_selection(
            &mut stats_file,
            game.players.iter().map(|player| player.role),
        );
        stats_file.clone()
    };
    let stats_path = ctx.data().stats_path.clone();
    match tokio::task::spawn_blocking(move || stats::save_stats(&*stats_path, &stats_snapshot))
        .await
    {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("failed to save role selection history: {error:?}"),
        Err(error) => eprintln!("failed to join role selection history save task: {error:?}"),
    }
    let mut running_game = RunningGame {
        guild_id,
        channel_id,
        participant_user_ids,
        spectator_user_ids,
        reveal_death_roles: config_snapshot.reveal_death_roles,
        anonymous_enabled: config_snapshot.anonymous_mode,
        game,
        started_at: Instant::now(),
        started_at_iso: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        ended_at_iso: None,
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
        dead_role_chat_visible_from_days: HashMap::new(),
        anonymous_shaman_input_channel_ids: HashMap::new(),
        anonymous_shaman_input_channel_owners: HashMap::new(),
        anonymous_role_input_channel_ids: HashMap::new(),
        anonymous_role_input_channels: HashMap::new(),
        anonymous_role_input_status_message_ids: HashMap::new(),
        anonymous_role_status_texts: HashMap::new(),
        anonymous_webhooks: HashMap::new(),
        anonymous_webhook_creation_locks: HashMap::new(),
        channel_role_ids: None,
        source_category_id: None,
        permission_overwrite_cache: HashMap::new(),
        verified_member_ids: HashSet::new(),
        personal_channel_creation_locks: HashMap::new(),
        original_game_channel_overwrites: HashMap::new(),
        game_channel_overwrites: HashMap::new(),
        member_channel_overwrites: HashMap::new(),
        original_slowmode_delays: HashMap::new(),
        channel_slowmode_cache: HashMap::new(),
        private_channel_ids: HashMap::new(),
        private_role_status_message_ids: HashMap::new(),
        private_role_status_texts: HashMap::new(),
        memo_channel_ids: HashMap::new(),
        shaman_channel_id: None,
        shaman_status_message_id: None,
        shaman_status_text: None,
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
        // 중지된 판의 역할 배정을 밸런싱 이력에 남긴다. 안 남기면 다음 판
        // 팀 배정이 중지 전과 같은 이력을 보고 같은 팀을 거의 그대로 다시 뽑는다.
        let aborted_players = {
            let running_read = running.read().await;
            running_read
                .game
                .players
                .iter()
                .map(|player| {
                    (
                        player.user_id,
                        player.name.clone(),
                        running_read
                            .initial_roles
                            .get(&player.user_id)
                            .copied()
                            .unwrap_or(player.role),
                    )
                })
                .collect::<Vec<_>>()
        };
        let stats_snapshot = {
            let mut stats_file = ctx.data().stats.write().await;
            stats::record_aborted_assignments(&mut stats_file, aborted_players);
            stats_file.clone()
        };
        let stats_path = ctx.data().stats_path.clone();
        match tokio::task::spawn_blocking(move || stats::save_stats(&*stats_path, &stats_snapshot))
            .await
        {
            Ok(Ok(())) => {}
            Ok(Err(error)) => eprintln!("failed to save aborted assignments: {error:?}"),
            Err(error) => eprintln!("failed to join aborted assignment save task: {error:?}"),
        }
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
        ctx.channel_id(),
        !cleaned_running_game,
    )
    .await;
    reply_embed_with_channel_fallback(
        ctx,
        format!(
            "남아 있던 게임 채널, 역할, 권한, 슬로우모드를 정리했습니다.\n추가 삭제 채널: {}개\n삭제 실패 채널: {}개\n역할 제거: {}개\n메인 채널 권한 정리: {}개",
            summary.channels_deleted,
            summary.channel_delete_failures,
            summary.role_removals,
            summary.permissions_reset,
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
    let (author, target) = {
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
        (author, target)
    };

    let Some(roles) = running_channel_roles(ctx.serenity_context(), ctx.data(), &running).await
    else {
        return Err("failed to load game channel roles".into());
    };
    let category = running_source_category(ctx.serenity_context(), &running).await;
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
        stats::rating_rank(stats_file, entry.rating, entry.rating_games),
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
    rename = "랭크컷",
    description_localized("ko", "현재 랭크 커트라인을 확인합니다.")
)]
pub async fn show_rank_cutoffs(ctx: Context<'_>) -> Result<(), Error> {
    let stats_read = ctx.data().stats.read().await;
    let Some(cutoffs) = stats::rank_cutoffs(&stats_read) else {
        drop(stats_read);
        reply_embed(
            ctx,
            "아직 배치(레이팅 10판)를 마친 플레이어가 없어 랭크 커트라인이 없습니다.",
            "랭크 커트라인",
            serenity::Colour::GOLD,
            false,
        )
        .await?;
        return Ok(());
    };
    let pool_size = stats::ranked_pool_size(&stats_read);
    let my_line = {
        let user_id = ctx.author().id.get();
        stats_read.users.get(&user_id.to_string()).map(|entry| {
            format!(
                "

내 랭크: **{}** ({}점)",
                stats::rating_rank(&stats_read, entry.rating, entry.rating_games),
                entry.rating
            )
        })
    };
    drop(stats_read);
    let bands = [
        ("X", "상위 10%"),
        ("SS", "상위 25%"),
        ("S", "상위 45%"),
        ("A", "상위 70%"),
        ("B", "상위 90%"),
        ("C", "그 외"),
    ];
    let lines = cutoffs
        .iter()
        .map(|(rank, cutoff)| {
            let band = bands
                .iter()
                .find(|(band_rank, _)| band_rank == rank)
                .map(|(_, band)| *band)
                .unwrap_or("");
            if *rank == "C" {
                format!("**{rank}** ({band}) - 커트라인 없음")
            } else {
                format!("**{rank}** ({band}) - {cutoff}점 이상")
            }
        })
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    reply_embed(
        ctx,
        format!(
            "커트라인은 배치를 마친 플레이어 {pool_size}명의 현재 분포 기준이며, 판이 끝날 때마다 움직입니다.

{lines}{}",
            my_line.unwrap_or_default()
        ),
        "랭크 커트라인",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
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

#[cfg(test)]
mod tests;

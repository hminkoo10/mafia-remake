// 역할: 마피아 게임 루프(game_loop), 밤/낮/투표 단계 진행(run_night, run_day, run_vote),
//        역할 배분, 야간 행동 DM, 경찰 결과 공지

#![allow(unused_imports, clippy::too_many_arguments, clippy::collapsible_if)]

use super::{
    COMPLETED_REPLAY_LIMIT, CONFIRM_VOTE_SECONDS, Context, DAY_EXTENSION_VOTE_SECONDS,
    DISCUSSION_EXTENSION_SECONDS, Data, Error, PRIVATE_CHAT_ROLES, RunningGame,
};
use crate::channel::*;
use crate::commands::{draw_lb_text, fill_circle, fill_rect, image_color, truncate_for_board};
use crate::embed::*;
use ab_glyph::FontArc;
use anyhow::{Context as AnyhowContext, Result, bail};
use dashmap::{DashMap, mapref::entry::Entry};
use image::{ImageFormat, Rgb, RgbImage};
use mafia_remake::config;
use mafia_remake::game::{MafiaGame, majority_required};
use mafia_remake::model::{
    CONTRACTOR_GUESS_ROLES, ConfirmVoteResult, NightResult, Phase, Player, Role, VoteResult, Winner,
};
use mafia_remake::stats;
use poise::serenity_prelude as serenity;
use poise::serenity_prelude::Mentionable;
use rand::seq::{IndexedRandom, SliceRandom};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::io::Cursor;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Notify, RwLock};

pub async fn game_loop(
    ctx: serenity::Context,
    data: Data,
    running: Arc<RwLock<RunningGame>>,
) -> Result<()> {
    let result = game_loop_inner(&ctx, &data, &running).await;
    if let Err(error) = &result {
        eprintln!("game loop failed; forcing cleanup: {error:?}");
    }
    cleanup_game(&ctx, &data, &running).await;
    let guild_id = running.read().await.guild_id;
    remove_current_entry(&data.games, guild_id, &running);
    result
}

async fn game_loop_inner(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) -> Result<()> {
    let config = data.config.read().await.clone();
    setup_game_channels(ctx, data, running).await?;
    {
        let running_read = running.read().await;
        let game = &running_read.game;
        send_channel_embed(
            &ctx.http,
            running_read.channel_id,
            public_game_settings_text(game, &config, "게임 방 설정입니다."),
            "방 설정",
            serenity::Colour::GOLD,
            vec![],
        )
        .await?;
        send_channel_embed(
            &ctx.http,
            running_read.channel_id,
            game_rule_text(game, &config, running_read.reveal_death_roles),
            "게임 설명",
            serenity::Colour::GOLD,
            vec![],
        )
        .await?;
    }
    send_roles(ctx, running, &config).await;
    upsert_game_status(ctx, running).await;
    loop {
        {
            let running_read = running.read().await;
            if running_read.game.phase == Phase::Ended {
                break;
            }
        }
        run_night(ctx, data, running).await?;
        if running.read().await.game.phase == Phase::Ended {
            break;
        }
        if announce_winner(ctx, data, running).await? {
            break;
        }
        run_day(ctx, data, running).await?;
        if running.read().await.game.phase == Phase::Ended {
            break;
        }
        run_vote(ctx, data, running).await?;
        if running.read().await.game.phase == Phase::Ended {
            break;
        }
        if announce_winner(ctx, data, running).await? {
            break;
        }
    }
    Ok(())
}

fn remove_current_entry<K, T>(entries: &DashMap<K, Arc<T>>, key: K, current: &Arc<T>) -> bool
where
    K: Eq + Hash,
{
    match entries.entry(key) {
        Entry::Occupied(entry) if Arc::ptr_eq(entry.get(), current) => {
            entry.remove();
            true
        }
        Entry::Occupied(_) | Entry::Vacant(_) => false,
    }
}

pub async fn send_roles(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    config: &config::BotConfig,
) {
    let (channel_id, payloads) = {
        let running_read = running.read().await;
        let payloads = running_read
            .game
            .players
            .iter()
            .map(|player| {
                let anonymous_notice = if running_read.anonymous_enabled {
                    let alias = running_read
                        .anonymous_aliases
                        .get(&player.user_id)
                        .cloned()
                        .unwrap_or_else(|| "익명".to_string());
                    format!(
                        "\n\n익명 이름: **{alias}**\n채팅은 서버에 생성된 본인 익명 입력 채널에서만 진행하세요."
                    )
                } else {
                    String::new()
                };
                (
                    player.clone(),
                    format!(
                        "{}\n\n방 설정\n{}\n\n게임 설명\n{}\n\n본인 역할 설명은 `/마피아능력`, 전체 역할 설명은 `/역할설명`으로 다시 확인할 수 있습니다.{}",
                        role_message(&running_read.game, player),
                        public_game_settings_text(
                            &running_read.game,
                            config,
                            "현재 게임 설정입니다."
                        ),
                        game_rule_text(
                            &running_read.game,
                            config,
                            running_read.reveal_death_roles
                        ),
                        anonymous_notice
                    ),
                )
            })
            .collect::<Vec<_>>();
        (running_read.channel_id, payloads)
    };
    let mut failed_names = Vec::new();
    for (player, message) in payloads {
        if !send_player_secret(ctx, running, &player, message, vec![]).await {
            failed_names.push(player.name);
        }
    }
    if !failed_names.is_empty() {
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            format!(
                "비밀 메시지를 보낼 수 없는 참가자: {}",
                failed_names.join(", ")
            ),
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
        )
        .await;
    }
    let _ = send_channel_embed(
        &ctx.http,
        channel_id,
        "역할 배정이 끝났습니다. 각자 비밀 메시지와 역할별 비공개 채널을 확인하세요.",
        "역할 배정 완료",
        serenity::Colour::DARK_GREEN,
        vec![],
    )
    .await;
}

pub fn role_message(game: &MafiaGame, player: &Player) -> String {
    let team = if game.is_cult_team(player) {
        "교주팀"
    } else if game.is_mafia_team(player) {
        "마피아팀"
    } else if player.role == Role::Joker {
        "중립"
    } else {
        "시민팀"
    };
    let mut guide = role_short_guide(player.role).to_string();
    if player.role == Role::Mercenary
        && let Some(client) = game.mercenary_client(player.user_id)
    {
        guide.push_str(&format!("\n의뢰인: **{}**", client.name));
    }
    format!(
        "당신의 역할은 **{}** 입니다.\n진영: **{}**\n\n{}",
        player.role.value(),
        team,
        guide
    )
}

pub fn role_short_guide(role: Role) -> &'static str {
    match role {
        Role::Mafia => "밤마다 제거할 대상을 선택합니다.",
        Role::Doctor => "밤마다 보호할 대상을 선택합니다.",
        Role::Police => "밤마다 한 명을 조사합니다.",
        Role::Agent => "밤마다 시민팀 지령 정보를 받습니다.",
        Role::Vigilante => "낮에 조사하고 밤에 숙청할 수 있습니다.",
        Role::Inspector => {
            "밤에 한 명을 수사해 같은 팀이면 직업을 확인하고 대상에게 자신의 정체를 알립니다."
        }
        Role::Detective => "밤 행동의 이동 경로를 추적합니다.",
        Role::Shaman => "사망자를 성불하고 직업을 확인합니다.",
        Role::Priest => "사망자를 한 번 소생시킬 수 있습니다.",
        Role::Reporter => "두 번째 밤부터 특종으로 직업을 공개합니다.",
        Role::Hacker => "낮에 해킹해 직업을 확인하고 능력을 우회합니다.",
        Role::Terrorist => "지목한 위험 대상을 함께 데려갈 수 있습니다.",
        Role::Lover => "연인과 정보를 공유하고 서로를 지킵니다.",
        Role::Soldier => "마피아 공격을 한 번 버팁니다.",
        Role::Spy => "밤마다 직업을 확인하고 마피아와 접선합니다.",
        Role::Contractor => "두 명의 직업을 맞히면 암살합니다.",
        Role::Thief => "지목 투표한 대상의 능력을 훔칩니다.",
        Role::Witch => "밤에 대상을 개구리로 저주합니다.",
        Role::Scientist => "사망 후 다음 밤 부활합니다.",
        Role::Madam => "지목 투표로 선택한 대상을 유혹합니다.",
        Role::Godfather => "세 번째 밤부터 확정 처치합니다.",
        Role::CultLeader => "홀수날 밤마다 포교합니다.",
        Role::Fanatic => "교주팀 여부를 확인하고 교주를 찾습니다.",
        Role::Joker => "낮 처형으로 단독 승리합니다.",
        Role::Politician => "투표가 2표이며 처형 면역이 있습니다.",
        Role::Judge => "찬반투표 결과를 뒤집을 수 있습니다.",
        Role::Gangster => "밤에 한 명의 다음 낮 투표권을 빼앗습니다.",
        Role::Prophet => "4번째 낮까지 생존하면 소속팀이 승리합니다.",
        Role::Psychologist => "낮에 두 명이 같은 팀인지 봅니다.",
        Role::Hypnotist => "밤마다 최면을 누적하고 낮에 한꺼번에 깨워 비시민 직업을 확인합니다.",
        Role::Mercenary => "의뢰인이 밤에 사망한 뒤 밤마다 한 명을 처형할 수 있습니다.",
        Role::Graverobber => "첫날 사망자의 직업을 이어받습니다.",
        _ => "낮 토론과 투표로 승리를 노리세요.",
    }
}

pub fn death_role_text(running: &RunningGame, player: &Player) -> String {
    if running.reveal_death_roles {
        format!("직업은 **{}** 입니다.", player.role.value())
    } else {
        "직업은 공개되지 않습니다.".to_string()
    }
}

pub async fn trigger_timed_night_events(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) -> Result<()> {
    let (guild_id, cursed_players, witch_contacts, cult_bells, revived_players) = {
        let mut running_write = running.write().await;
        if running_write.game.phase != Phase::Night {
            return Ok(());
        }
        let (cursed_players, witch_contacts) =
            running_write.game.apply_witch_curses(&HashSet::new());
        let cult_bells = running_write.game.consume_cult_bells();
        let revived_players = running_write.game.revive_pending_scientists();
        (
            running_write.guild_id,
            cursed_players,
            witch_contacts,
            cult_bells,
            revived_players,
        )
    };

    if cursed_players.is_empty()
        && witch_contacts.is_empty()
        && cult_bells == 0
        && revived_players.is_empty()
    {
        return Ok(());
    }

    for player in &cursed_players {
        set_frog_channel_member_access(ctx, running, player, true, true).await;
        set_frog_game_channel_permission(ctx, running, player, false).await;
        disable_private_role_channels_for_player(ctx, running, player).await;
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
    if !cursed_players.is_empty() {
        send_game_embed(
            ctx,
            running,
            "마녀의 저주가 발동했습니다.\n누군가 다음 밤까지 개구리가 되었습니다.",
            "마녀 저주",
            serenity::Colour::ORANGE,
            vec![],
            false,
            true,
        )
        .await?;
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
    Ok(())
}

pub async fn run_night(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) -> Result<()> {
    let (
        actors,
        restored_frogs,
        hacker_results,
        vigilante_results,
        godfather_contacts,
        seconds,
        notify,
    ) = {
        let config = data.config.read().await.clone();
        let mut running_write = running.write().await;
        running_write.game.phase = Phase::Night;
        running_write.phase_deadline =
            Some(Instant::now() + Duration::from_secs(config.night_seconds));
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
            running_write.night_notify.clone(),
        )
    };
    upsert_game_status(ctx, running).await;
    set_game_channel_chat(ctx, data, running, false).await;
    sync_private_role_chat_permissions(ctx, data, running).await;
    sync_lover_chat_access(ctx, data, running).await;
    sync_cult_team_channel_access(ctx, data, running).await;
    sync_scientist_mafia_permissions(ctx, data, running).await;
    sync_madam_seduction_permissions(ctx, running).await;
    sync_shaman_chat_access(ctx, data, running).await;
    for player in &restored_frogs {
        set_frog_channel_member_access(ctx, running, player, false, false).await;
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
    let mut failed_names = Vec::new();
    for actor in actors {
        if !send_night_action_dm(ctx, running, &actor).await {
            failed_names.push(actor.name);
        }
    }
    if !failed_names.is_empty() {
        send_game_embed(
            ctx,
            running,
            format!(
                "밤 행동 선택지를 보낼 수 없는 참가자: {}",
                failed_names.join(", ")
            ),
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
            false,
            true,
        )
        .await?;
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
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(seconds)) => {}
            _ = notify.notified() => {}
        }
    } else {
        let reached_ten_seconds = tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(seconds - 10)) => true,
            _ = notify.notified() => false,
        };
        if running.read().await.game.phase == Phase::Ended {
            return Ok(());
        }
        {
            let mut running_write = running.write().await;
            running_write.night_timed_events_due = true;
        }
        if reached_ten_seconds {
            send_game_embed(
                ctx,
                running,
                "밤 시간이 10초 남았습니다. 아직 행동하지 않았다면 지금 선택하세요.",
                "밤 10초 전",
                serenity::Colour::GOLD,
                vec![],
                false,
                true,
            )
            .await?;
            trigger_timed_night_events(ctx, data, running).await?;
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(10)) => {}
                _ = notify.notified() => {}
            }
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
    let result = {
        let mut running_write = running.write().await;
        running_write.game.resolve_night()?
    };
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
        if doctor_saved {
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
        .chain(&result.witch_contacts)
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
) -> bool {
    let (guild_id, role, can_change, targets) = {
        let running_read = running.read().await;
        let role = effective_night_role(&running_read.game, actor);
        let targets = if role == Role::Contractor {
            running_read.game.contractor_contract_targets(actor)
        } else {
            night_targets(&running_read.game, actor)
        };
        (
            running_read.guild_id,
            role,
            running_read.game.night_action_can_be_changed(actor),
            targets,
        )
    };
    if targets.is_empty() && role != Role::Reporter {
        return true;
    };
    if role == Role::Contractor {
        return send_player_secret(
            ctx,
            running,
            actor,
            "청부업자 밤 행동을 선택하세요.\n두 명과 각 직업을 추측합니다. 둘 중 한 명이라도 마피아를 정확히 맞히면 접선합니다.\n밤이 끝나기 전 다시 제출하면 청부 대상을 변경할 수 있습니다.\n첫날 밤에는 사용할 수 없고, 수사직과 직업이 공개된 사람은 대상에서 제외됩니다.",
            contractor_contract_components(guild_id, actor.user_id, &targets),
        )
        .await;
    }
    let prompt = if can_change {
        format!(
            "{} 밤 행동을 선택하세요\n밤이 끝나기 전 다시 선택하면 대상을 변경할 수 있습니다.",
            role.value()
        )
    } else {
        format!("{} 밤 행동을 선택하세요", role.value())
    };
    send_player_secret(
        ctx,
        running,
        actor,
        prompt,
        night_action_components(guild_id, actor.user_id, role, &targets),
    )
    .await
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

pub fn contractor_contract_components(
    guild_id: serenity::GuildId,
    actor_id: u64,
    targets: &[Player],
) -> Vec<serenity::CreateActionRow> {
    (0..2)
        .map(|slot| {
            let target_options = targets
                .iter()
                .take(25)
                .map(|target| {
                    serenity::CreateSelectMenuOption::new(
                        target.name.chars().take(100).collect::<String>(),
                        target.user_id.to_string(),
                    )
                })
                .collect::<Vec<_>>();
            serenity::CreateActionRow::SelectMenu(
                serenity::CreateSelectMenu::new(
                    format!("contractor_target:{}:{}:{}", guild_id.get(), actor_id, slot),
                    serenity::CreateSelectMenuKind::String {
                        options: target_options,
                    },
                )
                .placeholder(format!("{}번째 청부 대상", slot + 1))
                .min_values(1)
                .max_values(1),
            )
        })
        .chain([serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new(format!(
                "contractor_roles:{}:{}",
                guild_id.get(),
                actor_id
            ))
            .label("직업 입력/청부 확정")
            .style(serenity::ButtonStyle::Danger),
        ])])
        .collect()
}

pub fn contractor_role_modal(guild_id: serenity::GuildId, actor_id: u64) -> serenity::CreateModal {
    serenity::CreateModal::new(
        format!("contractor_roles:{}:{}", guild_id.get(), actor_id),
        "청부 직업 입력",
    )
    .components(vec![
        serenity::CreateActionRow::InputText(
            serenity::CreateInputText::new(
                serenity::InputTextStyle::Short,
                "첫 번째 대상 직업",
                "first_role",
            )
            .placeholder("예: 마피아")
            .min_length(1)
            .max_length(30),
        ),
        serenity::CreateActionRow::InputText(
            serenity::CreateInputText::new(
                serenity::InputTextStyle::Short,
                "두 번째 대상 직업",
                "second_role",
            )
            .placeholder("예: 시민")
            .min_length(1)
            .max_length(30),
        ),
    ])
}

pub fn night_placeholder(role: Role) -> &'static str {
    match role {
        Role::Mafia => "공격할 대상을 선택하세요",
        Role::Doctor => "보호할 대상을 선택하세요",
        Role::Nurse => "처방/치료 대상을 선택하세요",
        Role::Police => "조사할 대상을 선택하세요",
        Role::Inspector => "수사할 대상을 선택하세요",
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
        result.inspector_results.clone(),
        result.inspector_target_notices.clone(),
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
            "경찰 조사 대상이 과반에 도달하지 못해 이번 밤 조사 결과가 없습니다.".to_string()
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
            "경찰 조사는 성공하지 못했습니다. 대상이 과반에 도달하지 못했거나 선택이 완료되지 않았습니다.",
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
            format!("[의뢰] 의뢰인은 **{}** 님입니다.", client.name),
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
    let (guild_id, vote_notify, seconds, alive) = {
        let mut running_write = running.write().await;
        running_write.game.start_vote()?;
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
        running_write.record_replay_event(
            "nomination_vote_resolved",
            None,
            &target_ids,
            serde_json::json!({
                "executed_user_id": result.executed.as_ref().map(|player| player.user_id),
                "tied": result.tied,
                "skipped": result.skipped,
                "vote_counts": vote_counts,
                "madam_seduced_user_ids": result.madam_seduced.iter().map(|player| player.user_id).collect::<Vec<_>>(),
                "madam_newly_contacted_user_ids": result.madam_newly_contacted.iter().map(|player| player.user_id).collect::<Vec<_>>(),
                "blocked_voter_user_ids": result.blocked_voters.iter().map(|player| player.user_id).collect::<Vec<_>>(),
            }),
        );
        result
    };
    handle_madam_seduction_result(ctx, data, running, &vote_result).await;
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
    {
        let mut running_write = running.write().await;
        running_write.final_defense_user_id = Some(nominee.user_id);
        running_write.phase_deadline = Some(Instant::now() + Duration::from_secs(20));
    }
    sync_anonymous_general_chat_permissions(ctx, running).await;
    set_channel_slowmode(ctx, running, 0).await;
    if !running.read().await.game.is_frog(&nominee)
        && !running.read().await.game.is_madam_seduced(&nominee)
    {
        set_member_game_channel_chat(ctx, running, &nominee, true).await;
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
        running_write.record_replay_event(
            "confirmation_vote_resolved",
            None,
            &target_ids,
            serde_json::json!({
                "nominee_user_id": nominee.user_id,
                "executed_user_id": result.executed.as_ref().map(|player| player.user_id),
                "extra_killed_user_ids": result.extra_killed.iter().map(|player| player.user_id).collect::<Vec<_>>(),
                "approved": result.approved,
                "tied": result.tied,
                "blocked_by_politician": result.blocked_by_politician,
                "vote_counts": vote_counts,
                "judge_user_id": result.judge.as_ref().map(|player| player.user_id),
                "judge_choice": result.judge_choice,
                "decided_by_judge": result.decided_by_judge,
            }),
        );
        result
    };
    set_channel_slowmode(ctx, running, config.chat_slowmode_seconds).await;
    let summary = confirmation_vote_summary(&confirm_result, confirm_context);
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
    let (message, color, include_dead) = if confirm_result.blocked_by_politician {
        (
            format!(
                "찬반투표 결과, {} 님은 **정치인** 입니다.\n[정치인은 투표로 죽지 않습니다]\n\n{} 님은 처형되지 않고 밤으로 넘어갑니다.{judge_notice}\n\n찬반투표 집계\n{summary}",
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
                result_message.push_str(
                    "\n테러리스트의 [산화]가 발동해 지목 중이던 적 팀도 함께 사망했습니다.",
                );
            } else {
                result_message.push_str(
                    "\n처형 대상이 지목하고 있던 시민팀이 아닌 대상도 함께 사망했습니다.",
                );
            }
        }
        (
            format!(
                "{result_message}\n\n사망자\n{killed_lines}{judge_notice}\n\n찬반투표 집계\n{summary}"
            ),
            serenity::Colour::GOLD,
            true,
        )
    } else if confirm_result.tied {
        (
            format!(
                "찬반투표가 동률이라 처형하지 않습니다.{judge_notice}\n\n찬반투표 집계\n{summary}"
            ),
            serenity::Colour::GOLD,
            false,
        )
    } else {
        let reject_message = confirmation_rejection_message(&confirm_result, confirm_context);
        (
            format!("{reject_message}{judge_notice}\n\n찬반투표 집계\n{summary}"),
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

#[derive(Debug, Clone, Copy)]
struct ConfirmationVoteContext {
    eligible_voters: usize,
    submitted_voters: usize,
}

fn confirmation_vote_context(game: &MafiaGame) -> ConfirmationVoteContext {
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

fn confirmation_vote_summary(
    confirm_result: &ConfirmVoteResult,
    context: ConfirmationVoteContext,
) -> String {
    let yes = confirm_result.vote_counts.get(&true).copied().unwrap_or(0);
    let no = confirm_result.vote_counts.get(&false).copied().unwrap_or(0);
    let submitted_vote_count = yes + no;
    let required_yes = confirmation_required_yes(confirm_result);
    let abstained = context
        .eligible_voters
        .saturating_sub(context.submitted_voters);
    format!(
        "찬성 {yes}표 / 반대 {no}표 / 미투표 {abstained}명\n처형 기준: 찬성 {required_yes}표 이상 (투표수 {submitted_vote_count}표 기준)"
    )
}

fn confirmation_required_yes(confirm_result: &ConfirmVoteResult) -> i32 {
    let yes = confirm_result.vote_counts.get(&true).copied().unwrap_or(0);
    let no = confirm_result.vote_counts.get(&false).copied().unwrap_or(0);
    let submitted_vote_count = yes + no;
    if submitted_vote_count <= 0 {
        1
    } else {
        submitted_vote_count / 2 + 1
    }
}

fn confirmation_rejection_message(
    confirm_result: &ConfirmVoteResult,
    _context: ConfirmationVoteContext,
) -> String {
    if confirm_result.decided_by_judge {
        return "판사의 선택으로 처형하지 않습니다.".to_string();
    }
    let yes = confirm_result.vote_counts.get(&true).copied().unwrap_or(0);
    let no = confirm_result.vote_counts.get(&false).copied().unwrap_or(0);
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

#[derive(Clone, Debug)]
pub struct GameResultImageRow {
    name: String,
    role: String,
    team: String,
    alive: bool,
    before: Option<i64>,
    after: Option<i64>,
    delta: Option<i64>,
    team_delta: Option<i64>,
    role_delta: Option<i64>,
    streak_delta: Option<i64>,
    reasons: Vec<String>,
}

pub fn winner_result_text(winner: Winner) -> &'static str {
    match winner {
        Winner::Mafia => "마피아 승리!",
        Winner::Joker => "조커 승리!",
        Winner::Cult => "교주팀 승리!",
        Winner::Citizen => "시민 승리!",
    }
}

pub fn game_result_display_name(running: &RunningGame, player: &Player) -> String {
    if running.anonymous_enabled {
        let alias = running
            .anonymous_aliases
            .get(&player.user_id)
            .map(String::as_str)
            .unwrap_or("익명");
        let real_name = running
            .anonymous_original_names
            .get(&player.user_id)
            .map(String::as_str)
            .unwrap_or(&player.name);
        format!("{alias} = {real_name}")
    } else {
        player.name.clone()
    }
}

pub fn game_result_rows(
    running: &RunningGame,
    rating_log: &[stats::GameRatingLogItem],
) -> Vec<GameResultImageRow> {
    let rating_by_id = rating_log
        .iter()
        .map(|item| (item.user_id, item))
        .collect::<HashMap<_, _>>();
    let mut players = running.game.players.clone();
    players.sort_by_key(|player| game_result_display_name(running, player).to_lowercase());
    players
        .iter()
        .map(|player| {
            let initial_role = running
                .initial_roles
                .get(&player.user_id)
                .copied()
                .unwrap_or(player.role);
            let role = if initial_role == player.role {
                player.role.value().to_string()
            } else {
                format!("{} -> {}", initial_role.value(), player.role.value())
            };
            let rating = rating_by_id.get(&player.user_id).copied();
            GameResultImageRow {
                name: game_result_display_name(running, player),
                role,
                team: final_team_text(&running.game, player).to_string(),
                alive: player.alive,
                before: rating.map(|item| item.before),
                after: rating.map(|item| item.after),
                delta: rating.map(|item| item.delta),
                team_delta: rating.map(|item| item.team_delta),
                role_delta: rating.map(|item| item.role_delta),
                streak_delta: rating.map(|item| item.streak_delta),
                reasons: rating.map_or_else(Vec::new, |item| item.reasons.clone()),
            }
        })
        .collect()
}

pub fn render_game_result_image(
    winner: Winner,
    elapsed_seconds: i64,
    rows: Vec<GameResultImageRow>,
) -> Option<Vec<u8>> {
    const WIDTH: u32 = 1420;
    const TOP: i32 = 44;
    const SIDE: i32 = 46;
    const HEADER_HEIGHT: i32 = 172;
    const ROW_HEIGHT: i32 = 112;
    const FOOTER: i32 = 56;

    let table_top = TOP + HEADER_HEIGHT + 26;
    let height = (table_top + ROW_HEIGHT * rows.len() as i32 + FOOTER).max(520) as u32;
    let mut image = RgbImage::from_pixel(WIDTH, height, image_color("#edf2f7"));
    let font = FontArc::try_from_slice(include_bytes!("../MalangmalangR.ttf")).ok()?;
    let text = image_color("#172033");
    let muted = image_color("#64748b");
    let soft = image_color("#f8fafc");
    let white = image_color("#ffffff");
    let line = image_color("#d9e2ef");
    let accent = winner_color(winner);

    fill_rect(&mut image, 0, 0, WIDTH, 18, accent);
    fill_rect(&mut image, SIDE, TOP, WIDTH - SIDE as u32 * 2, 150, white);
    fill_rect(&mut image, SIDE, TOP, 10, 150, accent);
    draw_lb_text(
        &mut image,
        &font,
        48.0,
        SIDE + 30,
        TOP + 24,
        winner_result_text(winner),
        text,
    );
    draw_lb_text(
        &mut image,
        &font,
        25.0,
        SIDE + 34,
        TOP + 88,
        format!(
            "플레이 시간 {} · 참가자 {}명 · 최종 역할 / 랭크 / 레이팅 정리",
            stats::play_duration_text(elapsed_seconds),
            rows.len()
        ),
        muted,
    );
    let badge_x = WIDTH as i32 - SIDE - 252;
    fill_rect(&mut image, badge_x, TOP + 44, 220, 54, accent);
    draw_lb_text(
        &mut image,
        &font,
        28.0,
        badge_x + 32,
        TOP + 58,
        winner.value(),
        image_color("#ffffff"),
    );

    fill_rect(
        &mut image,
        SIDE,
        table_top - 52,
        WIDTH - SIDE as u32 * 2,
        52,
        image_color("#1f2937"),
    );
    for (x, label) in [
        (SIDE + 38, "플레이어"),
        (SIDE + 372, "최종 역할"),
        (SIDE + 640, "레이팅"),
        (SIDE + 910, "변동"),
        (SIDE + 1088, "랭크/사유"),
    ] {
        draw_lb_text(
            &mut image,
            &font,
            23.0,
            x,
            table_top - 38,
            label,
            image_color("#f8fafc"),
        );
    }

    for (index, row) in rows.iter().enumerate() {
        let y = table_top + index as i32 * ROW_HEIGHT;
        let row_fill = if index % 2 == 0 { white } else { soft };
        fill_rect(
            &mut image,
            SIDE,
            y,
            WIDTH - SIDE as u32 * 2,
            ROW_HEIGHT as u32,
            row_fill,
        );
        fill_rect(
            &mut image,
            SIDE,
            y + ROW_HEIGHT - 1,
            WIDTH - SIDE as u32 * 2,
            1,
            line,
        );
        fill_rect(
            &mut image,
            SIDE,
            y,
            8,
            ROW_HEIGHT as u32,
            team_color(&row.team),
        );
        fill_circle(
            &mut image,
            (SIDE + 32, y + 46),
            16,
            if row.alive {
                image_color("#22c55e")
            } else {
                image_color("#ef4444")
            },
        );
        draw_lb_text(
            &mut image,
            &font,
            28.0,
            SIDE + 62,
            y + 18,
            truncate_for_board(&row.name, 18),
            text,
        );
        draw_lb_text(
            &mut image,
            &font,
            20.0,
            SIDE + 64,
            y + 58,
            if row.alive { "생존" } else { "사망" },
            muted,
        );
        draw_lb_text(
            &mut image,
            &font,
            26.0,
            SIDE + 372,
            y + 20,
            truncate_for_board(&row.role, 15),
            text,
        );
        draw_lb_text(
            &mut image,
            &font,
            20.0,
            SIDE + 374,
            y + 58,
            &row.team,
            team_color(&row.team),
        );
        draw_rating_block(&mut image, &font, row, SIDE + 640, y, text, muted);
        draw_delta_badge(&mut image, &font, row, SIDE + 910, y);
        draw_rank_and_reason(&mut image, &font, row, SIDE + 1088, y, text, muted);
    }

    draw_lb_text(
        &mut image,
        &font,
        19.0,
        SIDE,
        height as i32 - 34,
        "마피아 게임 진행 메시지",
        muted,
    );
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .ok()?;
    Some(bytes.into_inner())
}

fn draw_rating_block(
    image: &mut RgbImage,
    font: &FontArc,
    row: &GameResultImageRow,
    x: i32,
    y: i32,
    text: Rgb<u8>,
    muted: Rgb<u8>,
) {
    if let (Some(before), Some(after)) = (row.before, row.after) {
        draw_lb_text(
            image,
            font,
            25.0,
            x,
            y + 20,
            format!("{before} -> {after}"),
            text,
        );
        draw_lb_text(
            image,
            font,
            20.0,
            x,
            y + 58,
            format!(
                "{} -> {}",
                stats::rating_rank(before),
                stats::rating_rank(after)
            ),
            muted,
        );
    } else {
        draw_lb_text(image, font, 24.0, x, y + 34, "기록 없음", muted);
    }
}

fn draw_delta_badge(
    image: &mut RgbImage,
    font: &FontArc,
    row: &GameResultImageRow,
    x: i32,
    y: i32,
) {
    let Some(delta) = row.delta else {
        draw_lb_text(image, font, 23.0, x, y + 34, "-", image_color("#94a3b8"));
        return;
    };
    let fill = if delta > 0 {
        image_color("#dcfce7")
    } else if delta < 0 {
        image_color("#fee2e2")
    } else {
        image_color("#e2e8f0")
    };
    let color = if delta > 0 {
        image_color("#15803d")
    } else if delta < 0 {
        image_color("#b91c1c")
    } else {
        image_color("#475569")
    };
    fill_rect(image, x, y + 22, 128, 42, fill);
    draw_lb_text(
        image,
        font,
        25.0,
        x + 18,
        y + 30,
        format!("{delta:+}"),
        color,
    );
    let detail = format!(
        "팀 {:+} / 직업 {:+} / 연승 {:+}",
        row.team_delta.unwrap_or(0),
        row.role_delta.unwrap_or(0),
        row.streak_delta.unwrap_or(0)
    );
    draw_lb_text(
        image,
        font,
        18.0,
        x,
        y + 70,
        truncate_for_board(&detail, 18),
        image_color("#64748b"),
    );
}

fn draw_rank_and_reason(
    image: &mut RgbImage,
    font: &FontArc,
    row: &GameResultImageRow,
    x: i32,
    y: i32,
    text: Rgb<u8>,
    muted: Rgb<u8>,
) {
    if let (Some(before), Some(after)) = (row.before, row.after) {
        let before_rank = stats::rating_rank(before);
        let after_rank = stats::rating_rank(after);
        let rank_text = if before_rank == after_rank {
            format!("{after_rank} 랭크 유지")
        } else if after > before {
            format!("승급 {before_rank} -> {after_rank}")
        } else {
            format!("강등 {before_rank} -> {after_rank}")
        };
        draw_lb_text(image, font, 24.0, x, y + 18, rank_text, text);
    } else {
        draw_lb_text(image, font, 24.0, x, y + 18, "랭크 기록 없음", muted);
    }
    let reason = if row.reasons.is_empty() {
        "사유 없음".to_string()
    } else {
        row.reasons.join(", ")
    };
    draw_lb_text(
        image,
        font,
        18.0,
        x,
        y + 56,
        truncate_for_board(&reason, 34),
        muted,
    );
}

fn winner_color(winner: Winner) -> Rgb<u8> {
    match winner {
        Winner::Mafia => image_color("#dc2626"),
        Winner::Joker => image_color("#7c3aed"),
        Winner::Cult => image_color("#0891b2"),
        Winner::Citizen => image_color("#16a34a"),
    }
}

fn team_color(team: &str) -> Rgb<u8> {
    match team {
        "마피아팀" => image_color("#dc2626"),
        "교주팀" => image_color("#0891b2"),
        "중립" => image_color("#7c3aed"),
        _ => image_color("#16a34a"),
    }
}

pub async fn send_game_result_image(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    image: Vec<u8>,
) -> serenity::Result<serenity::Message> {
    const FILENAME: &str = "mafia_game_result.png";
    let (channel_id, anonymous_enabled, targets) = {
        let running_read = running.read().await;
        let targets = if running_read.anonymous_enabled {
            running_read
                .game
                .players
                .iter()
                .filter_map(|player| {
                    running_read
                        .anonymous_input_channel_ids
                        .get(&player.user_id)
                        .copied()
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        (
            running_read.channel_id,
            running_read.anonymous_enabled,
            targets,
        )
    };
    let embed = make_embed(
        "게임 종료 결과를 이미지로 정리했습니다.",
        "게임 종료",
        serenity::Colour::DARK_GREEN,
    )
    .attachment(FILENAME);
    let sent = channel_id
        .send_message(
            &ctx.http,
            serenity::CreateMessage::new()
                .embed(embed.clone())
                .add_file(serenity::CreateAttachment::bytes(image.clone(), FILENAME)),
        )
        .await?;
    if anonymous_enabled {
        for target in targets {
            let _ = target
                .send_message(
                    &ctx.http,
                    serenity::CreateMessage::new()
                        .embed(embed.clone())
                        .add_file(serenity::CreateAttachment::bytes(image.clone(), FILENAME)),
                )
                .await;
        }
    }
    Ok(sent)
}

pub async fn announce_winner(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) -> Result<bool> {
    let winner = running.read().await.game.winner();
    let Some(winner) = winner else {
        return Ok(false);
    };
    let (roles_text, elapsed_seconds, record_payload) = {
        let mut running_write = running.write().await;
        running_write.game.phase = Phase::Ended;
        let elapsed_seconds = running_write.started_at.elapsed().as_secs() as i64;
        let record_payload = if running_write.stats_recorded {
            None
        } else {
            running_write.stats_recorded = true;
            running_write.record_replay_event(
                "game_ended",
                None,
                &[],
                serde_json::json!({
                    "winner": winner.value(),
                    "winner_key": format!("{:?}", winner),
                    "elapsed_seconds": elapsed_seconds,
                }),
            );
            Some((
                running_write.game.clone(),
                running_write.initial_roles.clone(),
                elapsed_seconds,
            ))
        };
        (
            final_role_reveal_text(&running_write),
            elapsed_seconds,
            record_payload,
        )
    };
    upsert_game_status(ctx, running).await;
    let mut rating_log = Vec::new();
    let mut rating_log_chunks = Vec::new();
    let mut rank_change_chunks = Vec::new();
    if let Some((game_snapshot, initial_roles, elapsed_seconds)) = record_payload {
        let (recorded_rating_log, stats_snapshot) = {
            let mut stats_file = data.stats.write().await;
            let rating_log = stats::record_game_stats(
                &mut stats_file,
                &game_snapshot,
                &initial_roles,
                elapsed_seconds,
                winner,
            );
            (rating_log, stats_file.clone())
        };
        rating_log_chunks = stats::game_rating_log_chunks(&recorded_rating_log, 3500);
        rank_change_chunks = stats::game_rank_change_chunks(&recorded_rating_log, 3500);
        rating_log = recorded_rating_log;
        let stats_path = data.stats_path.clone();
        match tokio::task::spawn_blocking(move || stats::save_stats(&*stats_path, &stats_snapshot))
            .await
        {
            Ok(Ok(())) => {}
            Ok(Err(error)) => eprintln!("failed to save stats after game end: {error:?}"),
            Err(error) => eprintln!("failed to join stats save task after game end: {error:?}"),
        }
    }
    let completed_replay = {
        let running_read = running.read().await;
        running_read.replay_snapshot("completed", Some(winner), &rating_log)
    };
    {
        let mut completed_replays = data.completed_replays.write().await;
        let game_key = completed_replay["game_key"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        if let Some(index) = completed_replays
            .iter()
            .position(|replay| replay["game_key"].as_str() == Some(game_key.as_str()))
        {
            completed_replays.remove(index);
        }
        completed_replays.push_front(completed_replay);
        while completed_replays.len() > COMPLETED_REPLAY_LIMIT {
            completed_replays.pop_back();
        }
    }
    let rows = {
        let running_read = running.read().await;
        game_result_rows(&running_read, &rating_log)
    };
    match tokio::task::spawn_blocking(move || {
        render_game_result_image(winner, elapsed_seconds, rows)
    })
    .await
    {
        Ok(Some(image)) => match send_game_result_image(ctx, running, image).await {
            Ok(_) => return Ok(true),
            Err(error) => eprintln!("failed to announce game result image: {error:?}"),
        },
        Ok(None) => eprintln!("failed to render game result image"),
        Err(error) => eprintln!("failed to join game result image task: {error:?}"),
    }
    if let Err(error) = send_game_embed(
        ctx,
        running,
        format!(
            "{}\n플레이 시간: **{}**\n\n최종 역할 공개\n{}",
            winner_result_text(winner),
            stats::play_duration_text(elapsed_seconds),
            roles_text
        ),
        "게임 종료",
        serenity::Colour::DARK_GREEN,
        vec![],
        true,
        true,
    )
    .await
    {
        eprintln!("failed to announce game winner: {error:?}");
    }
    for (index, chunk) in rank_change_chunks.into_iter().enumerate() {
        let title = if index == 0 {
            "이번 판 랭크 변동".to_string()
        } else {
            format!("이번 판 랭크 변동 {}", index + 1)
        };
        if let Err(error) = send_game_embed(
            ctx,
            running,
            chunk,
            &title,
            serenity::Colour::GOLD,
            vec![],
            false,
            true,
        )
        .await
        {
            eprintln!("failed to announce rank changes: {error:?}");
        }
    }
    for (index, chunk) in rating_log_chunks.into_iter().enumerate() {
        let title = if index == 0 {
            "이번 판 레이팅 로그".to_string()
        } else {
            format!("이번 판 레이팅 로그 {}", index + 1)
        };
        if let Err(error) = send_game_embed(
            ctx,
            running,
            chunk,
            &title,
            serenity::Colour::BLUE,
            vec![],
            false,
            true,
        )
        .await
        {
            eprintln!("failed to announce rating log: {error:?}");
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_game_loop_cannot_remove_new_game_entry() {
        let games = DashMap::new();
        let stale = Arc::new(());
        let current = Arc::new(());
        games.insert(1_u64, current.clone());

        assert!(!remove_current_entry(&games, 1, &stale));
        assert!(Arc::ptr_eq(games.get(&1).unwrap().value(), &current));
        assert!(remove_current_entry(&games, 1, &current));
        assert!(games.is_empty());
    }

    #[test]
    fn confirmation_summary_uses_submitted_vote_threshold() {
        let result = ConfirmVoteResult {
            vote_counts: HashMap::from([(true, 3), (false, 2)]),
            ..Default::default()
        };
        let context = ConfirmationVoteContext {
            eligible_voters: 7,
            submitted_voters: 5,
        };
        assert_eq!(
            confirmation_vote_summary(&result, context),
            "찬성 3표 / 반대 2표 / 미투표 2명\n처형 기준: 찬성 3표 이상 (투표수 5표 기준)"
        );
    }

    #[test]
    fn confirmation_summary_requires_strict_majority_for_even_votes() {
        let result = ConfirmVoteResult {
            vote_counts: HashMap::from([(true, 4), (false, 4)]),
            ..Default::default()
        };
        let context = ConfirmationVoteContext {
            eligible_voters: 9,
            submitted_voters: 8,
        };
        assert_eq!(
            confirmation_vote_summary(&result, context),
            "찬성 4표 / 반대 4표 / 미투표 1명\n처형 기준: 찬성 5표 이상 (투표수 8표 기준)"
        );
        assert_eq!(
            confirmation_rejection_message(&result, context),
            "찬성과 반대가 같아 처형하지 않습니다."
        );
    }

    #[test]
    fn confirmation_rejection_message_reports_no_leading() {
        let result = ConfirmVoteResult {
            vote_counts: HashMap::from([(true, 2), (false, 3)]),
            ..Default::default()
        };
        let context = ConfirmationVoteContext {
            eligible_voters: 5,
            submitted_voters: 5,
        };

        assert_eq!(
            confirmation_rejection_message(&result, context),
            "반대가 많아 처형하지 않습니다."
        );
    }

    #[test]
    fn contractor_components_stay_within_discord_limits() {
        let targets = (0..30)
            .map(|index| Player::new(1000 + index, format!("대상{index}"), Role::Citizen))
            .collect::<Vec<_>>();
        let components = contractor_contract_components(serenity::GuildId::new(1), 42, &targets);
        let json = serde_json::to_value(&components).unwrap();
        let rows = json.as_array().unwrap();

        assert!(rows.len() <= 5);
        for row in rows {
            for component in row["components"].as_array().unwrap() {
                if let Some(options) = component.get("options").and_then(|value| value.as_array()) {
                    assert!(options.len() <= 25);
                }
            }
        }
    }

    #[test]
    fn game_result_image_renders_png() {
        let rows = vec![
            GameResultImageRow {
                name: "Alpha".to_string(),
                role: Role::Mafia.value().to_string(),
                team: "마피아팀".to_string(),
                alive: true,
                before: Some(1000),
                after: Some(1032),
                delta: Some(32),
                team_delta: Some(24),
                role_delta: Some(4),
                streak_delta: Some(4),
                reasons: vec!["소속 진영 승리".to_string()],
            },
            GameResultImageRow {
                name: "Beta".to_string(),
                role: Role::Doctor.value().to_string(),
                team: "시민팀".to_string(),
                alive: false,
                before: Some(1000),
                after: Some(982),
                delta: Some(-18),
                team_delta: Some(-20),
                role_delta: Some(2),
                streak_delta: Some(0),
                reasons: vec!["패배".to_string()],
            },
        ];

        let image = render_game_result_image(Winner::Mafia, 310, rows).unwrap();

        assert!(image.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(image.len() > 1024);
    }
}

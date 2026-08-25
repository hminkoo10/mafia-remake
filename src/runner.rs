// 역할: 마피아 게임 루프(game_loop), 밤/낮/투표 단계 진행(run_night, run_day, run_vote),
//        역할 배분, 야간 행동 DM, 경찰 결과 공지

#![allow(unused_imports, clippy::too_many_arguments, clippy::collapsible_if)]

use super::{
    COMPLETED_REPLAY_LIMIT, CONFIRM_VOTE_SECONDS, Context, ContractorContractDraft,
    DAY_EXTENSION_VOTE_SECONDS, DISCUSSION_EXTENSION_SECONDS, Data, Error, PRIVATE_CHAT_ROLES,
    RunningGame,
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
    ConfirmVoteResult, ContractorGuessRoleGroup, NightResult, Phase, Player, Role, VoteResult,
    Winner, contractor_guessable_roles_for_group,
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
    let (guild_id, channel_id, payloads) = {
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
        (running_read.guild_id, running_read.channel_id, payloads)
    };
    let mut failures = Vec::new();
    for (player, message) in payloads {
        if let Err(error) =
            send_player_secret_detailed(ctx, running, &player, message, vec![]).await
        {
            eprintln!(
                "secret delivery failed: stage=role_assignment guild_id={} channel_id={} user_id={} player={:?} {}",
                guild_id.get(),
                channel_id.get(),
                player.user_id,
                player.name,
                error.log_detail(),
            );
            failures.push(format!("{} ({})", player.name, error.public_reason()));
        }
    }
    if !failures.is_empty() {
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            format!(
                "비밀 메시지를 보낼 수 없는 참가자와 원인:\n{}\n\n서버 콘솔에는 Discord 원문 오류와 채널/사용자 ID를 기록했습니다.",
                failures.join("\n")
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
    let mut message = format!(
        "당신의 역할은 **{}** 입니다.\n진영: **{}**\n\n{}",
        player.role.value(),
        team,
        role_short_guide(player.role)
    );
    // [사기] 사기꾼은 게임 시작 시 사기 대상과 변장 직업을 바로 알게 된다.
    if player.role == Role::Fraudster {
        if let Some((target, disguised_role)) = game.fraudster_disguise_info(player.user_id) {
            message.push_str(&format!(
                "\n\n[사기] {}님의 직업은 **{}**입니다.\n당신은 **{}**{} 변장했습니다. 조사 판정이 {}{} 표시됩니다.\n[교섭] 당신 또는 사기 대상이 마피아팀의 처형 대상이 되면 마피아팀과 접선하며, 당신은 마피아팀에게 처형되지 않습니다.",
                target.name,
                disguised_role.value(),
                disguised_role.value(),
                mafia_remake::model::korean_ro_particle(disguised_role.value()),
                disguised_role.value(),
                mafia_remake::model::korean_ro_particle(disguised_role.value()),
            ));
        } else if game
            .fraudster_blocked_by_soldier
            .contains_key(&player.user_id)
        {
            // [불침번]에 막힌 경우. 군인의 정체는 사기꾼에게 알려주지 않는다.
            message.push_str(
                "\n\n[사기] 사기 대상이 불침번을 서고 있어 변장에 실패했습니다. 이번 게임에는 변장 없이 사기꾼 그대로 판정됩니다.",
            );
        }
    }
    // 개인 티어 안내 (비공개).
    let tier = game.player_tiers.get(&player.user_id).copied().unwrap_or(2);
    let abilities = game.player_tier_abilities(player.user_id);
    if abilities.is_empty() {
        message.push_str(&format!(
            "\n\n당신의 티어: **{}티어** (티어 능력 없음)",
            tier
        ));
    } else {
        message.push_str(&format!("\n\n당신의 티어: **{}티어**", tier));
        for ability in &abilities {
            message.push_str(&format!(
                "\n티어 능력 [{}]: {}",
                ability.value(),
                ability.description()
            ));
        }
    }
    // [불침번] 군인은 게임 시작 시 자신을 노린 사기를 막아낸 사실을 안다.
    if player.role == Role::Soldier {
        let blocked_fraudsters = game
            .fraudster_blocked_by_soldier
            .iter()
            .filter(|(_, soldier_id)| **soldier_id == player.user_id)
            .filter_map(|(fraudster_id, _)| game.get_player(*fraudster_id))
            .map(|fraudster| {
                format!(
                    "\n\n[불침번] 사기꾼 {}님의 사기를 막아냈습니다.",
                    fraudster.name
                )
            })
            .collect::<String>();
        message.push_str(&blocked_fraudsters);
    }
    message
}

fn mercenary_contract_received_message() -> &'static str {
    "누군가로부터 의뢰를 받았습니다."
}

pub fn role_short_guide(role: Role) -> &'static str {
    match role {
        Role::Mafia => "밤마다 제거할 대상을 선택합니다.",
        Role::Doctor => "밤마다 보호할 대상을 선택합니다.",
        Role::Police => {
            "밤마다 한 명을 조사해 즉시 결과를 확인합니다. 대상은 제출 후 바꿀 수 없습니다."
        }
        Role::Agent => "밤마다 시민팀 지령 정보를 받습니다.",
        Role::Vigilante => "낮에 조사(1회, 즉시 결과)하고 밤에 숙청할 수 있습니다.",
        Role::Inspector => {
            "게임 중 한 번만 수사할 수 있고, 결과는 제출 즉시 나옵니다. 같은 팀이면 직업을 확인하며 대상에게 자신의 정체를 알립니다. 다른 팀이면 \"시민팀이 아닙니다\"만 나오고 대상은 알지 못합니다."
        }
        Role::Detective => "밤 행동의 이동 경로를 추적합니다.",
        Role::CivilServant => {
            "밤마다 직업 하나를 조회해 그 직업을 가진 플레이어를 사망자까지 포함해 알아냅니다. 경찰 계열과 시민은 조회할 수 없습니다."
        }
        Role::Paparazzi => {
            "하루에 한 번, 시민팀이 처음으로 알아낸 다른 사람의 직업 정보를 함께 공유받습니다."
        }
        Role::Shaman => "사망자를 성불하고 직업을 확인합니다.",
        Role::Priest => "사망자를 한 번 소생시킬 수 있습니다.",
        Role::Reporter => "두 번째 밤부터 특종으로 직업을 공개합니다.",
        Role::Hacker => "낮에 해킹해 직업을 확인하고 능력을 우회합니다.",
        Role::Terrorist => "지목한 위험 대상을 함께 데려갈 수 있습니다.",
        Role::Lover => "연인과 정보를 공유하고 서로를 지킵니다.",
        Role::Soldier => {
            "마피아 공격을 한 번 버티고, 불침번으로 스파이의 첩보·도둑의 도벽·사기꾼의 사기·청부업자의 청부를 막아내며 그 정체를 알아냅니다."
        }
        Role::Spy => {
            "밤마다 한 명의 직업을 알아내고, 마피아를 찾아내면 그 밤 한 번 더 첩보를 사용합니다."
        }
        Role::Fraudster => {
            "시민 한 명의 직업으로 변장해 조사를 속이고, 변장 대상이나 자신이 마피아의 표적이 되면 접선합니다."
        }
        Role::Contractor => "두 명의 직업을 맞히면 암살합니다.",
        Role::Thief => {
            "마지막으로 지목 투표한 대상의 능력을 훔칩니다. 결과는 투표가 끝난 뒤 전달됩니다."
        }
        Role::Witch => "밤에 대상을 개구리로 저주합니다.",
        Role::Scientist => {
            "처음부터 마피아팀입니다. 첫 사망 전에는 미접선 보조처럼 시민 판정을 받고, 첫 사망 후 접선되어 다음 밤 부활합니다."
        }
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

fn remaining_night_wait(deadline: Instant, now: Instant) -> Duration {
    deadline.saturating_duration_since(now)
}

async fn wait_for_night_deadline_or_action(deadline: Instant, notify: &Notify) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(remaining_night_wait(deadline, Instant::now())) => true,
        _ = notify.notified() => false,
    }
}

struct TimedNightEvents {
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

async fn take_timed_night_events(running: &Arc<RwLock<RunningGame>>) -> Option<TimedNightEvents> {
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

async fn apply_timed_night_event_side_effects(
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
    for holder in loudspeakers {
        set_member_game_channel_chat(ctx, running, &holder, true).await;
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
    let target_rows = (0..2).map(|slot| {
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
    });
    // 대상마다 전용 직업 셀렉트를 준다. 하나의 셀렉트를 두 대상이 공유하면 어느
    // 대상에 적용되는지가 숨은 상태가 되어, 1번 대상 직업을 못 고르는 것처럼 보인다.
    let role_rows = (0..2).map(|slot| {
        let role_options = contractor_guessable_roles_for_group(draft.role_group)
            .take(25)
            .map(|role| serenity::CreateSelectMenuOption::new(role.value(), role.value()))
            .collect::<Vec<_>>();
        let placeholder = match draft.guessed_roles[slot] {
            Some(role) => format!("{}번 대상 직업: {}", slot + 1, role.value()),
            None => format!(
                "{}번 대상 직업 선택 ({})",
                slot + 1,
                draft.role_group.label()
            ),
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
    });
    let category_and_submit_buttons = serenity::CreateActionRow::Buttons(vec![
        serenity::CreateButton::new(format!(
            "contractor_group:{}:{}:{}",
            guild_id.get(),
            actor_id,
            ContractorGuessRoleGroup::Citizen.component_value()
        ))
        .label(ContractorGuessRoleGroup::Citizen.label())
        .style(if draft.role_group == ContractorGuessRoleGroup::Citizen {
            serenity::ButtonStyle::Primary
        } else {
            serenity::ButtonStyle::Secondary
        }),
        serenity::CreateButton::new(format!(
            "contractor_group:{}:{}:{}",
            guild_id.get(),
            actor_id,
            ContractorGuessRoleGroup::MafiaCultNeutral.component_value()
        ))
        .label(ContractorGuessRoleGroup::MafiaCultNeutral.label())
        .style(
            if draft.role_group == ContractorGuessRoleGroup::MafiaCultNeutral {
                serenity::ButtonStyle::Primary
            } else {
                serenity::ButtonStyle::Secondary
            },
        ),
        serenity::CreateButton::new(format!("contractor_submit:{}:{}", guild_id.get(), actor_id))
            .label("청부 확정")
            .style(serenity::ButtonStyle::Success),
    ]);

    target_rows
        .chain(role_rows)
        .chain([category_and_submit_buttons])
        .collect()
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
        "두 명과 각 직업을 추측합니다. 둘 중 한 명이라도 마피아를 정확히 맞히면 접선합니다.\n첫날 밤에는 사용할 수 없고, 직업이 공개된 사람은 대상에서 제외됩니다. 경찰 계열도 대상으로 고를 수 있지만 경찰 계열 직업은 추측할 수 없습니다.\n\n{}\n{}\n\n직업 목록: **{}** (추측 가능한 직업이 25개를 넘어 팀별로 나눠 보여줍니다. 아래 버튼으로 목록을 바꾸면 이미 고른 직업은 그대로 유지됩니다.)\n밤이 끝나기 전 다시 확정하면 청부 대상을 변경할 수 있습니다.",
        target_line(0),
        target_line(1),
        draft.role_group.label(),
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
    // 밤 동안의 [확성] 개인 허용을 원상 복구한 뒤 낮 채팅을 연다.
    restore_member_game_channel_chat(ctx, running).await;
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
async fn deliver_thief_steal_results(
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

fn terrorist_execution_message(terrorist: &Player, target: &Player) -> String {
    format!(
        "[테러리스트 {}님이 {}님을 습격하였습니다.]",
        terrorist.name, target.name
    )
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

fn confirmation_vote_summary_section(
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

fn confirmation_weighted_counts(confirm_result: &ConfirmVoteResult) -> &HashMap<bool, i32> {
    if confirm_result.weighted_vote_counts.is_empty() {
        &confirm_result.vote_counts
    } else {
        &confirm_result.weighted_vote_counts
    }
}

fn confirmation_weighted_vote_count(confirm_result: &ConfirmVoteResult) -> i32 {
    let counts = confirmation_weighted_counts(confirm_result);
    counts.values().copied().sum()
}

fn confirmation_required_yes(confirm_result: &ConfirmVoteResult) -> i32 {
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

fn confirmation_rejection_message(
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

#[derive(Clone, Debug)]
pub struct GameResultImageRow {
    name: String,
    role: String,
    team: String,
    /// "3티어 [가호]" / "2티어" — 게임 결과에 공개되는 개인 티어.
    tier_text: String,
    alive: bool,
    before: Option<i64>,
    after: Option<i64>,
    before_rank: Option<String>,
    after_rank: Option<String>,
    delta: Option<i64>,
    team_delta: Option<i64>,
    role_delta: Option<i64>,
    streak_delta: Option<i64>,
    win_streak: Option<i64>,
    best_win_streak: Option<i64>,
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

pub fn prophet_victory_message(game: &MafiaGame, winner: Winner) -> Option<String> {
    if winner != Winner::Citizen {
        return None;
    }
    let prophet = game.winning_prophet()?;
    Some(format!(
        "예언자 {}님의 힘으로 시민팀이 승리하였습니다!",
        prophet.name
    ))
}

pub fn game_result_display_name(running: &RunningGame, player: &Player) -> String {
    game_result_label(running, player.user_id, &player.name)
}

/// 게임 결과 표기용 이름. 익명 게임이면 "별명 = 실명"으로 번호의 정체를 함께 공개한다.
pub fn game_result_label(running: &RunningGame, user_id: u64, fallback: &str) -> String {
    if running.anonymous_enabled {
        let alias = running
            .anonymous_aliases
            .get(&user_id)
            .map(String::as_str)
            .unwrap_or("익명");
        let real_name = running
            .anonymous_original_names
            .get(&user_id)
            .map(String::as_str)
            .unwrap_or(fallback);
        format!("{alias} = {real_name}")
    } else {
        fallback.to_string()
    }
}

/// 익명 게임의 `game.players`는 이름이 별명으로 덮여 있다. 통계/레이팅에는 실명이
/// 남아야 하므로 기록 직전에 원래 이름으로 되돌린 스냅샷을 만든다.
fn stats_game_snapshot(running: &RunningGame) -> MafiaGame {
    let mut snapshot = running.game.clone();
    if running.anonymous_enabled {
        for player in &mut snapshot.players {
            if let Some(original) = running.anonymous_original_names.get(&player.user_id) {
                player.name.clone_from(original);
            }
        }
    }
    snapshot
}

/// 레이팅/랭크 변동 안내는 실명으로 기록되지만, 익명 게임 결과에서는 어떤 번호가
/// 누구였는지도 함께 보여준다.
fn rating_log_with_result_labels(
    running: &RunningGame,
    rating_log: &[stats::GameRatingLogItem],
) -> Vec<stats::GameRatingLogItem> {
    rating_log
        .iter()
        .map(|item| {
            let mut item = item.clone();
            item.name = game_result_label(running, item.user_id, &item.name);
            item
        })
        .collect()
}

/// 게임 결과에 공개할 티어 표기.
pub fn game_result_tier_text(game: &MafiaGame, user_id: u64) -> String {
    let tier = game.player_tiers.get(&user_id).copied().unwrap_or(2);
    let abilities = game.player_tier_abilities(user_id);
    if abilities.is_empty() {
        format!("{}티어", tier)
    } else {
        format!(
            "{}티어 [{}]",
            tier,
            abilities
                .iter()
                .map(|ability| ability.value())
                .collect::<Vec<_>>()
                .join(", ")
        )
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
                tier_text: game_result_tier_text(&running.game, player.user_id),
                alive: player.alive,
                before: rating.map(|item| item.before),
                after: rating.map(|item| item.after),
                before_rank: rating.map(|item| item.before_rank.clone()),
                after_rank: rating.map(|item| item.after_rank.clone()),
                delta: rating.map(|item| item.delta),
                team_delta: rating.map(|item| item.team_delta),
                role_delta: rating.map(|item| item.role_delta),
                streak_delta: rating.map(|item| item.streak_delta),
                win_streak: rating.map(|item| item.win_streak),
                best_win_streak: rating.map(|item| item.best_win_streak),
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
    const WIDTH: u32 = 2240;
    const TOP: i32 = 44;
    const SIDE: i32 = 56;
    const HEADER_HEIGHT: i32 = 172;
    const FOOTER: i32 = 56;
    const COL_PLAYER: i32 = SIDE + 42;
    const COL_ROLE: i32 = SIDE + 420;
    const COL_RATING: i32 = SIDE + 720;
    const COL_DELTA: i32 = SIDE + 1010;
    const COL_STREAK: i32 = SIDE + 1164;
    const COL_REASON: i32 = SIDE + 1400;

    let table_top = TOP + HEADER_HEIGHT + 26;
    let row_heights = rows.iter().map(game_result_row_height).collect::<Vec<_>>();
    let table_height = row_heights.iter().sum::<i32>();
    let height = (table_top + table_height + FOOTER).max(520) as u32;
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
    let badge_x = WIDTH as i32 - SIDE - 282;
    fill_rect(&mut image, badge_x, TOP + 44, 250, 54, accent);
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
        (COL_PLAYER, "플레이어"),
        (COL_ROLE, "최종 역할"),
        (COL_RATING, "레이팅"),
        (COL_DELTA, "변동"),
        (COL_STREAK, "연승"),
        (COL_REASON, "랭크/사유"),
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

    let mut y = table_top;
    for (index, row) in rows.iter().enumerate() {
        let row_height = row_heights[index];
        let row_fill = if index % 2 == 0 { white } else { soft };
        fill_rect(
            &mut image,
            SIDE,
            y,
            WIDTH - SIDE as u32 * 2,
            row_height as u32,
            row_fill,
        );
        fill_rect(
            &mut image,
            SIDE,
            y + row_height - 1,
            WIDTH - SIDE as u32 * 2,
            1,
            line,
        );
        fill_rect(
            &mut image,
            SIDE,
            y,
            8,
            row_height as u32,
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
            SIDE + 68,
            y + 18,
            truncate_for_board(&row.name, 22),
            text,
        );
        draw_lb_text(
            &mut image,
            &font,
            20.0,
            SIDE + 70,
            y + 58,
            if row.alive { "생존" } else { "사망" },
            muted,
        );
        draw_lb_text(
            &mut image,
            &font,
            26.0,
            COL_ROLE,
            y + 20,
            truncate_for_board(&row.role, 18),
            text,
        );
        draw_lb_text(
            &mut image,
            &font,
            20.0,
            COL_ROLE + 2,
            y + 58,
            format!("{} · {}", row.team, row.tier_text),
            team_color(&row.team),
        );
        draw_rating_block(&mut image, &font, row, COL_RATING, y, text, muted);
        draw_delta_badge(&mut image, &font, row, COL_DELTA, y);
        draw_streak_badge(&mut image, &font, row, COL_STREAK, y);
        draw_rank_and_reason(&mut image, &font, row, COL_REASON, y, text, muted);
        y += row_height;
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

fn game_result_reason_text(row: &GameResultImageRow) -> String {
    if row.reasons.is_empty() {
        "사유 없음".to_string()
    } else {
        row.reasons.join(", ")
    }
}

fn wrap_result_reason(reason: &str) -> Vec<String> {
    wrap_text_by_chars(reason, 32)
}

fn game_result_row_height(row: &GameResultImageRow) -> i32 {
    let reason_lines = wrap_result_reason(&game_result_reason_text(row))
        .len()
        .max(1) as i32;
    (86 + reason_lines * 24).max(126)
}

fn wrap_text_by_chars(text: &str, max_chars: usize) -> Vec<String> {
    if text.trim().is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let separator = usize::from(!current.is_empty());
        if current.chars().count() + separator + word.chars().count() > max_chars
            && !current.is_empty()
        {
            lines.push(current);
            current = String::new();
        }
        if word.chars().count() > max_chars {
            if !current.is_empty() {
                lines.push(current);
                current = String::new();
            }
            let mut chunk = String::new();
            for ch in word.chars() {
                if chunk.chars().count() >= max_chars {
                    lines.push(chunk);
                    chunk = String::new();
                }
                chunk.push(ch);
            }
            if !chunk.is_empty() {
                current = chunk;
            }
            continue;
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
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
                row.before_rank.as_deref().unwrap_or("?"),
                row.after_rank.as_deref().unwrap_or("?")
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
    let detail = game_result_delta_detail(row);
    draw_lb_text(image, font, 18.0, x, y + 70, detail, image_color("#64748b"));
}

fn game_result_delta_detail(row: &GameResultImageRow) -> String {
    format!(
        "팀 {:+} · 직업 {:+}",
        row.team_delta.unwrap_or(0),
        row.role_delta.unwrap_or(0)
    )
}

fn draw_streak_badge(
    image: &mut RgbImage,
    font: &FontArc,
    row: &GameResultImageRow,
    x: i32,
    y: i32,
) {
    let Some(current) = row.win_streak else {
        draw_lb_text(image, font, 23.0, x, y + 34, "-", image_color("#94a3b8"));
        return;
    };
    let best = row.best_win_streak.unwrap_or(current);
    let (fill, color) = if current > 0 {
        (image_color("#dcfce7"), image_color("#15803d"))
    } else {
        (image_color("#e2e8f0"), image_color("#475569"))
    };
    fill_rect(image, x, y + 22, 208, 42, fill);
    draw_lb_text(
        image,
        font,
        23.0,
        x + 14,
        y + 30,
        format!("현재 {current}연승"),
        color,
    );
    draw_lb_text(
        image,
        font,
        18.0,
        x,
        y + 72,
        format!("최고 {best}연승"),
        image_color("#64748b"),
    );
    draw_lb_text(
        image,
        font,
        18.0,
        x,
        y + 98,
        format!("보너스 {:+}", row.streak_delta.unwrap_or(0)),
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
        let before_rank = row.before_rank.as_deref().unwrap_or("?");
        let after_rank = row.after_rank.as_deref().unwrap_or("?");
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
    let reason = game_result_reason_text(row);
    for (index, line) in wrap_result_reason(&reason).iter().enumerate() {
        draw_lb_text(
            image,
            font,
            18.0,
            x,
            y + 56 + index as i32 * 24,
            line,
            muted,
        );
    }
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
    let (winner, prophet_message) = {
        let running_read = running.read().await;
        let Some(winner) = running_read.game.winner() else {
            return Ok(false);
        };
        (winner, prophet_victory_message(&running_read.game, winner))
    };
    let (roles_text, elapsed_seconds, record_payload) = {
        let mut running_write = running.write().await;
        running_write.game.phase = Phase::Ended;
        let elapsed_seconds = running_write.started_at.elapsed().as_secs() as i64;
        if running_write.ended_at_iso.is_none() {
            running_write.ended_at_iso =
                Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
        }
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
                stats_game_snapshot(&running_write),
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
    if let Some(message) = prophet_message
        && let Err(error) = send_game_embed(
            ctx,
            running,
            message,
            "예언자 승리",
            serenity::Colour::DARK_GREEN,
            vec![],
            true,
            true,
        )
        .await
    {
        eprintln!("failed to announce prophet victory: {error:?}");
    }
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
        let labeled_rating_log = {
            let running_read = running.read().await;
            rating_log_with_result_labels(&running_read, &recorded_rating_log)
        };
        rating_log_chunks = stats::game_rating_log_chunks(&labeled_rating_log, 3500);
        rank_change_chunks = stats::game_rank_change_chunks(&labeled_rating_log, 3500);
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
        let completed_replays_path = data.completed_replays_path.clone();
        let completed_replays_snapshot = completed_replays.clone();
        tokio::spawn(async move {
            match tokio::task::spawn_blocking(move || {
                crate::web_settings::save_completed_replays(
                    &*completed_replays_path,
                    &completed_replays_snapshot,
                )
            })
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(error)) => eprintln!("failed to save replay history: {error:?}"),
                Err(error) => eprintln!("failed to join replay history save task: {error:?}"),
            }
        });
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
    use crate::channel::tests::dead_chat_test_running;

    /// 익명 게임 진행 중과 같은 상태: `game.players`의 이름이 별명으로 덮여 있고
    /// 실명은 `anonymous_original_names`에만 남아 있다. 참가자 순서는 무작위이므로
    /// 별명은 user_id 순으로 매긴다.
    fn anonymous_result_test_running() -> RunningGame {
        let mut running = dead_chat_test_running();
        running.anonymous_enabled = true;
        let mut user_ids = running
            .game
            .players
            .iter()
            .map(|player| player.user_id)
            .collect::<Vec<_>>();
        user_ids.sort_unstable();
        for (index, user_id) in user_ids.into_iter().enumerate() {
            running
                .anonymous_aliases
                .insert(user_id, format!("{}번", index + 1));
        }
        for player in &mut running.game.players {
            running
                .anonymous_original_names
                .insert(player.user_id, player.name.clone());
            player
                .name
                .clone_from(&running.anonymous_aliases[&player.user_id]);
        }
        running
    }

    #[test]
    fn anonymous_game_result_names_reveal_the_player_behind_each_alias() {
        let running = anonymous_result_test_running();
        let player = running
            .game
            .players
            .iter()
            .find(|player| player.user_id == 3)
            .unwrap();

        assert_eq!(player.name, "3번");
        assert_eq!(game_result_display_name(&running, player), "3번 = p3");
    }

    #[test]
    fn stats_snapshot_records_real_names_for_anonymous_games() {
        let running = anonymous_result_test_running();

        let snapshot = stats_game_snapshot(&running);

        let mut names = snapshot
            .players
            .iter()
            .map(|player| player.name.clone())
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, ["p1", "p2", "p3", "p4"]);
    }

    #[test]
    fn anonymous_rank_change_lines_show_alias_and_real_name() {
        let running = anonymous_result_test_running();
        let log = vec![stats::GameRatingLogItem {
            user_id: 2,
            name: "p2".to_string(),
            role: Role::Citizen.value().to_string(),
            before: 1000,
            after: 1030,
            before_rank: "실버".to_string(),
            after_rank: "실버".to_string(),
            delta: 30,
            team_delta: 30,
            role_delta: 0,
            streak_delta: 0,
            win_streak: 1,
            best_win_streak: 1,
            reasons: Vec::new(),
        }];

        let labeled = rating_log_with_result_labels(&running, &log);

        assert_eq!(labeled[0].name, "2번 = p2");
    }

    #[test]
    fn non_anonymous_game_result_names_are_left_alone() {
        let mut running = anonymous_result_test_running();
        running.anonymous_enabled = false;
        let player = running.game.players[0].clone();
        let alias = player.name.clone();

        assert_eq!(game_result_display_name(&running, &player), alias);
        assert_eq!(stats_game_snapshot(&running).players[0].name, alias);
    }

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
    fn night_wait_does_not_restart_after_delayed_anonymous_broadcast() {
        let started_at = Instant::now();
        let deadline = started_at + Duration::from_secs(30);

        assert_eq!(
            remaining_night_wait(deadline, started_at + Duration::from_secs(20)),
            Duration::from_secs(10)
        );
        assert_eq!(
            remaining_night_wait(deadline, started_at + Duration::from_secs(35)),
            Duration::ZERO
        );
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
    fn terrorist_execution_message_matches_public_format() {
        let terrorist = Player::new(1, "구현민", Role::Terrorist);
        let target = Player::new(2, "설재경", Role::Mafia);

        assert_eq!(
            terrorist_execution_message(&terrorist, &target),
            "[테러리스트 구현민님이 설재경님을 습격하였습니다.]"
        );
    }

    #[test]
    fn mercenary_contract_message_hides_client_identity() {
        assert_eq!(
            mercenary_contract_received_message(),
            "누군가로부터 의뢰를 받았습니다."
        );
    }

    #[test]
    fn mercenary_role_message_does_not_reveal_client_name() {
        let mut game = MafiaGame::new(
            vec![
                (1, "마피아".to_string()),
                (2, "용병".to_string()),
                (3, "비밀의뢰인".to_string()),
            ],
            1,
            0,
            0,
            Vec::new(),
        )
        .unwrap();
        game.get_player_mut(2).unwrap().role = Role::Mercenary;
        game.get_player_mut(3).unwrap().role = Role::Citizen;
        game.mercenary_client_ids.clear();
        game.mercenary_client_ids.insert(2, 3);

        let message = role_message(&game, game.get_player(2).unwrap());

        assert!(!message.contains("비밀의뢰인"));
        assert!(!message.contains("의뢰인:"));
    }

    #[test]
    fn scientist_role_message_names_mafia_team_before_first_death() {
        let mut game = MafiaGame::new(
            vec![
                (1, "One".to_string()),
                (2, "Scientist".to_string()),
                (3, "Three".to_string()),
            ],
            1,
            0,
            0,
            Vec::new(),
        )
        .unwrap();
        game.get_player_mut(2).unwrap().role = Role::Scientist;

        let message = role_message(&game, game.get_player(2).unwrap());

        assert!(message.contains("진영: **마피아팀**"));
        assert!(!message.contains("진영: **시민팀**"));
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
    fn confirmation_summary_can_hide_vote_counts() {
        let result = ConfirmVoteResult {
            vote_counts: HashMap::from([(true, 3), (false, 2)]),
            ..Default::default()
        };
        let context = ConfirmationVoteContext {
            eligible_voters: 7,
            submitted_voters: 5,
        };

        assert_eq!(
            confirmation_vote_summary_section(&result, context, false),
            ""
        );
    }

    #[test]
    fn confirmation_summary_displays_raw_counts_with_weighted_threshold() {
        let result = ConfirmVoteResult {
            vote_counts: HashMap::from([(true, 1), (false, 1)]),
            weighted_vote_counts: HashMap::from([(true, 2), (false, 1)]),
            ..Default::default()
        };
        let context = ConfirmationVoteContext {
            eligible_voters: 2,
            submitted_voters: 2,
        };

        assert_eq!(
            confirmation_vote_summary(&result, context),
            "찬성 1표 / 반대 1표 / 미투표 0명\n처형 기준: 찬성 처리값 2 이상 (처리 투표수 3 기준)"
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
    fn prophet_victory_message_names_the_prophet() {
        let players = (1..=5)
            .map(|user_id| (user_id, format!("P{user_id}")))
            .collect::<Vec<_>>();
        let mut game = MafiaGame::new(players, 1, 0, 0, Vec::new()).unwrap();
        let prophet = game.get_player_mut(2).unwrap();
        prophet.role = Role::Prophet;
        prophet.name = "설재경".to_string();
        game.phase = Phase::Day;
        game.day_number = 4;

        assert_eq!(
            prophet_victory_message(&game, Winner::Citizen).as_deref(),
            Some("예언자 설재경님의 힘으로 시민팀이 승리하였습니다!")
        );
        assert_eq!(prophet_victory_message(&game, Winner::Mafia), None);
    }

    #[test]
    fn contractor_components_stay_within_discord_limits() {
        let targets = (0..30)
            .map(|index| Player::new(1000 + index, format!("대상{index}"), Role::Citizen))
            .collect::<Vec<_>>();
        let components = contractor_contract_components(
            serenity::GuildId::new(1),
            42,
            &targets,
            &ContractorContractDraft::default(),
        );
        let json = serde_json::to_value(&components).unwrap();
        let rows = json.as_array().unwrap();

        // 대상 셀렉트 2개 + 대상별 직업 셀렉트 2개 + 버튼 1줄 = Discord 상한인 5줄.
        assert_eq!(rows.len(), 5);
        let mut select_ids = Vec::new();
        for row in rows {
            for component in row["components"].as_array().unwrap() {
                if let Some(options) = component.get("options").and_then(|value| value.as_array()) {
                    assert!(!options.is_empty());
                    assert!(options.len() <= 25);
                    select_ids.push(component["custom_id"].as_str().unwrap().to_string());
                }
            }
        }
        assert_eq!(
            select_ids,
            [
                "contractor_target:1:42:0",
                "contractor_target:1:42:1",
                "contractor_role:1:42:0",
                "contractor_role:1:42:1",
            ]
        );
    }

    /// 두 직업 목록 모두 셀렉트 상한 안에 들어와야 한다. 하나라도 넘으면 Discord가
    /// 메시지를 거부해 청부 화면 자체가 뜨지 않는다.
    #[test]
    fn every_contractor_role_group_fits_one_select() {
        for group in [
            ContractorGuessRoleGroup::Citizen,
            ContractorGuessRoleGroup::MafiaCultNeutral,
        ] {
            let count = contractor_guessable_roles_for_group(group).count();
            assert!(count > 0 && count <= 25, "{group:?} has {count} roles");
        }
    }

    /// 게임 결과에는 티어와 능력이 공개된다.
    #[test]
    fn game_result_tier_text_shows_tier_and_ability() {
        let mut running = dead_chat_test_running();
        let first = running.game.players[0].user_id;
        let second = running.game.players[1].user_id;
        running.game.player_tiers.insert(first, 4);
        running.game.player_tiers.insert(second, 2);
        running
            .game
            .tier_abilities
            .insert(first, vec![mafia_remake::model::TierAbility::Cleanup]);

        assert_eq!(game_result_tier_text(&running.game, first), "4티어 [수습]");
        assert_eq!(game_result_tier_text(&running.game, second), "2티어");
        // 배정 기록이 없으면 기본 2티어로 표기한다.
        assert_eq!(game_result_tier_text(&running.game, 999_999), "2티어");
    }

    #[test]
    fn game_result_image_renders_png() {
        let rows = vec![
            GameResultImageRow {
                name: "Long Reason".to_string(),
                role: Role::Doctor.value().to_string(),
                team: "시민팀".to_string(),
                tier_text: "3티어 [가호]".to_string(),
                alive: true,
                before: Some(1043),
                after: Some(1077),
                before_rank: Some("실버".to_string()),
                after_rank: Some("골드".to_string()),
                delta: Some(34),
                team_delta: Some(29),
                role_delta: Some(1),
                streak_delta: Some(4),
                win_streak: Some(3),
                best_win_streak: Some(7),
                reasons: vec![
                    "소속 진영 승리, 의사 보호 운영 기여 +1, 3연승 보너스 +4, 레이팅 구간 보정 x1.20"
                        .to_string(),
                ],
            },
            GameResultImageRow {
                name: "Alpha".to_string(),
                role: Role::Mafia.value().to_string(),
                team: "마피아팀".to_string(),
                tier_text: "3티어 [가호]".to_string(),
                alive: true,
                before: Some(1000),
                after: Some(1032),
                before_rank: Some("실버".to_string()),
                after_rank: Some("골드".to_string()),
                delta: Some(32),
                team_delta: Some(24),
                role_delta: Some(4),
                streak_delta: Some(4),
                win_streak: Some(2),
                best_win_streak: Some(2),
                reasons: vec!["소속 진영 승리".to_string()],
            },
            GameResultImageRow {
                name: "Beta".to_string(),
                role: Role::Doctor.value().to_string(),
                team: "시민팀".to_string(),
                tier_text: "3티어 [가호]".to_string(),
                alive: false,
                before: Some(1000),
                after: Some(982),
                before_rank: Some("실버".to_string()),
                after_rank: Some("골드".to_string()),
                delta: Some(-18),
                team_delta: Some(-20),
                role_delta: Some(2),
                streak_delta: Some(0),
                win_streak: Some(0),
                best_win_streak: Some(4),
                reasons: vec!["패배".to_string()],
            },
        ];

        assert_eq!(game_result_delta_detail(&rows[0]), "팀 +29 · 직업 +1");

        let image = render_game_result_image(Winner::Mafia, 310, rows).unwrap();

        assert!(image.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(image.len() > 1024);
        let decoded = image::load_from_memory(&image).unwrap().to_rgb8();
        assert_eq!(decoded.width(), 2240);
        assert!(decoded.height() > 520);
        assert_eq!(*decoded.get_pixel(1222, 266), image_color("#dcfce7"));
    }
}

// 역할: Discord 채널 생성·권한 관리, 익명 채널, 비공개 역할 채널, 멤버 접근 동기화,
//        게임 상태 메시지 업서트, 사망 처리, 청부 결과, 정화 효과

#![allow(unused_imports, clippy::too_many_arguments, clippy::collapsible_if)]

use super::{
    ChannelRoleIds, Context, ContractorContractDraft, DEAD_PLAYER_ROLE, Data, Error,
    GAME_NOTIFICATION_ROLE, MAX_GAME_PLAYERS, PRIVATE_CHAT_ROLES, PersonalChannelKind, Recruitment,
    RunningGame, SHAMAN_CHAT_CHANNEL_NAME, SPECTATOR_ROLE,
};
use crate::embed::*;
use anyhow::{Context as AnyhowContext, Result, bail};
use mafia_remake::game::MafiaGame;
use mafia_remake::model::{
    CITIZEN_SPECIAL_ROLES, CONTRACTOR_GUESS_ROLES, ConfirmVoteResult, MAFIA_SPECIAL_ROLES,
    NEUTRAL_SPECIAL_ROLES, NightResult, PUBLIC_CITIZEN_SPECIAL_ROLES, PUBLIC_CULT_SPECIAL_ROLES,
    PUBLIC_MAFIA_SPECIAL_ROLES, PUBLIC_NEUTRAL_SPECIAL_ROLES, Phase, Player, Role, VoteResult,
    Winner,
};
use mafia_remake::stats;
use mafia_remake::{config, system_random};
use poise::serenity_prelude as serenity;
use poise::serenity_prelude::Mentionable;
use rand::seq::{IndexedRandom, SliceRandom};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::sync::{Notify, RwLock};
use tokio::task::JoinSet;

mod chat_gate;
mod cleanup;
mod creation;
mod permissions_sync;
mod recruitment;
mod roles_setup;
pub(crate) use self::chat_gate::*;
pub(crate) use self::cleanup::*;
pub(crate) use self::creation::*;
pub(crate) use self::permissions_sync::*;
pub(crate) use self::recruitment::*;
pub(crate) use self::roles_setup::*;

const LEGACY_FROG_CHAT_CHANNEL_NAME: &str = "개구리-채팅방";
const DISCORD_WRITE_CONCURRENCY: usize = 4;
const RECRUITMENT_UPDATE_DEBOUNCE: Duration = Duration::from_millis(300);

const ANIMAL_ALIASES: &[&str] = &[
    "사자",
    "호랑이",
    "고양이",
    "강아지",
    "토끼",
    "판다",
    "곰",
    "여우",
    "늑대",
    "돼지",
    "원숭이",
    "코끼리",
    "기린",
    "펭귄",
    "오리",
    "병아리",
    "부엉이",
    "독수리",
    "거북이",
    "돌고래",
    "상어",
    "고래",
    "악어",
    "뱀",
    "나비",
    "벌",
    "개미",
    "달팽이",
    "문어",
    "물고기",
    "게",
    "새우",
    "오징어",
    "말",
    "얼룩말",
    "소",
    "양",
    "염소",
    "닭",
    "쥐",
    "햄스터",
    "사슴",
    "라마",
    "캥거루",
    "하마",
    "코뿔소",
    "박쥐",
    "고슴도치",
    "수달",
    "비버",
    "너구리",
    "스컹크",
    "공작",
    "앵무새",
    "백조",
    "플라밍고",
    "칠면조",
    "고릴라",
    "오랑우탄",
    "물개",
];

pub const NUMBER_AVATAR_COLORS: &[&str] = &[
    "e11d48", "2563eb", "16a34a", "f59e0b", "7c3aed", "0891b2", "db2777", "65a30d", "dc2626",
    "4f46e5", "0f766e", "ea580c", "9333ea", "0284c7", "ca8a04", "be123c", "1d4ed8", "15803d",
    "b45309", "6d28d9", "0369a1", "a21caf", "047857", "c2410c",
];

pub fn sanitize_channel_part(value: &str) -> String {
    value.replace([' ', '/'], "-").to_lowercase()
}

pub fn private_channel_name(role: Role) -> &'static str {
    match role {
        Role::Mafia => "마피아-비밀방",
        Role::Police => "경찰-비밀방",
        Role::Agent => "요원-비밀방",
        Role::Vigilante => "자경단원-비밀방",
        Role::Doctor => "의사-비밀방",
        Role::CultLeader => "교주-비밀방",
        Role::Lover => "연인-비밀방",
        _ => "역할-비밀방",
    }
}

pub fn normalized_anonymous_name_mode(config: &config::BotConfig) -> &str {
    if config.anonymous_name_mode == "number" {
        "number"
    } else {
        "animal"
    }
}

pub fn anonymous_name_mode_text(config: &config::BotConfig) -> &'static str {
    if normalized_anonymous_name_mode(config) == "number" {
        "숫자 이름"
    } else {
        "동물 이름"
    }
}

pub fn animal_emoji_code(label: &str) -> Option<&'static str> {
    match label {
        "사자" => Some("1f981"),
        "호랑이" => Some("1f42f"),
        "고양이" => Some("1f431"),
        "강아지" => Some("1f436"),
        "토끼" => Some("1f430"),
        "판다" => Some("1f43c"),
        "곰" => Some("1f43b"),
        "여우" => Some("1f98a"),
        "늑대" => Some("1f43a"),
        "돼지" => Some("1f437"),
        "원숭이" => Some("1f435"),
        "코끼리" => Some("1f418"),
        "기린" => Some("1f992"),
        "펭귄" => Some("1f427"),
        "오리" => Some("1f986"),
        "병아리" => Some("1f424"),
        "부엉이" => Some("1f989"),
        "독수리" => Some("1f985"),
        "거북이" => Some("1f422"),
        "돌고래" => Some("1f42c"),
        "상어" => Some("1f988"),
        "고래" => Some("1f433"),
        "악어" => Some("1f40a"),
        "뱀" => Some("1f40d"),
        "나비" => Some("1f98b"),
        "벌" => Some("1f41d"),
        "개미" => Some("1f41c"),
        "달팽이" => Some("1f40c"),
        "문어" => Some("1f419"),
        "물고기" => Some("1f41f"),
        "게" => Some("1f980"),
        "새우" => Some("1f990"),
        "오징어" => Some("1f991"),
        "말" => Some("1f434"),
        "얼룩말" => Some("1f993"),
        "소" => Some("1f42e"),
        "양" => Some("1f411"),
        "염소" => Some("1f410"),
        "닭" => Some("1f414"),
        "쥐" => Some("1f42d"),
        "햄스터" => Some("1f439"),
        "사슴" => Some("1f98c"),
        "라마" => Some("1f999"),
        "캥거루" => Some("1f998"),
        "하마" => Some("1f99b"),
        "코뿔소" => Some("1f98f"),
        "박쥐" => Some("1f987"),
        "고슴도치" => Some("1f994"),
        "수달" => Some("1f9a6"),
        "비버" => Some("1f9ab"),
        "너구리" => Some("1f99d"),
        "스컹크" => Some("1f9a8"),
        "공작" => Some("1f99a"),
        "앵무새" => Some("1f99c"),
        "백조" => Some("1f9a2"),
        "플라밍고" => Some("1f9a9"),
        "칠면조" => Some("1f983"),
        "고릴라" => Some("1f98d"),
        "오랑우탄" => Some("1f9a7"),
        "물개" => Some("1f9ad"),
        _ => None,
    }
}

pub fn max_player_setting_text(config: &config::BotConfig) -> String {
    if config.max_player_count == 0 {
        format!("제한 없음(봇 최대 {MAX_GAME_PLAYERS}명)")
    } else {
        format!("{}명", effective_max_player_count(config))
    }
}

pub fn permission_overwrite(
    kind: serenity::PermissionOverwriteType,
    can_view: bool,
    can_chat: bool,
    can_create_threads: bool,
) -> serenity::PermissionOverwrite {
    let view_bits =
        serenity::Permissions::VIEW_CHANNEL | serenity::Permissions::READ_MESSAGE_HISTORY;
    let chat_bits = serenity::Permissions::SEND_MESSAGES
        | serenity::Permissions::SEND_MESSAGES_IN_THREADS
        | serenity::Permissions::ADD_REACTIONS;
    let thread_bits = serenity::Permissions::CREATE_PUBLIC_THREADS
        | serenity::Permissions::CREATE_PRIVATE_THREADS;

    let mut allow = serenity::Permissions::empty();
    let mut deny = serenity::Permissions::empty();
    if can_view {
        allow |= view_bits;
    } else {
        deny |= view_bits;
    }
    if can_chat {
        allow |= chat_bits;
    } else {
        deny |= chat_bits;
    }
    if can_chat && can_create_threads {
        allow |= thread_bits;
    } else {
        deny |= thread_bits;
    }

    serenity::PermissionOverwrite { allow, deny, kind }
}

pub fn set_chat_permission_bits(overwrite: &mut serenity::PermissionOverwrite, can_chat: bool) {
    let chat_bits = serenity::Permissions::SEND_MESSAGES
        | serenity::Permissions::SEND_MESSAGES_IN_THREADS
        | serenity::Permissions::ADD_REACTIONS;
    let thread_bits = serenity::Permissions::CREATE_PUBLIC_THREADS
        | serenity::Permissions::CREATE_PRIVATE_THREADS;
    let bits = chat_bits | thread_bits;
    overwrite.allow.remove(bits);
    overwrite.deny.remove(bits);
    if can_chat {
        overwrite.allow |= bits;
    } else {
        overwrite.deny |= bits;
    }
}

fn permission_cache_key(
    channel_id: serenity::ChannelId,
    kind: serenity::PermissionOverwriteType,
) -> Option<(u64, u64, bool)> {
    match kind {
        serenity::PermissionOverwriteType::Member(user_id) => {
            Some((channel_id.get(), user_id.get(), true))
        }
        serenity::PermissionOverwriteType::Role(role_id) => {
            Some((channel_id.get(), role_id.get(), false))
        }
        _ => None,
    }
}

fn remember_channel_permissions(
    running: &mut RunningGame,
    channel_id: serenity::ChannelId,
    overwrites: &[serenity::PermissionOverwrite],
) {
    for overwrite in overwrites {
        if let Some(key) = permission_cache_key(channel_id, overwrite.kind) {
            running
                .permission_overwrite_cache
                .insert(key, overwrite.clone());
        }
    }
}

fn remembered_permission(
    running: &RunningGame,
    channel_id: serenity::ChannelId,
    kind: serenity::PermissionOverwriteType,
) -> Option<serenity::PermissionOverwrite> {
    permission_cache_key(channel_id, kind)
        .and_then(|key| running.permission_overwrite_cache.get(&key).cloned())
}

async fn apply_permission_if_changed(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    channel_id: serenity::ChannelId,
    overwrite: serenity::PermissionOverwrite,
) -> bool {
    let key = permission_cache_key(channel_id, overwrite.kind);
    if let Some(key) = key {
        let running_read = running.read().await;
        if running_read.permission_overwrite_cache.get(&key) == Some(&overwrite) {
            return true;
        }
    }

    match crate::http_pool::with_fallback(ctx, |http| {
        let overwrite = overwrite.clone();
        async move { channel_id.create_permission(&http, overwrite).await }
    })
    .await
    {
        Ok(()) => {
            if let Some(key) = key {
                running
                    .write()
                    .await
                    .permission_overwrite_cache
                    .insert(key, overwrite);
            }
            true
        }
        Err(error) => {
            eprintln!(
                "failed to apply channel permission: channel_id={} kind={:?} error={error:?}",
                channel_id.get(),
                overwrite.kind
            );
            false
        }
    }
}

async fn apply_permission_updates(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    updates: Vec<(serenity::ChannelId, serenity::PermissionOverwrite)>,
) {
    for chunk in updates.chunks(DISCORD_WRITE_CONCURRENCY) {
        let mut jobs = JoinSet::new();
        for (channel_id, overwrite) in chunk.iter().cloned() {
            let ctx = ctx.clone();
            let running = Arc::clone(running);
            jobs.spawn(async move {
                apply_permission_if_changed(&ctx, &running, channel_id, overwrite).await;
            });
        }
        while jobs.join_next().await.is_some() {}
    }
}

async fn delete_permission_and_invalidate(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    channel_id: serenity::ChannelId,
    kind: serenity::PermissionOverwriteType,
) -> bool {
    match crate::http_pool::with_fallback(ctx, |http| async move {
        channel_id.delete_permission(&http, kind).await
    })
    .await
    {
        Ok(()) => {
            if let Some(key) = permission_cache_key(channel_id, kind) {
                running
                    .write()
                    .await
                    .permission_overwrite_cache
                    .remove(&key);
            }
            true
        }
        Err(error) => {
            eprintln!(
                "failed to delete channel permission: channel_id={} kind={kind:?} error={error:?}",
                channel_id.get()
            );
            false
        }
    }
}

pub fn private_channel_overwrite(
    kind: serenity::PermissionOverwriteType,
    can_chat: bool,
) -> serenity::PermissionOverwrite {
    permission_overwrite(kind, can_chat, can_chat, can_chat)
}

pub fn dead_channel_overwrite(
    kind: serenity::PermissionOverwriteType,
    can_view: bool,
    can_chat: bool,
) -> serenity::PermissionOverwrite {
    permission_overwrite(kind, can_view, can_chat, can_chat)
}

pub fn anonymous_input_overwrite(
    kind: serenity::PermissionOverwriteType,
    can_view: bool,
    can_chat: bool,
) -> serenity::PermissionOverwrite {
    permission_overwrite(kind, can_view, can_chat, false)
}

pub fn spectator_channel_overwrite(role_id: serenity::RoleId) -> serenity::PermissionOverwrite {
    permission_overwrite(
        serenity::PermissionOverwriteType::Role(role_id),
        true,
        false,
        false,
    )
}

pub async fn channel_role_ids(
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    config: &config::BotConfig,
    bot_user_id: serenity::UserId,
) -> Result<ChannelRoleIds> {
    let roles = guild_id.roles(&ctx.http).await?;
    let find_role = |name: &str| {
        roles
            .values()
            .find(|role| role.name == name)
            .map(|role| role.id)
    };
    Ok(ChannelRoleIds {
        everyone: guild_id.everyone_role(),
        participant: find_role(&config.participant_role),
        spectator: find_role(SPECTATOR_ROLE),
        manager: find_role(&config.manager_role),
        dead: find_role(DEAD_PLAYER_ROLE),
        bot: bot_user_id,
    })
}

pub fn add_common_hidden_overwrites(
    overwrites: &mut Vec<serenity::PermissionOverwrite>,
    roles: ChannelRoleIds,
    private: bool,
) {
    overwrites.push(private_channel_overwrite(
        serenity::PermissionOverwriteType::Role(roles.everyone),
        false,
    ));
    if let Some(role_id) = roles.participant {
        overwrites.push(private_channel_overwrite(
            serenity::PermissionOverwriteType::Role(role_id),
            false,
        ));
    }
    if let Some(role_id) = roles.spectator {
        overwrites.push(spectator_channel_overwrite(role_id));
    }
    if let Some(role_id) = roles.manager {
        overwrites.push(private_channel_overwrite(
            serenity::PermissionOverwriteType::Role(role_id),
            false,
        ));
    }
    overwrites.push(if private {
        private_channel_overwrite(serenity::PermissionOverwriteType::Member(roles.bot), true)
    } else {
        anonymous_input_overwrite(
            serenity::PermissionOverwriteType::Member(roles.bot),
            true,
            true,
        )
    });
}

pub fn anonymous_base_overwrites(
    roles: ChannelRoleIds,
    participant_can_view: bool,
    participant_can_chat: bool,
    default_can_view: bool,
    default_can_chat: bool,
) -> Vec<serenity::PermissionOverwrite> {
    let mut overwrites = vec![anonymous_input_overwrite(
        serenity::PermissionOverwriteType::Role(roles.everyone),
        default_can_view,
        default_can_chat,
    )];
    if let Some(role_id) = roles.participant {
        overwrites.push(anonymous_input_overwrite(
            serenity::PermissionOverwriteType::Role(role_id),
            participant_can_view,
            participant_can_chat,
        ));
    }
    if let Some(role_id) = roles.spectator {
        overwrites.push(spectator_channel_overwrite(role_id));
    }
    if let Some(role_id) = roles.manager {
        overwrites.push(anonymous_input_overwrite(
            serenity::PermissionOverwriteType::Role(role_id),
            false,
            false,
        ));
    }
    overwrites.push(anonymous_input_overwrite(
        serenity::PermissionOverwriteType::Member(roles.bot),
        true,
        true,
    ));
    overwrites
}

pub async fn source_category(
    ctx: &serenity::Context,
    channel_id: serenity::ChannelId,
) -> Option<serenity::ChannelId> {
    let channel = channel_id.to_channel(&ctx.http).await.ok()?.guild()?;
    match channel.kind {
        serenity::ChannelType::PublicThread
        | serenity::ChannelType::PrivateThread
        | serenity::ChannelType::NewsThread => {
            let parent_id = channel.parent_id?;
            parent_id
                .to_channel(&ctx.http)
                .await
                .ok()?
                .guild()?
                .parent_id
        }
        _ => channel.parent_id,
    }
}

pub async fn running_source_category(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
) -> Option<serenity::ChannelId> {
    if let Some(category_id) = running.read().await.source_category_id {
        return category_id;
    }
    let channel_id = running.read().await.channel_id;
    let category_id = source_category(ctx, channel_id).await;
    let mut running_write = running.write().await;
    if running_write.source_category_id.is_none() {
        running_write.source_category_id = Some(category_id);
    }
    running_write.source_category_id.flatten()
}

async fn verify_game_member(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    user_id: u64,
) -> serenity::Result<()> {
    if running.read().await.verified_member_ids.contains(&user_id) {
        return Ok(());
    }
    let guild_id = running.read().await.guild_id;
    guild_id.member(ctx, serenity::UserId::new(user_id)).await?;
    running.write().await.verified_member_ids.insert(user_id);
    Ok(())
}

async fn personal_channel_creation_lock(
    running: &Arc<RwLock<RunningGame>>,
    user_id: u64,
    kind: PersonalChannelKind,
) -> Arc<tokio::sync::Mutex<()>> {
    running
        .write()
        .await
        .personal_channel_creation_locks
        .entry((user_id, kind))
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

#[allow(clippy::too_many_arguments)]
pub async fn create_text_channel_safe(
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    name: &str,
    overwrites: Vec<serenity::PermissionOverwrite>,
    category: Option<serenity::ChannelId>,
    reason: &'static str,
    slowmode_delay: u64,
    topic: Option<String>,
) -> Option<serenity::GuildChannel> {
    let slowmode = slowmode_delay.min(21600) as u16;

    // 채널 생성은 워커 토큰으로 우회하되, 실패 시 메인 토큰으로 폴백한다.
    // 빌더는 호출마다 소비되므로 with_fallback 클로저 안에서 매번 새로 만든다.
    let create_with = |with_category: bool| {
        let overwrites = overwrites.clone();
        let topic = topic.clone();
        crate::http_pool::with_fallback(ctx, move |http| {
            let overwrites = overwrites.clone();
            let topic = topic.clone();
            async move {
                let mut builder = serenity::CreateChannel::new(name)
                    .kind(serenity::ChannelType::Text)
                    .permissions(overwrites)
                    .rate_limit_per_user(slowmode)
                    .audit_log_reason(reason);
                if with_category && let Some(category_id) = category {
                    builder = builder.category(category_id);
                }
                if let Some(topic) = topic {
                    builder = builder.topic(topic.chars().take(1024).collect::<String>());
                }
                guild_id.create_channel(&http, builder).await
            }
        })
    };

    match create_with(true).await {
        Ok(channel) => Some(channel),
        Err(primary_error) if category.is_some() => match create_with(false).await {
            Ok(channel) => Some(channel),
            Err(fallback_error) => {
                eprintln!(
                    "failed to create game channel: guild_id={} name={name:?} category_id={:?} reason={reason:?} primary_error={primary_error:?} fallback_error={fallback_error:?}",
                    guild_id.get(),
                    category.map(|id| id.get()),
                );
                None
            }
        },
        Err(error) => {
            eprintln!(
                "failed to create game channel: guild_id={} name={name:?} category_id=None reason={reason:?} error={error:?}",
                guild_id.get(),
            );
            None
        }
    }
}

async fn find_text_channel_by_name(
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    name: &str,
    category: Option<serenity::ChannelId>,
) -> Option<serenity::GuildChannel> {
    let channels = guild_id.channels(&ctx.http).await.ok()?;
    channels.into_values().find(|channel| {
        channel.kind == serenity::ChannelType::Text
            && channel.name == name
            && category.is_none_or(|category| channel.parent_id == Some(category))
    })
}

pub fn status_display_name(running: &RunningGame, player: &Player) -> String {
    if running.anonymous_enabled {
        running
            .anonymous_aliases
            .get(&player.user_id)
            .cloned()
            .unwrap_or_else(|| player.name.clone())
    } else {
        player.name.clone()
    }
}

pub fn mafia_night_target_status_text(running: &RunningGame) -> String {
    if running.game.phase != Phase::Night {
        return String::new();
    }
    let mut actors = running
        .game
        .players
        .iter()
        .filter(|player| {
            player.alive
                && (player.role == Role::Mafia
                    || (player.role == Role::Thief
                        && running.game.thief_night_role(player) == Some(Role::Mafia)))
                && running.game.can_mafia_attack(player, None)
        })
        .cloned()
        .collect::<Vec<_>>();
    if actors.is_empty() {
        return String::new();
    }
    actors.sort_by_key(|player| status_display_name(running, player).to_lowercase());
    let mut lines = vec!["마피아 처치 선택 현황".to_string()];
    for actor in actors {
        let target = running
            .game
            .mafia_display_targets
            .get(&actor.user_id)
            .or_else(|| running.game.mafia_targets.get(&actor.user_id))
            .and_then(|target_id| running.game.get_player(*target_id));
        let target_name = target
            .map(|target| status_display_name(running, target))
            .unwrap_or_else(|| "미선택".to_string());
        lines.push(format!(
            "- {} → {}",
            status_display_name(running, &actor),
            target_name
        ));
    }
    lines.join("\n")
}

pub fn assign_anonymous_aliases(running: &mut RunningGame, config: &config::BotConfig) {
    let mut players = running
        .game
        .players
        .iter()
        .map(|player| player.user_id)
        .collect::<Vec<_>>();
    players.sort_unstable();

    let mut aliases = if normalized_anonymous_name_mode(config) == "number" {
        (1..=players.len())
            .map(|index| format!("{index}번"))
            .collect::<Vec<_>>()
    } else {
        ANIMAL_ALIASES
            .iter()
            .map(|alias| (*alias).to_string())
            .collect::<Vec<_>>()
    };
    aliases.shuffle(&mut system_random::rng());
    running.anonymous_aliases = players
        .into_iter()
        .enumerate()
        .map(|(index, user_id)| {
            (
                user_id,
                aliases
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| format!("{}번", index + 1)),
            )
        })
        .collect();
}

pub fn apply_anonymous_player_names(running: &mut RunningGame) {
    if !running.anonymous_enabled {
        return;
    }
    if running.anonymous_original_names.is_empty() {
        running.anonymous_original_names = running
            .game
            .players
            .iter()
            .map(|player| (player.user_id, player.name.clone()))
            .collect();
    }
    for player in &mut running.game.players {
        if let Some(alias) = running.anonymous_aliases.get(&player.user_id) {
            player.name.clone_from(alias);
        }
    }
}

pub fn lover_chat_is_open(game: &MafiaGame) -> bool {
    game.phase == Phase::Night
        && game
            .alive_players()
            .into_iter()
            .filter(|player| player.role == Role::Lover && !game.is_frog(player))
            .count()
            >= 2
}

pub fn can_use_anonymous_general_chat(running: &RunningGame, player: &Player) -> bool {
    if !player.alive {
        return false;
    }
    // 최후변론 대상자는 마담에게 유혹당했어도 자신의 변론은 할 수 있어야 한다.
    // (개구리 저주는 말 자체를 잃는 상태라 그대로 막는다.)
    if running.game.phase == Phase::FinalDefense
        && running.final_defense_user_id == Some(player.user_id)
    {
        return !running.game.is_frog(player);
    }
    // [확성] 보유자는 밤에도 전체 채팅을 쓸 수 있다.
    if running.game.phase == Phase::Night && running.game.is_loudspeaker_active(player) {
        return true;
    }
    if is_player_chat_silenced(running, player) {
        return false;
    }
    running.game.phase == Phase::Day && running.day_chat_open
}

pub fn is_player_chat_silenced(running: &RunningGame, player: &Player) -> bool {
    running.game.is_frog(player) || running.game.is_madam_seduced(player)
}

pub fn can_use_anonymous_role_chat(running: &RunningGame, player: &Player, role: Role) -> bool {
    if running.game.phase != Phase::Night {
        return false;
    }
    if running.game.is_frog(player) || running.game.is_madam_seduced(player) {
        return false;
    }
    if role == Role::Lover {
        return player.alive && player.role == Role::Lover && lover_chat_is_open(&running.game);
    }
    if player.alive
        && running
            .anonymous_role_input_channel_ids
            .contains_key(&(player.user_id, role))
    {
        return true;
    }
    if role == Role::Mafia {
        return player.alive && running.game.is_known_mafia_team(player);
    }
    player.alive && player.role == role
}

pub fn private_role_member_can_view(game: &MafiaGame, role: Role, player: &Player) -> bool {
    let pending_scientist_revive = role == Role::Mafia
        && player.role == Role::Scientist
        && game.scientist_contacted.contains(&player.user_id)
        && game.scientist_pending_revive_ids.contains(&player.user_id);
    if (!player.alive && !pending_scientist_revive)
        || game.is_frog(player)
        || game.is_madam_seduced(player)
    {
        return false;
    }
    match role {
        Role::Mafia => game.is_known_mafia_team(player),
        Role::Doctor => {
            player.role == Role::Doctor
                || (player.role == Role::Nurse && game.nurse_contacted.contains(&player.user_id))
        }
        Role::CultLeader => game.is_cult_team(player),
        Role::Lover => player.role == Role::Lover && lover_chat_is_open(game),
        _ => player.role == role,
    }
}

pub fn private_role_member_can_chat(game: &MafiaGame, role: Role, player: &Player) -> bool {
    if game.phase != Phase::Night
        || !player.alive
        || !private_role_member_can_view(game, role, player)
    {
        return false;
    }
    if role == Role::Lover {
        return player.role == Role::Lover && lover_chat_is_open(game);
    }
    if role == Role::CultLeader {
        return player.role == Role::CultLeader;
    }
    true
}

pub fn can_use_anonymous_dead_chat(running: &RunningGame, player: &Player) -> bool {
    !player.alive
        && running.dead_chat_unlocked_ids.contains(&player.user_id)
        && !running.game.purified_dead_ids.contains(&player.user_id)
}

pub fn can_receive_role_chat_as_dead(running: &RunningGame, player: &Player) -> bool {
    can_use_anonymous_dead_chat(running, player)
        && running.game.phase == Phase::Night
        && running
            .dead_role_chat_visible_from_days
            .get(&player.user_id)
            .is_none_or(|visible_from_day| running.game.day_number >= *visible_from_day)
}

pub fn can_use_anonymous_shaman_chat(running: &RunningGame, player: &Player) -> bool {
    if !player.alive {
        return can_use_anonymous_dead_chat(running, player);
    }
    player.role == Role::Shaman
        && running.game.phase == Phase::Night
        && !running.game.is_frog(player)
        && !running.game.is_madam_seduced(player)
}

fn record_dead_chat_deaths(running: &mut RunningGame, dead_players: &[Player]) {
    let unlock_now = running.game.phase == Phase::Day;
    let role_chat_visible_from_day = running.game.day_number.saturating_add(1);
    for player in dead_players {
        running
            .dead_role_chat_visible_from_days
            .entry(player.user_id)
            .or_insert(role_chat_visible_from_day);
        if unlock_now {
            running.pending_dead_chat_user_ids.remove(&player.user_id);
            running.dead_chat_unlocked_ids.insert(player.user_id);
        } else if !running.dead_chat_unlocked_ids.contains(&player.user_id) {
            running.pending_dead_chat_user_ids.insert(player.user_id);
        }
    }
}

fn dead_chat_unlock_candidates(running: &RunningGame) -> Vec<Player> {
    if !matches!(running.game.phase, Phase::Day | Phase::Night) {
        return Vec::new();
    }

    let mut user_ids = running
        .pending_dead_chat_user_ids
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    user_ids.extend(
        running
            .dead_chat_unlocked_ids
            .iter()
            .filter(|user_id| {
                !running
                    .anonymous_dead_input_channel_ids
                    .contains_key(user_id)
            })
            .copied(),
    );

    let mut players = user_ids
        .into_iter()
        .filter_map(|user_id| running.game.get_player(user_id).cloned())
        .filter(|player| !player.alive)
        .collect::<Vec<_>>();
    players.sort_by_key(|player| player.user_id);
    players
}

pub fn role_chat_player_ids(game: &MafiaGame, role: Role) -> Vec<u64> {
    game.alive_players()
        .into_iter()
        .filter(|player| {
            if role == Role::Mafia {
                game.is_known_mafia_team(player)
            } else {
                player.role == role
            }
        })
        .map(|player| player.user_id)
        .collect()
}

pub fn anonymous_role_status_player_ids(running: &RunningGame, role: Role) -> Vec<u64> {
    let granted_ids = running
        .anonymous_role_input_channel_ids
        .keys()
        .filter_map(|(user_id, granted_role)| (*granted_role == role).then_some(*user_id))
        .collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let mut players = running
        .game
        .alive_players()
        .into_iter()
        .filter(|player| !running.game.is_frog(player))
        .filter(|player| {
            granted_ids.contains(&player.user_id)
                || (role == Role::Mafia && running.game.is_known_mafia_team(player))
                || (role == Role::CultLeader && running.game.is_cult_team(player))
                || player.role == role
        })
        .filter(|player| seen.insert(player.user_id))
        .map(|player| player.user_id)
        .collect::<Vec<_>>();
    players.sort_by_key(|user_id| {
        running
            .game
            .get_player(*user_id)
            .map(|player| status_display_name(running, player).to_lowercase())
            .unwrap_or_default()
    });
    players
}

pub fn role_status_player_ids(running: &RunningGame, role: Role) -> Vec<u64> {
    if running.anonymous_enabled {
        anonymous_role_status_player_ids(running, role)
    } else {
        role_chat_player_ids(&running.game, role)
    }
}

pub fn should_create_private_role_channel(game: &MafiaGame, role: Role) -> bool {
    game.players.iter().any(|player| player.role == role)
        || (role == Role::Mafia
            && game
                .players
                .iter()
                .any(|player| player.role.is_mafia_team() && player.role != Role::Villain))
}

pub fn special_role_rule_text(role: Role) -> String {
    if role == Role::Lover {
        return "연인은 두 명이 함께 배정됩니다.\n연인 대화방은 밤에만 열리며, 두 연인이 모두 생존 중일 때 사용할 수 있습니다."
            .to_string();
    }
    let action = match role {
        Role::Mafia => "공격",
        Role::Doctor => "보호",
        Role::Police => "조사",
        Role::Agent => "공작",
        Role::Vigilante => "숙청",
        _ => "행동",
    };
    format!(
        "{}가 여러 명이면 같은 대상이 살아있는 {} 인원의 과반 이상을 받아야 {action}이 행사됩니다.\n동률이거나 과반에 못 미치면 그 밤 행동은 행사되지 않습니다.",
        role.value(),
        role.value()
    )
}

pub async fn require_manager(ctx: Context<'_>) -> Result<bool, Error> {
    let Some(guild_id) = ctx.guild_id() else {
        reply_embed(
            ctx,
            "서버 안에서만 사용할 수 있습니다.",
            "권한 오류",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(false);
    };
    let manager_role = ctx.data().config.read().await.manager_role.clone();
    let member = guild_id
        .member(ctx.serenity_context(), ctx.author().id)
        .await?;
    let roles = guild_id.roles(ctx.serenity_context()).await?;
    let allowed = member.roles.iter().any(|role_id| {
        roles
            .get(role_id)
            .is_some_and(|role| role.name == manager_role)
    });
    if !allowed {
        reply_embed(
            ctx,
            format!("'{manager_role}' 역할을 가진 사람만 사용할 수 있습니다."),
            "권한 오류",
            serenity::Colour::RED,
            true,
        )
        .await?;
    }
    Ok(allowed)
}

#[cfg(test)]
pub(crate) mod tests;

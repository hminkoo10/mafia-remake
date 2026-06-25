// 역할: Discord 채널 생성·권한 관리, 익명 채널, 비공개 역할 채널, 멤버 접근 동기화,
//        게임 상태 메시지 업서트, 사망 처리, 청부 결과, 정화 효과

#![allow(unused_imports, clippy::too_many_arguments, clippy::collapsible_if)]

use super::{
    Context, Data, Error, RunningGame, Recruitment, ContractorContractDraft,
    PRIVATE_CHAT_ROLES, GAME_NOTIFICATION_ROLE, SPECTATOR_ROLE, DEAD_PLAYER_ROLE,
    SHAMAN_CHAT_CHANNEL_NAME, FROG_CHAT_CHANNEL_NAME,
    MAX_GAME_PLAYERS, RECRUITMENT_SECONDS,
};
use crate::embed::*;
use anyhow::{Context as AnyhowContext, Result, bail};
use mafia_remake::config;
use mafia_remake::game::MafiaGame;
use mafia_remake::model::{
    CITIZEN_SPECIAL_ROLES, CONTRACTOR_GUESS_ROLES, MAFIA_SPECIAL_ROLES, NEUTRAL_SPECIAL_ROLES,
    PUBLIC_CITIZEN_SPECIAL_ROLES, PUBLIC_CULT_SPECIAL_ROLES, PUBLIC_MAFIA_SPECIAL_ROLES,
    PUBLIC_NEUTRAL_SPECIAL_ROLES,
    ConfirmVoteResult, NightResult, Phase, Player, Role, VoteResult, Winner,
};
use mafia_remake::stats;
use poise::serenity_prelude as serenity;
use poise::serenity_prelude::Mentionable;
use rand::seq::{IndexedRandom, SliceRandom};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Notify, RwLock};


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

#[derive(Clone, Copy)]
pub(crate) struct ChannelRoleIds {
    everyone: serenity::RoleId,
    participant: Option<serenity::RoleId>,
    spectator: Option<serenity::RoleId>,
    manager: Option<serenity::RoleId>,
    dead: Option<serenity::RoleId>,
    bot: serenity::UserId,
}

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
    let mut builder = serenity::CreateChannel::new(name)
        .kind(serenity::ChannelType::Text)
        .permissions(overwrites.clone())
        .rate_limit_per_user(slowmode)
        .audit_log_reason(reason);
    if let Some(category_id) = category {
        builder = builder.category(category_id);
    }
    if let Some(topic) = topic.clone() {
        builder = builder.topic(topic.chars().take(1024).collect::<String>());
    }

    match guild_id.create_channel(&ctx.http, builder).await {
        Ok(channel) => Some(channel),
        Err(_) if category.is_some() => {
            let mut fallback = serenity::CreateChannel::new(name)
                .kind(serenity::ChannelType::Text)
                .permissions(overwrites)
                .rate_limit_per_user(slowmode)
                .audit_log_reason(reason);
            if let Some(topic) = topic {
                fallback = fallback.topic(topic.chars().take(1024).collect::<String>());
            }
            guild_id.create_channel(&ctx.http, fallback).await.ok()
        }
        Err(_) => None,
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
                && player.role == Role::Mafia
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
    aliases.shuffle(&mut rand::rng());
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
    if !player.alive || running.game.is_frog(player) || running.game.is_madam_seduced(player) {
        return false;
    }
    if running.game.phase == Phase::Day && running.day_chat_open {
        return true;
    }
    running.game.phase == Phase::FinalDefense
        && running.final_defense_user_id == Some(player.user_id)
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

pub fn private_role_member_can_chat(game: &MafiaGame, role: Role, player: &Player) -> bool {
    if game.phase != Phase::Night
        || !player.alive
        || game.is_frog(player)
        || game.is_madam_seduced(player)
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
    !player.alive && !running.game.purified_dead_ids.contains(&player.user_id)
}

pub fn can_use_anonymous_shaman_chat(running: &RunningGame, player: &Player) -> bool {
    if !player.alive {
        return !running.game.purified_dead_ids.contains(&player.user_id);
    }
    player.role == Role::Shaman
        && running.game.phase == Phase::Night
        && !running.game.is_frog(player)
        && !running.game.is_madam_seduced(player)
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

pub fn is_blacklisted(config: &config::BotConfig, user_id: u64) -> bool {
    config.blacklist_user_ids.contains(&user_id)
}

pub fn enabled_special_roles(config: &config::BotConfig, pool: &[Role]) -> Vec<Role> {
    pool.iter()
        .copied()
        .filter(|role| match role {
            Role::Detective => config.enable_detective,
            Role::Shaman => config.enable_shaman,
            Role::Graverobber => config.enable_graverobber,
            Role::Spy => config.enable_spy,
            Role::Contractor => config.enable_contractor,
            Role::Witch => config.enable_witch,
            Role::Scientist => config.enable_scientist,
            Role::Madam => config.enable_madam,
            Role::Godfather => config.enable_godfather,
            Role::Joker => config.enable_joker,
            Role::Politician => config.enable_politician,
            Role::Judge => config.enable_judge,
            Role::Reporter => config.enable_reporter,
            Role::Hacker => config.enable_hacker,
            Role::Terrorist => config.enable_terrorist,
            Role::Lover => config.enable_lover,
            Role::Priest => config.enable_priest,
            Role::Soldier => config.enable_soldier,
            Role::Nurse => config.enable_nurse,
            Role::Gangster => config.enable_gangster,
            Role::Prophet => config.enable_prophet,
            Role::Psychologist => config.enable_psychologist,
            Role::Mercenary => config.enable_mercenary,
            Role::Thief => config.enable_thief,
            _ => true,
        })
        .collect()
}

pub fn choose_special_roles(config: &config::BotConfig) -> Result<Vec<Role>> {
    let mut rng = rand::rng();
    let mut selected = Vec::new();
    let mut citizen_candidates = enabled_special_roles(config, CITIZEN_SPECIAL_ROLES);
    citizen_candidates.shuffle(&mut rng);
    let mut citizen_selected = Vec::new();
    if !select_special_roles_for_slots(
        &citizen_candidates,
        config.citizen_special_count as usize,
        &mut citizen_selected,
    ) {
        bail!(
            "활성화된 시민 특수 역할로 설정한 인원 수를 구성할 수 없습니다. 연인은 2명으로 계산됩니다."
        );
    }
    selected.extend(citizen_selected);

    for (pool, count) in [
        (MAFIA_SPECIAL_ROLES, config.mafia_special_count as usize),
        (NEUTRAL_SPECIAL_ROLES, config.neutral_special_count as usize),
    ] {
        let candidates = enabled_special_roles(config, pool);
        if count > candidates.len() {
            bail!(
                "{} 중 활성화된 역할보다 선택할 특수룰 수가 많습니다.",
                pool.iter()
                    .map(|role| role.value())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        selected.extend(candidates.choose_multiple(&mut rng, count).copied());
    }
    Ok(selected)
}

pub fn choose_special_roles_balanced(
    config: &config::BotConfig,
    role_history: &HashMap<Role, i64>,
) -> Result<Vec<Role>> {
    let mut selected = Vec::new();
    let citizen_candidates =
        balanced_special_candidates(config, CITIZEN_SPECIAL_ROLES, role_history);
    let Some(citizen_selected) = select_balanced_special_roles_for_slots(
        &citizen_candidates,
        config.citizen_special_count as usize,
        role_history,
    ) else {
        bail!(
            "활성화된 시민 특수 역할로 설정된 인원 수를 구성할 수 없습니다. 연인은 2명으로 계산합니다."
        );
    };
    selected.extend(citizen_selected);

    for (pool, count) in [
        (MAFIA_SPECIAL_ROLES, config.mafia_special_count as usize),
        (NEUTRAL_SPECIAL_ROLES, config.neutral_special_count as usize),
    ] {
        let candidates = balanced_special_candidates(config, pool, role_history);
        if count > candidates.len() {
            bail!(
                "{} 중 활성화된 역할보다 선택할 특수룰 수가 많습니다.",
                pool.iter()
                    .map(|role| role.value())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        selected.extend(candidates.into_iter().take(count));
    }
    Ok(selected)
}

fn balanced_special_candidates(
    config: &config::BotConfig,
    pool: &[Role],
    role_history: &HashMap<Role, i64>,
) -> Vec<Role> {
    let mut candidates = enabled_special_roles(config, pool);
    candidates.shuffle(&mut rand::rng());
    candidates.sort_by_key(|role| {
        (
            role_history.get(role).copied().unwrap_or(0),
            special_role_player_count(*role),
        )
    });
    candidates
}

fn select_balanced_special_roles_for_slots(
    candidates: &[Role],
    remaining_slots: usize,
    role_history: &HashMap<Role, i64>,
) -> Option<Vec<Role>> {
    fn search(
        candidates: &[Role],
        index: usize,
        remaining_slots: usize,
        role_history: &HashMap<Role, i64>,
        selected: &mut Vec<Role>,
        score: i64,
        best: &mut Option<(i64, Vec<Role>)>,
    ) {
        if remaining_slots == 0 {
            if best.as_ref().is_none_or(|(best_score, _)| score < *best_score) {
                *best = Some((score, selected.clone()));
            }
            return;
        }
        if index >= candidates.len() {
            return;
        }

        let role = candidates[index];
        let slots = special_role_player_count(role);
        if slots <= remaining_slots {
            selected.push(role);
            search(
                candidates,
                index + 1,
                remaining_slots - slots,
                role_history,
                selected,
                score + role_history.get(&role).copied().unwrap_or(0) * slots as i64,
                best,
            );
            selected.pop();
        }
        search(
            candidates,
            index + 1,
            remaining_slots,
            role_history,
            selected,
            score,
            best,
        );
    }

    let mut selected = Vec::new();
    let mut best = None;
    search(
        candidates,
        0,
        remaining_slots,
        role_history,
        &mut selected,
        0,
        &mut best,
    );
    best.map(|(_, roles)| roles)
}

fn select_special_roles_for_slots(
    candidates: &[Role],
    remaining_slots: usize,
    selected: &mut Vec<Role>,
) -> bool {
    if remaining_slots == 0 {
        return true;
    }
    for (index, role) in candidates.iter().enumerate() {
        let slots = special_role_player_count(*role);
        if slots > remaining_slots {
            continue;
        }
        selected.push(*role);
        if select_special_roles_for_slots(&candidates[index + 1..], remaining_slots - slots, selected) {
            return true;
        }
        selected.pop();
    }
    false
}

fn special_role_player_count(role: Role) -> usize {
    if role == Role::Lover { 2 } else { 1 }
}

pub fn expand_special_roles(roles: &[Role]) -> Vec<Role> {
    let mut expanded = Vec::new();
    for role in roles {
        if *role == Role::Lover {
            expanded.extend([Role::Lover, Role::Lover]);
        } else {
            expanded.push(*role);
        }
    }
    expanded
}

pub fn selected_role_counts(
    config: &config::BotConfig,
    special_roles: &[Role],
) -> Result<HashMap<Role, usize>> {
    selected_role_counts_with_history(config, special_roles, None)
}

pub fn selected_role_counts_balanced(
    config: &config::BotConfig,
    special_roles: &[Role],
    role_history: &HashMap<Role, i64>,
) -> Result<HashMap<Role, usize>> {
    selected_role_counts_with_history(config, special_roles, Some(role_history))
}

fn selected_role_counts_with_history(
    config: &config::BotConfig,
    special_roles: &[Role],
    role_history: Option<&HashMap<Role, i64>>,
) -> Result<HashMap<Role, usize>> {
    let mafia_special_count = special_roles
        .iter()
        .filter(|role| role.is_mafia_team())
        .count();
    if mafia_special_count > config.default_mafia_count as usize {
        bail!(
            "마피아 특수룰 수는 전체 마피아 수보다 많을 수 없습니다. 현재 마피아 {}명, 마피아 특수 {}명입니다.",
            config.default_mafia_count,
            mafia_special_count
        );
    }
    if config.default_mafia_count as usize - mafia_special_count < 1 {
        bail!(
            "접선 전 특수 마피아만으로는 게임을 진행할 수 없습니다. 일반 마피아가 최소 1명 필요합니다."
        );
    }
    let mut counts = HashMap::new();
    counts.insert(
        Role::Mafia,
        config.default_mafia_count as usize - mafia_special_count,
    );
    counts.insert(Role::Doctor, config.default_doctor_count as usize);
    if config.default_police_count > 0 {
        let investigation = role_history
            .map(|history| balanced_investigation_role(config, history))
            .unwrap_or_else(|| random_investigation_role(config));
        counts.insert(investigation, config.default_police_count as usize);
    }
    for role in special_roles {
        *counts.entry(*role).or_default() += if *role == Role::Lover { 2 } else { 1 };
    }
    if config.enable_cult_team {
        *counts.entry(Role::CultLeader).or_default() += 1;
        *counts.entry(Role::Fanatic).or_default() += 1;
    }
    Ok(counts)
}

fn balanced_investigation_role(
    config: &config::BotConfig,
    role_history: &HashMap<Role, i64>,
) -> Role {
    let mut candidates = investigation_role_candidates(config);
    candidates.shuffle(&mut rand::rng());
    candidates.sort_by_key(|role| role_history.get(role).copied().unwrap_or(0));
    *candidates.first().unwrap_or(&Role::Police)
}

pub fn random_investigation_role(config: &config::BotConfig) -> Role {
    let candidates = investigation_role_candidates(config);
    let mut rng = rand::rng();
    *candidates.choose(&mut rng).unwrap_or(&Role::Police)
}

fn investigation_role_candidates(config: &config::BotConfig) -> Vec<Role> {
    let mut candidates = vec![Role::Police];
    if config.use_agent {
        candidates.push(Role::Agent);
    }
    if config.use_vigilante {
        candidates.push(Role::Vigilante);
    }
    candidates
}

pub fn minimum_player_count(role_counts: &HashMap<Role, usize>) -> usize {
    let special_count = role_counts.values().sum::<usize>();
    let mafia_count = role_counts
        .iter()
        .filter(|(role, _)| role.is_mafia_team())
        .map(|(_, count)| *count)
        .sum::<usize>();
    3.max(special_count).max(mafia_count * 2 + 1)
}

pub fn effective_max_player_count(config: &config::BotConfig) -> usize {
    if config.max_player_count == 0 {
        MAX_GAME_PLAYERS
    } else {
        (config.max_player_count as usize).min(MAX_GAME_PLAYERS)
    }
}

pub fn count_group(role_counts: &HashMap<Role, usize>, roles: &[Role]) -> usize {
    roles
        .iter()
        .map(|role| role_counts.get(role).copied().unwrap_or(0))
        .sum()
}

pub fn public_role_count_text_from_counts(
    role_counts: &HashMap<Role, usize>,
    total_players: Option<usize>,
) -> String {
    let mafia_special = count_group(role_counts, PUBLIC_MAFIA_SPECIAL_ROLES);
    let mafia_total = role_counts.get(&Role::Mafia).copied().unwrap_or(0) + mafia_special;
    let doctor_total = role_counts.get(&Role::Doctor).copied().unwrap_or(0);
    let police_total = role_counts.get(&Role::Police).copied().unwrap_or(0);
    let agent_total = role_counts.get(&Role::Agent).copied().unwrap_or(0);
    let vigilante_total = role_counts.get(&Role::Vigilante).copied().unwrap_or(0);
    let citizen_special = count_group(role_counts, PUBLIC_CITIZEN_SPECIAL_ROLES);
    let neutral_special = count_group(role_counts, PUBLIC_NEUTRAL_SPECIAL_ROLES);
    let cult_total = count_group(role_counts, PUBLIC_CULT_SPECIAL_ROLES);
    let citizen_text = if let Some(total_players) = total_players {
        let citizen_total = total_players.saturating_sub(
            mafia_total
                + doctor_total
                + police_total
                + agent_total
                + vigilante_total
                + neutral_special
                + cult_total,
        );
        format!("시민 {citizen_total}명(중 특수 {citizen_special}명)")
    } else {
        format!("시민 변동(중 특수 {citizen_special}명)")
    };
    let mut parts = vec![
        format!("마피아 {mafia_total}명(중 특수 {mafia_special}명)"),
        format!("의사 {doctor_total}명"),
        format!("수사직 {}명", police_total + agent_total + vigilante_total),
        citizen_text,
    ];
    if neutral_special > 0 {
        parts.push(format!("중립 특수 {neutral_special}명"));
    }
    if cult_total > 0 {
        parts.push(format!("교주팀 {cult_total}명"));
    }
    parts.join(", ")
}

pub fn public_role_count_text(game: &MafiaGame) -> String {
    let mut counts = HashMap::new();
    for player in &game.players {
        *counts.entry(player.role).or_default() += 1;
    }
    format!(
        "역할 구성: {}",
        public_role_count_text_from_counts(&counts, Some(game.players.len()))
    )
}

pub fn public_game_settings_text(game: &MafiaGame, config: &config::BotConfig, prefix: &str) -> String {
    format!(
        "{prefix}\n{}\n최대 참가 인원: {}\n교주팀: {}\n사망 시 직업 공개: {}\n경찰 조사 성공 여부 공개: {}\n아침 생존 마피아 수 공개: {}\n채팅 슬로우모드: {}초\n익명 채팅: {}{}",
        public_role_count_text(game),
        max_player_setting_text(config),
        if config.enable_cult_team {
            "켜짐 - 교주 1명, 광신도 1명 필수 배정"
        } else {
            "꺼짐"
        },
        if config.reveal_death_roles {
            "공개"
        } else {
            "비공개"
        },
        if config.reveal_public_police_status {
            "공개"
        } else {
            "비공개"
        },
        if config.reveal_morning_mafia_count {
            "공개"
        } else {
            "비공개"
        },
        config.chat_slowmode_seconds,
        if config.anonymous_mode {
            "켜짐"
        } else {
            "꺼짐"
        },
        if config.anonymous_mode {
            format!(" ({})", anonymous_name_mode_text(config))
        } else {
            String::new()
        }
    )
}

pub fn game_rule_text(
    game: &MafiaGame,
    config: &config::BotConfig,
    reveal_death_roles: bool,
) -> String {
    let death_rule = if reveal_death_roles {
        "사망자의 직업은 즉시 공개됩니다."
    } else {
        "사망자의 직업은 즉시 공개되지 않습니다."
    };
    format!(
        "{}\n\n게임은 밤과 낮을 반복합니다.\n- 역할 설명: 전체 역할 설명은 `/역할설명`, 본인 역할 설명은 `/마피아능력`으로 확인할 수 있습니다.\n- 밤: 게임 채널 채팅과 반응이 비활성화되고, 밤 행동이 있는 역할은 DM으로 행동합니다.\n- 낮: 생존자는 자유롭게 토론합니다. 생존자 과반이 `바로 투표`를 누르면 토론을 끝내고 지목 투표로 넘어갑니다. 시간이 끝나면 생존자 과반으로 1분 연장을 정할 수 있고, 연장은 낮마다 1번만 가능합니다.\n- 마피아 수 공개: 아침 생존 마피아 수는 {}.\n- 투표: 생존자는 최후변론에 세울 사람 또는 스킵을 선택합니다. 지목자는 20초 동안 혼자 최후변론을 하고, 이후 찬반투표 과반 결과를 따릅니다.\n- 경찰 공개: 조사 성공 여부는 {}. 실제 조사 결과는 경찰에게만 전달됩니다.\n- 채팅: 낮 토론 슬로우모드는 {}초이며 최후변론 중에는 해제됩니다.\n- 사망자: {death_rule} 게임 채널 채팅/반응 권한은 제거되고 '{DEAD_PLAYER_ROLE}' 역할이 부여됩니다.\n\n승리 조건\n- 시민 진영: 모든 마피아를 제거하면 승리합니다.\n- 마피아 진영: 생존 마피아 수가 나머지 생존자 수 이상이면 승리합니다.\n- 교주팀: 교주팀 생존자가 비교주팀 생존자 이상이면 승리합니다.\n- 조커: 낮 투표로 처형되면 즉시 단독 승리합니다.",
        public_role_count_text(game),
        if config.reveal_morning_mafia_count {
            "공개됩니다"
        } else {
            "공개되지 않습니다"
        },
        if config.reveal_public_police_status {
            "공개됩니다"
        } else {
            "공개되지 않습니다"
        },
        config.chat_slowmode_seconds
    )
}

pub fn enabled_special_role_names(config: &config::BotConfig) -> String {
    let roles = [
        Role::Detective,
        Role::Shaman,
        Role::Graverobber,
        Role::Spy,
        Role::Contractor,
        Role::Thief,
        Role::Witch,
        Role::Scientist,
        Role::Madam,
        Role::Godfather,
        Role::Joker,
        Role::Politician,
        Role::Judge,
        Role::Reporter,
        Role::Hacker,
        Role::Terrorist,
        Role::Lover,
        Role::Priest,
        Role::Soldier,
        Role::Nurse,
        Role::Gangster,
        Role::Prophet,
        Role::Psychologist,
        Role::Mercenary,
        Role::CultLeader,
        Role::Fanatic,
    ]
    .into_iter()
    .filter(|role| match role {
        Role::Detective => config.enable_detective,
        Role::Shaman => config.enable_shaman,
        Role::Graverobber => config.enable_graverobber,
        Role::Spy => config.enable_spy,
        Role::Contractor => config.enable_contractor,
        Role::Thief => config.enable_thief,
        Role::Witch => config.enable_witch,
        Role::Scientist => config.enable_scientist,
        Role::Madam => config.enable_madam,
        Role::Godfather => config.enable_godfather,
        Role::Joker => config.enable_joker,
        Role::Politician => config.enable_politician,
        Role::Judge => config.enable_judge,
        Role::Reporter => config.enable_reporter,
        Role::Hacker => config.enable_hacker,
        Role::Terrorist => config.enable_terrorist,
        Role::Lover => config.enable_lover,
        Role::Priest => config.enable_priest,
        Role::Soldier => config.enable_soldier,
        Role::Nurse => config.enable_nurse,
        Role::Gangster => config.enable_gangster,
        Role::Prophet => config.enable_prophet,
        Role::Psychologist => config.enable_psychologist,
        Role::Mercenary => config.enable_mercenary,
        Role::CultLeader | Role::Fanatic => config.enable_cult_team,
        _ => false,
    })
    .map(|role| role.value())
    .collect::<Vec<_>>();
    if roles.is_empty() {
        "없음".to_string()
    } else {
        roles.join(", ")
    }
}

pub fn investigation_candidates_text(config: &config::BotConfig) -> String {
    let mut candidates = vec!["경찰"];
    if config.use_agent {
        candidates.push("요원");
    }
    if config.use_vigilante {
        candidates.push("자경단원");
    }
    candidates.join(", ")
}

pub fn current_settings_text(config: &config::BotConfig, prefix: &str) -> String {
    format!(
        "{prefix}\n게임 상태: {}\n기본 직업: 마피아 {}명, 의사 {}명, 수사직 {}명\n최대 참가 인원: {}\n특수룰 수: 시민 {}개, 마피아 {}개, 중립 {}개\n활성 특수룰: {}\n수사직 후보: {}\n교주팀: {}\n채팅 슬로우모드: {}초\n사망 시 직업 공개: {}\n경찰 조사 성공 여부 공개: {}\n아침 생존 마피아 수 공개: {}\n익명 채팅: {}\n익명 이름 방식: {}",
        if config.game_enabled {
            "활성화"
        } else {
            "비활성화"
        },
        config.default_mafia_count,
        config.default_doctor_count,
        config.default_police_count,
        max_player_setting_text(config),
        config.citizen_special_count,
        config.mafia_special_count,
        config.neutral_special_count,
        enabled_special_role_names(config),
        investigation_candidates_text(config),
        if config.enable_cult_team {
            "켜짐 - 교주 1명, 광신도 1명 필수 배정"
        } else {
            "꺼짐"
        },
        config.chat_slowmode_seconds,
        if config.reveal_death_roles {
            "공개"
        } else {
            "비공개"
        },
        if config.reveal_public_police_status {
            "공개"
        } else {
            "비공개"
        },
        if config.reveal_morning_mafia_count {
            "공개"
        } else {
            "비공개"
        },
        if config.anonymous_mode {
            "켜짐"
        } else {
            "꺼짐"
        },
        anonymous_name_mode_text(config),
    )
}

pub const RECRUITMENT_STATUS_OPEN: &str = "\u{BAA8}\u{C9D1} \u{C911}\u{C785}\u{B2C8}\u{B2E4}.";
pub const RECRUITMENT_STATUS_CANCELLED: &str =
    "\u{BAA8}\u{C9D1}\u{C774} \u{CDE8}\u{C18C}\u{B418}\u{C5C8}\u{C2B5}\u{B2C8}\u{B2E4}.";

pub fn recruitment_embed(
    recruitment: &Recruitment,
    config: &config::BotConfig,
    status: &str,
) -> serenity::CreateEmbed {
    let mut joined = recruitment
        .joined_names
        .values()
        .cloned()
        .collect::<Vec<_>>();
    joined.sort_by_key(|name| name.to_lowercase());
    let joined_text = if joined.is_empty() {
        "아직 참가자가 없습니다.".to_string()
    } else {
        joined
            .iter()
            .enumerate()
            .map(|(idx, name)| format!("{}. {name}", idx + 1))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut spectators = recruitment
        .spectator_names
        .values()
        .cloned()
        .collect::<Vec<_>>();
    spectators.sort_by_key(|name| name.to_lowercase());
    let spectator_text = if spectators.is_empty() {
        "아직 관전자가 없습니다.".to_string()
    } else {
        spectators
            .iter()
            .enumerate()
            .map(|(idx, name)| format!("{}. {name}", idx + 1))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let shortage = recruitment
        .minimum_players
        .saturating_sub(recruitment.joined_ids.len());
    let minimum_text = if shortage == 0 {
        format!("최소 시작 인원 **{}명** 충족", recruitment.minimum_players)
    } else {
        format!(
            "최소 시작 인원 **{}명**까지 **{}명** 더 필요",
            recruitment.minimum_players, shortage
        )
    };
    let remaining = recruitment
        .max_players
        .saturating_sub(recruitment.joined_ids.len());
    make_embed(
        format!(
            "최대 {RECRUITMENT_SECONDS}초 동안 참가자를 모집합니다.\n참가 버튼을 누르면 게임 참가자로 등록되고, '{}' 역할이 부여됩니다.\n관전 버튼을 누르면 '{SPECTATOR_ROLE}' 역할이 부여되고 게임 채널을 읽을 수 있습니다.\n주최자는 `시작` 버튼으로 즉시 시작하거나 `취소` 버튼으로 모집을 취소할 수 있습니다.\n\n역할 구성: {}\n사망 시 직업 공개: {}\n경찰 조사 성공 여부 공개: {}\n아침 생존 마피아 수 공개: {}\n{}\n\n최대 참가 인원 **{}명**까지 **{}명** 더 참가 가능\n\n현재 참가자 **{}/{}명**\n{}\n\n현재 관전자 **{}명**\n{}\n\n{}",
            config.participant_role,
            public_role_count_text_from_counts(&recruitment.role_counts, None),
            if config.reveal_death_roles {
                "공개"
            } else {
                "비공개"
            },
            if config.reveal_public_police_status {
                "공개"
            } else {
                "비공개"
            },
            if config.reveal_morning_mafia_count {
                "공개"
            } else {
                "비공개"
            },
            minimum_text,
            recruitment.max_players,
            remaining,
            recruitment.joined_ids.len(),
            recruitment.max_players,
            joined_text,
            recruitment.spectator_ids.len(),
            spectator_text,
            status
        ),
        "참가자 모집",
        serenity::Colour::DARK_GREEN,
    )
}

pub fn recruitment_components(
    guild_id: serenity::GuildId,
    disabled: bool,
) -> Vec<serenity::CreateActionRow> {
    let guild_key = guild_id.get();
    vec![serenity::CreateActionRow::Buttons(vec![
        serenity::CreateButton::new(format!("join:{guild_key}"))
            .label("참가")
            .style(serenity::ButtonStyle::Success)
            .disabled(disabled),
        serenity::CreateButton::new(format!("spectate:{guild_key}"))
            .label("관전")
            .style(serenity::ButtonStyle::Secondary)
            .disabled(disabled),
        serenity::CreateButton::new(format!("startnow:{guild_key}"))
            .label("시작")
            .style(serenity::ButtonStyle::Primary)
            .disabled(disabled),
        serenity::CreateButton::new(format!("cancelrec:{guild_key}"))
            .label("취소")
            .style(serenity::ButtonStyle::Danger)
            .disabled(disabled),
    ])]
}

pub async fn update_recruitment_message(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
    recruitment: &Recruitment,
    status: &str,
    disabled: bool,
) {
    let config = data.config.read().await.clone();
    if let Err(error) = component
        .channel_id
        .edit_message(
            &ctx.http,
            component.message.id,
            serenity::EditMessage::new()
                .embed(recruitment_embed(recruitment, &config, status))
                .components(recruitment_components(guild_id, disabled)),
        )
        .await
    {
        eprintln!("failed to update recruitment message: {error:?}");
    }
}
pub async fn setup_game_channels(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) -> Result<()> {
    let config = data.config.read().await.clone();
    let (guild_id, channel_id) = {
        let running_read = running.read().await;
        (running_read.guild_id, running_read.channel_id)
    };
    let roles = channel_role_ids(ctx, guild_id, &config, data.bot_user_id).await?;
    let category = source_category(ctx, channel_id).await;

    set_spectator_game_channel_access(ctx, running, roles).await;
    create_anonymous_chat_channels(ctx, running, &config, roles, category).await?;
    hide_original_game_channel_for_anonymous(ctx, running, roles).await;
    create_private_role_channels(ctx, running, roles, category).await?;
    sync_cult_team_channel_access(ctx, data, running).await;
    create_memo_channels(ctx, running, roles, category).await?;
    create_shaman_chat_channel(ctx, running, roles, category).await?;
    create_frog_chat_channel(ctx, running, roles, category).await?;
    Ok(())
}

pub async fn hide_original_game_channel_for_anonymous(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    roles: ChannelRoleIds,
) {
    let (anonymous_enabled, channel_id) = {
        let running_read = running.read().await;
        (running_read.anonymous_enabled, running_read.channel_id)
    };
    if !anonymous_enabled {
        return;
    }
    let Some(participant_role_id) = roles.participant else {
        return;
    };
    let Some(channel) = channel_id
        .to_channel(&ctx.http)
        .await
        .ok()
        .and_then(|channel| channel.guild())
    else {
        return;
    };
    let original = channel
        .permission_overwrites
        .iter()
        .find(|overwrite| {
            overwrite.kind == serenity::PermissionOverwriteType::Role(participant_role_id)
        })
        .cloned();
    {
        let mut running_write = running.write().await;
        running_write
            .original_game_channel_overwrites
            .entry(participant_role_id)
            .or_insert(original);
    }
    let _ = channel_id
        .create_permission(
            &ctx.http,
            anonymous_input_overwrite(
                serenity::PermissionOverwriteType::Role(participant_role_id),
                false,
                false,
            ),
        )
        .await;
    let _ = channel_id
        .create_permission(
            &ctx.http,
            anonymous_input_overwrite(
                serenity::PermissionOverwriteType::Member(roles.bot),
                true,
                true,
            ),
        )
        .await;
}

pub async fn set_spectator_game_channel_access(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    roles: ChannelRoleIds,
) {
    let Some(spectator_role_id) = roles.spectator else {
        return;
    };
    let channel_id = running.read().await.channel_id;
    let Some(channel) = channel_id
        .to_channel(&ctx.http)
        .await
        .ok()
        .and_then(|channel| channel.guild())
    else {
        return;
    };
    let kind = serenity::PermissionOverwriteType::Role(spectator_role_id);
    let original = channel
        .permission_overwrites
        .iter()
        .find(|overwrite| overwrite.kind == kind)
        .cloned();
    {
        let mut running_write = running.write().await;
        running_write
            .game_channel_overwrites
            .entry(spectator_role_id)
            .or_insert_with(|| original.clone());
    }
    let _ = channel_id
        .create_permission(&ctx.http, spectator_channel_overwrite(spectator_role_id))
        .await;
}

pub async fn create_anonymous_chat_channels(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    config: &config::BotConfig,
    roles: ChannelRoleIds,
    category: Option<serenity::ChannelId>,
) -> Result<()> {
    {
        let mut running_write = running.write().await;
        if !running_write.anonymous_enabled {
            return Ok(());
        }
        assign_anonymous_aliases(&mut running_write, config);
        apply_anonymous_player_names(&mut running_write);
    }

    let players = { running.read().await.game.players.clone() };
    for player in players {
        let (guild_id, alias, can_chat) = {
            let running_read = running.read().await;
            let Some(player_state) = running_read.game.get_player(player.user_id) else {
                continue;
            };
            (
                running_read.guild_id,
                running_read
                    .anonymous_aliases
                    .get(&player.user_id)
                    .cloned()
                    .unwrap_or_else(|| player.name.clone()),
                can_use_anonymous_general_chat(&running_read, player_state),
            )
        };
        if guild_id
            .member(ctx, serenity::UserId::new(player.user_id))
            .await
            .is_err()
        {
            continue;
        }

        let mut overwrites = anonymous_base_overwrites(roles, false, false, false, false);
        overwrites.push(anonymous_input_overwrite(
            serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
            true,
            can_chat,
        ));
        let Some(input_channel) = create_text_channel_safe(
            ctx,
            guild_id,
            &format!("{}-채팅", sanitize_channel_part(&alias)),
            overwrites,
            category,
            "마피아 게임 개인 익명 입력 채널 생성",
            config.chat_slowmode_seconds,
            None,
        )
        .await
        else {
            continue;
        };
        {
            let mut running_write = running.write().await;
            running_write
                .anonymous_input_channel_ids
                .insert(player.user_id, input_channel.id);
            running_write
                .anonymous_input_channel_owners
                .insert(input_channel.id, player.user_id);
        }
        let _ = send_channel_embed(
            &ctx.http,
            input_channel.id,
            format!(
                "당신의 익명 이름은 **{alias}** 입니다.\n이 개인 채널이 일반 채팅을 대체합니다.\n여기에 쓰면 모든 참가자의 개인 채팅방에 익명으로 전달됩니다."
            ),
            "익명 입력 채널",
            serenity::Colour::DARK_GREEN,
            vec![],
        )
        .await;
    }
    Ok(())
}

pub fn role_channel_status_text(running: &RunningGame, role: Role) -> String {
    let mut players = role_status_player_ids(running, role)
        .into_iter()
        .filter_map(|user_id| running.game.get_player(user_id))
        .collect::<Vec<_>>();
    players.sort_by_key(|player| status_display_name(running, player).to_lowercase());
    let mut text = if players.is_empty() {
        "현재 생존: 없음".to_string()
    } else {
        format!(
            "현재 생존: {}",
            players
                .into_iter()
                .map(|player| status_display_name(running, player))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    if role == Role::Mafia {
        let mafia_status = mafia_night_target_status_text(running);
        if !mafia_status.is_empty() {
            text = format!("{text}\n\n{mafia_status}");
        }
    }
    text
}

pub fn status_player_list<'a>(
    running: &RunningGame,
    players: impl IntoIterator<Item = &'a Player>,
) -> String {
    let mut names = players
        .into_iter()
        .map(|player| status_display_name(running, player))
        .collect::<Vec<_>>();
    if names.is_empty() {
        return "없음".to_string();
    }
    names.sort_by_key(|name| name.to_lowercase());
    let shown = names.iter().take(40).cloned().collect::<Vec<_>>();
    let suffix = if names.len() > shown.len() {
        format!(" 외 {}명", names.len() - shown.len())
    } else {
        String::new()
    };
    format!("{}{suffix}", shown.join(", "))
}

pub fn game_status_text(running: &RunningGame) -> String {
    let alive = running.game.alive_players();
    let dead = running.game.dead_players();
    format!(
        "{}일차 / 현재 단계: {}\n생존자 **{}명** / 사망자 **{}명**\n\n생존자 목록\n{}\n\n사망자 목록\n{}",
        running.game.day_number,
        running.game.phase.value(),
        alive.len(),
        dead.len(),
        status_player_list(running, alive.iter().copied()),
        status_player_list(running, dead.iter().copied())
    )
}

pub async fn upsert_game_status(ctx: &serenity::Context, running: &Arc<RwLock<RunningGame>>) {
    let (channel_id, message_id, status_text, unchanged) = {
        let running_read = running.read().await;
        let status_text = game_status_text(&running_read);
        let unchanged = running_read
            .game_status_text
            .as_ref()
            .is_some_and(|cached| cached == &status_text);
        (
            running_read.channel_id,
            running_read.game_status_message_id,
            status_text,
            unchanged,
        )
    };
    if unchanged {
        return;
    }
    if let Some(message_id) = message_id {
        let edit_result = channel_id
            .edit_message(
                &ctx.http,
                message_id,
                serenity::EditMessage::new().embed(make_embed(
                    status_text.clone(),
                    "게임 현황",
                    serenity::Colour::DARK_GREEN,
                )),
            )
            .await;
        if edit_result.is_ok() {
            running.write().await.game_status_text = Some(status_text);
            return;
        }
    }
    if let Ok(message) = send_channel_embed(
        &ctx.http,
        channel_id,
        status_text.clone(),
        "게임 현황",
        serenity::Colour::DARK_GREEN,
        vec![],
    )
    .await
    {
        let mut running_write = running.write().await;
        running_write.game_status_message_id = Some(message.id);
        running_write.game_status_text = Some(status_text);
    }
}

pub fn final_team_text(game: &MafiaGame, player: &Player) -> &'static str {
    if game.is_cult_team(player) {
        "교주팀"
    } else if game.is_mafia_team(player) {
        "마피아팀"
    } else if player.role == Role::Joker {
        "중립"
    } else {
        "시민팀"
    }
}

pub fn final_role_reveal_text(running: &RunningGame) -> String {
    let role_detail = |player: &Player| {
        let state = if player.alive { "" } else { " (사망)" };
        format!(
            "{}{} / 최종 진영: {}",
            player.role.value(),
            state,
            final_team_text(&running.game, player)
        )
    };
    let mut players = running.game.players.clone();
    if running.anonymous_enabled {
        players.sort_by_key(|player| {
            running
                .anonymous_aliases
                .get(&player.user_id)
                .unwrap_or(&player.name)
                .to_lowercase()
        });
        players
            .iter()
            .map(|player| {
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
                format!("- {alias} = {real_name}: {}", role_detail(player))
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        players.sort_by_key(|player| player.name.to_lowercase());
        players
            .iter()
            .map(|player| format!("- {}: {}", player.name, role_detail(player)))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub fn private_role_status_player_ids(running: &RunningGame, player: &Player) -> (String, Vec<u64>) {
    if running.game.is_cult_team(player) {
        return (
            "내 교주팀".to_string(),
            running
                .game
                .players
                .iter()
                .filter(|target| running.game.is_cult_team(target))
                .map(|target| target.user_id)
                .collect(),
        );
    }
    if running.game.is_known_mafia_team(player) {
        return (
            "내 마피아팀".to_string(),
            running
                .game
                .players
                .iter()
                .filter(|target| running.game.is_known_mafia_team(target))
                .map(|target| target.user_id)
                .collect(),
        );
    }
    (
        format!("내 역할({})", player.role.value()),
        running
            .game
            .players
            .iter()
            .filter(|target| target.role == player.role)
            .map(|target| target.user_id)
            .collect(),
    )
}

pub fn command_status_text(running: &RunningGame, requester_id: u64) -> String {
    let message = game_status_text(running);
    let Some(player) = running.game.get_player(requester_id) else {
        return message;
    };
    if !running.anonymous_enabled {
        return message;
    }
    let (label, same_group_ids) = private_role_status_player_ids(running, player);
    let same_group = same_group_ids
        .into_iter()
        .filter_map(|user_id| running.game.get_player(user_id))
        .collect::<Vec<_>>();
    let alive = same_group
        .iter()
        .copied()
        .filter(|target| target.alive)
        .collect::<Vec<_>>();
    let dead = same_group
        .iter()
        .copied()
        .filter(|target| !target.alive)
        .collect::<Vec<_>>();
    format!(
        "{message}\n\n{label} 현황\n생존 **{}명** / 사망 **{}명**\n생존: {}\n사망: {}",
        alive.len(),
        dead.len(),
        status_player_list(running, alive),
        status_player_list(running, dead)
    )
}

pub async fn create_anonymous_role_channels(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    roles: ChannelRoleIds,
    category: Option<serenity::ChannelId>,
) -> Result<Vec<Role>> {
    let mut failed_roles = Vec::new();
    for &role in PRIVATE_CHAT_ROLES {
        let (guild_id, should_create, player_ids, status_text) = {
            let running_read = running.read().await;
            (
                running_read.guild_id,
                should_create_private_role_channel(&running_read.game, role),
                role_chat_player_ids(&running_read.game, role),
                role_channel_status_text(&running_read, role),
            )
        };
        if !should_create {
            continue;
        }
        let mut created_for_role = false;
        for user_id in player_ids {
            let (alias, can_chat) = {
                let running_read = running.read().await;
                let Some(player) = running_read.game.get_player(user_id) else {
                    continue;
                };
                (
                    running_read
                        .anonymous_aliases
                        .get(&user_id)
                        .cloned()
                        .unwrap_or_else(|| player.name.clone()),
                    can_use_anonymous_role_chat(&running_read, player, role),
                )
            };
            if guild_id
                .member(ctx, serenity::UserId::new(user_id))
                .await
                .is_err()
            {
                continue;
            }
            let mut overwrites = anonymous_base_overwrites(roles, false, false, false, false);
            overwrites.push(anonymous_input_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(user_id)),
                true,
                can_chat,
            ));
            let topic = format!("{} 익명 채팅 | {status_text}", role.value());
            let Some(channel) = create_text_channel_safe(
                ctx,
                guild_id,
                &format!("{}-{}-채팅", sanitize_channel_part(&alias), role.value()),
                overwrites,
                category,
                "마피아 게임 역할별 익명 입력 채널 생성",
                0,
                Some(topic.clone()),
            )
            .await
            else {
                continue;
            };
            {
                let mut running_write = running.write().await;
                running_write
                    .anonymous_role_input_channel_ids
                    .insert((user_id, role), channel.id);
                running_write
                    .anonymous_role_input_channels
                    .insert(channel.id, (user_id, role));
                running_write
                    .anonymous_channel_topics
                    .insert(channel.id, topic.chars().take(1024).collect::<String>());
            }
            let _ = send_channel_embed(
                &ctx.http,
                channel.id,
                format!(
                    "{} 전용 익명 입력 채널입니다.\n이곳에 쓰면 같은 {} 채팅 참가자에게 익명으로 전달됩니다.\n\n{}",
                    role.value(),
                    role.value(),
                    special_role_rule_text(role)
                ),
                "역할 익명 채널",
                serenity::Colour::DARK_GREEN,
                vec![],
            )
            .await;
            if let Ok(message) = send_channel_embed(
                &ctx.http,
                channel.id,
                status_text.clone(),
                &format!("{} 채팅 현황", role.value()),
                serenity::Colour::DARK_GREEN,
                vec![],
            )
            .await
            {
                let mut running_write = running.write().await;
                running_write
                    .anonymous_role_input_status_message_ids
                    .insert((user_id, role), message.id);
                running_write
                    .anonymous_role_status_texts
                    .insert((user_id, role), status_text.clone());
            }
            created_for_role = true;
        }
        if !created_for_role && should_create {
            failed_roles.push(role);
        }
    }
    Ok(failed_roles)
}

pub async fn create_private_role_channels(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    roles: ChannelRoleIds,
    category: Option<serenity::ChannelId>,
) -> Result<()> {
    if running.read().await.anonymous_enabled {
        let failed_roles = create_anonymous_role_channels(ctx, running, roles, category).await?;
        if !failed_roles.is_empty() {
            let channel_id = running.read().await.channel_id;
            let _ = send_channel_embed(
                &ctx.http,
                channel_id,
                format!(
                    "익명 역할 개인 채팅방 생성 실패: {}",
                    failed_roles
                        .into_iter()
                        .map(|role| role.value())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                "마피아 게임",
                serenity::Colour::RED,
                vec![],
            )
            .await;
        }
        return Ok(());
    }

    let mut failed_roles = Vec::new();
    for &role in PRIVATE_CHAT_ROLES {
        let (guild_id, should_create, players, status_text) = {
            let running_read = running.read().await;
            (
                running_read.guild_id,
                should_create_private_role_channel(&running_read.game, role),
                running_read
                    .game
                    .players
                    .iter()
                    .filter(|player| player.role == role)
                    .cloned()
                    .collect::<Vec<_>>(),
                role_channel_status_text(&running_read, role),
            )
        };
        if !should_create {
            continue;
        }

        let mut overwrites = Vec::new();
        add_common_hidden_overwrites(&mut overwrites, roles, true);
        for player in players {
            if guild_id
                .member(ctx, serenity::UserId::new(player.user_id))
                .await
                .is_err()
            {
                continue;
            }
            let can_open = role != Role::Lover || {
                let running_read = running.read().await;
                lover_chat_is_open(&running_read.game)
            };
            overwrites.push(private_channel_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
                can_open,
            ));
        }

        let Some(private_channel) = create_text_channel_safe(
            ctx,
            guild_id,
            private_channel_name(role),
            overwrites,
            category,
            "마피아 게임 역할별 비공개 채팅방 생성",
            0,
            None,
        )
        .await
        else {
            failed_roles.push(role);
            continue;
        };
        running
            .write()
            .await
            .private_channel_ids
            .insert(role, private_channel.id);
        let _ = send_channel_embed(
            &ctx.http,
            private_channel.id,
            format!(
                "{} 전용 비공개 채팅방입니다. 살아있는 {}만 볼 수 있습니다.\n\n{}",
                role.value(),
                role.value(),
                special_role_rule_text(role)
            ),
            "역할 비공개 채널",
            serenity::Colour::DARK_GREEN,
            vec![],
        )
        .await;
        if let Ok(message) = send_channel_embed(
            &ctx.http,
            private_channel.id,
            status_text.clone(),
            &format!("{} 채팅 현황", role.value()),
            serenity::Colour::DARK_GREEN,
            vec![],
        )
        .await
        {
            let mut running_write = running.write().await;
            running_write
                .private_role_status_message_ids
                .insert(role, message.id);
            running_write
                .private_role_status_texts
                .insert(role, status_text);
        }
    }

    if !failed_roles.is_empty() {
        let channel_id = running.read().await.channel_id;
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            format!(
                "역할별 비공개 채널 생성에 실패했습니다: {}\n봇에게 채널 관리 권한이 있는지 확인하세요.",
                failed_roles
                    .into_iter()
                    .map(|role| role.value())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
        )
        .await;
    }
    Ok(())
}

pub async fn upsert_private_role_status_message(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    role: Role,
) {
    let (channel_id, message_id, status_text, unchanged) = {
        let running_read = running.read().await;
        let Some(channel_id) = running_read.private_channel_ids.get(&role).copied() else {
            return;
        };
        let status_text = role_channel_status_text(&running_read, role);
        let unchanged = running_read
            .private_role_status_texts
            .get(&role)
            .is_some_and(|cached| cached == &status_text);
        (
            channel_id,
            running_read
                .private_role_status_message_ids
                .get(&role)
                .copied(),
            status_text,
            unchanged,
        )
    };
    if unchanged {
        return;
    }
    let title = format!("{} 채팅 현황", role.value());
    if let Some(message_id) = message_id {
        let edit_result = channel_id
            .edit_message(
                &ctx.http,
                message_id,
                serenity::EditMessage::new().embed(make_embed(
                    status_text.clone(),
                    &title,
                    serenity::Colour::DARK_GREEN,
                )),
            )
            .await;
        if edit_result.is_ok() {
            running
                .write()
                .await
                .private_role_status_texts
                .insert(role, status_text);
            return;
        }
    }
    if let Ok(message) = send_channel_embed(
        &ctx.http,
        channel_id,
        status_text.clone(),
        &title,
        serenity::Colour::DARK_GREEN,
        vec![],
    )
    .await
    {
        let mut running_write = running.write().await;
        running_write
            .private_role_status_message_ids
            .insert(role, message.id);
        running_write
            .private_role_status_texts
            .insert(role, status_text);
    }
}

pub async fn upsert_anonymous_role_status_message(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    channel_id: serenity::ChannelId,
    role: Role,
    key: (u64, Role),
) {
    let (message_id, status_text, unchanged) = {
        let running_read = running.read().await;
        let status_text = role_channel_status_text(&running_read, role);
        let unchanged = running_read
            .anonymous_role_status_texts
            .get(&key)
            .is_some_and(|cached| cached == &status_text);
        (
            running_read
                .anonymous_role_input_status_message_ids
                .get(&key)
                .copied(),
            status_text,
            unchanged,
        )
    };
    if unchanged {
        return;
    }
    let title = format!("{} 채팅 현황", role.value());
    if let Some(message_id) = message_id {
        let edit_result = channel_id
            .edit_message(
                &ctx.http,
                message_id,
                serenity::EditMessage::new().embed(make_embed(
                    status_text.clone(),
                    &title,
                    serenity::Colour::DARK_GREEN,
                )),
            )
            .await;
        if edit_result.is_ok() {
            running
                .write()
                .await
                .anonymous_role_status_texts
                .insert(key, status_text);
            return;
        }
    }
    if let Ok(message) = send_channel_embed(
        &ctx.http,
        channel_id,
        status_text.clone(),
        &title,
        serenity::Colour::DARK_GREEN,
        vec![],
    )
    .await
    {
        let mut running_write = running.write().await;
        running_write
            .anonymous_role_input_status_message_ids
            .insert(key, message.id);
        running_write
            .anonymous_role_status_texts
            .insert(key, status_text);
    }
}

pub async fn sync_anonymous_role_statuses(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    update_messages: bool,
) {
    let updates = {
        let running_read = running.read().await;
        if !running_read.anonymous_enabled {
            return;
        }
        let mut updates = Vec::new();
        for &role in PRIVATE_CHAT_ROLES {
            if !should_create_private_role_channel(&running_read.game, role) {
                continue;
            }
            let topic = format!(
                "{} 익명 채팅 | {}",
                role.value(),
                role_channel_status_text(&running_read, role)
            )
            .chars()
            .take(1024)
            .collect::<String>();
            for (&(user_id, input_role), &channel_id) in
                &running_read.anonymous_role_input_channel_ids
            {
                if input_role == role {
                    updates.push((user_id, role, channel_id, topic.clone()));
                }
            }
        }
        updates
    };
    for (user_id, role, channel_id, topic) in updates {
        let needs_topic_update = {
            let running_read = running.read().await;
            running_read.anonymous_channel_topics.get(&channel_id) != Some(&topic)
        };
        if needs_topic_update
            && channel_id
                .edit(&ctx.http, serenity::EditChannel::new().topic(topic.clone()))
                .await
                .is_ok()
        {
            running
                .write()
                .await
                .anonymous_channel_topics
                .insert(channel_id, topic);
        }
        if update_messages {
            upsert_anonymous_role_status_message(ctx, running, channel_id, role, (user_id, role))
                .await;
        }
    }
}

pub fn shaman_chat_status_text(running: &RunningGame) -> &'static str {
    if running.anonymous_enabled {
        "사망자와 영매가 접신하는 채팅입니다.\n영매는 이 채널만 볼 수 있으며, 밤에만 말할 수 있습니다.\n익명 모드에서는 각자의 영매 개인 채널을 사용하세요."
    } else {
        "사망자와 영매가 접신하는 채팅입니다.\n영매는 이 채널만 볼 수 있으며, 밤에만 말할 수 있습니다."
    }
}

pub async fn upsert_shaman_chat_status(ctx: &serenity::Context, running: &Arc<RwLock<RunningGame>>) {
    let (channel_id, message_id, status_text, unchanged) = {
        let running_read = running.read().await;
        let Some(channel_id) = running_read.shaman_channel_id else {
            return;
        };
        let status_text = shaman_chat_status_text(&running_read).to_string();
        let unchanged = running_read
            .shaman_status_text
            .as_ref()
            .is_some_and(|cached| cached == &status_text);
        (
            channel_id,
            running_read.shaman_status_message_id,
            status_text,
            unchanged,
        )
    };
    if unchanged {
        return;
    }
    if let Some(message_id) = message_id {
        let edit_result = channel_id
            .edit_message(
                &ctx.http,
                message_id,
                serenity::EditMessage::new().embed(make_embed(
                    status_text.clone(),
                    "영매 채팅 상태",
                    serenity::Colour::DARK_GREEN,
                )),
            )
            .await;
        if edit_result.is_ok() {
            running.write().await.shaman_status_text = Some(status_text);
            return;
        }
    }
    if let Ok(message) = send_channel_embed(
        &ctx.http,
        channel_id,
        status_text.clone(),
        "영매 채팅 상태",
        serenity::Colour::DARK_GREEN,
        vec![],
    )
    .await
    {
        let mut running_write = running.write().await;
        running_write.shaman_status_message_id = Some(message.id);
        running_write.shaman_status_text = Some(status_text);
    }
}

pub async fn ensure_memo_channel(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
    roles: ChannelRoleIds,
    category: Option<serenity::ChannelId>,
) -> Option<serenity::ChannelId> {
    if let Some(channel_id) = running
        .read()
        .await
        .memo_channel_ids
        .get(&player.user_id)
        .copied()
    {
        return Some(channel_id);
    }
    let (guild_id, display_name) = {
        let running_read = running.read().await;
        (
            running_read.guild_id,
            status_display_name(&running_read, player),
        )
    };
    if guild_id
        .member(ctx, serenity::UserId::new(player.user_id))
        .await
        .is_err()
    {
        return None;
    }
    let mut overwrites = Vec::new();
    add_common_hidden_overwrites(&mut overwrites, roles, true);
    overwrites.push(private_channel_overwrite(
        serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
        true,
    ));
    let channel = create_text_channel_safe(
        ctx,
        guild_id,
        &format!("{}-메모", sanitize_channel_part(&display_name)),
        overwrites,
        category,
        "마피아 게임 개인 메모 채널 생성",
        0,
        None,
    )
    .await?;
    running
        .write()
        .await
        .memo_channel_ids
        .insert(player.user_id, channel.id);
    let _ = send_channel_embed(
        &ctx.http,
        channel.id,
        "개인 메모 채널입니다.\n`/메모 참가자 메모내용`으로 참가자별 메모를 저장하고, `/메모 참가자`로 저장한 메모를 다시 볼 수 있습니다.",
        "메모 채널",
        serenity::Colour::DARK_GREEN,
        vec![],
    )
    .await;
    Some(channel.id)
}

pub async fn ensure_anonymous_dead_input_channel(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
    roles: ChannelRoleIds,
    category: Option<serenity::ChannelId>,
    can_chat: bool,
) -> Option<serenity::ChannelId> {
    let (guild_id, alias) = {
        let running_read = running.read().await;
        if !is_game_channel_creation_allowed(running_read.game.phase) {
            return None;
        }
        (
            running_read.guild_id,
            if running_read.anonymous_enabled {
                running_read
                    .anonymous_aliases
                    .get(&player.user_id)
                    .cloned()
                    .unwrap_or_else(|| player.name.clone())
            } else {
                player.name.clone()
            },
        )
    };
    if guild_id
        .member(ctx, serenity::UserId::new(player.user_id))
        .await
        .is_err()
    {
        return None;
    }
    let channel_name = format!("{}-사망자-채팅", sanitize_channel_part(&alias));
    let mut running_write = running.write().await;
    if !is_game_channel_creation_allowed(running_write.game.phase) {
        return None;
    }
    if let Some(channel_id) = running_write
        .anonymous_dead_input_channel_ids
        .get(&player.user_id)
        .copied()
    {
        drop(running_write);
        let _ = channel_id
            .create_permission(
                &ctx.http,
                anonymous_input_overwrite(
                    serenity::PermissionOverwriteType::Member(serenity::UserId::new(
                        player.user_id,
                    )),
                    true,
                    can_chat,
                ),
            )
            .await;
        return Some(channel_id);
    }

    if let Some(channel) = find_text_channel_by_name(ctx, guild_id, &channel_name, category).await {
        let channel_id = channel.id;
        running_write
            .anonymous_dead_input_channel_ids
            .insert(player.user_id, channel_id);
        running_write
            .anonymous_dead_input_channel_owners
            .insert(channel_id, player.user_id);
        drop(running_write);
        let _ = channel_id
            .create_permission(
                &ctx.http,
                anonymous_input_overwrite(
                    serenity::PermissionOverwriteType::Member(serenity::UserId::new(
                        player.user_id,
                    )),
                    true,
                    can_chat,
                ),
            )
            .await;
        return Some(channel_id);
    }

    let mut overwrites = anonymous_base_overwrites(roles, false, false, false, false);
    overwrites.push(anonymous_input_overwrite(
        serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
        true,
        can_chat,
    ));
    let channel = create_text_channel_safe(
        ctx,
        guild_id,
        &channel_name,
        overwrites,
        category,
        "마피아 게임 사망자 개인 채팅 채널 생성",
        0,
        None,
    )
    .await?;
    running_write
        .anonymous_dead_input_channel_ids
        .insert(player.user_id, channel.id);
    running_write
        .anonymous_dead_input_channel_owners
        .insert(channel.id, player.user_id);
    drop(running_write);
    let _ = send_channel_embed(
        &ctx.http,
        channel.id,
        "사망자 개인 채팅 채널입니다.\n여기에 쓰면 사망자 채팅을 볼 수 있는 사람들의 사망자 개인 채널로만 전달됩니다.",
        "사망자 개인 채팅",
        serenity::Colour::DARK_GREEN,
        vec![],
    )
    .await;
    Some(channel.id)
}

fn is_game_channel_creation_allowed(phase: Phase) -> bool {
    phase != Phase::Ended
}

pub async fn ensure_anonymous_shaman_input_channel(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
    roles: ChannelRoleIds,
    category: Option<serenity::ChannelId>,
    can_chat: bool,
) -> Option<serenity::ChannelId> {
    let (guild_id, alias) = {
        let running_read = running.read().await;
        if !running_read.anonymous_enabled {
            return None;
        }
        (
            running_read.guild_id,
            running_read
                .anonymous_aliases
                .get(&player.user_id)
                .cloned()
                .unwrap_or_else(|| player.user_id.to_string()),
        )
    };
    if guild_id
        .member(ctx, serenity::UserId::new(player.user_id))
        .await
        .is_err()
    {
        return None;
    }
    let channel_name = format!("{}-영매-채팅", sanitize_channel_part(&alias));
    let mut running_write = running.write().await;
    if !is_game_channel_creation_allowed(running_write.game.phase) {
        return None;
    }
    if let Some(channel_id) = running_write
        .anonymous_shaman_input_channel_ids
        .get(&player.user_id)
        .copied()
    {
        drop(running_write);
        let _ = channel_id
            .create_permission(
                &ctx.http,
                anonymous_input_overwrite(
                    serenity::PermissionOverwriteType::Member(serenity::UserId::new(
                        player.user_id,
                    )),
                    true,
                    can_chat,
                ),
            )
            .await;
        return Some(channel_id);
    }

    if let Some(channel) = find_text_channel_by_name(ctx, guild_id, &channel_name, category).await {
        let channel_id = channel.id;
        running_write
            .anonymous_shaman_input_channel_ids
            .insert(player.user_id, channel_id);
        running_write
            .anonymous_shaman_input_channel_owners
            .insert(channel_id, player.user_id);
        drop(running_write);
        let _ = channel_id
            .create_permission(
                &ctx.http,
                anonymous_input_overwrite(
                    serenity::PermissionOverwriteType::Member(serenity::UserId::new(
                        player.user_id,
                    )),
                    true,
                    can_chat,
                ),
            )
            .await;
        return Some(channel_id);
    }

    let mut overwrites = anonymous_base_overwrites(roles, false, false, false, false);
    overwrites.push(anonymous_input_overwrite(
        serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
        true,
        can_chat,
    ));
    let channel = create_text_channel_safe(
        ctx,
        guild_id,
        &channel_name,
        overwrites,
        category,
        "마피아 게임 익명 영매 입력 채널 생성",
        0,
        None,
    )
    .await?;
    running_write
        .anonymous_shaman_input_channel_ids
        .insert(player.user_id, channel.id);
    running_write
        .anonymous_shaman_input_channel_owners
        .insert(channel.id, player.user_id);
    drop(running_write);
    let _ = send_channel_embed(
        &ctx.http,
        channel.id,
        "영매 익명 채팅 개인 채널입니다.\n여기에 쓰면 영매 채팅을 볼 수 있는 사람들의 영매 개인 채널로만 전달됩니다.",
        "익명 영매 채팅",
        serenity::Colour::DARK_GREEN,
        vec![],
    )
    .await;
    Some(channel.id)
}

pub async fn create_memo_channels(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    roles: ChannelRoleIds,
    category: Option<serenity::ChannelId>,
) -> Result<()> {
    let players = { running.read().await.game.players.clone() };
    let mut failed_names = Vec::new();
    for player in players {
        if ensure_memo_channel(ctx, running, &player, roles, category)
            .await
            .is_none()
        {
            let running_read = running.read().await;
            failed_names.push(status_display_name(&running_read, &player));
        }
    }
    if !failed_names.is_empty() {
        let channel_id = running.read().await.channel_id;
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            format!("개인 메모 채널 생성 실패: {}", failed_names.join(", ")),
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
        )
        .await;
    }
    Ok(())
}

pub async fn create_shaman_chat_channel(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    roles: ChannelRoleIds,
    category: Option<serenity::ChannelId>,
) -> Result<()> {
    let (guild_id, has_shaman, anonymous_enabled, shamans) = {
        let running_read = running.read().await;
        (
            running_read.guild_id,
            running_read
                .game
                .players
                .iter()
                .any(|player| player.role == Role::Shaman),
            running_read.anonymous_enabled,
            running_read
                .game
                .alive_players()
                .into_iter()
                .filter(|player| player.role == Role::Shaman)
                .cloned()
                .collect::<Vec<_>>(),
        )
    };
    if !has_shaman {
        return Ok(());
    }
    let mut overwrites = vec![dead_channel_overwrite(
        serenity::PermissionOverwriteType::Role(roles.everyone),
        false,
        false,
    )];
    if let Some(role_id) = roles.participant {
        overwrites.push(dead_channel_overwrite(
            serenity::PermissionOverwriteType::Role(role_id),
            false,
            false,
        ));
    }
    if let Some(role_id) = roles.dead {
        overwrites.push(dead_channel_overwrite(
            serenity::PermissionOverwriteType::Role(role_id),
            true,
            !anonymous_enabled,
        ));
    }
    if let Some(role_id) = roles.spectator {
        overwrites.push(spectator_channel_overwrite(role_id));
    }
    if let Some(role_id) = roles.manager {
        overwrites.push(dead_channel_overwrite(
            serenity::PermissionOverwriteType::Role(role_id),
            false,
            false,
        ));
    }
    overwrites.push(dead_channel_overwrite(
        serenity::PermissionOverwriteType::Member(roles.bot),
        true,
        true,
    ));
    for player in shamans {
        if guild_id
            .member(ctx, serenity::UserId::new(player.user_id))
            .await
            .is_ok()
        {
            overwrites.push(dead_channel_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
                true,
                false,
            ));
        }
    }

    let Some(channel) = create_text_channel_safe(
        ctx,
        guild_id,
        SHAMAN_CHAT_CHANNEL_NAME,
        overwrites,
        category,
        "마피아 게임 영매 채팅방 생성",
        0,
        None,
    )
    .await
    else {
        let channel_id = running.read().await.channel_id;
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            "영매 채팅방 생성에 실패했습니다. 봇에게 채널 관리 권한이 있는지 확인하세요.",
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
        )
        .await;
        return Ok(());
    };
    running.write().await.shaman_channel_id = Some(channel.id);
    let _ = send_channel_embed(
        &ctx.http,
        channel.id,
        "영매와 사망자가 접신하는 채팅방입니다.\n사망자는 이곳에서 대화할 수 있고, 영매는 밤에만 말할 수 있습니다.\n영매는 사망자 채팅방을 볼 수 없습니다.",
        "영매 채팅방",
        serenity::Colour::DARK_GREEN,
        vec![],
    )
    .await;
    upsert_shaman_chat_status(ctx, running).await;
    if anonymous_enabled {
        let shamans = {
            let running_read = running.read().await;
            running_read
                .game
                .alive_players()
                .into_iter()
                .filter(|player| player.role == Role::Shaman)
                .cloned()
                .collect::<Vec<_>>()
        };
        for shaman in shamans {
            let _ = ensure_anonymous_shaman_input_channel(
                ctx, running, &shaman, roles, category, false,
            )
            .await;
        }
    }
    Ok(())
}

pub async fn create_frog_chat_channel(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    roles: ChannelRoleIds,
    category: Option<serenity::ChannelId>,
) -> Result<()> {
    let (guild_id, has_witch) = {
        let running_read = running.read().await;
        (
            running_read.guild_id,
            running_read
                .game
                .players
                .iter()
                .any(|player| player.role == Role::Witch),
        )
    };
    if !has_witch {
        return Ok(());
    }
    let mut overwrites = vec![dead_channel_overwrite(
        serenity::PermissionOverwriteType::Role(roles.everyone),
        false,
        false,
    )];
    if let Some(role_id) = roles.participant {
        overwrites.push(dead_channel_overwrite(
            serenity::PermissionOverwriteType::Role(role_id),
            false,
            false,
        ));
    }
    if let Some(role_id) = roles.spectator {
        overwrites.push(spectator_channel_overwrite(role_id));
    }
    if let Some(role_id) = roles.manager {
        overwrites.push(dead_channel_overwrite(
            serenity::PermissionOverwriteType::Role(role_id),
            false,
            false,
        ));
    }
    overwrites.push(dead_channel_overwrite(
        serenity::PermissionOverwriteType::Member(roles.bot),
        true,
        true,
    ));
    let Some(channel) = create_text_channel_safe(
        ctx,
        guild_id,
        FROG_CHAT_CHANNEL_NAME,
        overwrites,
        category,
        "마피아 게임 개구리 채팅방 생성",
        0,
        None,
    )
    .await
    else {
        let channel_id = running.read().await.channel_id;
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            "개구리 채팅방 생성에 실패했습니다. 봇에게 채널 관리 권한이 있는지 확인하세요.",
            "마피아 게임",
            serenity::Colour::RED,
            vec![],
        )
        .await;
        return Ok(());
    };
    running.write().await.frog_channel_id = Some(channel.id);
    let _ = send_channel_embed(
        &ctx.http,
        channel.id,
        "개구리 전용 채팅방입니다.\n저주에 걸린 참가자가 이곳에 쓴 말은 게임 채널에 개굴 소리로 전달됩니다.",
        "개구리 채팅방",
        serenity::Colour::DARK_GREEN,
        vec![],
    )
    .await;
    Ok(())
}

pub async fn set_frog_channel_member_access(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
    can_view: bool,
    can_chat: bool,
) {
    let Some(channel_id) = running.read().await.frog_channel_id else {
        return;
    };
    let _ = channel_id
        .create_permission(
            &ctx.http,
            dead_channel_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
                can_view,
                can_chat,
            ),
        )
        .await;
}

pub async fn set_frog_game_channel_permission(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
    can_chat: bool,
) {
    let channel_id = running.read().await.channel_id;
    let Some(channel) = channel_id
        .to_channel(&ctx.http)
        .await
        .ok()
        .and_then(|channel| channel.guild())
    else {
        return;
    };
    let kind = serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id));
    let original = channel
        .permission_overwrites
        .iter()
        .find(|overwrite| overwrite.kind == kind)
        .cloned();
    {
        let mut running_write = running.write().await;
        running_write
            .frog_game_channel_overwrites
            .entry(player.user_id)
            .or_insert_with(|| original.clone());
    }
    let mut overwrite = original.unwrap_or(serenity::PermissionOverwrite {
        allow: serenity::Permissions::empty(),
        deny: serenity::Permissions::empty(),
        kind,
    });
    set_chat_permission_bits(&mut overwrite, can_chat);
    let _ = channel_id.create_permission(&ctx.http, overwrite).await;
}

pub async fn restore_frog_game_channel_permission(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
) {
    let (channel_id, original) = {
        let mut running_write = running.write().await;
        (
            running_write.channel_id,
            running_write
                .frog_game_channel_overwrites
                .remove(&player.user_id),
        )
    };
    let kind = serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id));
    match original {
        Some(Some(overwrite)) => {
            let _ = channel_id.create_permission(&ctx.http, overwrite).await;
        }
        Some(None) => {
            let _ = channel_id.delete_permission(&ctx.http, kind).await;
        }
        None => {}
    }
}

pub async fn restore_all_frog_game_channel_permissions(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
) {
    let players = {
        let running_read = running.read().await;
        running_read
            .frog_game_channel_overwrites
            .keys()
            .filter_map(|user_id| running_read.game.get_player(*user_id))
            .cloned()
            .collect::<Vec<_>>()
    };
    for player in players {
        restore_frog_game_channel_permission(ctx, running, &player).await;
    }
}

pub async fn sync_madam_seduction_permissions(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
) {
    if running.read().await.anonymous_enabled {
        sync_anonymous_general_chat_permissions(ctx, running).await;
        sync_anonymous_role_statuses(ctx, running, true).await;
        return;
    }
    let (channel_id, seduced_ids) = {
        let running_read = running.read().await;
        (
            running_read.channel_id,
            running_read
                .game
                .alive_players()
                .into_iter()
                .filter(|player| running_read.game.is_madam_seduced(player))
                .map(|player| player.user_id)
                .collect::<HashSet<_>>(),
        )
    };
    let Some(channel) = channel_id
        .to_channel(&ctx.http)
        .await
        .ok()
        .and_then(|channel| channel.guild())
    else {
        running
            .write()
            .await
            .madam_seduction_channel_overwrites
            .clear();
        return;
    };
    for user_id in &seduced_ids {
        let kind = serenity::PermissionOverwriteType::Member(serenity::UserId::new(*user_id));
        let original = channel
            .permission_overwrites
            .iter()
            .find(|overwrite| overwrite.kind == kind)
            .cloned();
        {
            let mut running_write = running.write().await;
            running_write
                .madam_seduction_channel_overwrites
                .entry(*user_id)
                .or_insert_with(|| original.clone());
        }
        let mut overwrite = original.unwrap_or(serenity::PermissionOverwrite {
            allow: serenity::Permissions::empty(),
            deny: serenity::Permissions::empty(),
            kind,
        });
        set_chat_permission_bits(&mut overwrite, false);
        let _ = channel_id.create_permission(&ctx.http, overwrite).await;
    }

    let restore_ids = {
        let running_read = running.read().await;
        running_read
            .madam_seduction_channel_overwrites
            .keys()
            .filter(|user_id| !seduced_ids.contains(user_id))
            .copied()
            .collect::<Vec<_>>()
    };
    for user_id in restore_ids {
        restore_madam_seduction_permission(ctx, running, user_id).await;
    }
}

pub async fn restore_madam_seduction_permission(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    user_id: u64,
) {
    let (channel_id, original) = {
        let mut running_write = running.write().await;
        (
            running_write.channel_id,
            running_write
                .madam_seduction_channel_overwrites
                .remove(&user_id),
        )
    };
    let kind = serenity::PermissionOverwriteType::Member(serenity::UserId::new(user_id));
    match original {
        Some(Some(overwrite)) => {
            let _ = channel_id.create_permission(&ctx.http, overwrite).await;
        }
        Some(None) => {
            let _ = channel_id.delete_permission(&ctx.http, kind).await;
        }
        None => {}
    }
}

pub async fn restore_all_madam_seduction_permissions(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
) {
    let user_ids = {
        let running_read = running.read().await;
        running_read
            .madam_seduction_channel_overwrites
            .keys()
            .copied()
            .collect::<Vec<_>>()
    };
    for user_id in user_ids {
        restore_madam_seduction_permission(ctx, running, user_id).await;
    }
}

pub async fn set_shaman_channel_member_access(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
    can_view: bool,
    can_chat: bool,
) {
    let Some(channel_id) = running.read().await.shaman_channel_id else {
        return;
    };
    let _ = channel_id
        .create_permission(
            &ctx.http,
            dead_channel_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
                can_view,
                can_chat,
            ),
        )
        .await;
    upsert_shaman_chat_status(ctx, running).await;
}

pub async fn sync_shaman_chat_access(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) {
    let (has_shaman_channel, anonymous_enabled, source_channel_id, players) = {
        let running_read = running.read().await;
        (
            running_read.shaman_channel_id.is_some(),
            running_read.anonymous_enabled,
            running_read.channel_id,
            running_read
                .game
                .players
                .iter()
                .filter(|player| {
                    player.role == Role::Shaman
                        || running_read
                            .anonymous_shaman_input_channel_ids
                            .contains_key(&player.user_id)
                })
                .cloned()
                .collect::<Vec<_>>(),
        )
    };
    if !has_shaman_channel {
        return;
    }
    let anonymous_context = if anonymous_enabled {
        let roles = running_channel_roles(ctx, data, running).await;
        let category = source_category(ctx, source_channel_id).await;
        roles.map(|roles| (roles, category))
    } else {
        None
    };
    for player in players {
        let can_shaman_chat = {
            let running_read = running.read().await;
            running_read
                .game
                .get_player(player.user_id)
                .is_some_and(|player| can_use_anonymous_shaman_chat(&running_read, player))
        };
        if player.role == Role::Shaman {
            set_shaman_channel_member_access(
                ctx,
                running,
                &player,
                true,
                !anonymous_enabled && can_shaman_chat,
            )
            .await;
        }
        if let Some((roles, category)) = anonymous_context {
            let _ = ensure_anonymous_shaman_input_channel(
                ctx,
                running,
                &player,
                roles,
                category,
                can_shaman_chat,
            )
            .await;
        }
    }
}

pub async fn set_anonymous_role_channel_access(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    roles: ChannelRoleIds,
    role: Role,
    player: &Player,
    can_view: bool,
    can_chat: bool,
) {
    let (guild_id, source_channel_id, existing_channel_id, alias, status_text) = {
        let running_read = running.read().await;
        (
            running_read.guild_id,
            running_read.channel_id,
            running_read
                .anonymous_role_input_channel_ids
                .get(&(player.user_id, role))
                .copied(),
            running_read
                .anonymous_aliases
                .get(&player.user_id)
                .cloned()
                .unwrap_or_else(|| player.name.clone()),
            role_channel_status_text(&running_read, role),
        )
    };
    if guild_id
        .member(ctx, serenity::UserId::new(player.user_id))
        .await
        .is_err()
    {
        return;
    }
    let channel_id = if let Some(channel_id) = existing_channel_id {
        channel_id
    } else if can_view {
        let category = source_category(ctx, source_channel_id).await;
        let mut overwrites = anonymous_base_overwrites(roles, false, false, false, false);
        overwrites.push(anonymous_input_overwrite(
            serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
            true,
            can_chat,
        ));
        let Some(channel) = create_text_channel_safe(
            ctx,
            guild_id,
            &format!("{}-{}-채팅", sanitize_channel_part(&alias), role.value()),
            overwrites,
            category,
            "마피아 게임 익명 역할 채팅 권한 동기화",
            0,
            Some(format!("{} 익명 채팅 | {status_text}", role.value())),
        )
        .await
        else {
            return;
        };
        {
            let mut running_write = running.write().await;
            running_write
                .anonymous_role_input_channel_ids
                .insert((player.user_id, role), channel.id);
            running_write
                .anonymous_role_input_channels
                .insert(channel.id, (player.user_id, role));
            running_write.anonymous_channel_topics.insert(
                channel.id,
                format!("{} 익명 채팅 | {status_text}", role.value())
                    .chars()
                    .take(1024)
                    .collect::<String>(),
            );
        }
        let (message, title) = if can_chat {
            (
                format!(
                    "{} 역할 개인 채팅 채널입니다.\n여기에 쓰면 같은 역할의 개인 채팅방에 익명으로 전달됩니다.\n이 채널 하나에서 역할 대화와 밤 행동을 처리하세요.",
                    role.value()
                ),
                "익명 역할 입력",
            )
        } else {
            (
                format!(
                    "{} 역할 보기 전용 채널입니다.\n이 채널에서 역할 대화를 확인할 수 있습니다.",
                    role.value()
                ),
                "익명 역할 채팅",
            )
        };
        let _ = send_channel_embed(
            &ctx.http,
            channel.id,
            message,
            title,
            serenity::Colour::DARK_GREEN,
            vec![],
        )
        .await;
        if let Ok(status_message) = send_channel_embed(
            &ctx.http,
            channel.id,
            status_text.clone(),
            &format!("{} 채팅 현황", role.value()),
            serenity::Colour::DARK_GREEN,
            vec![],
        )
        .await
        {
            let mut running_write = running.write().await;
            running_write
                .anonymous_role_input_status_message_ids
                .insert((player.user_id, role), status_message.id);
            running_write
                .anonymous_role_status_texts
                .insert((player.user_id, role), status_text.clone());
        }
        channel.id
    } else {
        return;
    };
    let _ = channel_id
        .create_permission(
            &ctx.http,
            anonymous_input_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
                can_view,
                can_chat,
            ),
        )
        .await;
}

pub async fn set_private_role_member_view_access(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    role: Role,
    player: &Player,
    can_view: bool,
    can_chat: bool,
) {
    let can_chat = {
        let running_read = running.read().await;
        can_chat && !running_read.game.is_madam_seduced(player)
    };
    let Some(channel_id) = running.read().await.private_channel_ids.get(&role).copied() else {
        return;
    };
    let _ = channel_id
        .create_permission(
            &ctx.http,
            dead_channel_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
                can_view,
                can_chat,
            ),
        )
        .await;
    upsert_private_role_status_message(ctx, running, role).await;
}

pub async fn set_private_role_member_access(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    role: Role,
    player: &Player,
    can_chat: bool,
) {
    let can_chat = {
        let running_read = running.read().await;
        can_chat && !running_read.game.is_madam_seduced(player)
    };
    let Some(channel_id) = running.read().await.private_channel_ids.get(&role).copied() else {
        return;
    };
    let _ = channel_id
        .create_permission(
            &ctx.http,
            private_channel_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
                can_chat,
            ),
        )
        .await;
    upsert_private_role_status_message(ctx, running, role).await;
}

pub async fn disable_private_role_channels_for_player(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
) {
    let anonymous_updates = {
        let running_read = running.read().await;
        if running_read.anonymous_enabled {
            Some(
                running_read
                    .anonymous_role_input_channel_ids
                    .iter()
                    .filter_map(|(&(user_id, role), &channel_id)| {
                        (user_id == player.user_id).then_some((role, channel_id))
                    })
                    .collect::<Vec<_>>(),
            )
        } else {
            None
        }
    };
    if let Some(updates) = anonymous_updates {
        for (role, channel_id) in updates {
            let _ = channel_id
                .create_permission(
                    &ctx.http,
                    anonymous_input_overwrite(
                        serenity::PermissionOverwriteType::Member(serenity::UserId::new(
                            player.user_id,
                        )),
                        false,
                        false,
                    ),
                )
                .await;
            upsert_anonymous_role_status_message(
                ctx,
                running,
                channel_id,
                role,
                (player.user_id, role),
            )
            .await;
        }
        sync_anonymous_role_statuses(ctx, running, true).await;
        return;
    }
    let roles = {
        let running_read = running.read().await;
        running_read
            .private_channel_ids
            .keys()
            .copied()
            .collect::<Vec<_>>()
    };
    for role in roles {
        set_private_role_member_access(ctx, running, role, player, false).await;
    }
}

pub async fn grant_private_role_member_access(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
    role: Role,
    player: &Player,
) {
    let anonymous_enabled = running.read().await.anonymous_enabled;
    if anonymous_enabled {
        let Some(roles) = running_channel_roles(ctx, data, running).await else {
            return;
        };
        let can_access = {
            let running_read = running.read().await;
            player.alive
                && !running_read.game.is_frog(player)
                && !running_read.game.is_madam_seduced(player)
        };
        let can_chat = {
            let running_read = running.read().await;
            can_access && private_role_member_can_chat(&running_read.game, role, player)
        };
        set_anonymous_role_channel_access(
            ctx, running, roles, role, player, can_access, can_chat,
        )
        .await;
        sync_anonymous_role_statuses(ctx, running, true).await;
        return;
    }
    let can_chat = {
        let running_read = running.read().await;
        private_role_member_can_chat(&running_read.game, role, player)
    };
    set_private_role_member_view_access(ctx, running, role, player, true, can_chat).await;
}

pub async fn sync_private_role_chat_permissions(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) {
    let anonymous_enabled = running.read().await.anonymous_enabled;
    if anonymous_enabled {
        let Some(roles) = running_channel_roles(ctx, data, running).await else {
            return;
        };
        let updates = {
            let running_read = running.read().await;
            running_read
                .anonymous_role_input_channel_ids
                .keys()
                .filter_map(|(user_id, role)| {
                    let player = running_read.game.get_player(*user_id)?.clone();
                    let can_view = player.alive
                        && !running_read.game.is_frog(&player)
                        && !running_read.game.is_madam_seduced(&player);
                    let can_chat =
                        can_view && private_role_member_can_chat(&running_read.game, *role, &player);
                    Some((*role, player, can_view, can_chat))
                })
                .collect::<Vec<_>>()
        };
        for (role, player, can_view, can_chat) in updates {
            set_anonymous_role_channel_access(
                ctx, running, roles, role, &player, can_view, can_chat,
            )
            .await;
        }
        sync_anonymous_role_statuses(ctx, running, true).await;
        return;
    }

    let channel_ids = running.read().await.private_channel_ids.clone();
    for (role, channel_id) in channel_ids {
        let Some(channel) = channel_id
            .to_channel(&ctx.http)
            .await
            .ok()
            .and_then(|channel| channel.guild())
        else {
            continue;
        };
        let member_overwrites = channel
            .permission_overwrites
            .iter()
            .filter_map(|overwrite| match overwrite.kind {
                serenity::PermissionOverwriteType::Member(user_id) => {
                    let can_view = overwrite.allow.contains(serenity::Permissions::VIEW_CHANNEL)
                        && !overwrite.deny.contains(serenity::Permissions::VIEW_CHANNEL);
                    Some((user_id.get(), can_view))
                }
                serenity::PermissionOverwriteType::Role(_) => None,
                _ => None,
            })
            .collect::<Vec<_>>();
        for (user_id, can_view) in member_overwrites {
            let update = {
                let running_read = running.read().await;
                let player = running_read.game.get_player(user_id).cloned();
                player.map(|player| {
                    let can_chat =
                        can_view && private_role_member_can_chat(&running_read.game, role, &player);
                    (player, can_chat)
                })
            };
            if let Some((player, can_chat)) = update {
                set_private_role_member_view_access(
                    ctx, running, role, &player, can_view, can_chat,
                )
                .await;
            }
        }
    }
}

pub async fn running_channel_roles(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) -> Option<ChannelRoleIds> {
    let config = data.config.read().await.clone();
    let guild_id = running.read().await.guild_id;
    channel_role_ids(ctx, guild_id, &config, data.bot_user_id)
        .await
        .ok()
}

pub async fn sync_lover_chat_access(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) {
    let (has_lover, anonymous_enabled, can_open, players) = {
        let running_read = running.read().await;
        (
            running_read
                .game
                .players
                .iter()
                .any(|player| player.role == Role::Lover),
            running_read.anonymous_enabled,
            lover_chat_is_open(&running_read.game),
            running_read.game.players.clone(),
        )
    };
    if !has_lover {
        return;
    }
    if anonymous_enabled {
        let Some(roles) = running_channel_roles(ctx, data, running).await else {
            return;
        };
        for player in players.iter().filter(|player| player.role == Role::Lover) {
            let can_access = {
                let running_read = running.read().await;
                can_open
                    && player.alive
                    && !running_read.game.is_frog(player)
                    && !running_read.game.is_madam_seduced(player)
            };
            set_anonymous_role_channel_access(
                ctx,
                running,
                roles,
                Role::Lover,
                player,
                can_access,
                can_access,
            )
            .await;
        }
        sync_anonymous_role_statuses(ctx, running, true).await;
        return;
    }
    for player in players.iter().filter(|player| player.role == Role::Lover) {
        let can_access = {
            let running_read = running.read().await;
            can_open && player.alive && !running_read.game.is_frog(player)
        };
        set_private_role_member_access(ctx, running, Role::Lover, player, can_access).await;
    }
}

pub async fn sync_cult_team_channel_access(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) {
    let (has_cult_team, anonymous_enabled, players) = {
        let running_read = running.read().await;
        (
            running_read
                .game
                .players
                .iter()
                .any(|player| matches!(player.role, Role::CultLeader | Role::Fanatic)),
            running_read.anonymous_enabled,
            running_read.game.players.clone(),
        )
    };
    if !has_cult_team {
        return;
    }
    if anonymous_enabled {
        let Some(roles) = running_channel_roles(ctx, data, running).await else {
            return;
        };
        for player in &players {
            let (can_view, can_chat) = {
                let running_read = running.read().await;
                let can_view = player.alive
                    && !running_read.game.is_frog(player)
                    && running_read.game.is_cult_team(player);
                let can_chat = can_view
                    && private_role_member_can_chat(&running_read.game, Role::CultLeader, player);
                (can_view, can_chat)
            };
            set_anonymous_role_channel_access(
                ctx,
                running,
                roles,
                Role::CultLeader,
                player,
                can_view,
                can_chat,
            )
            .await;
        }
        sync_anonymous_role_statuses(ctx, running, true).await;
        return;
    }
    for player in &players {
        let (can_view, can_chat) = {
            let running_read = running.read().await;
            let can_view = player.alive
                && !running_read.game.is_frog(player)
                && running_read.game.is_cult_team(player);
            let can_chat = can_view
                && private_role_member_can_chat(&running_read.game, Role::CultLeader, player);
            (can_view, can_chat)
        };
        set_private_role_member_view_access(
            ctx,
            running,
            Role::CultLeader,
            player,
            can_view,
            can_chat,
        )
        .await;
    }
}

pub async fn sync_scientist_mafia_permissions(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) {
    let scientist_players = {
        let running_read = running.read().await;
        running_read
            .game
            .players
            .iter()
            .filter(|player| {
                player.role == Role::Scientist
                    && running_read
                        .game
                        .scientist_contacted
                        .contains(&player.user_id)
                    && (player.alive
                        || running_read
                            .game
                            .scientist_pending_revive_ids
                            .contains(&player.user_id))
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    if scientist_players.is_empty() {
        return;
    }
    let anonymous_enabled = running.read().await.anonymous_enabled;
    if anonymous_enabled {
        let Some(roles) = running_channel_roles(ctx, data, running).await else {
            return;
        };
        for player in &scientist_players {
            let can_chat = {
                let running_read = running.read().await;
                private_role_member_can_chat(&running_read.game, Role::Mafia, player)
            };
            set_anonymous_role_channel_access(
                ctx,
                running,
                roles,
                Role::Mafia,
                player,
                true,
                can_chat,
            )
            .await;
        }
        sync_anonymous_role_statuses(ctx, running, true).await;
        return;
    }
    for player in &scientist_players {
        let can_chat = {
            let running_read = running.read().await;
            private_role_member_can_chat(&running_read.game, Role::Mafia, player)
        };
        set_private_role_member_view_access(ctx, running, Role::Mafia, player, true, can_chat)
            .await;
    }
}

pub async fn restore_revived_player_roles(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    roles: ChannelRoleIds,
    player: &Player,
) {
    let guild_id = running.read().await.guild_id;
    if let Ok(member) = guild_id
        .member(ctx, serenity::UserId::new(player.user_id))
        .await
    {
        if let Some(participant_role_id) = roles.participant {
            let _ = member.add_role(ctx, participant_role_id).await;
        }
        if let Some(dead_role_id) = roles.dead {
            let _ = member.remove_role(ctx, dead_role_id).await;
        }
    }
    set_shaman_channel_member_access(ctx, running, player, false, false).await;
    set_frog_channel_member_access(ctx, running, player, false, false).await;
    let anonymous_channel_ids = {
        let running_read = running.read().await;
        [
            running_read
                .anonymous_dead_input_channel_ids
                .get(&player.user_id)
                .copied(),
            running_read
                .anonymous_shaman_input_channel_ids
                .get(&player.user_id)
                .copied(),
        ]
    };
    for channel_id in anonymous_channel_ids.into_iter().flatten() {
        let _ = channel_id
            .create_permission(
                &ctx.http,
                anonymous_input_overwrite(
                    serenity::PermissionOverwriteType::Member(serenity::UserId::new(
                        player.user_id,
                    )),
                    false,
                    false,
                ),
            )
            .await;
    }
    restore_frog_game_channel_permission(ctx, running, player).await;
    let grant_roles = {
        let running_read = running.read().await;
        let mut roles = Vec::new();
        if PRIVATE_CHAT_ROLES.contains(&player.role)
            && (player.role != Role::Lover || lover_chat_is_open(&running_read.game))
        {
            roles.push(player.role);
        }
        if running_read.game.is_known_mafia_team(player) {
            roles.push(Role::Mafia);
        }
        roles.sort_by_key(|role| role.value());
        roles.dedup();
        roles
    };
    for role in grant_roles {
        if running.read().await.anonymous_enabled {
            let can_access = {
                let running_read = running.read().await;
                player.alive
                    && !running_read.game.is_frog(player)
                    && !running_read.game.is_madam_seduced(player)
            };
            let can_chat = {
                let running_read = running.read().await;
                can_access && private_role_member_can_chat(&running_read.game, role, player)
            };
            set_anonymous_role_channel_access(
                ctx, running, roles, role, player, can_access, can_chat,
            )
            .await;
        } else {
            let can_chat = {
                let running_read = running.read().await;
                private_role_member_can_chat(&running_read.game, role, player)
            };
            set_private_role_member_view_access(ctx, running, role, player, true, can_chat).await;
        }
    }
    sync_anonymous_general_chat_permissions(ctx, running).await;
    sync_anonymous_role_statuses(ctx, running, true).await;
}

pub async fn apply_purification_side_effects(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
    purified_user_ids: &[u64],
) {
    if purified_user_ids.is_empty() {
        return;
    }
    let config = data.config.read().await.clone();
    let (guild_id, channel_id, anonymous_enabled) = {
        let running_read = running.read().await;
        (
            running_read.guild_id,
            running_read.channel_id,
            running_read.anonymous_enabled,
        )
    };
    let roles = match channel_role_ids(ctx, guild_id, &config, data.bot_user_id).await {
        Ok(roles) => roles,
        Err(_) => return,
    };
    let category = source_category(ctx, channel_id).await;
    for user_id in purified_user_ids {
        let player = running.read().await.game.get_player(*user_id).cloned();
        let Some(player) = player else {
            continue;
        };
        set_shaman_channel_member_access(ctx, running, &player, true, false).await;
        let _ = ensure_anonymous_dead_input_channel(ctx, running, &player, roles, category, false)
            .await;
        if anonymous_enabled {
            let _ = ensure_anonymous_shaman_input_channel(
                ctx, running, &player, roles, category, false,
            )
            .await;
        }
    }
}

pub fn anonymous_vote_summary(game: &MafiaGame, result: &VoteResult) -> String {
    if result.vote_counts.is_empty() {
        return "투표 없음".to_string();
    }
    let mut rows = result
        .vote_counts
        .iter()
        .map(|(target_id, count)| {
            let name = target_id.map_or_else(
                || "스킵".to_string(),
                |id| {
                    game.get_player(id)
                        .map(|player| player.name.clone())
                        .unwrap_or_else(|| id.to_string())
                },
            );
            (name, *count)
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then_with(|| left.0.to_lowercase().cmp(&right.0.to_lowercase()))
    });
    rows.into_iter()
        .map(|(name, count)| format!("- {name}: {count}표"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub async fn handle_madam_seduction_result(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
    result: &VoteResult,
) {
    if result.madam_seduced.is_empty() && result.madam_newly_contacted.is_empty() {
        return;
    }
    for player in &result.madam_seduced {
        let _ = send_player_secret(
            ctx,
            running,
            player,
            "마담에게 유혹당했습니다. 다음 낮이 될 때까지 능력을 사용할 수 없고 말할 수 없습니다.\n마피아팀이라면 능력 사용은 가능하지만, 유혹 중에는 마피아 비밀방에도 말할 수 없습니다.",
            vec![],
        )
        .await;
        disable_private_role_channels_for_player(ctx, running, player).await;
    }
    let known_mafia_players = {
        let running_read = running.read().await;
        running_read
            .game
            .alive_players()
            .into_iter()
            .filter(|player| running_read.game.is_known_mafia_team(player))
            .cloned()
            .collect::<Vec<_>>()
    };
    for player in known_mafia_players {
        grant_private_role_member_access(ctx, data, running, Role::Mafia, &player).await;
    }
    for madam in result
        .madam_newly_contacted
        .iter()
        .filter(|player| player.alive)
    {
        grant_private_role_member_access(ctx, data, running, Role::Mafia, madam).await;
        let _ = send_player_secret(
            ctx,
            running,
            madam,
            "[접대] 마피아팀과 접선했습니다. 이제 마피아 비밀방에서 밤 대화가 가능합니다.",
            vec![],
        )
        .await;
    }
    sync_madam_seduction_permissions(ctx, running).await;
}

pub async fn cleanup_game(ctx: &serenity::Context, data: &Data, running: &Arc<RwLock<RunningGame>>) {
    // Block in-flight personal dead-chat creation before collecting channels to delete.
    running.write().await.game.phase = Phase::Ended;
    restore_channel_slowmode(ctx, running).await;
    restore_member_game_channel_chat(ctx, running).await;
    restore_game_channel_chat(ctx, running).await;
    restore_all_frog_game_channel_permissions(ctx, running).await;
    restore_all_madam_seduction_permissions(ctx, running).await;
    let channel_ids = {
        let running_read = running.read().await;
        let mut channel_ids = Vec::new();
        channel_ids.extend(running_read.private_channel_ids.values().copied());
        channel_ids.extend(running_read.memo_channel_ids.values().copied());
        channel_ids.extend(running_read.anonymous_input_channel_ids.values().copied());
        channel_ids.extend(
            running_read
                .anonymous_dead_input_channel_ids
                .values()
                .copied(),
        );
        channel_ids.extend(
            running_read
                .anonymous_shaman_input_channel_ids
                .values()
                .copied(),
        );
        channel_ids.extend(
            running_read
                .anonymous_role_input_channel_ids
                .values()
                .copied(),
        );
        if let Some(channel_id) = running_read.shaman_channel_id {
            channel_ids.push(channel_id);
        }
        if let Some(channel_id) = running_read.frog_channel_id {
            channel_ids.push(channel_id);
        }
        channel_ids
    };

    let mut seen = HashSet::new();
    for channel_id in channel_ids {
        if seen.insert(channel_id) {
            if let Err(error) = channel_id.delete(&ctx.http).await {
                eprintln!("failed to delete game channel {}: {error:?}", channel_id.get());
            }
        }
    }

    let (guild_id, participant_user_ids, spectator_user_ids) = {
        let running_read = running.read().await;
        (
            running_read.guild_id,
            running_read
                .participant_user_ids
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            running_read
                .spectator_user_ids
                .iter()
                .copied()
                .collect::<Vec<_>>(),
        )
    };
    let config = data.config.read().await.clone();
    if let Ok(roles) = channel_role_ids(ctx, guild_id, &config, data.bot_user_id).await {
        for user_id in participant_user_ids {
            if let Ok(member) = guild_id.member(ctx, serenity::UserId::new(user_id)).await {
        let participant_cleanup_role_ids = [roles.participant, roles.dead]
            .into_iter()
            .flatten()
            .collect::<HashSet<_>>();
                let _ = remove_cleanup_roles_from_member_snapshot(
                    ctx,
                    guild_id,
                    member.user.id,
                    &member.roles,
                    &participant_cleanup_role_ids,
                )
                .await;
            }
        }
        if let Some(role_id) = roles.spectator {
            for user_id in spectator_user_ids {
                if let Ok(member) = guild_id.member(ctx, serenity::UserId::new(user_id)).await {
                    let role_ids = HashSet::from([role_id]);
                    let _ = remove_cleanup_roles_from_member_snapshot(
                        ctx,
                        guild_id,
                        member.user.id,
                        &member.roles,
                        &role_ids,
                    )
                    .await;
                }
            }
        }
    }

    let (source_channel_id, original_overwrites) = {
        let running_read = running.read().await;
        (
            running_read.channel_id,
            running_read.original_game_channel_overwrites.clone(),
        )
    };
    for (role_id, overwrite) in original_overwrites {
        match overwrite {
            Some(overwrite) => {
                let _ = source_channel_id
                    .create_permission(&ctx.http, overwrite)
                    .await;
            }
            None => {
                let _ = source_channel_id
                    .delete_permission(&ctx.http, serenity::PermissionOverwriteType::Role(role_id))
                    .await;
            }
        }
    }

    let mut running_write = running.write().await;
    if !running_write.anonymous_original_names.is_empty() {
        let original_names = running_write.anonymous_original_names.clone();
        for player in &mut running_write.game.players {
            if let Some(original) = original_names.get(&player.user_id) {
                player.name.clone_from(original);
            }
        }
    }
    running_write.private_channel_ids.clear();
    running_write.private_role_status_message_ids.clear();
    running_write.private_role_status_texts.clear();
    running_write.game_status_message_id = None;
    running_write.game_status_text = None;
    running_write.memo_channel_ids.clear();
    running_write.anonymous_input_channel_ids.clear();
    running_write.anonymous_input_channel_owners.clear();
    running_write.anonymous_dead_input_channel_ids.clear();
    running_write.anonymous_dead_input_channel_owners.clear();
    running_write.anonymous_shaman_input_channel_ids.clear();
    running_write.anonymous_shaman_input_channel_owners.clear();
    running_write.anonymous_role_input_channel_ids.clear();
    running_write.anonymous_role_input_channels.clear();
    running_write
        .anonymous_role_input_status_message_ids
        .clear();
    running_write.anonymous_role_status_texts.clear();
    running_write.anonymous_channel_topics.clear();
    running_write.anonymous_aliases.clear();
    running_write.anonymous_original_names.clear();
    running_write.anonymous_webhook_urls.clear();
    running_write.original_game_channel_overwrites.clear();
    running_write.game_channel_overwrites.clear();
    running_write.member_channel_overwrites.clear();
    running_write.original_slowmode_delays.clear();
    running_write.shaman_channel_id = None;
    running_write.shaman_status_message_id = None;
    running_write.shaman_status_text = None;
    running_write.frog_channel_id = None;
    running_write.frog_game_channel_overwrites.clear();
    running_write.madam_seduction_channel_overwrites.clear();
}

#[derive(Default)]
pub(crate) struct ForcedCleanupSummary {
    pub channels_deleted: usize,
    pub role_removals: usize,
}

pub async fn cleanup_orphaned_game_artifacts(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
) -> ForcedCleanupSummary {
    let config = data.config.read().await.clone();
    full_role_sweep: bool,
    let roles = channel_role_ids(ctx, guild_id, &config, data.bot_user_id)
        .await
        .ok();
    let mut summary = ForcedCleanupSummary::default();

    if let Ok(channels) = guild_id.channels(&ctx.http).await {
        let mut channel_ids = channels
            .into_values()
            .filter(|channel| should_force_delete_game_channel(channel, roles))
            .map(|channel| channel.id)
            .collect::<Vec<_>>();
        channel_ids.sort_by_key(|id| id.get());
        channel_ids.dedup();
        for channel_id in channel_ids {
            if channel_id.delete(&ctx.http).await.is_ok() {
                summary.channels_deleted += 1;
            }
        }
    }

    let Some(roles) = roles else {
        return summary;
    };
    let role_ids = [roles.participant, roles.dead, roles.spectator]
        .into_iter()
        .flatten()
        .collect::<HashSet<_>>();
    if role_ids.is_empty() {
        return summary;
    }

    let mut after = None;
    loop {
    if !full_role_sweep {
        return summary;
    }

        let Ok(members) = guild_id.members(&ctx.http, Some(1000), after).await else {
            break;
        };
        if members.is_empty() {
            break;
        }
        for member in &members {
            summary.role_removals += remove_cleanup_roles_from_member_snapshot(
                ctx,
                guild_id,
                member.user.id,
                &member.roles,
                &role_ids,
            )
            .await;
        }
        let count = members.len();
        after = members.last().map(|member| member.user.id);
        if count < 1000 {
            break;
        }
    }

    summary
}

fn should_force_delete_game_channel(
    channel: &serenity::GuildChannel,
async fn remove_cleanup_roles_from_member_snapshot(
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    user_id: serenity::UserId,
    member_roles: &[serenity::RoleId],
    role_ids: &HashSet<serenity::RoleId>,
) -> usize {
    let mut removed = 0;
    let remaining_roles = member_roles
        .iter()
        .copied()
        .filter(|role_id| {
            if role_ids.contains(role_id) {
                removed += 1;
                false
            } else {
                true
            }
        })
        .collect::<Vec<_>>();
    if removed == 0 {
        return 0;
    }
    if guild_id
        .edit_member(
            &ctx.http,
            user_id,
            serenity::EditMember::new().roles(remaining_roles),
        )
        .await
        .is_ok()
    {
        removed
    } else {
        0
    }
}

    roles: Option<ChannelRoleIds>,
) -> bool {
    if channel.kind != serenity::ChannelType::Text {
        return false;
    }
    let name = channel.name.as_str();
    if name == SHAMAN_CHAT_CHANNEL_NAME
        || name == FROG_CHAT_CHANNEL_NAME
        || PRIVATE_CHAT_ROLES
            .iter()
            .any(|role| name == private_channel_name(*role))
    {
        return true;
    }
    if name.ends_with(&format!("-{}-채팅", DEAD_PLAYER_ROLE))
        || name.ends_with(&format!("-{}-채팅", Role::Shaman.value()))
        || name.ends_with("-메모")
        || PRIVATE_CHAT_ROLES
            .iter()
            .any(|role| name.ends_with(&format!("-{}-채팅", role.value())))
    {
        return true;
    }
    roles.is_some_and(|roles| {
        name.ends_with("-채팅")
            && channel.permission_overwrites.iter().any(|overwrite| {
                overwrite.kind == serenity::PermissionOverwriteType::Role(roles.everyone)
                    && overwrite.deny.contains(serenity::Permissions::VIEW_CHANNEL)
            })
    })
}

pub async fn sync_anonymous_general_chat_permissions(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
) {
    let updates = {
        let running_read = running.read().await;
        if !running_read.anonymous_enabled {
            return;
        }
        running_read
            .game
            .players
            .iter()
            .filter_map(|player| {
                let channel_id = running_read
                    .anonymous_input_channel_ids
                    .get(&player.user_id)
                    .copied()?;
                Some((
                    channel_id,
                    player.user_id,
                    can_use_anonymous_general_chat(&running_read, player),
                ))
            })
            .collect::<Vec<_>>()
    };
    for (channel_id, user_id, can_chat) in updates {
        let _ = channel_id
            .create_permission(
                &ctx.http,
                anonymous_input_overwrite(
                    serenity::PermissionOverwriteType::Member(serenity::UserId::new(user_id)),
                    true,
                    can_chat,
                ),
            )
            .await;
    }
}

pub async fn set_game_channel_chat(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
    mut participants_can_chat: bool,
) {
    let anonymous_enabled = running.read().await.anonymous_enabled;
    if anonymous_enabled {
        sync_anonymous_general_chat_permissions(ctx, running).await;
        participants_can_chat = false;
    }
    let Some(roles) = running_channel_roles(ctx, data, running).await else {
        return;
    };
    let channel_id = running.read().await.channel_id;
    let Some(channel) = channel_id
        .to_channel(&ctx.http)
        .await
        .ok()
        .and_then(|channel| channel.guild())
    else {
        return;
    };
    let mut targets = vec![(roles.everyone, false)];
    if let Some(participant_role_id) = roles.participant {
        targets.push((participant_role_id, participants_can_chat));
    }
    for (role_id, can_chat) in targets {
        let kind = serenity::PermissionOverwriteType::Role(role_id);
        let current = channel
            .permission_overwrites
            .iter()
            .find(|overwrite| overwrite.kind == kind)
            .cloned();
        {
            let mut running_write = running.write().await;
            if !running_write.game_channel_overwrites.contains_key(&role_id) {
                let original = running_write
                    .original_game_channel_overwrites
                    .get(&role_id)
                    .cloned()
                    .unwrap_or_else(|| current.clone());
                running_write
                    .game_channel_overwrites
                    .insert(role_id, original);
            }
        }
        let mut overwrite = current.unwrap_or(serenity::PermissionOverwrite {
            allow: serenity::Permissions::empty(),
            deny: serenity::Permissions::empty(),
            kind,
        });
        set_chat_permission_bits(&mut overwrite, can_chat);
        let _ = channel_id.create_permission(&ctx.http, overwrite).await;
    }
}

pub async fn set_member_game_channel_chat(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
    can_chat: bool,
) {
    if running.read().await.anonymous_enabled {
        sync_anonymous_general_chat_permissions(ctx, running).await;
        return;
    }
    let channel_id = running.read().await.channel_id;
    let Some(channel) = channel_id
        .to_channel(&ctx.http)
        .await
        .ok()
        .and_then(|channel| channel.guild())
    else {
        return;
    };
    let kind = serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id));
    let current = channel
        .permission_overwrites
        .iter()
        .find(|overwrite| overwrite.kind == kind)
        .cloned();
    {
        let mut running_write = running.write().await;
        running_write
            .member_channel_overwrites
            .entry(player.user_id)
            .or_insert_with(|| current.clone());
    }
    let mut overwrite = current.unwrap_or(serenity::PermissionOverwrite {
        allow: serenity::Permissions::empty(),
        deny: serenity::Permissions::empty(),
        kind,
    });
    set_chat_permission_bits(&mut overwrite, can_chat);
    let _ = channel_id.create_permission(&ctx.http, overwrite).await;
}

pub async fn restore_member_game_channel_chat(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
) {
    let (channel_id, originals) = {
        let mut running_write = running.write().await;
        (
            running_write.channel_id,
            std::mem::take(&mut running_write.member_channel_overwrites),
        )
    };
    for (user_id, original) in originals {
        let kind = serenity::PermissionOverwriteType::Member(serenity::UserId::new(user_id));
        match original {
            Some(overwrite) => {
                let _ = channel_id.create_permission(&ctx.http, overwrite).await;
            }
            None => {
                let _ = channel_id.delete_permission(&ctx.http, kind).await;
            }
        }
    }
}

pub async fn restore_game_channel_chat(ctx: &serenity::Context, running: &Arc<RwLock<RunningGame>>) {
    let (channel_id, originals) = {
        let mut running_write = running.write().await;
        (
            running_write.channel_id,
            std::mem::take(&mut running_write.game_channel_overwrites),
        )
    };
    for (role_id, original) in originals {
        let kind = serenity::PermissionOverwriteType::Role(role_id);
        match original {
            Some(overwrite) => {
                let _ = channel_id.create_permission(&ctx.http, overwrite).await;
            }
            None => {
                let _ = channel_id.delete_permission(&ctx.http, kind).await;
            }
        }
    }
}

pub fn slowmode_channel_ids(running: &RunningGame) -> Vec<serenity::ChannelId> {
    vec![running.channel_id]
}

pub async fn set_one_channel_slowmode(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    channel_id: serenity::ChannelId,
    seconds: u64,
) {
    let Some(channel) = channel_id
        .to_channel(&ctx.http)
        .await
        .ok()
        .and_then(|channel| channel.guild())
    else {
        return;
    };
    let slowmode = seconds.min(21600) as u16;
    {
        let mut running_write = running.write().await;
        running_write
            .original_slowmode_delays
            .entry(channel_id)
            .or_insert_with(|| channel.rate_limit_per_user.unwrap_or(0));
    }
    if channel.rate_limit_per_user.unwrap_or(0) == slowmode {
        return;
    }
    if let Err(error) = channel_id
        .edit(
            &ctx.http,
            serenity::EditChannel::new().rate_limit_per_user(slowmode),
        )
        .await
    {
        eprintln!("failed to set slowmode for {}: {error:?}", channel_id.get());
    }
}

pub async fn set_channel_slowmode(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    seconds: u64,
) {
    let channel_ids = {
        let running_read = running.read().await;
        slowmode_channel_ids(&running_read)
    };
    for channel_id in channel_ids {
        set_one_channel_slowmode(ctx, running, channel_id, seconds).await;
    }
}

pub async fn restore_channel_slowmode(ctx: &serenity::Context, running: &Arc<RwLock<RunningGame>>) {
    let originals = {
        let mut running_write = running.write().await;
        std::mem::take(&mut running_write.original_slowmode_delays)
    };
    for (channel_id, delay) in originals {
        if let Err(error) = channel_id
            .edit(
                &ctx.http,
                serenity::EditChannel::new().rate_limit_per_user(delay),
            )
            .await
        {
            eprintln!(
                "failed to restore slowmode for {}: {error:?}",
                channel_id.get()
            );
        }
    }
}

pub async fn apply_death_side_effects(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
    dead_players: &[Player],
) {
    if dead_players.is_empty() {
        return;
    }
    let config = data.config.read().await.clone();
    let (guild_id, channel_id) = {
        let running_read = running.read().await;
        (running_read.guild_id, running_read.channel_id)
    };
    let Ok(roles) = channel_role_ids(ctx, guild_id, &config, data.bot_user_id).await else {
        return;
    };
    for player in dead_players {
        if let Ok(member) = guild_id
            .member(ctx, serenity::UserId::new(player.user_id))
            .await
        {
            if let Some(participant_role_id) = roles.participant {
                let _ = member.remove_role(ctx, participant_role_id).await;
            }
            if let Some(dead_role_id) = roles.dead {
                let _ = member.add_role(ctx, dead_role_id).await;
            }
        }
        let can_dead_chat = {
            let running_read = running.read().await;
            !running_read
                .game
                .purified_dead_ids
                .contains(&player.user_id)
        };
        set_shaman_channel_member_access(ctx, running, player, true, can_dead_chat).await;
        set_frog_channel_member_access(ctx, running, player, false, false).await;
        restore_frog_game_channel_permission(ctx, running, player).await;
        disable_private_role_channels_for_player(ctx, running, player).await;
    }
    let category = source_category(ctx, channel_id).await;
    let anonymous_enabled = running.read().await.anonymous_enabled;
    for player in dead_players {
        let can_chat = {
            let running_read = running.read().await;
            running_read
                .game
                .get_player(player.user_id)
                .is_some_and(|player| can_use_anonymous_dead_chat(&running_read, player))
        };
        let _ =
            ensure_anonymous_dead_input_channel(ctx, running, player, roles, category, can_chat)
                .await;
        if anonymous_enabled && running.read().await.shaman_channel_id.is_some() {
            let can_shaman_chat = {
                let running_read = running.read().await;
                running_read
                    .game
                    .get_player(player.user_id)
                    .is_some_and(|player| can_use_anonymous_shaman_chat(&running_read, player))
            };
            let _ = ensure_anonymous_shaman_input_channel(
                ctx,
                running,
                player,
                roles,
                category,
                can_shaman_chat,
            )
            .await;
        }
    }
    if anonymous_enabled {
        sync_anonymous_general_chat_permissions(ctx, running).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role_chat_test_game() -> MafiaGame {
        MafiaGame::new(
            vec![
                (1, "p1".to_string()),
                (2, "p2".to_string()),
                (3, "p3".to_string()),
                (4, "p4".to_string()),
            ],
            1,
            1,
            1,
            Vec::new(),
        )
        .unwrap()
    }

    #[test]
    fn balanced_special_selection_prefers_less_seen_roles() {
        let mut history = std::collections::HashMap::new();
        history.insert(Role::Spy, 10);
        history.insert(Role::Contractor, 5);

        let selected = select_balanced_special_roles_for_slots(
            &[Role::Spy, Role::Contractor, Role::Thief],
            1,
            &history,
        )
        .unwrap();

        assert_eq!(selected, vec![Role::Thief]);
    }

    #[test]
    fn balanced_special_selection_counts_lover_slots() {
        let mut history = std::collections::HashMap::new();
        history.insert(Role::Lover, 100);

        let selected = select_balanced_special_roles_for_slots(
            &[Role::Lover, Role::Detective, Role::Shaman],
            2,
            &history,
        )
        .unwrap();

        assert_eq!(selected, vec![Role::Detective, Role::Shaman]);
    }

    #[test]
    fn private_role_chat_closed_during_day() {
        let mut game = role_chat_test_game();
        let doctor = game
            .players
            .iter()
            .find(|player| player.role == Role::Doctor)
            .cloned()
            .unwrap();

        game.phase = Phase::Day;

        assert!(!private_role_member_can_chat(&game, Role::Doctor, &doctor));
    }

    #[test]
    fn private_role_chat_open_during_night() {
        let game = role_chat_test_game();
        let doctor = game
            .players
            .iter()
            .find(|player| player.role == Role::Doctor)
            .cloned()
            .unwrap();

        assert!(private_role_member_can_chat(&game, Role::Doctor, &doctor));
    }

    #[test]
    fn ended_game_cannot_create_personal_dead_chat() {
        assert!(!is_game_channel_creation_allowed(Phase::Ended));
        assert!(is_game_channel_creation_allowed(Phase::Night));
    }
}

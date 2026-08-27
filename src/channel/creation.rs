// channel/creation.rs — 게임 채널·개인 채널 생성과 상태 메시지

use super::*;

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
    {
        let mut running_write = running.write().await;
        running_write.channel_role_ids = Some(roles);
        running_write.source_category_id = Some(category);
    }

    set_spectator_game_channel_access(ctx, running, roles).await;
    create_anonymous_chat_channels(ctx, running, &config, roles, category).await?;
    hide_original_game_channel_for_anonymous(ctx, running, roles).await;
    create_private_role_channels(ctx, running, roles, category).await?;
    sync_cult_team_channel_access(ctx, data, running).await;
    create_memo_channels(ctx, running, roles, category).await?;
    create_shaman_chat_channel(ctx, running, roles, category).await?;
    apply_slowmode_bypass_overwrites(ctx, running).await;
    Ok(())
}

/// [달변] 슬로우모드 무시: Discord는 메시지 관리 권한 보유자에게 슬로우모드를
/// 적용하지 않으므로, 게임 채널에 한해 보유자에게 그 권한을 준다. 이 멤버는 게임
/// 채널의 메시지를 관리(삭제/고정)할 수도 있게 되는 부작용이 있다 — 친구 서버
/// 전제의 절충이다. cleanup_game에서 오버라이트를 제거한다.
pub async fn apply_slowmode_bypass_overwrites(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
) {
    let (channel_id, holder_ids) = {
        let running_read = running.read().await;
        (
            running_read.channel_id,
            slowmode_bypass_holder_ids(&running_read.game),
        )
    };
    for user_id in &holder_ids {
        let overwrite = serenity::PermissionOverwrite {
            allow: serenity::Permissions::MANAGE_MESSAGES,
            deny: serenity::Permissions::empty(),
            kind: serenity::PermissionOverwriteType::Member(serenity::UserId::new(*user_id)),
        };
        apply_permission_if_changed(ctx, running, channel_id, overwrite).await;
    }
    // 익명 게임은 채팅이 개인 익명 입력 채널에서 이뤄지므로, 게임 채널
    // 오버라이트만으로는 [달변]이 아무 효과가 없다. 보유자의 입력 채널
    // 슬로우모드를 0으로 풀어준다.
    let holder_input_channels = {
        let running_read = running.read().await;
        if !running_read.anonymous_enabled {
            Vec::new()
        } else {
            holder_ids
                .iter()
                .filter_map(|user_id| {
                    running_read
                        .anonymous_input_channel_ids
                        .get(user_id)
                        .copied()
                })
                .collect::<Vec<_>>()
        }
    };
    for input_channel_id in holder_input_channels {
        set_one_channel_slowmode(ctx, running, input_channel_id, 0).await;
    }
}

pub fn slowmode_bypass_holder_ids(game: &MafiaGame) -> Vec<u64> {
    game.tier_abilities
        .iter()
        .filter(|(_, abilities)| {
            abilities.contains(&mafia_remake::model::TierAbility::SlowmodeBypass)
        })
        .map(|(user_id, _)| *user_id)
        .collect()
}

/// cleanup에서 [달변] 오버라이트를 제거한다.
pub async fn remove_slowmode_bypass_overwrites(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
) {
    let (channel_id, holder_ids) = {
        let running_read = running.read().await;
        (
            running_read.channel_id,
            slowmode_bypass_holder_ids(&running_read.game),
        )
    };
    for user_id in holder_ids {
        delete_permission_and_invalidate(
            ctx,
            running,
            channel_id,
            serenity::PermissionOverwriteType::Member(serenity::UserId::new(user_id)),
        )
        .await;
    }
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
    let bot_original = channel
        .permission_overwrites
        .iter()
        .find(|overwrite| overwrite.kind == serenity::PermissionOverwriteType::Member(roles.bot))
        .cloned();
    {
        let mut running_write = running.write().await;
        running_write
            .original_game_channel_overwrites
            .entry(participant_role_id)
            .or_insert(original);
        running_write
            .member_channel_overwrites
            .entry(roles.bot.get())
            .or_insert(bot_original);
    }
    apply_permission_if_changed(
        ctx,
        running,
        channel_id,
        anonymous_input_overwrite(
            serenity::PermissionOverwriteType::Role(participant_role_id),
            false,
            false,
        ),
    )
    .await;
    apply_permission_if_changed(
        ctx,
        running,
        channel_id,
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
    apply_permission_if_changed(
        ctx,
        running,
        channel_id,
        spectator_channel_overwrite(spectator_role_id),
    )
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
    let mut failed_players = Vec::new();
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
        if let Err(error) = verify_game_member(ctx, running, player.user_id).await {
            eprintln!(
                "failed to resolve anonymous chat member: guild_id={} user_id={} player={:?} error={error:?}",
                guild_id.get(),
                player.user_id,
                player.name,
            );
            failed_players.push(player.name.clone());
            continue;
        }

        let mut overwrites = anonymous_base_overwrites(roles, false, false, false, false);
        overwrites.push(anonymous_input_overwrite(
            serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
            true,
            can_chat,
        ));
        let initial_overwrites = overwrites.clone();
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
            failed_players.push(player.name.clone());
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
            remember_channel_permissions(&mut running_write, input_channel.id, &initial_overwrites);
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
    if !failed_players.is_empty() {
        bail!("익명 개인 채널 생성 실패: {}", failed_players.join(", "));
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
            "{}{} / 최종 진영: {} / {}",
            player.role.value(),
            state,
            final_team_text(&running.game, player),
            crate::runner::game_result_tier_text(&running.game, player.user_id)
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

pub fn private_role_status_player_ids(
    running: &RunningGame,
    player: &Player,
) -> (String, Vec<u64>) {
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
            if verify_game_member(ctx, running, user_id).await.is_err() {
                continue;
            }
            let mut overwrites = anonymous_base_overwrites(roles, false, false, false, false);
            overwrites.push(anonymous_input_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(user_id)),
                true,
                can_chat,
            ));
            let initial_overwrites = overwrites.clone();
            let Some(channel) = create_text_channel_safe(
                ctx,
                guild_id,
                &format!("{}-{}-채팅", sanitize_channel_part(&alias), role.value()),
                overwrites,
                category,
                "마피아 게임 역할별 익명 입력 채널 생성",
                0,
                Some(anonymous_role_channel_topic(role)),
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
                remember_channel_permissions(&mut running_write, channel.id, &initial_overwrites);
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
            if verify_game_member(ctx, running, player.user_id)
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

        let initial_overwrites = overwrites.clone();
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
        {
            let mut running_write = running.write().await;
            running_write
                .private_channel_ids
                .insert(role, private_channel.id);
            remember_channel_permissions(
                &mut running_write,
                private_channel.id,
                &initial_overwrites,
            );
        }
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
    if running.read().await.anonymous_enabled {
        sync_anonymous_role_status(ctx, running, role).await;
        return;
    }
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

/// 역할 익명 채널 토픽. 채널 토픽은 생성 시 한 번만 정하므로 시간이 지나면 낡을 수
/// 있는 현황은 넣지 않는다(현황은 채널 내 상태 메시지가 담당).
pub(crate) fn anonymous_role_channel_topic(role: Role) -> String {
    format!("{} 익명 채팅", role.value())
}

pub(crate) fn anonymous_role_status_targets(
    running: &RunningGame,
    role: Role,
) -> Option<Vec<((u64, Role), serenity::ChannelId)>> {
    if !running.anonymous_enabled {
        return None;
    }
    let mut targets = running
        .anonymous_role_input_channel_ids
        .iter()
        .filter_map(|(&key, &channel_id)| (key.1 == role).then_some((key, channel_id)))
        .collect::<Vec<_>>();
    targets.sort_by_key(|((user_id, _), _)| *user_id);
    Some(targets)
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

// 역할 현황은 채널 내 상태 메시지로만 갱신한다. 채널 토픽 수정(PATCH /channels)은
// Discord가 채널당 10분에 2회로 제한하므로, 상태가 바뀔 때마다 토픽을 고치면 세 번째
// 요청부터 최대 10분씩 429 대기에 걸린다. 이 대기는 워커 토큰으로도 피할 수 없고(채널
// 단위 제한) 게임 루프에서 그대로 await되어 게임 전체가 멈춘다. 토픽은 채널을 만들 때
// 한 번만 정하고 이후에는 절대 수정하지 않는다.
pub(crate) async fn sync_anonymous_role_status(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    role: Role,
) {
    let targets = {
        let running_read = running.read().await;
        if !running_read.anonymous_enabled
            || !should_create_private_role_channel(&running_read.game, role)
        {
            return;
        }
        anonymous_role_status_targets(&running_read, role).unwrap_or_default()
    };
    for ((user_id, _), channel_id) in targets {
        upsert_anonymous_role_status_message(ctx, running, channel_id, role, (user_id, role)).await;
    }
}

pub async fn sync_anonymous_role_statuses(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
) {
    for &role in PRIVATE_CHAT_ROLES {
        sync_anonymous_role_status(ctx, running, role).await;
    }
}

pub fn shaman_chat_status_text(running: &RunningGame) -> &'static str {
    if running.anonymous_enabled {
        "사망자와 영매의 익명 릴레이용 내부 채널입니다.\n익명 모드에서는 각자의 영매 개인 채널만 사용합니다."
    } else {
        "사망자와 영매가 접신하는 채팅입니다.\n영매는 이 채널만 볼 수 있으며, 밤에만 말할 수 있습니다."
    }
}

pub async fn upsert_shaman_chat_status(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
) {
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
    let creation_lock =
        personal_channel_creation_lock(running, player.user_id, PersonalChannelKind::Memo).await;
    let creation_guard = creation_lock.lock().await;
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
    if verify_game_member(ctx, running, player.user_id)
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
    let initial_overwrites = overwrites.clone();
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
    {
        let mut running_write = running.write().await;
        running_write
            .memo_channel_ids
            .insert(player.user_id, channel.id);
        remember_channel_permissions(&mut running_write, channel.id, &initial_overwrites);
    }
    drop(creation_guard);
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
    let (guild_id, alias, existing_channel_id) = {
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
            running_read
                .anonymous_dead_input_channel_ids
                .get(&player.user_id)
                .copied(),
        )
    };
    if let Some(channel_id) = existing_channel_id {
        apply_permission_if_changed(
            ctx,
            running,
            channel_id,
            anonymous_input_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
                true,
                can_chat,
            ),
        )
        .await;
        return Some(channel_id);
    }
    let creation_lock =
        personal_channel_creation_lock(running, player.user_id, PersonalChannelKind::Dead).await;
    let creation_guard = creation_lock.lock().await;
    {
        let running_read = running.read().await;
        if !is_game_channel_creation_allowed(running_read.game.phase) {
            return None;
        }
        if let Some(channel_id) = running_read
            .anonymous_dead_input_channel_ids
            .get(&player.user_id)
            .copied()
        {
            drop(running_read);
            apply_permission_if_changed(
                ctx,
                running,
                channel_id,
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
    }
    if verify_game_member(ctx, running, player.user_id)
        .await
        .is_err()
    {
        return None;
    }
    let channel_name = format!("{}-사망자-채팅", sanitize_channel_part(&alias));
    if let Some(channel) = find_text_channel_by_name(ctx, guild_id, &channel_name, category).await {
        let channel_id = channel.id;
        {
            let mut running_write = running.write().await;
            if !is_game_channel_creation_allowed(running_write.game.phase) {
                return None;
            }
            running_write
                .anonymous_dead_input_channel_ids
                .insert(player.user_id, channel_id);
            running_write
                .anonymous_dead_input_channel_owners
                .insert(channel_id, player.user_id);
            remember_channel_permissions(
                &mut running_write,
                channel_id,
                &channel.permission_overwrites,
            );
        }
        drop(creation_guard);
        apply_permission_if_changed(
            ctx,
            running,
            channel_id,
            anonymous_input_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
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
    let initial_overwrites = overwrites.clone();
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
    let keep_channel = {
        let mut running_write = running.write().await;
        if is_game_channel_creation_allowed(running_write.game.phase) {
            running_write
                .anonymous_dead_input_channel_ids
                .insert(player.user_id, channel.id);
            running_write
                .anonymous_dead_input_channel_owners
                .insert(channel.id, player.user_id);
            remember_channel_permissions(&mut running_write, channel.id, &initial_overwrites);
            true
        } else {
            false
        }
    };
    drop(creation_guard);
    if !keep_channel {
        let deleted_channel_id = channel.id;
        let _ = crate::http_pool::with_fallback(ctx, |http| async move {
            deleted_channel_id.delete(&http).await
        })
        .await;
        return None;
    }
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

pub(crate) fn is_game_channel_creation_allowed(phase: Phase) -> bool {
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
    let (guild_id, alias, existing_channel_id) = {
        let running_read = running.read().await;
        if !running_read.anonymous_enabled
            || !is_game_channel_creation_allowed(running_read.game.phase)
        {
            return None;
        }
        (
            running_read.guild_id,
            running_read
                .anonymous_aliases
                .get(&player.user_id)
                .cloned()
                .unwrap_or_else(|| player.user_id.to_string()),
            running_read
                .anonymous_shaman_input_channel_ids
                .get(&player.user_id)
                .copied(),
        )
    };
    if let Some(channel_id) = existing_channel_id {
        apply_permission_if_changed(
            ctx,
            running,
            channel_id,
            anonymous_input_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
                true,
                can_chat,
            ),
        )
        .await;
        return Some(channel_id);
    }
    let creation_lock =
        personal_channel_creation_lock(running, player.user_id, PersonalChannelKind::Shaman).await;
    let creation_guard = creation_lock.lock().await;
    {
        let running_read = running.read().await;
        if !running_read.anonymous_enabled
            || !is_game_channel_creation_allowed(running_read.game.phase)
        {
            return None;
        }
        if let Some(channel_id) = running_read
            .anonymous_shaman_input_channel_ids
            .get(&player.user_id)
            .copied()
        {
            drop(running_read);
            apply_permission_if_changed(
                ctx,
                running,
                channel_id,
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
    }
    if verify_game_member(ctx, running, player.user_id)
        .await
        .is_err()
    {
        return None;
    }
    let channel_name = format!("{}-영매-채팅", sanitize_channel_part(&alias));
    if let Some(channel) = find_text_channel_by_name(ctx, guild_id, &channel_name, category).await {
        let channel_id = channel.id;
        {
            let mut running_write = running.write().await;
            if !running_write.anonymous_enabled
                || !is_game_channel_creation_allowed(running_write.game.phase)
            {
                return None;
            }
            running_write
                .anonymous_shaman_input_channel_ids
                .insert(player.user_id, channel_id);
            running_write
                .anonymous_shaman_input_channel_owners
                .insert(channel_id, player.user_id);
            remember_channel_permissions(
                &mut running_write,
                channel_id,
                &channel.permission_overwrites,
            );
        }
        drop(creation_guard);
        apply_permission_if_changed(
            ctx,
            running,
            channel_id,
            anonymous_input_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
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
    let initial_overwrites = overwrites.clone();
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
    let keep_channel = {
        let mut running_write = running.write().await;
        if running_write.anonymous_enabled
            && is_game_channel_creation_allowed(running_write.game.phase)
        {
            running_write
                .anonymous_shaman_input_channel_ids
                .insert(player.user_id, channel.id);
            running_write
                .anonymous_shaman_input_channel_owners
                .insert(channel.id, player.user_id);
            remember_channel_permissions(&mut running_write, channel.id, &initial_overwrites);
            true
        } else {
            false
        }
    };
    drop(creation_guard);
    if !keep_channel {
        let deleted_channel_id = channel.id;
        let _ = crate::http_pool::with_fallback(ctx, |http| async move {
            deleted_channel_id.delete(&http).await
        })
        .await;
        return None;
    }
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
            !anonymous_enabled,
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
        if verify_game_member(ctx, running, player.user_id)
            .await
            .is_ok()
        {
            overwrites.push(dead_channel_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
                !anonymous_enabled,
                false,
            ));
        }
    }
    let initial_overwrites = overwrites.clone();

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
    {
        let mut running_write = running.write().await;
        running_write.shaman_channel_id = Some(channel.id);
        remember_channel_permissions(&mut running_write, channel.id, &initial_overwrites);
    }
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

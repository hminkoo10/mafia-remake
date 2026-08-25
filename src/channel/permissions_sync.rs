// channel/permissions_sync.rs — 개구리·마담·역할 채널 등 권한 동기화

use super::*;

pub async fn deny_frog_game_channel_chat(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
) {
    if running.read().await.anonymous_enabled {
        sync_anonymous_general_chat_permissions(ctx, running).await;
        return;
    }
    let channel_id = running.read().await.channel_id;
    let kind = serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id));
    let remembered = {
        let running_read = running.read().await;
        remembered_permission(&running_read, channel_id, kind)
    };
    let current = if remembered.is_some() {
        remembered
    } else {
        let Some(channel) = channel_id
            .to_channel(&ctx.http)
            .await
            .ok()
            .and_then(|channel| channel.guild())
        else {
            return;
        };
        channel
            .permission_overwrites
            .iter()
            .find(|overwrite| overwrite.kind == kind)
            .cloned()
    };
    {
        let mut running_write = running.write().await;
        running_write
            .frog_game_channel_overwrites
            .entry(player.user_id)
            .or_insert_with(|| current.clone());
    }
    let mut overwrite = current.unwrap_or(serenity::PermissionOverwrite {
        allow: serenity::Permissions::empty(),
        deny: serenity::Permissions::empty(),
        kind,
    });
    set_chat_permission_bits(&mut overwrite, false);
    apply_permission_if_changed(ctx, running, channel_id, overwrite).await;
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
            apply_permission_if_changed(ctx, running, channel_id, overwrite).await;
        }
        Some(None) => {
            delete_permission_and_invalidate(ctx, running, channel_id, kind).await;
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
        sync_anonymous_role_statuses(ctx, running).await;
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
    let remembered = {
        let running_read = running.read().await;
        seduced_ids
            .iter()
            .map(|user_id| {
                let kind =
                    serenity::PermissionOverwriteType::Member(serenity::UserId::new(*user_id));
                (
                    *user_id,
                    remembered_permission(&running_read, channel_id, kind),
                )
            })
            .collect::<HashMap<_, _>>()
    };
    let channel = if remembered.values().any(Option::is_none) {
        let Some(channel) = channel_id
            .to_channel(&ctx.http)
            .await
            .ok()
            .and_then(|channel| channel.guild())
        else {
            return;
        };
        Some(channel)
    } else {
        None
    };
    for user_id in &seduced_ids {
        let kind = serenity::PermissionOverwriteType::Member(serenity::UserId::new(*user_id));
        let original = remembered.get(user_id).cloned().flatten().or_else(|| {
            channel.as_ref().and_then(|channel| {
                channel
                    .permission_overwrites
                    .iter()
                    .find(|overwrite| overwrite.kind == kind)
                    .cloned()
            })
        });
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
        apply_permission_if_changed(ctx, running, channel_id, overwrite).await;
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
            apply_permission_if_changed(ctx, running, channel_id, overwrite).await;
        }
        Some(None) => {
            delete_permission_and_invalidate(ctx, running, channel_id, kind).await;
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
    let (channel_id, anonymous_enabled) = {
        let running_read = running.read().await;
        let Some(channel_id) = running_read.shaman_channel_id else {
            return;
        };
        (channel_id, running_read.anonymous_enabled)
    };
    let (can_view, can_chat) = shared_shaman_member_access(anonymous_enabled, can_view, can_chat);
    apply_permission_if_changed(
        ctx,
        running,
        channel_id,
        dead_channel_overwrite(
            serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
            can_view,
            can_chat,
        ),
    )
    .await;
    upsert_shaman_chat_status(ctx, running).await;
}

pub(crate) fn shared_shaman_member_access(
    anonymous_enabled: bool,
    can_view: bool,
    can_chat: bool,
) -> (bool, bool) {
    if anonymous_enabled {
        (false, false)
    } else {
        (can_view, can_chat)
    }
}

pub async fn sync_shaman_chat_access(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) {
    let (has_shaman_channel, anonymous_enabled, players) = {
        let running_read = running.read().await;
        (
            running_read.shaman_channel_id.is_some(),
            running_read.anonymous_enabled,
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
        let category = running_source_category(ctx, running).await;
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
    let (guild_id, mut existing_channel_id, alias, status_text) = {
        let running_read = running.read().await;
        (
            running_read.guild_id,
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
    let creation_lock = if existing_channel_id.is_none() && can_view {
        Some(
            personal_channel_creation_lock(
                running,
                player.user_id,
                PersonalChannelKind::Role(role),
            )
            .await,
        )
    } else {
        None
    };
    let creation_guard = if let Some(lock) = creation_lock.as_ref() {
        Some(lock.lock().await)
    } else {
        None
    };
    if existing_channel_id.is_none() {
        existing_channel_id = running
            .read()
            .await
            .anonymous_role_input_channel_ids
            .get(&(player.user_id, role))
            .copied();
    }
    if existing_channel_id.is_none() {
        if !can_view
            || verify_game_member(ctx, running, player.user_id)
                .await
                .is_err()
        {
            return;
        }
    }
    let channel_id = if let Some(channel_id) = existing_channel_id {
        channel_id
    } else if can_view {
        let category = running_source_category(ctx, running).await;
        let mut overwrites = anonymous_base_overwrites(roles, false, false, false, false);
        overwrites.push(anonymous_input_overwrite(
            serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
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
            "마피아 게임 익명 역할 채팅 권한 동기화",
            0,
            Some(anonymous_role_channel_topic(role)),
        )
        .await
        else {
            return;
        };
        let keep_channel = {
            let mut running_write = running.write().await;
            if running_write.anonymous_enabled
                && is_game_channel_creation_allowed(running_write.game.phase)
            {
                running_write
                    .anonymous_role_input_channel_ids
                    .insert((player.user_id, role), channel.id);
                running_write
                    .anonymous_role_input_channels
                    .insert(channel.id, (player.user_id, role));
                remember_channel_permissions(&mut running_write, channel.id, &initial_overwrites);
                true
            } else {
                false
            }
        };
        if !keep_channel {
            let deleted_channel_id = channel.id;
            let _ = crate::http_pool::with_fallback(ctx, |http| async move {
                deleted_channel_id.delete(&http).await
            })
            .await;
            return;
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
    drop(creation_guard);
    apply_permission_if_changed(
        ctx,
        running,
        channel_id,
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
    apply_permission_if_changed(
        ctx,
        running,
        channel_id,
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
    apply_permission_if_changed(
        ctx,
        running,
        channel_id,
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
            apply_permission_if_changed(
                ctx,
                running,
                channel_id,
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
        sync_anonymous_role_statuses(ctx, running).await;
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
        set_anonymous_role_channel_access(ctx, running, roles, role, player, can_access, can_chat)
            .await;
        sync_anonymous_role_statuses(ctx, running).await;
        return;
    }
    let can_chat = {
        let running_read = running.read().await;
        private_role_member_can_chat(&running_read.game, role, player)
    };
    set_private_role_member_view_access(ctx, running, role, player, true, can_chat).await;
}

pub fn private_roles_to_restore(running: &RunningGame, player: &Player) -> Vec<Role> {
    if !player.alive || running.game.is_frog(player) || running.game.is_madam_seduced(player) {
        return Vec::new();
    }
    let mut roles = Vec::new();
    if PRIVATE_CHAT_ROLES.contains(&player.role)
        && (player.role != Role::Lover || lover_chat_is_open(&running.game))
    {
        roles.push(player.role);
    }
    if running.game.is_known_mafia_team(player) {
        roles.push(Role::Mafia);
    }
    roles.sort_by_key(|role| role.value());
    roles.dedup();
    roles
}

pub async fn restore_private_role_channels_for_player(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
    player: &Player,
) {
    let grant_roles = {
        let running_read = running.read().await;
        private_roles_to_restore(&running_read, player)
    };
    for role in grant_roles {
        grant_private_role_member_access(ctx, data, running, role, player).await;
    }
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
                    let can_chat = can_view
                        && private_role_member_can_chat(&running_read.game, *role, &player);
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
        sync_anonymous_role_statuses(ctx, running).await;
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
        let mut member_ids = channel
            .permission_overwrites
            .iter()
            .filter_map(|overwrite| match overwrite.kind {
                serenity::PermissionOverwriteType::Member(user_id) => Some(user_id.get()),
                serenity::PermissionOverwriteType::Role(_) => None,
                _ => None,
            })
            .collect::<HashSet<_>>();
        {
            let running_read = running.read().await;
            member_ids.extend(
                running_read
                    .game
                    .players
                    .iter()
                    .filter(|player| private_role_member_can_view(&running_read.game, role, player))
                    .map(|player| player.user_id),
            );
        }
        let mut member_ids = member_ids.into_iter().collect::<Vec<_>>();
        member_ids.sort_unstable();
        for user_id in member_ids {
            let update = {
                let running_read = running.read().await;
                let player = running_read.game.get_player(user_id).cloned();
                player.map(|player| {
                    let can_view = private_role_member_can_view(&running_read.game, role, &player);
                    let can_chat = private_role_member_can_chat(&running_read.game, role, &player);
                    (player, can_view, can_chat)
                })
            };
            if let Some((player, can_view, can_chat)) = update {
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
    if let Some(roles) = running.read().await.channel_role_ids {
        return Some(roles);
    }
    let config = data.config.read().await.clone();
    let guild_id = running.read().await.guild_id;
    let roles = channel_role_ids(ctx, guild_id, &config, data.bot_user_id)
        .await
        .ok()?;
    let mut running_write = running.write().await;
    if running_write.channel_role_ids.is_none() {
        running_write.channel_role_ids = Some(roles);
    }
    running_write.channel_role_ids
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
        sync_anonymous_role_statuses(ctx, running).await;
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
        sync_anonymous_role_statuses(ctx, running).await;
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
        sync_anonymous_role_statuses(ctx, running).await;
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

pub(crate) fn swapped_member_roles(
    member_roles: &[serenity::RoleId],
    remove_role: Option<serenity::RoleId>,
    add_role: Option<serenity::RoleId>,
) -> Option<Vec<serenity::RoleId>> {
    let mut roles = member_roles
        .iter()
        .copied()
        .filter(|role_id| Some(*role_id) != remove_role)
        .collect::<Vec<_>>();
    if let Some(role_id) = add_role
        && !roles.contains(&role_id)
    {
        roles.push(role_id);
    }
    (roles != member_roles).then_some(roles)
}

pub(crate) async fn swap_member_game_roles(
    ctx: &serenity::Context,
    guild_id: serenity::GuildId,
    member: &serenity::Member,
    remove_role: Option<serenity::RoleId>,
    add_role: Option<serenity::RoleId>,
) -> bool {
    let Some(roles) = swapped_member_roles(&member.roles, remove_role, add_role) else {
        return true;
    };
    let user_id = member.user.id;
    match crate::http_pool::with_fallback(ctx, |http| {
        let roles = roles.clone();
        async move {
            guild_id
                .edit_member(&http, user_id, serenity::EditMember::new().roles(roles))
                .await
        }
    })
    .await
    {
        Ok(_) => true,
        Err(error) => {
            eprintln!(
                "failed to swap game roles: guild_id={} user_id={} remove={remove_role:?} add={add_role:?} error={error:?}",
                guild_id.get(),
                member.user.id.get()
            );
            false
        }
    }
}

pub async fn restore_revived_player_roles(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    roles: ChannelRoleIds,
    player: &Player,
) {
    let guild_id = running.read().await.guild_id;
    {
        let mut running_write = running.write().await;
        running_write.dead_chat_unlocked_ids.remove(&player.user_id);
        running_write
            .pending_dead_chat_user_ids
            .remove(&player.user_id);
        running_write
            .dead_role_chat_visible_from_days
            .remove(&player.user_id);
    }
    if let Ok(member) = guild_id
        .member(ctx, serenity::UserId::new(player.user_id))
        .await
    {
        swap_member_game_roles(ctx, guild_id, &member, roles.dead, roles.participant).await;
    }
    set_shaman_channel_member_access(ctx, running, player, false, false).await;
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
        apply_permission_if_changed(
            ctx,
            running,
            channel_id,
            anonymous_input_overwrite(
                serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id)),
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
    sync_anonymous_role_statuses(ctx, running).await;
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
    {
        let mut running_write = running.write().await;
        for user_id in purified_user_ids {
            running_write.dead_chat_unlocked_ids.remove(user_id);
            running_write.pending_dead_chat_user_ids.remove(user_id);
            running_write
                .dead_role_chat_visible_from_days
                .remove(user_id);
        }
    }
    let anonymous_enabled = running.read().await.anonymous_enabled;
    let roles = match running_channel_roles(ctx, data, running).await {
        Some(roles) => roles,
        None => return,
    };
    let category = running_source_category(ctx, running).await;
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
            "마담에게 유혹당했습니다. 다음 낮 투표가 끝날 때까지 능력을 사용할 수 없고 말할 수 없습니다.\n마피아팀이라면 능력 사용은 가능하지만, 유혹 중에는 마피아 비밀방에도 말할 수 없습니다.",
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

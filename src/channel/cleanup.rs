// channel/cleanup.rs — 게임 종료 시 채널·역할·권한 정리

use super::*;

pub(crate) const CLEANUP_RETRY_ATTEMPTS: usize = 3;
pub(crate) const CLEANUP_DELETE_CONCURRENCY: usize = 4;

pub(crate) async fn delete_game_channel_with_retry(
    http: &serenity::Http,
    channel_id: serenity::ChannelId,
) -> std::result::Result<(), String> {
    let mut last_error = None;
    for attempt in 1..=CLEANUP_RETRY_ATTEMPTS {
        match channel_id.delete(http).await {
            Ok(_) => return Ok(()),
            Err(error) => {
                last_error = Some(format!("{error:?}"));
                if attempt < CLEANUP_RETRY_ATTEMPTS {
                    tokio::time::sleep(Duration::from_millis(250 * attempt as u64)).await;
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| "unknown Discord error".to_string()))
}

pub(crate) async fn delete_game_channels(
    ctx: &serenity::Context,
    channel_ids: impl IntoIterator<Item = serenity::ChannelId>,
) -> (usize, Vec<String>) {
    let mut channel_ids = channel_ids.into_iter().collect::<Vec<_>>();
    channel_ids.sort_by_key(|id| id.get());
    channel_ids.dedup();
    let mut deleted = 0;
    let mut failures = Vec::new();

    for batch in channel_ids.chunks(CLEANUP_DELETE_CONCURRENCY) {
        let mut jobs = JoinSet::new();
        for &channel_id in batch {
            let http = ctx.http.clone();
            jobs.spawn(async move {
                (
                    channel_id,
                    delete_game_channel_with_retry(http.as_ref(), channel_id).await,
                )
            });
        }
        while let Some(result) = jobs.join_next().await {
            match result {
                Ok((_, Ok(()))) => deleted += 1,
                Ok((channel_id, Err(error))) => {
                    failures.push(format!("channel_id={} error={error}", channel_id.get()))
                }
                Err(error) => failures.push(format!("cleanup task failed: {error:?}")),
            }
        }
    }
    (deleted, failures)
}

pub(crate) async fn restore_permission_with_retry(
    ctx: &serenity::Context,
    running: Option<&Arc<RwLock<RunningGame>>>,
    channel_id: serenity::ChannelId,
    kind: serenity::PermissionOverwriteType,
    original: Option<serenity::PermissionOverwrite>,
) -> bool {
    let key = permission_cache_key(channel_id, kind);
    if let (Some(running), Some(key)) = (running, key) {
        let current = running
            .read()
            .await
            .permission_overwrite_cache
            .get(&key)
            .cloned();
        if current == original {
            return true;
        }
    }
    let mut last_error = None;
    for attempt in 1..=CLEANUP_RETRY_ATTEMPTS {
        let result = if let Some(overwrite) = original.clone() {
            crate::http_pool::with_fallback(ctx, |http| {
                let overwrite = overwrite.clone();
                async move { channel_id.create_permission(&http, overwrite).await }
            })
            .await
        } else {
            crate::http_pool::with_fallback(ctx, |http| async move {
                channel_id.delete_permission(&http, kind).await
            })
            .await
            .map(|_| ())
        };
        match result {
            Ok(()) => {
                if let (Some(running), Some(key)) = (running, key) {
                    let mut running_write = running.write().await;
                    if let Some(overwrite) = original.clone() {
                        running_write
                            .permission_overwrite_cache
                            .insert(key, overwrite);
                    } else {
                        running_write.permission_overwrite_cache.remove(&key);
                    }
                }
                return true;
            }
            Err(error) => {
                last_error = Some(error);
                if attempt < CLEANUP_RETRY_ATTEMPTS {
                    tokio::time::sleep(Duration::from_millis(250 * attempt as u64)).await;
                }
            }
        }
    }
    eprintln!(
        "failed to restore game channel permission: channel_id={} kind={kind:?} error={last_error:?}",
        channel_id.get(),
    );
    false
}

pub async fn cleanup_game(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) {
    // Block in-flight personal dead-chat creation before collecting channels to delete.
    running.write().await.game.phase = Phase::Ended;
    restore_channel_slowmode(ctx, running).await;
    remove_slowmode_bypass_overwrites(ctx, running).await;
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
        channel_ids
    };

    let (_, delete_failures) = delete_game_channels(ctx, channel_ids).await;
    for failure in delete_failures {
        eprintln!("failed to delete game channel after retries: {failure}");
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
    if let Some(roles) = running_channel_roles(ctx, data, running).await {
        let participant_cleanup_role_ids = [roles.participant, roles.dead]
            .into_iter()
            .flatten()
            .collect::<HashSet<_>>();
        let mut cleanup_targets = participant_user_ids
            .into_iter()
            .map(|user_id| (user_id, participant_cleanup_role_ids.clone()))
            .collect::<HashMap<_, _>>();
        if let Some(role_id) = roles.spectator {
            for user_id in spectator_user_ids {
                cleanup_targets.entry(user_id).or_default().insert(role_id);
            }
        }
        let mut member_snapshots = guild_id
            .members(&ctx.http, Some(1000), None)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|member| (member.user.id.get(), member))
            .collect::<HashMap<_, _>>();
        for (user_id, role_ids) in cleanup_targets {
            let member = if let Some(member) = member_snapshots.remove(&user_id) {
                Some(member)
            } else {
                guild_id
                    .member(ctx, serenity::UserId::new(user_id))
                    .await
                    .ok()
            };
            if let Some(member) = member {
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

    let (source_channel_id, original_overwrites) = {
        let running_read = running.read().await;
        (
            running_read.channel_id,
            running_read.original_game_channel_overwrites.clone(),
        )
    };
    for (role_id, overwrite) in original_overwrites {
        restore_permission_with_retry(
            ctx,
            Some(running),
            source_channel_id,
            serenity::PermissionOverwriteType::Role(role_id),
            overwrite,
        )
        .await;
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
    running_write.dead_chat_unlocked_ids.clear();
    running_write.pending_dead_chat_user_ids.clear();
    running_write.dead_role_chat_visible_from_days.clear();
    running_write.anonymous_shaman_input_channel_ids.clear();
    running_write.anonymous_shaman_input_channel_owners.clear();
    running_write.anonymous_role_input_channel_ids.clear();
    running_write.anonymous_role_input_channels.clear();
    running_write
        .anonymous_role_input_status_message_ids
        .clear();
    running_write.anonymous_role_status_texts.clear();
    running_write.anonymous_aliases.clear();
    running_write.anonymous_original_names.clear();
    running_write.anonymous_webhooks.clear();
    running_write.anonymous_webhook_creation_locks.clear();
    running_write.channel_role_ids = None;
    running_write.source_category_id = None;
    running_write.permission_overwrite_cache.clear();
    running_write.verified_member_ids.clear();
    running_write.personal_channel_creation_locks.clear();
    running_write.original_game_channel_overwrites.clear();
    running_write.game_channel_overwrites.clear();
    running_write.member_channel_overwrites.clear();
    running_write.original_slowmode_delays.clear();
    running_write.channel_slowmode_cache.clear();
    running_write.shaman_channel_id = None;
    running_write.shaman_status_message_id = None;
    running_write.shaman_status_text = None;
    running_write.frog_game_channel_overwrites.clear();
    running_write.madam_seduction_channel_overwrites.clear();
}

#[derive(Default)]
pub(crate) struct ForcedCleanupSummary {
    pub channels_deleted: usize,
    pub channel_delete_failures: usize,
    pub role_removals: usize,
    pub permissions_reset: usize,
}

pub(crate) fn stripped_game_permission_overwrite(
    mut overwrite: serenity::PermissionOverwrite,
    clear_view: bool,
) -> Option<serenity::PermissionOverwrite> {
    let mut bits = serenity::Permissions::SEND_MESSAGES
        | serenity::Permissions::SEND_MESSAGES_IN_THREADS
        | serenity::Permissions::ADD_REACTIONS
        | serenity::Permissions::CREATE_PUBLIC_THREADS
        | serenity::Permissions::CREATE_PRIVATE_THREADS;
    if clear_view {
        bits |= serenity::Permissions::VIEW_CHANNEL | serenity::Permissions::READ_MESSAGE_HISTORY;
    }
    overwrite.allow.remove(bits);
    overwrite.deny.remove(bits);
    (!overwrite.allow.is_empty() || !overwrite.deny.is_empty()).then_some(overwrite)
}

pub(crate) async fn cleanup_orphaned_main_channel_permissions(
    ctx: &serenity::Context,
    channel_id: serenity::ChannelId,
    roles: ChannelRoleIds,
    game_member_ids: &HashSet<u64>,
) -> usize {
    let Some(channel) = channel_id
        .to_channel(&ctx.http)
        .await
        .ok()
        .and_then(|channel| channel.guild())
    else {
        eprintln!(
            "failed to load main channel for forced permission cleanup: channel_id={}",
            channel_id.get()
        );
        return 0;
    };
    let game_role_ids = [roles.participant, roles.dead, roles.spectator]
        .into_iter()
        .flatten()
        .collect::<HashSet<_>>();
    let mut reset = 0;
    for overwrite in channel.permission_overwrites {
        let (managed, clear_view) = match overwrite.kind {
            serenity::PermissionOverwriteType::Role(role_id) if role_id == roles.everyone => {
                (true, false)
            }
            serenity::PermissionOverwriteType::Role(role_id)
                if game_role_ids.contains(&role_id) =>
            {
                (true, true)
            }
            serenity::PermissionOverwriteType::Member(user_id)
                if user_id == roles.bot || game_member_ids.contains(&user_id.get()) =>
            {
                (true, true)
            }
            _ => (false, false),
        };
        if !managed {
            continue;
        }
        let kind = overwrite.kind;
        let cleaned = stripped_game_permission_overwrite(overwrite, clear_view);
        if restore_permission_with_retry(ctx, None, channel_id, kind, cleaned).await {
            reset += 1;
        }
    }
    reset
}

pub async fn cleanup_orphaned_game_artifacts(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
    source_channel_id: serenity::ChannelId,
    full_role_sweep: bool,
) -> ForcedCleanupSummary {
    let config = data.config.read().await.clone();
    let roles = channel_role_ids(ctx, guild_id, &config, data.bot_user_id)
        .await
        .ok();
    let mut summary = ForcedCleanupSummary::default();

    if let Ok(channels) = guild_id.channels(&ctx.http).await {
        let channel_ids = channels
            .into_values()
            .filter(|channel| should_force_delete_game_channel(channel, roles))
            .map(|channel| channel.id)
            .collect::<Vec<_>>();
        let (deleted, failures) = delete_game_channels(ctx, channel_ids).await;
        summary.channels_deleted += deleted;
        summary.channel_delete_failures += failures.len();
        for failure in failures {
            eprintln!("failed to force-delete game channel after retries: {failure}");
        }
    }

    let Some(roles) = roles else {
        return summary;
    };
    let role_ids = [roles.participant, roles.dead, roles.spectator]
        .into_iter()
        .flatten()
        .collect::<HashSet<_>>();
    if !full_role_sweep {
        return summary;
    }

    let mut game_member_ids = HashSet::new();
    let mut after = None;
    loop {
        let Ok(members) = guild_id.members(&ctx.http, Some(1000), after).await else {
            break;
        };
        if members.is_empty() {
            break;
        }
        for member in &members {
            if member
                .roles
                .iter()
                .any(|role_id| role_ids.contains(role_id))
            {
                game_member_ids.insert(member.user.id.get());
            }
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

    summary.permissions_reset +=
        cleanup_orphaned_main_channel_permissions(ctx, source_channel_id, roles, &game_member_ids)
            .await;

    summary
}

pub(crate) async fn remove_cleanup_roles_from_member_snapshot(
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
    let edited = crate::http_pool::with_fallback(ctx, |http| {
        let remaining_roles = remaining_roles.clone();
        async move {
            guild_id
                .edit_member(
                    &http,
                    user_id,
                    serenity::EditMember::new().roles(remaining_roles),
                )
                .await
        }
    })
    .await;
    if edited.is_ok() { removed } else { 0 }
}

pub(crate) fn should_force_delete_game_channel(
    channel: &serenity::GuildChannel,
    roles: Option<ChannelRoleIds>,
) -> bool {
    if channel.kind != serenity::ChannelType::Text {
        return false;
    }
    let name = channel.name.as_str();
    if name == SHAMAN_CHAT_CHANNEL_NAME
        || name == LEGACY_FROG_CHAT_CHANNEL_NAME
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

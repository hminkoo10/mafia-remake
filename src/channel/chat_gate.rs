// channel/chat_gate.rs — 게임 채널 채팅 개폐·슬로우모드·사망 처리

use super::*;

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
    let updates = updates
        .into_iter()
        .map(|(channel_id, user_id, can_chat)| {
            (
                channel_id,
                anonymous_input_overwrite(
                    serenity::PermissionOverwriteType::Member(serenity::UserId::new(user_id)),
                    true,
                    can_chat,
                ),
            )
        })
        .collect();
    apply_permission_updates(ctx, running, updates).await;
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
    let mut targets = vec![(roles.everyone, false)];
    if let Some(participant_role_id) = roles.participant {
        targets.push((participant_role_id, participants_can_chat));
    }
    let targets = {
        let running_read = running.read().await;
        targets
            .into_iter()
            .map(|(role_id, can_chat)| {
                let kind = serenity::PermissionOverwriteType::Role(role_id);
                (
                    role_id,
                    can_chat,
                    remembered_permission(&running_read, channel_id, kind),
                )
            })
            .collect::<Vec<_>>()
    };
    let channel = if targets.iter().any(|(_, _, current)| current.is_none()) {
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
    for (role_id, can_chat, remembered) in targets {
        let kind = serenity::PermissionOverwriteType::Role(role_id);
        let current = remembered.or_else(|| {
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
        apply_permission_if_changed(ctx, running, channel_id, overwrite).await;
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
    let kind = serenity::PermissionOverwriteType::Member(serenity::UserId::new(player.user_id));
    let remembered = {
        let running_read = running.read().await;
        remembered_permission(&running_read, channel_id, kind)
    };
    let current = if remembered.is_some() {
        remembered
    } else {
        match channel_id
            .to_channel(&ctx.http)
            .await
            .ok()
            .and_then(|channel| channel.guild())
        {
            Some(channel) => channel
                .permission_overwrites
                .iter()
                .find(|overwrite| overwrite.kind == kind)
                .cloned(),
            None => {
                // 게임 시작 직후(첫 밤)에는 채널 생성 러시로 이 조회가 실패할 수
                // 있다. 여기서 포기하면 [확성] 등 멤버 권한이 조용히 안 열리므로
                // 빈 오버라이트를 바탕으로 계속 진행한다.
                eprintln!(
                    "failed to fetch game channel overwrites; proceeding with empty base: channel_id={} user_id={}",
                    channel_id.get(),
                    player.user_id
                );
                None
            }
        }
    };
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
    apply_permission_if_changed(ctx, running, channel_id, overwrite).await;
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
        restore_permission_with_retry(ctx, Some(running), channel_id, kind, original).await;
    }
}

pub async fn restore_game_channel_chat(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
) {
    let (channel_id, originals) = {
        let mut running_write = running.write().await;
        (
            running_write.channel_id,
            std::mem::take(&mut running_write.game_channel_overwrites),
        )
    };
    for (role_id, original) in originals {
        let kind = serenity::PermissionOverwriteType::Role(role_id);
        restore_permission_with_retry(ctx, Some(running), channel_id, kind, original).await;
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
    let slowmode = seconds.min(21600) as u16;
    let cached = running
        .read()
        .await
        .channel_slowmode_cache
        .get(&channel_id)
        .copied();
    if cached == Some(slowmode) {
        return;
    }
    if cached.is_none() {
        let Some(channel) = channel_id
            .to_channel(&ctx.http)
            .await
            .ok()
            .and_then(|channel| channel.guild())
        else {
            return;
        };
        let current = channel.rate_limit_per_user.unwrap_or(0);
        let mut running_write = running.write().await;
        running_write
            .original_slowmode_delays
            .entry(channel_id)
            .or_insert(current);
        running_write
            .channel_slowmode_cache
            .insert(channel_id, current);
        if current == slowmode {
            return;
        }
    }
    match crate::http_pool::with_fallback(ctx, |http| async move {
        channel_id
            .edit(
                &http,
                serenity::EditChannel::new().rate_limit_per_user(slowmode),
            )
            .await
    })
    .await
    {
        Ok(_) => {
            running
                .write()
                .await
                .channel_slowmode_cache
                .insert(channel_id, slowmode);
        }
        Err(error) => {
            eprintln!("failed to set slowmode for {}: {error:?}", channel_id.get());
        }
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
        if let Err(error) = crate::http_pool::with_fallback(ctx, |http| async move {
            channel_id
                .edit(
                    &http,
                    serenity::EditChannel::new().rate_limit_per_user(delay),
                )
                .await
        })
        .await
        {
            eprintln!(
                "failed to restore slowmode for {}: {error:?}",
                channel_id.get()
            );
        }
    }
}

pub async fn unlock_pending_dead_chats(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) {
    let (channel_id, anonymous_enabled, should_unlock) = {
        let running_read = running.read().await;
        (
            running_read.channel_id,
            running_read.anonymous_enabled,
            !dead_chat_unlock_candidates(&running_read).is_empty(),
        )
    };
    if !should_unlock {
        return;
    }
    let roles = match running_channel_roles(ctx, data, running).await {
        Some(roles) => roles,
        None => {
            eprintln!("failed to load roles for dead chat unlock");
            let _ = send_channel_embed(
                &ctx.http,
                channel_id,
                "사망자 채팅방 생성에 필요한 서버 역할 정보를 불러오지 못했습니다. 봇 권한을 확인하세요.",
                "사망자 채팅 생성 실패",
                serenity::Colour::RED,
                vec![],
            )
            .await;
            return;
        }
    };
    let category = running_source_category(ctx, running).await;
    let players = {
        let mut running_write = running.write().await;
        if !matches!(running_write.game.phase, Phase::Day | Phase::Night) {
            return;
        }
        let players = dead_chat_unlock_candidates(&running_write);
        for player in &players {
            running_write
                .pending_dead_chat_user_ids
                .remove(&player.user_id);
            running_write.dead_chat_unlocked_ids.insert(player.user_id);
        }
        players
    };
    if players.is_empty() {
        return;
    }
    let mut failed_dead_chat_names = Vec::new();
    for player in &players {
        let can_dead_chat = {
            let running_read = running.read().await;
            running_read
                .game
                .get_player(player.user_id)
                .is_some_and(|player| can_use_anonymous_dead_chat(&running_read, player))
        };
        set_shaman_channel_member_access(ctx, running, player, true, can_dead_chat).await;
        if can_dead_chat {
            if ensure_anonymous_dead_input_channel(ctx, running, player, roles, category, true)
                .await
                .is_none()
            {
                failed_dead_chat_names.push(player.name.clone());
            }
        }
        if anonymous_enabled && running.read().await.shaman_channel_id.is_some() {
            let can_shaman_chat = {
                let running_read = running.read().await;
                running_read
                    .game
                    .get_player(player.user_id)
                    .is_some_and(|player| can_use_anonymous_shaman_chat(&running_read, player))
            };
            if can_shaman_chat {
                let _ = ensure_anonymous_shaman_input_channel(
                    ctx, running, player, roles, category, true,
                )
                .await;
            }
        }
    }
    if !failed_dead_chat_names.is_empty() {
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            format!(
                "사망자 개인 채팅방을 만들 수 없는 참가자: {}",
                failed_dead_chat_names.join(", ")
            ),
            "사망자 채팅 생성 실패",
            serenity::Colour::RED,
            vec![],
        )
        .await;
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
    {
        let mut running_write = running.write().await;
        record_dead_chat_deaths(&mut running_write, dead_players);
    }
    let (guild_id, channel_id) = {
        let running_read = running.read().await;
        (running_read.guild_id, running_read.channel_id)
    };
    let roles = match running_channel_roles(ctx, data, running).await {
        Some(roles) => roles,
        None => {
            eprintln!("failed to load roles for death side effects");
            let _ = send_channel_embed(
                &ctx.http,
                channel_id,
                "사망자 역할/채팅방 처리에 필요한 서버 역할 정보를 불러오지 못했습니다. 봇 권한을 확인하세요.",
                "사망 처리 실패",
                serenity::Colour::RED,
                vec![],
            )
            .await;
            return;
        }
    };
    let mut failed_dead_chat_names = Vec::new();
    for player in dead_players {
        if let Ok(member) = guild_id
            .member(ctx, serenity::UserId::new(player.user_id))
            .await
        {
            swap_member_game_roles(ctx, guild_id, &member, roles.participant, roles.dead).await;
        }
        let can_dead_chat = {
            let running_read = running.read().await;
            running_read
                .game
                .get_player(player.user_id)
                .is_some_and(|player| can_use_anonymous_dead_chat(&running_read, player))
        };
        set_shaman_channel_member_access(ctx, running, player, true, can_dead_chat).await;
        restore_frog_game_channel_permission(ctx, running, player).await;
        disable_private_role_channels_for_player(ctx, running, player).await;
    }
    let category = running_source_category(ctx, running).await;
    let anonymous_enabled = running.read().await.anonymous_enabled;
    for player in dead_players {
        let can_chat = {
            let running_read = running.read().await;
            running_read
                .game
                .get_player(player.user_id)
                .is_some_and(|player| can_use_anonymous_dead_chat(&running_read, player))
        };
        let dead_channel_exists = running
            .read()
            .await
            .anonymous_dead_input_channel_ids
            .contains_key(&player.user_id);
        if can_chat || dead_channel_exists {
            if ensure_anonymous_dead_input_channel(ctx, running, player, roles, category, can_chat)
                .await
                .is_none()
                && can_chat
            {
                failed_dead_chat_names.push(player.name.clone());
            }
        }
        if anonymous_enabled && running.read().await.shaman_channel_id.is_some() {
            let can_shaman_chat = {
                let running_read = running.read().await;
                running_read
                    .game
                    .get_player(player.user_id)
                    .is_some_and(|player| can_use_anonymous_shaman_chat(&running_read, player))
            };
            let shaman_channel_exists = running
                .read()
                .await
                .anonymous_shaman_input_channel_ids
                .contains_key(&player.user_id);
            if can_shaman_chat || shaman_channel_exists {
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
    }
    if !failed_dead_chat_names.is_empty() {
        let _ = send_channel_embed(
            &ctx.http,
            channel_id,
            format!(
                "사망자 개인 채팅방을 만들 수 없는 참가자: {}",
                failed_dead_chat_names.join(", ")
            ),
            "사망자 채팅 생성 실패",
            serenity::Colour::RED,
            vec![],
        )
        .await;
    }
    if anonymous_enabled {
        sync_anonymous_general_chat_permissions(ctx, running).await;
    }
}

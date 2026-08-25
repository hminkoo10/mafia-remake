// commands/anonymous_chat.rs — 익명 채팅 전송·릴레이와 메시지 이벤트 처리 (확성 포함)

use super::*;

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
    if let Some(webhook) = running
        .read()
        .await
        .anonymous_webhooks
        .get(&channel_id)
        .cloned()
    {
        return Some(webhook);
    }

    let creation_lock = {
        let mut running_write = running.write().await;
        running_write
            .anonymous_webhook_creation_locks
            .entry(channel_id)
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    };
    let _creation_guard = creation_lock.lock().await;
    if let Some(webhook) = running
        .read()
        .await
        .anonymous_webhooks
        .get(&channel_id)
        .cloned()
    {
        return Some(webhook);
    }

    let webhook = match crate::http_pool::with_fallback(ctx, |http| async move {
        channel_id
            .create_webhook(
                &http,
                serenity::CreateWebhook::new("Mafia Anonymous")
                    .audit_log_reason("마피아 게임 익명 채팅 웹훅 생성"),
            )
            .await
    })
    .await
    {
        Ok(webhook) => webhook,
        Err(error) => {
            eprintln!(
                "failed to create anonymous webhook for channel {}: {error:?}",
                channel_id.get()
            );
            return None;
        }
    };
    running
        .write()
        .await
        .anonymous_webhooks
        .insert(channel_id, webhook.clone());
    Some(webhook)
}

pub(crate) async fn send_anonymous_text_batch(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    channel_ids: Vec<serenity::ChannelId>,
    author_label: &str,
    body: &str,
) {
    for chunk in channel_ids.chunks(ANONYMOUS_DELIVERY_CONCURRENCY) {
        let mut deliveries = JoinSet::new();
        for &channel_id in chunk {
            let ctx = ctx.clone();
            let running = Arc::clone(running);
            let author_label = author_label.to_string();
            let body = body.to_string();
            deliveries.spawn(async move {
                send_anonymous_text(&ctx, &running, channel_id, &author_label, &body).await;
            });
        }
        while deliveries.join_next().await.is_some() {}
    }
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
        match webhook.execute(&ctx.http, false, builder).await {
            Ok(_) => return,
            Err(error) => eprintln!(
                "failed to execute anonymous webhook {} in channel {}: {error:?}",
                webhook.id.get(),
                channel_id.get()
            ),
        }
    }
    if let Err(error) = channel_id
        .send_message(
            &ctx.http,
            serenity::CreateMessage::new()
                .content(format!("{author_label}: {body}"))
                .allowed_mentions(no_mentions()),
        )
        .await
    {
        eprintln!(
            "failed to send anonymous fallback in channel {}: {error:?}",
            channel_id.get()
        );
    }
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
        match webhook.execute(&ctx.http, false, builder).await {
            Ok(_) => return,
            Err(error) => eprintln!(
                "failed to execute relayed webhook {} in channel {}: {error:?}",
                webhook.id.get(),
                channel_id.get()
            ),
        }
    }
    if let Err(error) = channel_id
        .send_message(
            &ctx.http,
            serenity::CreateMessage::new()
                .content(format!("{author_label}: {body}"))
                .allowed_mentions(no_mentions()),
        )
        .await
    {
        eprintln!(
            "failed to send relayed fallback in channel {}: {error:?}",
            channel_id.get()
        );
    }
}

pub fn message_author_display_name(message: &serenity::Message) -> String {
    message
        .member
        .as_ref()
        .and_then(|member| member.nick.clone())
        .or_else(|| message.author.global_name.clone())
        .unwrap_or_else(|| message.author.name.clone())
}

/// 익명 게임의 모든 발신자 표기. 역할이나 생사에 따라 다른 라벨을 붙이지 않고
/// 설정된 익명 이름(번호 또는 동물)만 쓴다.
pub fn anonymous_sender_label(running: &RunningGame, sender: &Player) -> String {
    if running.anonymous_enabled {
        running
            .anonymous_aliases
            .get(&sender.user_id)
            .cloned()
            .unwrap_or_else(|| "익명".to_string())
    } else {
        sender.name.clone()
    }
}

pub(crate) fn anonymous_shaman_recipient_ids(running: &RunningGame, sender_id: u64) -> Vec<u64> {
    running
        .game
        .players
        .iter()
        .filter(|viewer| {
            viewer.user_id != sender_id && can_use_anonymous_shaman_chat(running, viewer)
        })
        .map(|viewer| viewer.user_id)
        .collect()
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
            anonymous_sender_label(&running_read, sender),
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
    // 발신자 표기는 루프 밖에서 한 번만 정한다. 익명 게임이면 실명이 사망자 채팅으로
    // 새지 않도록 별명을 쓴다.
    let (sender_label, sender_avatar, viewers) = {
        let running_read = running.read().await;
        let Some(sender) = running_read.game.get_player(message.author.id.get()) else {
            return;
        };
        let (sender_label, sender_avatar) = if running_read.anonymous_enabled {
            (anonymous_sender_label(&running_read, sender), None)
        } else {
            (
                message_author_display_name(message),
                Some(message.author.face()),
            )
        };
        let viewers = running_read
            .game
            .players
            .iter()
            .filter(|player| can_receive_role_chat_as_dead(&running_read, player))
            .cloned()
            .collect::<Vec<_>>();
        (sender_label, sender_avatar, viewers)
    };
    if viewers.is_empty() {
        return;
    }
    let category = running_source_category(ctx, running).await;
    let body = format!("[{}채팅] {body}", role.value());
    for viewer in viewers {
        let (can_receive, can_chat) = {
            let running_read = running.read().await;
            running_read
                .game
                .get_player(viewer.user_id)
                .map_or((false, false), |player| {
                    (
                        can_receive_role_chat_as_dead(&running_read, player),
                        can_use_anonymous_dead_chat(&running_read, player),
                    )
                })
        };
        if !can_receive {
            continue;
        }
        if let Some(channel_id) =
            ensure_anonymous_dead_input_channel(ctx, running, &viewer, roles, category, can_chat)
                .await
        {
            send_webhook_text(
                ctx,
                running,
                channel_id,
                &sender_label,
                sender_avatar.clone(),
                &body,
            )
            .await;
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
    send_anonymous_text_batch(ctx, running, deliveries, &sender_alias, body).await;
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
    send_anonymous_text_batch(ctx, running, deliveries, &sender_alias, body).await;
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
    let (sender, viewers) = {
        let running_read = running.read().await;
        let Some(sender) = running_read.game.get_player(sender_id) else {
            return;
        };
        (
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
    let category = running_source_category(ctx, running).await;
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
    let (deliveries, sender_label) = {
        let running_read = running.read().await;
        let Some(sender) = running_read.game.get_player(sender_id) else {
            return;
        };
        let deliveries = anonymous_shaman_recipient_ids(&running_read, sender.user_id)
            .into_iter()
            .filter_map(|viewer_id| {
                running_read
                    .anonymous_shaman_input_channel_ids
                    .get(&viewer_id)
                    .copied()
            })
            .collect::<Vec<_>>();
        (deliveries, anonymous_sender_label(&running_read, sender))
    };
    send_anonymous_text_batch(ctx, running, deliveries, &sender_label, body).await;
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
    let (can_relay, is_frog) = {
        let running_read = running.read().await;
        let Some(player) = running_read.game.get_player(owner_id) else {
            return Ok(());
        };
        let is_frog = running_read.game.is_frog(player);
        let can_relay = match kind {
            AnonymousMessageKind::General { .. } => {
                can_use_anonymous_general_chat(&running_read, player)
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
        };
        (can_relay, is_frog)
    };
    if is_frog {
        let _ = message.delete(&ctx.http).await;
        return Ok(());
    }
    if !can_relay {
        return Ok(());
    }

    match kind {
        AnonymousMessageKind::General { .. } => {
            relay_anonymous_general_message(ctx, &running, owner_id, &body).await;
            // [확성] 밤 메시지는 보유자 전체에서 밤당 1회 + 인당 게임 중 1회.
            // 첫 사용이 확인되면 소모 처리하고 모든 보유자의 입력을 닫는다.
            let used_now = {
                let mut running_write = running.write().await;
                let is_night_loudspeaker = running_write.game.phase == Phase::Night
                    && running_write
                        .game
                        .get_player(owner_id)
                        .is_some_and(|player| running_write.game.is_loudspeaker_active(player));
                if is_night_loudspeaker {
                    running_write.game.mark_loudspeaker_used(owner_id);
                }
                is_night_loudspeaker
            };
            if used_now {
                close_loudspeakers_after_use(ctx, &running).await;
            }
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

/// [확성] 사용 직후 모든 보유자의 밤 채팅을 닫는다 (익명 게임은 릴레이 판정이
/// 이미 닫혔으므로 권한 동기화만 일어난다).
pub async fn close_loudspeakers_after_use(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
) {
    let holders = {
        let running_read = running.read().await;
        running_read.game.loudspeaker_holders()
    };
    for holder in holders {
        set_member_game_channel_chat(ctx, running, &holder, false).await;
    }
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
                .map(|player| {
                    (
                        player.clone(),
                        private_role_member_can_chat(&running_read.game, role, player),
                        private_role_member_can_view(&running_read.game, role, player),
                    )
                })
        };
        if let Some((player, can_relay, can_view)) = player {
            if !can_relay {
                let _ = message.delete(&ctx.http).await;
                set_private_role_member_view_access(ctx, &running, role, &player, can_view, false)
                    .await;
            } else {
                let body = anonymous_message_body(message);
                mirror_role_chat_to_dead(ctx, data, &running, message, role, &body).await;
            }
        }
        return Ok(());
    }

    let shaman_silenced = {
        let running_read = running.read().await;
        if running_read.shaman_channel_id == Some(message.channel_id) {
            running_read
                .game
                .get_player(message.author.id.get())
                .filter(|player| is_player_chat_silenced(&running_read, player))
                .cloned()
        } else {
            None
        }
    };
    if let Some(player) = shaman_silenced {
        let _ = message.delete(&ctx.http).await;
        set_shaman_channel_member_access(ctx, &running, &player, true, false).await;
        return Ok(());
    }

    // [확성] 일반 게임의 밤 채팅: 첫 메시지가 그 밤의 사용을 소모하고, 이미
    // 사용된 뒤의 보유자 메시지는 삭제한다.
    enum LoudspeakerAction {
        None,
        FirstUse,
        Blocked,
    }
    let loudspeaker_action = {
        let running_read = running.read().await;
        if running_read.channel_id == message.channel_id
            && running_read.game.phase == Phase::Night
            && !running_read.anonymous_enabled
        {
            match running_read.game.get_player(message.author.id.get()) {
                Some(player)
                    if running_read.game.has_tier_ability(
                        player.user_id,
                        mafia_remake::model::TierAbility::Loudspeaker,
                    ) =>
                {
                    if running_read.game.is_loudspeaker_active(player) {
                        LoudspeakerAction::FirstUse
                    } else {
                        LoudspeakerAction::Blocked
                    }
                }
                _ => LoudspeakerAction::None,
            }
        } else {
            LoudspeakerAction::None
        }
    };
    match loudspeaker_action {
        LoudspeakerAction::FirstUse => {
            running
                .write()
                .await
                .game
                .mark_loudspeaker_used(message.author.id.get());
            close_loudspeakers_after_use(ctx, &running).await;
            return Ok(());
        }
        LoudspeakerAction::Blocked => {
            let _ = message.delete(&ctx.http).await;
            return Ok(());
        }
        LoudspeakerAction::None => {}
    }

    let frog_game_message = {
        let running_read = running.read().await;
        running_read.channel_id == message.channel_id
            && running_read
                .game
                .get_player(message.author.id.get())
                .is_some_and(|player| running_read.game.is_frog(player))
    };
    if frog_game_message {
        let _ = message.delete(&ctx.http).await;
        return Ok(());
    }
    Ok(())
}

from __future__ import annotations

from bot import *  # noqa: F401,F403


__all__ = (
    'anonymous_message_body',
    'can_use_anonymous_general_chat',
    'can_use_anonymous_dead_chat',
    'can_use_anonymous_shaman_chat',
    'can_use_anonymous_role_chat',
    'anonymous_avatar_url',
    'send_anonymous_text',
    'send_member_webhook_text',
    'anonymous_webhook',
    'prepare_anonymous_webhook',
    'relay_to_channels',
    'send_anonymous_log',
    'relay_anonymous_general_message',
    'anonymous_dead_chat_viewers',
    'anonymous_shaman_chat_viewers',
    'anonymous_dead_sender_label',
    'send_dead_chat_text',
    'relay_anonymous_dead_message',
    'send_anonymous_shaman_log',
    'relay_anonymous_shaman_message',
    'relay_anonymous_role_message',
    'mirror_role_chat_to_dead',
    'set_anonymous_general_input_access',
    'set_anonymous_channel_slowmode',
    'ensure_anonymous_dead_input_channel',
    'set_anonymous_dead_input_access',
    'ensure_anonymous_shaman_input_channel',
    'set_anonymous_shaman_input_access',
    'set_anonymous_general_chat_permissions',
    'delete_message_quietly',
    'handle_anonymous_message',
    'on_message',
)


def anonymous_message_body(message: discord.Message) -> str:
    parts: list[str] = []
    if message.content.strip():
        parts.append(message.content.strip())
    if message.attachments:
        parts.extend(attachment.url for attachment in message.attachments)
    return "\n".join(parts) or "(내용 없음)"


def can_use_anonymous_general_chat(running: RunningGame, player: Player) -> bool:
    if not player.alive:
        return False
    if running.game.is_frog(player):
        return False
    if running.game.is_madam_seduced(player):
        return False
    if running.game.phase == Phase.DAY:
        return True
    return running.game.phase == Phase.FINAL_DEFENSE and running.final_defense_user_id == player.user_id


def can_use_anonymous_dead_chat(running: RunningGame, player: Player) -> bool:
    return not player.alive and player.user_id not in running.game.purified_dead_ids


def can_use_anonymous_shaman_chat(running: RunningGame, player: Player) -> bool:
    if not player.alive:
        return player.user_id not in running.game.purified_dead_ids
    return (
        player.role == Role.SHAMAN
        and running.game.phase == Phase.NIGHT
        and not running.game.is_frog(player)
        and not running.game.is_madam_seduced(player)
    )


def can_use_anonymous_role_chat(running: RunningGame, player: Player, role: Role) -> bool:
    if running.game.is_frog(player):
        return False
    if running.game.is_madam_seduced(player):
        return False
    if role == Role.LOVER:
        return player.alive and player.role == Role.LOVER and lover_chat_is_open(running.game)
    if player.alive and (player.user_id, role) in running.anonymous_role_input_channel_ids:
        return True
    if role == Role.MAFIA:
        return player.alive and running.game.is_known_mafia_team(player)
    return player.alive and player.role == role


def anonymous_avatar_url(author_label: str) -> str | None:
    if author_label.endswith("번") and author_label[:-1].isdigit():
        number = int(author_label[:-1])
        color = NUMBER_AVATAR_COLORS[(number - 1) % len(NUMBER_AVATAR_COLORS)]
        return f"https://dummyimage.com/128x128/{color}/ffffff.png&text={number}"
    emoji_code = ANIMAL_EMOJI_CODES.get(author_label)
    if not emoji_code:
        return None
    return f"https://cdn.jsdelivr.net/gh/twitter/twemoji@14.0.2/assets/72x72/{emoji_code}.png"


async def send_anonymous_text(
    channel: discord.abc.Messageable,
    author_label: str,
    body: str,
    *,
    running: RunningGame | None = None,
) -> None:
    if running and isinstance(channel, discord.TextChannel):
        webhook = await anonymous_webhook(channel, running)
        if webhook:
            with suppress(discord.HTTPException):
                send_kwargs = {
                    "username": author_label[:80],
                    "allowed_mentions": discord.AllowedMentions.none(),
                    "wait": False,
                }
                avatar_url = anonymous_avatar_url(author_label)
                if avatar_url:
                    send_kwargs["avatar_url"] = avatar_url
                await webhook.send(body, **send_kwargs)
                return
    with suppress(discord.HTTPException):
        await channel.send(
            f"{author_label}: {body}",
            allowed_mentions=discord.AllowedMentions.none(),
        )


async def send_member_webhook_text(
    channel: discord.abc.Messageable,
    author: discord.Member | discord.User,
    body: str,
    *,
    running: RunningGame,
) -> None:
    if isinstance(channel, discord.TextChannel):
        webhook = await anonymous_webhook(channel, running)
        if webhook:
            with suppress(discord.HTTPException):
                avatar = getattr(author, "display_avatar", None)
                send_kwargs = {
                    "username": getattr(author, "display_name", author.name)[:80],
                    "allowed_mentions": discord.AllowedMentions.none(),
                    "wait": False,
                }
                if avatar:
                    send_kwargs["avatar_url"] = avatar.url
                await webhook.send(body, **send_kwargs)
                return
    with suppress(discord.HTTPException):
        await channel.send(
            f"{getattr(author, 'display_name', author.name)}: {body}",
            allowed_mentions=discord.AllowedMentions.none(),
        )


async def anonymous_webhook(
    channel: discord.TextChannel,
    running: RunningGame,
) -> discord.Webhook | None:
    url = running.anonymous_webhook_urls.get(channel.id)
    if url:
        return discord.Webhook.from_url(url, client=bot)
    try:
        webhook = await channel.create_webhook(name="Mafia Anonymous")
    except discord.DiscordException:
        return None
    running.anonymous_webhook_urls[channel.id] = webhook.url
    return webhook


async def prepare_anonymous_webhook(
    channel: discord.TextChannel,
    running: RunningGame,
) -> None:
    await anonymous_webhook(channel, running)


async def relay_to_channels(
    deliveries: list[tuple[discord.abc.Messageable, str, str]],
    running: RunningGame,
) -> None:
    semaphore = asyncio.Semaphore(20)

    async def send_one(channel: discord.abc.Messageable, label: str, body: str) -> None:
        async with semaphore:
            await send_anonymous_text(channel, label, body, running=running)

    await asyncio.gather(
        *(send_one(channel, label, body) for channel, label, body in deliveries),
        return_exceptions=True,
    )


async def send_anonymous_log(
    guild: discord.Guild,
    running: RunningGame,
    *,
    player: Player,
    body: str,
    role: Role | None = None,
    context: str | None = None,
) -> None:
    channel = guild.get_channel(running.channel_id)
    if not isinstance(channel, discord.abc.Messageable):
        return
    alias = running.anonymous_aliases.get(player.user_id, player.name)
    label = role.value if role else context
    prefix = f"[익명 로그/{label}]" if label else "[익명 로그]"
    await send_anonymous_text(channel, prefix, f"{alias} - {body}")


async def relay_anonymous_general_message(
    guild: discord.Guild,
    running: RunningGame,
    sender: Player,
    body: str,
) -> None:
    sender_alias = running.anonymous_aliases.get(sender.user_id, "익명")
    deliveries: list[tuple[discord.abc.Messageable, str, str]] = []

    for viewer in running.game.alive_players():
        if viewer.user_id == sender.user_id:
            continue
        if running.game.is_frog(viewer):
            continue
        channel_id = running.anonymous_input_channel_ids.get(viewer.user_id)
        channel = guild.get_channel(channel_id) if channel_id else None
        if not isinstance(channel, discord.TextChannel):
            continue
        deliveries.append((channel, sender_alias, body))

    await relay_to_channels(deliveries, running)
    await send_anonymous_log(guild, running, player=sender, body=body, context="일반")


def anonymous_dead_chat_viewers(running: RunningGame) -> list[Player]:
    return [
        player
        for player in running.game.players
        if not player.alive and player.user_id not in running.game.purified_dead_ids
    ]


def anonymous_shaman_chat_viewers(running: RunningGame) -> list[Player]:
    viewers: list[Player] = []
    for player in running.game.players:
        if not player.alive and player.user_id not in running.game.purified_dead_ids:
            viewers.append(player)
        elif player.alive and player.role == Role.SHAMAN and not running.game.is_frog(player):
            viewers.append(player)
    return viewers


def anonymous_dead_sender_label(running: RunningGame, sender: Player) -> str:
    if sender.alive and sender.role == Role.SHAMAN:
        return "익명의 목소리"
    if running.anonymous_enabled:
        return running.anonymous_aliases.get(sender.user_id, "익명")
    return sender.name


async def send_dead_chat_text(
    guild: discord.Guild,
    running: RunningGame,
    channel: discord.TextChannel,
    sender: Player,
    body: str,
) -> None:
    if running.anonymous_enabled:
        await send_anonymous_text(
            channel,
            anonymous_dead_sender_label(running, sender),
            body,
            running=running,
        )
        return
    author = await get_guild_member(guild, sender.user_id)
    if author:
        await send_member_webhook_text(channel, author, body, running=running)
        return
    await send_anonymous_text(channel, sender.name, body, running=running)


async def relay_anonymous_dead_message(
    guild: discord.Guild,
    running: RunningGame,
    sender: Player,
    body: str,
) -> None:
    deliveries: list[discord.TextChannel] = []

    for viewer in anonymous_dead_chat_viewers(running):
        if viewer.user_id == sender.user_id:
            continue
        await set_anonymous_dead_input_access(
            guild,
            running,
            viewer,
            can_view=True,
            can_chat=can_use_anonymous_dead_chat(running, viewer),
            reason="마피아 게임 사망자 개인 채팅 권한 갱신",
        )
        channel_id = running.anonymous_dead_input_channel_ids.get(viewer.user_id)
        channel = guild.get_channel(channel_id) if channel_id else None
        if not isinstance(channel, discord.TextChannel):
            continue
        deliveries.append(channel)

    await asyncio.gather(
        *(send_dead_chat_text(guild, running, channel, sender, body) for channel in deliveries),
        return_exceptions=True,
    )


async def send_anonymous_shaman_log(
    guild: discord.Guild,
    running: RunningGame,
    *,
    player: Player,
    body: str,
) -> None:
    channel = guild.get_channel(running.shaman_channel_id) if running.shaman_channel_id else None
    if not isinstance(channel, discord.TextChannel):
        return
    alias = anonymous_dead_sender_label(running, player)
    await send_anonymous_text(
        channel,
        "[익명 로그/영매]",
        f"{alias} - {body}",
        running=running,
    )


async def relay_anonymous_shaman_message(
    guild: discord.Guild,
    running: RunningGame,
    sender: Player,
    body: str,
) -> None:
    sender_alias = anonymous_dead_sender_label(running, sender)
    deliveries: list[tuple[discord.abc.Messageable, str, str]] = []

    for viewer in anonymous_shaman_chat_viewers(running):
        if viewer.user_id == sender.user_id:
            continue
        channel_id = running.anonymous_shaman_input_channel_ids.get(viewer.user_id)
        channel = guild.get_channel(channel_id) if channel_id else None
        if not isinstance(channel, discord.TextChannel):
            continue
        deliveries.append((channel, sender_alias, body))

    await relay_to_channels(deliveries, running)
    await send_anonymous_shaman_log(guild, running, player=sender, body=body)


async def relay_anonymous_role_message(
    guild: discord.Guild,
    running: RunningGame,
    sender: Player,
    role: Role,
    body: str,
) -> None:
    sender_alias = running.anonymous_aliases.get(sender.user_id, "익명")
    deliveries: list[tuple[discord.abc.Messageable, str, str]] = []

    for viewer in anonymous_role_status_players(running, role):
        if viewer.user_id == sender.user_id:
            continue
        if not can_use_anonymous_role_chat(running, viewer, role):
            continue
        input_id = running.anonymous_role_input_channel_ids.get((viewer.user_id, role))
        input_channel = guild.get_channel(input_id) if input_id else None
        if not isinstance(input_channel, discord.TextChannel):
            continue
        deliveries.append((input_channel, sender_alias, body))

    await relay_to_channels(deliveries, running)
    await send_anonymous_log(guild, running, player=sender, body=body, role=role)


async def mirror_role_chat_to_dead(
    guild: discord.Guild,
    running: RunningGame,
    author: discord.Member | discord.User,
    role: Role,
    body: str,
) -> None:
    deliveries: list[discord.TextChannel] = []
    for viewer in anonymous_dead_chat_viewers(running):
        await set_anonymous_dead_input_access(
            guild,
            running,
            viewer,
            can_view=True,
            can_chat=can_use_anonymous_dead_chat(running, viewer),
            reason="마피아 게임 사망자 개인 채팅 권한 갱신",
        )
        channel_id = running.anonymous_dead_input_channel_ids.get(viewer.user_id)
        channel = guild.get_channel(channel_id) if channel_id else None
        if isinstance(channel, discord.TextChannel):
            deliveries.append(channel)
    await asyncio.gather(
        *(
            send_member_webhook_text(
                channel,
                author,
                f"[{role.value}채팅] {body}",
                running=running,
            )
            for channel in deliveries
        ),
        return_exceptions=True,
    )


async def set_anonymous_general_input_access(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
    *,
    can_chat: bool,
    reason: str,
) -> None:
    member = await get_guild_member(guild, player.user_id)
    if not member:
        return
    channel_id = running.anonymous_input_channel_ids.get(player.user_id)
    channel = guild.get_channel(channel_id) if channel_id else None
    if not isinstance(channel, discord.TextChannel):
        return
    can_use = can_chat and player.alive and not running.game.is_frog(player) and not running.game.is_madam_seduced(player)
    with suppress(discord.DiscordException):
        await set_permissions_if_changed(
            channel,
            member,
            overwrite=anonymous_input_overwrite(True, can_use),
            reason=reason,
            running=running,
        )


async def set_anonymous_channel_slowmode(
    guild: discord.Guild,
    channel_id: int | None,
    seconds: int,
    reason: str,
) -> None:
    channel = guild.get_channel(channel_id) if channel_id else None
    if not isinstance(channel, discord.TextChannel):
        return
    if channel.slowmode_delay == seconds:
        return
    with suppress(discord.DiscordException):
        await channel.edit(slowmode_delay=seconds, reason=reason)


async def ensure_anonymous_dead_input_channel(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
    *,
    can_chat: bool,
    reason: str,
) -> discord.TextChannel | None:
    member = await get_guild_member(guild, player.user_id)
    if not member:
        return None

    channel_id = running.anonymous_dead_input_channel_ids.get(player.user_id)
    channel = guild.get_channel(channel_id) if channel_id else None
    if isinstance(channel, discord.TextChannel):
        with suppress(discord.DiscordException):
            await set_permissions_if_changed(
                channel,
                member,
                overwrite=anonymous_input_overwrite(True, can_chat),
                reason=reason,
                running=running,
            )
        return channel

    source_channel = guild.get_channel(running.channel_id)
    category = source_channel_category(source_channel) if source_channel else None
    alias = running.anonymous_aliases.get(player.user_id, player.name) if running.anonymous_enabled else player.name
    overwrites = anonymous_base_overwrites(
        guild,
        participant_can_view=False,
        participant_can_chat=False,
        default_can_view=False,
        default_can_chat=False,
    )
    overwrites[member] = anonymous_input_overwrite(True, can_chat)
    channel = await create_text_channel_safe(
        guild,
        name=f"{sanitize_channel_part(alias)}-사망자-채팅",
        overwrites=overwrites,
        category=category,
        reason="마피아 게임 사망자 개인 채팅 채널 생성",
        slowmode_delay=0,
    )
    if not isinstance(channel, discord.TextChannel):
        return None

    running.anonymous_dead_input_channel_ids[player.user_id] = channel.id
    running.anonymous_dead_input_channel_owners[channel.id] = player.user_id
    await send_embed(
        channel,
        "사망자 개인 채팅 채널입니다.\n"
        "여기에 쓰면 사망자 채팅을 볼 수 있는 사람들의 사망자 개인 채널로만 전달됩니다.",
        title="사망자 개인 채팅",
        color=SUCCESS_EMBED_COLOR,
    )
    return channel


async def set_anonymous_dead_input_access(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
    *,
    can_view: bool,
    can_chat: bool,
    reason: str,
) -> None:
    member = await get_guild_member(guild, player.user_id)
    if not member:
        return
    channel_id = running.anonymous_dead_input_channel_ids.get(player.user_id)
    channel = guild.get_channel(channel_id) if channel_id else None
    if can_view and not isinstance(channel, discord.TextChannel):
        await ensure_anonymous_dead_input_channel(
            guild,
            running,
            player,
            can_chat=can_chat,
            reason=reason,
        )
        return
    if isinstance(channel, discord.TextChannel):
        with suppress(discord.DiscordException):
            await set_permissions_if_changed(
                channel,
                member,
                overwrite=anonymous_input_overwrite(can_view, can_chat if can_view else False),
                reason=reason,
                running=running,
            )


async def ensure_anonymous_shaman_input_channel(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
    *,
    can_chat: bool,
    reason: str,
) -> discord.TextChannel | None:
    member = await get_guild_member(guild, player.user_id)
    if not member:
        return None

    channel_id = running.anonymous_shaman_input_channel_ids.get(player.user_id)
    channel = guild.get_channel(channel_id) if channel_id else None
    if isinstance(channel, discord.TextChannel):
        with suppress(discord.DiscordException):
            await set_permissions_if_changed(
                channel,
                member,
                overwrite=anonymous_input_overwrite(True, can_chat),
                reason=reason,
                running=running,
            )
        return channel

    source_channel = guild.get_channel(running.channel_id)
    category = source_channel_category(source_channel) if source_channel else None
    alias = running.anonymous_aliases.get(player.user_id, str(player.user_id))
    overwrites = anonymous_base_overwrites(
        guild,
        participant_can_view=False,
        participant_can_chat=False,
        default_can_view=False,
        default_can_chat=False,
    )
    overwrites[member] = anonymous_input_overwrite(True, can_chat)
    channel = await create_text_channel_safe(
        guild,
        name=f"{sanitize_channel_part(alias)}-영매-채팅",
        overwrites=overwrites,
        category=category,
        reason="마피아 게임 익명 영매 입력 채널 생성",
        slowmode_delay=0,
    )
    if not isinstance(channel, discord.TextChannel):
        return None

    running.anonymous_shaman_input_channel_ids[player.user_id] = channel.id
    running.anonymous_shaman_input_channel_owners[channel.id] = player.user_id
    await send_embed(
        channel,
        "영매 익명 채팅 개인 채널입니다.\n"
        "여기에 쓰면 영매 채팅을 볼 수 있는 사람들의 영매 개인 채널로만 전달됩니다.",
        title="익명 영매 채팅",
        color=SUCCESS_EMBED_COLOR,
    )
    return channel


async def set_anonymous_shaman_input_access(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
    *,
    can_view: bool,
    can_chat: bool,
    reason: str,
) -> None:
    if not running.anonymous_enabled:
        return
    member = await get_guild_member(guild, player.user_id)
    if not member:
        return
    channel_id = running.anonymous_shaman_input_channel_ids.get(player.user_id)
    channel = guild.get_channel(channel_id) if channel_id else None
    if can_view and not isinstance(channel, discord.TextChannel):
        await ensure_anonymous_shaman_input_channel(
            guild,
            running,
            player,
            can_chat=can_chat,
            reason=reason,
        )
        return
    if isinstance(channel, discord.TextChannel):
        with suppress(discord.DiscordException):
            await set_permissions_if_changed(
                channel,
                member,
                overwrite=anonymous_input_overwrite(can_view, can_chat if can_view else False),
                reason=reason,
                running=running,
            )


async def set_anonymous_general_chat_permissions(
    guild: discord.Guild,
    running: RunningGame,
    *,
    can_chat: bool,
    reason: str,
) -> None:
    for player in running.game.alive_players():
        await set_anonymous_general_input_access(
            guild,
            running,
            player,
            can_chat=can_chat,
            reason=reason,
        )


async def delete_message_quietly(message: discord.Message) -> None:
    with suppress(discord.DiscordException):
        await message.delete()


async def handle_anonymous_message(
    message: discord.Message,
    running: RunningGame,
    *,
    owner_id: int,
    role: Role | None,
    dead_chat: bool = False,
    shaman_chat: bool = False,
) -> bool:
    if message.author.id != owner_id:
        await delete_message_quietly(message)
        return True
    if not message.guild:
        return True

    player = running.game.get_player(owner_id)
    if not player:
        return True

    body = anonymous_message_body(message)

    if dead_chat:
        if not can_use_anonymous_dead_chat(running, player):
            await set_anonymous_dead_input_access(
                message.guild,
                running,
                player,
                can_view=True,
                can_chat=False,
                reason="마피아 게임 사망자 채팅 불가 시간 권한 재적용",
            )
            return True
        await relay_anonymous_dead_message(message.guild, running, player, body)
        return True

    if shaman_chat:
        if not can_use_anonymous_shaman_chat(running, player):
            await set_anonymous_shaman_input_access(
                message.guild,
                running,
                player,
                can_view=True,
                can_chat=False,
                reason="마피아 게임 영매 채팅 불가 시간 권한 재적용",
            )
            return True
        await relay_anonymous_shaman_message(message.guild, running, player, body)
        return True

    if role is None:
        if not can_use_anonymous_general_chat(running, player):
            await set_anonymous_general_input_access(
                message.guild,
                running,
                player,
                can_chat=False,
                reason="마피아 게임 채팅 불가 시간 권한 재적용",
            )
            return True
        await relay_anonymous_general_message(message.guild, running, player, body)
        return True
    else:
        if not can_use_anonymous_role_chat(running, player, role):
            member = await get_guild_member(message.guild, player.user_id)
            if member and isinstance(message.channel, discord.TextChannel):
                with suppress(discord.DiscordException):
                    await set_permissions_if_changed(
                        message.channel,
                        member,
                        overwrite=anonymous_input_overwrite(True, False),
                        reason="마피아 게임 역할 채팅 불가 시간 권한 재적용",
                        running=running,
                    )
            return True
        await relay_anonymous_role_message(message.guild, running, player, role, body)
        await mirror_role_chat_to_dead(message.guild, running, message.author, role, body)
        return True


@bot.event
async def on_message(message: discord.Message) -> None:
    if message.author.bot:
        return

    for running in games.values():
        dead_owner_id = running.anonymous_dead_input_channel_owners.get(message.channel.id)
        if dead_owner_id is not None:
            await handle_anonymous_message(
                message,
                running,
                owner_id=dead_owner_id,
                role=None,
                dead_chat=True,
            )
            return

        shaman_owner_id = running.anonymous_shaman_input_channel_owners.get(message.channel.id)
        if shaman_owner_id is not None:
            await handle_anonymous_message(
                message,
                running,
                owner_id=shaman_owner_id,
                role=None,
                shaman_chat=True,
            )
            return

        owner_id = running.anonymous_input_channel_owners.get(message.channel.id)
        if owner_id is not None:
            await handle_anonymous_message(message, running, owner_id=owner_id, role=None)
            return

        role_input = running.anonymous_role_input_channels.get(message.channel.id)
        if role_input is not None:
            owner_id, role = role_input
            await handle_anonymous_message(message, running, owner_id=owner_id, role=role)
            return

        if message.guild and not running.anonymous_enabled:
            role = next(
                (
                    channel_role
                    for channel_role, channel_id in running.private_channel_ids.items()
                    if channel_id == message.channel.id
                ),
                None,
            )
            if role is not None:
                player = running.game.get_player(message.author.id)
                if player:
                    await mirror_role_chat_to_dead(message.guild, running, message.author, role, anonymous_message_body(message))
                return

        if message.channel.id != running.frog_channel_id:
            continue
        player = running.game.get_player(message.author.id)
        if not player or not running.game.is_frog(player):
            await delete_message_quietly(message)
            return
        if running.game.phase != Phase.DAY:
            await delete_message_quietly(message)
            return

        croak_count = max(1, len(message.content))
        game_channel = message.guild.get_channel(running.channel_id) if message.guild else None
        await delete_message_quietly(message)
        if isinstance(game_channel, discord.abc.Messageable):
            await send_embed(
                game_channel,
                f"개구리: {'개굴' * croak_count}",
                title="개구리 채팅",
            )
        return

    await bot.process_commands(message)

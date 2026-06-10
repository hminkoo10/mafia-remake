from __future__ import annotations

from bot import *  # noqa: F401,F403


__all__ = (
    'collect_participants',
    'members_with_role',
    'clear_existing_participant_roles',
    'clear_existing_spectator_roles',
    'collect_joined_participants',
    'get_guild_member',
    'clone_overwrite',
    'overwrites_equal',
    'set_permissions_if_changed',
    'cached_channel_overwrite',
    'remember_channel_overwrites',
    'supports_member_overwrites',
    'get_participant_role',
    'get_dead_player_role',
    'get_spectator_role',
    'ensure_spectator_role',
    'set_chat_values',
    'set_game_channel_chat',
    'set_spectator_game_channel_access',
    'set_channel_slowmode',
    'restore_channel_slowmode',
    'restore_game_channel_chat',
    'set_member_chat_permission',
    'restore_member_channel_chat',
    'sync_madam_seduction_permissions',
    'restore_madam_seduction_permissions',
    'remove_participant_role_from_dead',
    'add_dead_player_role',
    'remove_participant_roles_from_ids',
    'remove_dead_player_roles_from_ids',
    'remove_spectator_roles_from_ids',
    'remove_game_participant_roles',
    'remove_game_dead_player_roles',
    'remove_game_spectator_roles',
    'source_channel_category',
    'private_channel_overwrite',
    'dead_channel_overwrite',
    'spectator_channel_overwrite',
    'add_spectator_overwrite',
    'anonymous_input_overwrite',
    'sanitize_channel_part',
    'assign_anonymous_aliases',
    'apply_anonymous_player_names',
    'original_player_name',
    'create_text_channel_safe',
    'anonymous_base_overwrites',
    'ensure_memo_channel',
    'create_memo_channels',
    'create_anonymous_chat_channels',
    'hide_original_game_channel_for_anonymous',
    'restore_original_game_channel_for_anonymous',
    'anonymous_personal_channel',
    'send_anonymous_personal_embed',
    'broadcast_anonymous_personal_embed',
    'send_game_embed',
    'announce_cult_bells_now',
    'handle_madam_seduction_result',
    'send_player_secret',
    'role_chat_players',
    'lover_chat_is_open',
    'anonymous_role_status_players',
    'role_status_players',
    'mafia_night_target_status_text',
    'role_channel_status_text',
    'anonymous_role_status_text',
    'upsert_anonymous_role_status_message',
    'sync_anonymous_role_statuses',
    'upsert_private_role_status_message',
    'sync_role_status_message',
    'should_create_role_chat',
    'set_anonymous_role_access',
    'set_anonymous_role_view_only',
    'create_anonymous_role_channels',
    'create_private_role_channels',
    'shaman_chat_status_text',
    'upsert_shaman_chat_status',
    'create_shaman_chat_channel',
    'create_frog_chat_channel',
    'set_dead_channel_member_access',
    'set_shaman_channel_member_access',
    'set_frog_channel_member_access',
    'set_frog_game_channel_permission',
    'restore_frog_game_channel_permission',
    'sync_shaman_channel_permissions',
    'disable_anonymous_channels_for_player',
    'disable_private_role_channel_for_player',
    'private_role_channels',
    'sync_dead_players_private_role_channels',
    'add_player_to_private_role_channel',
    'set_player_private_channel_access',
    'sync_lover_chat_access',
    'sync_cult_team_channel_access',
    'refresh_player_private_channel_access',
    'delete_private_role_channels',
    'delete_memo_channels',
    'delete_anonymous_chat_channels',
    'cleanup_old_dead_chat_channels',
    'delete_shaman_chat_channel',
    'delete_frog_chat_channel',
    'warm_anonymous_startup_resources',
    'restore_all_frog_game_channel_permissions',
    'cleanup_game',
)


async def collect_participants(
    guild: discord.Guild,
    participant_role: discord.Role,
) -> list[discord.Member]:
    return await members_with_role(guild, participant_role, fail_if_empty_fetch_failed=True)


async def members_with_role(
    guild: discord.Guild,
    role: discord.Role,
    *,
    fail_if_empty_fetch_failed: bool = False,
) -> list[discord.Member]:
    members_by_id: dict[int, discord.Member] = {}
    for member in guild.members:
        if not member.bot and role in member.roles:
            members_by_id[member.id] = member

    try:
        async for member in guild.fetch_members(limit=None):
            if not member.bot and role in member.roles:
                members_by_id[member.id] = member
    except discord.DiscordException as error:
        if fail_if_empty_fetch_failed and not members_by_id:
            raise ValueError(
                "참가자 목록을 불러오지 못했습니다. "
                "Discord Developer Portal에서 Server Members Intent를 켰는지 확인하세요."
            ) from error

    return sorted(members_by_id.values(), key=lambda member: display_name(member).casefold())


async def clear_existing_participant_roles(
    guild: discord.Guild,
    participant_role: discord.Role,
) -> list[str]:
    failed_names: list[str] = []
    for member in await members_with_role(guild, participant_role):
        try:
            await member.remove_roles(
                participant_role,
                reason="마피아 게임 참가 모집 초기화",
            )
        except discord.DiscordException:
            failed_names.append(display_name(member))
    return failed_names


async def clear_existing_spectator_roles(
    guild: discord.Guild,
    spectator_role: discord.Role,
) -> list[str]:
    failed_names: list[str] = []
    for member in await members_with_role(guild, spectator_role):
        try:
            await member.remove_roles(
                spectator_role,
                reason="마피아 게임 관전 모집 초기화",
            )
        except discord.DiscordException:
            failed_names.append(display_name(member))
    return failed_names


async def collect_joined_participants(
    guild: discord.Guild,
    user_ids: set[int],
) -> list[discord.Member]:
    participants: list[discord.Member] = []
    for user_id in user_ids:
        member = await get_guild_member(guild, user_id)
        if member and not member.bot:
            participants.append(member)
    return sorted(participants, key=lambda member: display_name(member).casefold())


async def get_guild_member(guild: discord.Guild, user_id: int) -> discord.Member | None:
    member = guild.get_member(user_id)
    if member:
        return member

    try:
        return await guild.fetch_member(user_id)
    except discord.DiscordException:
        return None


def clone_overwrite(
    overwrite: discord.PermissionOverwrite | None,
) -> discord.PermissionOverwrite | None:
    if overwrite is None:
        return None
    return discord.PermissionOverwrite.from_pair(*overwrite.pair())


def overwrites_equal(
    left: discord.PermissionOverwrite | None,
    right: discord.PermissionOverwrite | None,
) -> bool:
    if left is None or right is None:
        return left is None and right is None
    return left.pair() == right.pair()


async def set_permissions_if_changed(
    channel: discord.abc.Messageable,
    target: discord.abc.Snowflake,
    *,
    overwrite: discord.PermissionOverwrite | None,
    reason: str,
    running: RunningGame | None = None,
) -> bool:
    if not hasattr(channel, "set_permissions"):
        return False
    channel_id = getattr(channel, "id", None)
    target_id = getattr(target, "id", None)
    cache_key = (channel_id, target_id) if channel_id is not None and target_id is not None else None
    desired = clone_overwrite(overwrite)
    current_marker = object()
    current: discord.PermissionOverwrite | None | object = current_marker
    if running is not None and cache_key is not None and cache_key in running.permission_overwrite_cache:
        current = clone_overwrite(running.permission_overwrite_cache[cache_key])
    if current is current_marker:
        current = clone_overwrite(getattr(channel, "overwrites", {}).get(target))

    if overwrites_equal(current if current is not current_marker else None, desired):
        return False

    await channel.set_permissions(target, overwrite=desired, reason=reason)
    if running is not None and cache_key is not None:
        running.permission_overwrite_cache[cache_key] = clone_overwrite(desired)
    return True


def cached_channel_overwrite(
    channel: discord.abc.Messageable,
    target: discord.abc.Snowflake,
    running: RunningGame,
) -> discord.PermissionOverwrite | None:
    channel_id = getattr(channel, "id", None)
    target_id = getattr(target, "id", None)
    if channel_id is not None and target_id is not None:
        cache_key = (channel_id, target_id)
        if cache_key in running.permission_overwrite_cache:
            return clone_overwrite(running.permission_overwrite_cache[cache_key])
    return clone_overwrite(getattr(channel, "overwrites", {}).get(target))


def remember_channel_overwrites(
    running: RunningGame,
    channel: discord.abc.Messageable,
    overwrites: dict[discord.abc.Snowflake, discord.PermissionOverwrite],
) -> None:
    channel_id = getattr(channel, "id", None)
    if channel_id is None:
        return
    for target, overwrite in overwrites.items():
        target_id = getattr(target, "id", None)
        if target_id is None:
            continue
        running.permission_overwrite_cache[(channel_id, target_id)] = clone_overwrite(overwrite)


def supports_member_overwrites(channel: discord.abc.Messageable) -> bool:
    return all(
        hasattr(channel, attribute)
        for attribute in ("overwrites", "overwrites_for", "set_permissions")
    )


def get_participant_role(guild: discord.Guild) -> discord.Role | None:
    return discord.utils.get(guild.roles, name=config.participant_role)


def get_dead_player_role(guild: discord.Guild) -> discord.Role | None:
    return discord.utils.get(guild.roles, name=DEAD_PLAYER_ROLE)


def get_spectator_role(guild: discord.Guild) -> discord.Role | None:
    return discord.utils.get(guild.roles, name=SPECTATOR_ROLE)


async def ensure_spectator_role(guild: discord.Guild) -> discord.Role | None:
    spectator_role = get_spectator_role(guild)
    if spectator_role:
        return spectator_role
    try:
        return await guild.create_role(
            name=SPECTATOR_ROLE,
            reason="마피아 게임 관전자 역할 생성",
        )
    except discord.DiscordException:
        return None


def set_chat_values(overwrite: discord.PermissionOverwrite, can_send: bool) -> None:
    overwrite.send_messages = can_send
    overwrite.send_messages_in_threads = can_send
    overwrite.create_public_threads = can_send
    overwrite.create_private_threads = can_send
    overwrite.add_reactions = can_send


async def set_game_channel_chat(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
    participants_can_chat: bool,
    reason: str,
) -> bool:
    if running.anonymous_enabled:
        await set_anonymous_general_chat_permissions(
            guild,
            running,
            can_chat=participants_can_chat,
            reason=reason,
        )
        participants_can_chat = False
    if not supports_member_overwrites(channel):
        return False

    participant_role = get_participant_role(guild)
    if not participant_role:
        await send_embed(
            channel,
            f"'{config.participant_role}' 역할을 찾을 수 없습니다.",
            color=ERROR_EMBED_COLOR,
        )
        return False

    targets = [
        (guild.default_role, False),
        (participant_role, participants_can_chat),
    ]
    failed = False
    for target, can_send in targets:
        if target.id not in running.game_channel_overwrites:
            running.game_channel_overwrites[target.id] = clone_overwrite(channel.overwrites.get(target))

        overwrite = cached_channel_overwrite(channel, target, running)
        if overwrite is None:
            overwrite = channel.overwrites_for(target)
        set_chat_values(overwrite, can_send)
        try:
            await set_permissions_if_changed(
                channel,
                target,
                overwrite=overwrite,
                reason=reason,
                running=running,
            )
        except discord.DiscordException:
            failed = True

    if failed:
        await send_embed(
            channel,
            "게임 채널 권한 변경에 실패했습니다. "
            "봇에게 채널 관리 권한이 있는지 확인하세요.",
            color=ERROR_EMBED_COLOR,
        )
    return not failed


async def set_spectator_game_channel_access(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
    reason: str,
) -> None:
    if not supports_member_overwrites(channel):
        return
    spectator_role = get_spectator_role(guild)
    if not spectator_role:
        return
    if spectator_role.id not in running.game_channel_overwrites:
        running.game_channel_overwrites[spectator_role.id] = clone_overwrite(
            channel.overwrites.get(spectator_role)
        )
    try:
        await set_permissions_if_changed(
            channel,
            spectator_role,
            overwrite=spectator_channel_overwrite(),
            reason=reason,
            running=running,
        )
    except discord.DiscordException:
        await send_embed(
            channel,
            "관전자 채널 권한 변경에 실패했습니다. 봇에게 채널 관리 권한이 있는지 확인하세요.",
            color=ERROR_EMBED_COLOR,
        )


async def set_channel_slowmode(
    channel: discord.abc.Messageable,
    running: RunningGame,
    seconds: int,
    reason: str,
) -> None:
    if not isinstance(channel, discord.TextChannel):
        return
    if running.original_slowmode_delay is None:
        running.original_slowmode_delay = channel.slowmode_delay
        running.original_slowmode_channel_id = channel.id
    try:
        await channel.edit(slowmode_delay=seconds, reason=reason)
    except discord.DiscordException:
        await send_embed(
            channel,
            "채널 슬로우모드 변경에 실패했습니다. 봇에게 채널 관리 권한이 있는지 확인하세요.",
            color=ERROR_EMBED_COLOR,
        )


async def restore_channel_slowmode(guild: discord.Guild, running: RunningGame) -> None:
    if running.original_slowmode_delay is None:
        return
    channel = guild.get_channel(running.original_slowmode_channel_id or running.channel_id)
    if not isinstance(channel, discord.TextChannel):
        return
    try:
        await channel.edit(
            slowmode_delay=running.original_slowmode_delay,
            reason="마피아 게임 종료로 슬로우모드 복구",
        )
    except discord.DiscordException:
        await send_embed(channel, "채널 슬로우모드 복구에 실패했습니다.", color=ERROR_EMBED_COLOR)


async def restore_game_channel_chat(guild: discord.Guild, running: RunningGame) -> None:
    if not running.game_channel_overwrites:
        return

    channel = guild.get_channel(running.channel_id)
    if not isinstance(channel, discord.abc.Messageable) or not supports_member_overwrites(channel):
        running.game_channel_overwrites.clear()
        return

    failed_targets: list[str] = []
    for target_id, original in list(running.game_channel_overwrites.items()):
        target = guild.default_role if target_id == guild.default_role.id else guild.get_role(target_id)
        if not target:
            running.game_channel_overwrites.pop(target_id, None)
            continue

        try:
            await set_permissions_if_changed(
                channel,
                target,
                overwrite=clone_overwrite(original),
                reason="마피아 게임 종료로 게임 채널 권한 복구",
                running=running,
            )
            running.game_channel_overwrites.pop(target_id, None)
        except discord.DiscordException:
            failed_targets.append(target.name)

    if failed_targets:
        await send_embed(
            channel,
            "게임 채널 권한 복구에 실패했습니다: "
            + ", ".join(failed_targets)
            + "\n봇에게 채널 관리 권한이 있는지 확인하세요.",
            color=ERROR_EMBED_COLOR,
        )


async def set_member_chat_permission(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
    player: Player,
    can_send: bool,
    reason: str,
) -> None:
    if running.anonymous_enabled:
        await set_anonymous_general_input_access(
            guild,
            running,
            player,
            can_chat=can_send,
            reason=reason,
        )
        if can_send:
            await set_anonymous_channel_slowmode(
                guild,
                running.anonymous_input_channel_ids.get(player.user_id),
                0,
                "마피아 게임 익명 최후변론 슬로우모드 해제",
            )
        return
    if not supports_member_overwrites(channel):
        return
    member = await get_guild_member(guild, player.user_id)
    if not member:
        return
    if member.id not in running.member_channel_overwrites:
        running.member_channel_overwrites[member.id] = clone_overwrite(channel.overwrites.get(member))
    overwrite = channel.overwrites_for(member)
    set_chat_values(overwrite, can_send)
    try:
        await set_permissions_if_changed(channel, member, overwrite=overwrite, reason=reason, running=running)
    except discord.DiscordException:
        await send_embed(channel, "최후변론 채팅 권한 변경에 실패했습니다.", color=ERROR_EMBED_COLOR)


async def restore_member_channel_chat(guild: discord.Guild, running: RunningGame) -> None:
    if running.anonymous_enabled and running.final_defense_user_id is not None:
        player = running.game.get_player(running.final_defense_user_id)
        if player:
            await set_anonymous_general_input_access(
                guild,
                running,
                player,
                can_chat=False,
                reason="마피아 게임 최후변론 종료로 익명 입력 권한 제거",
            )
            await set_anonymous_channel_slowmode(
                guild,
                running.anonymous_input_channel_ids.get(player.user_id),
                config.chat_slowmode_seconds,
                "마피아 게임 익명 최후변론 종료로 슬로우모드 복구",
            )
    running.final_defense_user_id = None
    if not running.member_channel_overwrites:
        return
    channel = guild.get_channel(running.channel_id)
    if not isinstance(channel, discord.abc.Messageable) or not supports_member_overwrites(channel):
        running.member_channel_overwrites.clear()
        return
    for user_id, original in list(running.member_channel_overwrites.items()):
        member = await get_guild_member(guild, user_id)
        if not member:
            running.member_channel_overwrites.pop(user_id, None)
            continue
        try:
            await set_permissions_if_changed(
                channel,
                member,
                overwrite=clone_overwrite(original),
                reason="마피아 게임 최후변론 채팅 권한 복구",
                running=running,
            )
        except discord.DiscordException:
            pass
        running.member_channel_overwrites.pop(user_id, None)


async def sync_madam_seduction_permissions(
    guild: discord.Guild,
    running: RunningGame,
    *,
    reason: str,
) -> None:
    channel = guild.get_channel(running.channel_id)
    for player in running.game.alive_players():
        if running.game.is_madam_seduced(player):
            for role in PRIVATE_CHAT_ROLES:
                await set_player_private_channel_access(
                    guild,
                    running,
                    role,
                    player,
                    can_chat=False,
                    reason=reason,
                )
        else:
            await refresh_player_private_channel_access(guild, running, player)

    if running.anonymous_enabled:
        for player in running.game.alive_players():
            await set_anonymous_general_input_access(
                guild,
                running,
                player,
                can_chat=can_use_anonymous_general_chat(running, player),
                reason=reason,
            )
        return
    if not isinstance(channel, discord.abc.Messageable) or not supports_member_overwrites(channel):
        running.madam_seduction_channel_overwrites.clear()
        return

    current_seduced = {
        player.user_id
        for player in running.game.alive_players()
        if running.game.is_madam_seduced(player)
    }
    for player in running.game.alive_players():
        member = await get_guild_member(guild, player.user_id)
        if not member:
            continue
        if player.user_id in current_seduced:
            if member.id not in running.madam_seduction_channel_overwrites:
                running.madam_seduction_channel_overwrites[member.id] = clone_overwrite(channel.overwrites.get(member))
            overwrite = channel.overwrites_for(member)
            set_chat_values(overwrite, False)
            with suppress(discord.DiscordException):
                await set_permissions_if_changed(channel, member, overwrite=overwrite, reason=reason, running=running)
        elif member.id in running.madam_seduction_channel_overwrites:
            original = running.madam_seduction_channel_overwrites.pop(member.id, None)
            with suppress(discord.DiscordException):
                await set_permissions_if_changed(
                    channel,
                    member,
                    overwrite=clone_overwrite(original),
                    reason=reason,
                    running=running,
                )

    for user_id, original in list(running.madam_seduction_channel_overwrites.items()):
        if user_id in current_seduced:
            continue
        member = await get_guild_member(guild, user_id)
        running.madam_seduction_channel_overwrites.pop(user_id, None)
        if member:
            with suppress(discord.DiscordException):
                await set_permissions_if_changed(
                    channel,
                    member,
                    overwrite=clone_overwrite(original),
                    reason=reason,
                    running=running,
                )


async def restore_madam_seduction_permissions(guild: discord.Guild, running: RunningGame) -> None:
    if running.anonymous_enabled:
        return
    channel = guild.get_channel(running.channel_id)
    if not isinstance(channel, discord.abc.Messageable) or not supports_member_overwrites(channel):
        running.madam_seduction_channel_overwrites.clear()
        return
    for user_id, original in list(running.madam_seduction_channel_overwrites.items()):
        member = await get_guild_member(guild, user_id)
        running.madam_seduction_channel_overwrites.pop(user_id, None)
        if member:
            with suppress(discord.DiscordException):
                await set_permissions_if_changed(
                    channel,
                    member,
                    overwrite=clone_overwrite(original),
                    reason="마피아 게임 종료로 마담 유혹 채팅 권한 복구",
                    running=running,
                )


async def remove_participant_role_from_dead(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
) -> bool:
    participant_role = get_participant_role(guild)
    if not participant_role:
        return False

    member = await get_guild_member(guild, player.user_id)
    if not member:
        return False
    if participant_role not in member.roles:
        return True

    try:
        await member.remove_roles(
            participant_role,
            reason="마피아 게임 사망자 발언 제한",
        )
        return True
    except discord.DiscordException:
        return False


async def add_dead_player_role(
    guild: discord.Guild,
    player: Player,
) -> bool | None:
    dead_role = get_dead_player_role(guild)
    if not dead_role:
        return None

    member = await get_guild_member(guild, player.user_id)
    if not member:
        return False
    if dead_role in member.roles:
        return True

    try:
        await member.add_roles(
            dead_role,
            reason="마피아 게임 사망자 역할 부여",
        )
        return True
    except discord.DiscordException:
        return False


async def remove_participant_roles_from_ids(
    guild: discord.Guild,
    user_ids: set[int],
    reason: str,
) -> list[str]:
    participant_role = get_participant_role(guild)
    if not participant_role:
        return [str(user_id) for user_id in sorted(user_ids)]

    failed_names: list[str] = []
    for user_id in sorted(user_ids):
        member = await get_guild_member(guild, user_id)
        if not member:
            continue

        try:
            if participant_role in member.roles:
                await member.remove_roles(
                    participant_role,
                    reason=reason,
                )
        except discord.DiscordException:
            failed_names.append(display_name(member))
    return failed_names


async def remove_dead_player_roles_from_ids(
    guild: discord.Guild,
    user_ids: set[int],
    reason: str,
) -> list[str]:
    dead_role = get_dead_player_role(guild)
    if not dead_role:
        return []

    failed_names: list[str] = []
    for user_id in sorted(user_ids):
        member = await get_guild_member(guild, user_id)
        if not member:
            continue

        try:
            if dead_role in member.roles:
                await member.remove_roles(
                    dead_role,
                    reason=reason,
                )
        except discord.DiscordException:
            failed_names.append(display_name(member))
    return failed_names


async def remove_spectator_roles_from_ids(
    guild: discord.Guild,
    user_ids: set[int],
    reason: str,
) -> list[str]:
    spectator_role = get_spectator_role(guild)
    if not spectator_role:
        return []

    failed_names: list[str] = []
    for user_id in sorted(user_ids):
        member = await get_guild_member(guild, user_id)
        if not member:
            continue

        try:
            if spectator_role in member.roles:
                await member.remove_roles(
                    spectator_role,
                    reason=reason,
                )
        except discord.DiscordException:
            failed_names.append(display_name(member))
    return failed_names


async def remove_game_participant_roles(guild: discord.Guild, running: RunningGame) -> None:
    if not running.participant_user_ids:
        return

    failed_names = await remove_participant_roles_from_ids(
        guild,
        running.participant_user_ids,
        "마피아 게임 종료로 참가자 역할 제거",
    )
    channel = guild.get_channel(running.channel_id)
    if failed_names and isinstance(channel, discord.abc.Messageable):
        await send_embed(
            channel,
            f"'{config.participant_role}' 역할 제거에 실패한 참가자: "
            + ", ".join(failed_names)
            + "\n봇에게 역할 관리 권한이 있고, 봇 역할이 참가자 역할보다 위에 있는지 확인하세요.",
            color=ERROR_EMBED_COLOR,
        )


async def remove_game_dead_player_roles(guild: discord.Guild, running: RunningGame) -> None:
    if not running.participant_user_ids:
        return

    failed_names = await remove_dead_player_roles_from_ids(
        guild,
        running.participant_user_ids,
        "마피아 게임 종료로 사망자 역할 제거",
    )
    channel = guild.get_channel(running.channel_id)
    if failed_names and isinstance(channel, discord.abc.Messageable):
        await send_embed(
            channel,
            f"'{DEAD_PLAYER_ROLE}' 역할 제거에 실패한 참가자: "
            + ", ".join(failed_names)
            + "\n봇에게 역할 관리 권한이 있고, 봇 역할이 사망자 역할보다 위에 있는지 확인하세요.",
            color=ERROR_EMBED_COLOR,
        )


async def remove_game_spectator_roles(guild: discord.Guild, running: RunningGame) -> None:
    if not running.spectator_user_ids:
        return

    failed_names = await remove_spectator_roles_from_ids(
        guild,
        running.spectator_user_ids,
        "마피아 게임 종료로 관전자 역할 제거",
    )
    channel = guild.get_channel(running.channel_id)
    if failed_names and isinstance(channel, discord.abc.Messageable):
        await send_embed(
            channel,
            f"'{SPECTATOR_ROLE}' 역할 제거에 실패한 관전자: "
            + ", ".join(failed_names)
            + "\n봇에게 역할 관리 권한이 있고, 봇 역할이 관전자 역할보다 위에 있는지 확인하세요.",
            color=ERROR_EMBED_COLOR,
        )


def source_channel_category(
    channel: discord.abc.Messageable,
) -> discord.CategoryChannel | None:
    if isinstance(channel, discord.TextChannel):
        return channel.category
    if isinstance(channel, discord.Thread) and isinstance(channel.parent, discord.TextChannel):
        return channel.parent.category
    return None


def private_channel_overwrite(can_chat: bool) -> discord.PermissionOverwrite:
    return discord.PermissionOverwrite(
        view_channel=can_chat,
        send_messages=can_chat,
        send_messages_in_threads=can_chat,
        create_public_threads=can_chat,
        create_private_threads=can_chat,
        read_message_history=can_chat,
        add_reactions=can_chat,
    )


def dead_channel_overwrite(can_view: bool, can_chat: bool) -> discord.PermissionOverwrite:
    return discord.PermissionOverwrite(
        view_channel=can_view,
        send_messages=can_chat,
        send_messages_in_threads=can_chat,
        create_public_threads=can_chat,
        create_private_threads=can_chat,
        read_message_history=can_view,
        add_reactions=can_chat,
    )


def spectator_channel_overwrite() -> discord.PermissionOverwrite:
    return discord.PermissionOverwrite(
        view_channel=True,
        send_messages=False,
        send_messages_in_threads=False,
        create_public_threads=False,
        create_private_threads=False,
        read_message_history=True,
        add_reactions=False,
    )


def add_spectator_overwrite(
    guild: discord.Guild,
    overwrites: dict[discord.abc.Snowflake, discord.PermissionOverwrite],
) -> None:
    spectator_role = get_spectator_role(guild)
    if spectator_role:
        overwrites[spectator_role] = spectator_channel_overwrite()


def anonymous_input_overwrite(can_view: bool, can_chat: bool) -> discord.PermissionOverwrite:
    return discord.PermissionOverwrite(
        view_channel=can_view,
        send_messages=can_chat,
        send_messages_in_threads=can_chat,
        create_public_threads=False,
        create_private_threads=False,
        read_message_history=can_view,
        add_reactions=can_chat,
    )


def sanitize_channel_part(value: str) -> str:
    return value.replace(" ", "-").replace("/", "-").lower()


def assign_anonymous_aliases(running: RunningGame) -> None:
    players = sorted(running.game.players, key=lambda player: player.user_id)
    if normalized_anonymous_name_mode() == "number":
        aliases = [f"{index}번" for index in range(1, len(players) + 1)]
    else:
        aliases = list(ANIMAL_ALIASES)
    secrets.SystemRandom().shuffle(aliases)
    running.anonymous_aliases = {
        player.user_id: aliases[index] if index < len(aliases) else f"{index + 1}번"
        for index, player in enumerate(players)
    }


def apply_anonymous_player_names(running: RunningGame) -> None:
    if not running.anonymous_enabled:
        return
    if not running.anonymous_original_names:
        running.anonymous_original_names = {
            player.user_id: player.name for player in running.game.players
        }
    for player in running.game.players:
        alias = running.anonymous_aliases.get(player.user_id)
        if alias:
            player.name = alias


def original_player_name(running: RunningGame, player: Player) -> str:
    return running.anonymous_original_names.get(player.user_id, player.name)


async def create_text_channel_safe(
    guild: discord.Guild,
    *,
    name: str,
    overwrites: dict[discord.abc.Snowflake, discord.PermissionOverwrite],
    category: discord.CategoryChannel | None,
    reason: str,
    slowmode_delay: int | None = None,
    topic: str | None = None,
) -> discord.TextChannel | None:
    options: dict[str, object] = {}
    if slowmode_delay is not None:
        options["slowmode_delay"] = slowmode_delay
    if topic is not None:
        options["topic"] = topic[:1024]
    try:
        return await guild.create_text_channel(
            name=name,
            overwrites=overwrites,
            category=category,
            reason=reason,
            **options,
        )
    except discord.DiscordException:
        try:
            return await guild.create_text_channel(
                name=name,
                overwrites=overwrites,
                reason=reason,
                **options,
            )
        except discord.DiscordException:
            return None


def anonymous_base_overwrites(
    guild: discord.Guild,
    *,
    participant_can_view: bool,
    participant_can_chat: bool,
    default_can_view: bool,
    default_can_chat: bool,
) -> dict[discord.abc.Snowflake, discord.PermissionOverwrite]:
    overwrites: dict[discord.abc.Snowflake, discord.PermissionOverwrite] = {
        guild.default_role: anonymous_input_overwrite(default_can_view, default_can_chat),
    }
    participant_role = get_participant_role(guild)
    manager_role = discord.utils.get(guild.roles, name=config.manager_role)
    bot_member = guild.me or (guild.get_member(bot.user.id) if bot.user else None)
    if participant_role:
        overwrites[participant_role] = anonymous_input_overwrite(participant_can_view, participant_can_chat)
    add_spectator_overwrite(guild, overwrites)
    if manager_role:
        overwrites[manager_role] = anonymous_input_overwrite(False, False)
    if bot_member:
        overwrites[bot_member] = anonymous_input_overwrite(True, True)
    return overwrites


async def ensure_memo_channel(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
) -> discord.TextChannel | None:
    channel_id = running.memo_channel_ids.get(player.user_id)
    channel = guild.get_channel(channel_id) if channel_id else None
    if isinstance(channel, discord.TextChannel):
        return channel

    member = await get_guild_member(guild, player.user_id)
    if not member:
        return None

    source_channel = guild.get_channel(running.channel_id)
    category = source_channel_category(source_channel) if source_channel else None
    participant_role = get_participant_role(guild)
    manager_role = discord.utils.get(guild.roles, name=config.manager_role)
    bot_member = guild.me or (guild.get_member(bot.user.id) if bot.user else None)
    overwrites: dict[discord.abc.Snowflake, discord.PermissionOverwrite] = {
        guild.default_role: private_channel_overwrite(False),
        member: private_channel_overwrite(True),
    }
    if participant_role:
        overwrites[participant_role] = private_channel_overwrite(False)
    add_spectator_overwrite(guild, overwrites)
    if manager_role:
        overwrites[manager_role] = private_channel_overwrite(False)
    if bot_member:
        overwrites[bot_member] = private_channel_overwrite(True)

    channel = await create_text_channel_safe(
        guild,
        name=f"{sanitize_channel_part(status_display_name(running, player))}-메모",
        overwrites=overwrites,
        category=category,
        reason="마피아 게임 개인 메모 채널 생성",
        slowmode_delay=0,
    )
    if not isinstance(channel, discord.TextChannel):
        return None

    running.memo_channel_ids[player.user_id] = channel.id
    await send_embed(
        channel,
        "개인 메모 채널입니다.\n"
        "`/메모 참가자 메모내용`으로 참가자별 메모를 저장하고, "
        "`/메모 참가자`로 저장한 메모를 다시 볼 수 있습니다.",
        title="메모 채널",
        color=SUCCESS_EMBED_COLOR,
    )
    return channel


async def create_memo_channels(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    failed_names: list[str] = []
    for player in running.game.players:
        memo_channel = await ensure_memo_channel(guild, running, player)
        if not memo_channel:
            failed_names.append(status_display_name(running, player))
    if failed_names:
        await send_embed(
            channel,
            "개인 메모 채널 생성 실패: " + ", ".join(failed_names),
            color=ERROR_EMBED_COLOR,
        )


async def create_anonymous_chat_channels(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    if not running.anonymous_enabled:
        return
    category = source_channel_category(channel)
    assign_anonymous_aliases(running)
    apply_anonymous_player_names(running)
    for player in running.game.players:
        member = await get_guild_member(guild, player.user_id)
        if not member:
            continue
        alias = running.anonymous_aliases[player.user_id]
        input_overwrites = anonymous_base_overwrites(
            guild,
            participant_can_view=False,
            participant_can_chat=False,
            default_can_view=False,
            default_can_chat=False,
        )
        input_overwrites[member] = anonymous_input_overwrite(
            True,
            can_use_anonymous_general_chat(running, player),
        )
        input_channel = await create_text_channel_safe(
            guild,
            name=f"{sanitize_channel_part(alias)}-채팅",
            overwrites=input_overwrites,
            category=category,
            reason="마피아 게임 개인 익명 입력 채널 생성",
            slowmode_delay=config.chat_slowmode_seconds,
        )
        if not input_channel:
            continue
        remember_channel_overwrites(running, input_channel, input_overwrites)
        running.anonymous_input_channel_ids[player.user_id] = input_channel.id
        running.anonymous_input_channel_owners[input_channel.id] = player.user_id
        await send_embed(
            input_channel,
            f"당신의 익명 이름은 **{alias}** 입니다.\n"
            "이 개인 채널이 일반 채팅을 대체합니다.\n"
            "여기에 쓰면 모든 참가자의 개인 채팅방에 익명으로 전달됩니다.",
            title="익명 입력 채널",
            color=SUCCESS_EMBED_COLOR,
        )


async def hide_original_game_channel_for_anonymous(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    if not running.anonymous_enabled or not supports_member_overwrites(channel):
        return
    participant_role = get_participant_role(guild)
    if participant_role:
        bot_member = guild.me or (guild.get_member(bot.user.id) if bot.user else None)
        if bot_member:
            if bot_member.id not in running.member_channel_overwrites:
                running.member_channel_overwrites[bot_member.id] = clone_overwrite(
                    channel.overwrites.get(bot_member)
                )
            bot_overwrite = channel.overwrites_for(bot_member)
            bot_overwrite.view_channel = True
            bot_overwrite.read_message_history = True
            set_chat_values(bot_overwrite, True)
            with suppress(discord.DiscordException):
                await set_permissions_if_changed(
                    channel,
                    bot_member,
                    overwrite=bot_overwrite,
                    reason="마피아 게임 익명 모드 진행을 위한 봇 권한 유지",
                    running=running,
                )
        if participant_role.id not in running.game_channel_overwrites:
            running.game_channel_overwrites[participant_role.id] = clone_overwrite(
                channel.overwrites.get(participant_role)
            )
        overwrite = channel.overwrites_for(participant_role)
        overwrite.view_channel = False
        overwrite.read_message_history = False
        set_chat_values(overwrite, False)
        with suppress(discord.DiscordException):
            await set_permissions_if_changed(
                channel,
                participant_role,
                overwrite=overwrite,
                reason="마피아 게임 익명 모드로 원본 채널 참가자 역할 열람 차단",
                running=running,
            )
        return

    for player in running.game.players:
        member = await get_guild_member(guild, player.user_id)
        if not member:
            continue
        if member.id not in running.anonymous_original_channel_overwrites:
            running.anonymous_original_channel_overwrites[member.id] = clone_overwrite(
                channel.overwrites.get(member)
            )
        overwrite = channel.overwrites_for(member)
        overwrite.view_channel = False
        overwrite.send_messages = False
        overwrite.read_message_history = False
        try:
            await set_permissions_if_changed(
                channel,
                member,
                overwrite=overwrite,
                reason="마피아 게임 익명 모드로 원본 채널 참가자 열람 차단",
                running=running,
            )
        except discord.DiscordException:
            continue


async def restore_original_game_channel_for_anonymous(
    guild: discord.Guild,
    running: RunningGame,
) -> None:
    if not running.anonymous_original_channel_overwrites:
        return
    channel = guild.get_channel(running.channel_id)
    if not isinstance(channel, discord.abc.Messageable) or not supports_member_overwrites(channel):
        running.anonymous_original_channel_overwrites.clear()
        return
    for user_id, original in list(running.anonymous_original_channel_overwrites.items()):
        member = await get_guild_member(guild, user_id)
        if not member:
            running.anonymous_original_channel_overwrites.pop(user_id, None)
            continue
        try:
            await set_permissions_if_changed(
                channel,
                member,
                overwrite=clone_overwrite(original),
                reason="마피아 게임 익명 모드 종료로 원본 채널 권한 복구",
                running=running,
            )
        except discord.DiscordException:
            pass
        running.anonymous_original_channel_overwrites.pop(user_id, None)


def anonymous_personal_channel(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
) -> discord.TextChannel | None:
    channel_id = running.anonymous_input_channel_ids.get(player.user_id)
    channel = guild.get_channel(channel_id) if channel_id else None
    return channel if isinstance(channel, discord.TextChannel) else None


async def send_anonymous_personal_embed(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
    message: str,
    *,
    view: discord.ui.View | None = None,
    title: str = "마피아 게임",
    color: discord.Color = DEFAULT_EMBED_COLOR,
) -> discord.Message | None:
    channel = anonymous_personal_channel(guild, running, player)
    if not channel:
        return None
    with suppress(discord.DiscordException):
        return await send_embed(channel, message, view=view, title=title, color=color)
    return None


async def broadcast_anonymous_personal_embed(
    guild: discord.Guild,
    running: RunningGame,
    message: str,
    *,
    view: discord.ui.View | None = None,
    title: str = "마피아 게임",
    color: discord.Color = DEFAULT_EMBED_COLOR,
    include_dead: bool = False,
) -> list[discord.Message]:
    if not running.anonymous_enabled:
        return []
    targets = running.game.players if include_dead else running.game.alive_players()
    if view is None:
        semaphore = asyncio.Semaphore(8)

        async def send_one(player: Player) -> discord.Message | None:
            async with semaphore:
                return await send_anonymous_personal_embed(
                    guild,
                    running,
                    player,
                    message,
                    view=None,
                    title=title,
                    color=color,
                )

        results = await asyncio.gather(
            *(send_one(player) for player in targets),
            return_exceptions=True,
        )
        return [result for result in results if isinstance(result, discord.Message)]

    messages: list[discord.Message] = []
    for player in targets:
        sent = await send_anonymous_personal_embed(
            guild,
            running,
            player,
            message,
            view=view,
            title=title,
            color=color,
        )
        if sent:
            messages.append(sent)
    return messages


async def send_game_embed(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
    message: str,
    *,
    view: discord.ui.View | None = None,
    title: str = "마피아 게임",
    color: discord.Color = DEFAULT_EMBED_COLOR,
    include_dead: bool = False,
    broadcast: bool = True,
) -> discord.Message:
    sent = await send_embed(channel, message, view=view, title=title, color=color)
    if broadcast:
        await broadcast_anonymous_personal_embed(
            guild,
            running,
            message,
            view=view,
            title=title,
            color=color,
            include_dead=include_dead,
        )
    return sent


async def announce_cult_bells_now(running: RunningGame, count: int) -> None:
    if count <= 0:
        return
    guild = bot.get_guild(running.guild_id)
    if not guild:
        return
    channel = guild.get_channel(running.channel_id)
    if not isinstance(channel, discord.abc.Messageable):
        return
    await send_game_embed(
        guild,
        channel,
        running,
        "\n".join("교주의 종소리가 울렸습니다." for _ in range(count)),
        title="교주 포교",
        color=WARNING_EMBED_COLOR,
        include_dead=True,
    )


async def handle_madam_seduction_result(
    guild: discord.Guild,
    running: RunningGame,
    result: VoteResult,
) -> None:
    if not result.madam_seduced:
        return

    for player in result.madam_seduced:
        await send_player_secret(
            guild,
            running,
            player,
            "마담에게 유혹당했습니다. 다음 낮이 될 때까지 능력을 사용할 수 없고 말할 수 없습니다.\n"
            "마피아팀이라면 능력 사용은 가능하지만, 유혹 중에는 마피아 비밀방에도 말할 수 없습니다.",
        )

    for player in running.game.alive_players():
        if running.game.is_known_mafia_team(player):
            await add_player_to_private_role_channel(guild, running, Role.MAFIA, player)

    for madam in running.game.alive_players():
        if madam.role == Role.MADAM and madam.user_id in running.game.madam_contacted:
            await add_player_to_private_role_channel(guild, running, Role.MAFIA, madam)
            await send_player_secret(
                guild,
                running,
                madam,
                "[접대] 마피아팀과 접선했습니다. 이제 마피아 비밀방에서 밤 대화가 가능합니다.",
            )

    await sync_madam_seduction_permissions(
        guild,
        running,
        reason="마피아 게임 마담 유혹으로 채팅 권한 갱신",
    )


async def send_player_secret(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
    message: str,
    view: discord.ui.View | None = None,
) -> bool:
    if running.anonymous_enabled:
        sent = await send_anonymous_personal_embed(
            guild,
            running,
            player,
            message,
            view=view,
            title="비밀 메시지",
        )
        if sent:
            return True
    member = await get_guild_member(guild, player.user_id)
    return bool(member and await send_private(member, message, view))


def role_chat_players(game: MafiaGame, role: Role) -> list[Player]:
    if role == Role.MAFIA:
        return [player for player in game.alive_players() if game.is_known_mafia_team(player)]
    return [player for player in game.alive_players() if player.role == role]


def lover_chat_is_open(game: MafiaGame) -> bool:
    return (
        game.phase == Phase.NIGHT
        and sum(
            1
            for player in game.alive_players()
            if player.role == Role.LOVER and not game.is_frog(player)
        ) >= 2
    )


def anonymous_role_status_players(running: RunningGame, role: Role) -> list[Player]:
    granted_ids = {
        user_id
        for (user_id, granted_role) in running.anonymous_role_input_channel_ids
        if granted_role == role
    }
    players: list[Player] = []
    for player in running.game.alive_players():
        if running.game.is_frog(player):
            continue
        if player.user_id in granted_ids:
            players.append(player)
        elif role == Role.MAFIA and running.game.is_known_mafia_team(player):
            players.append(player)
        elif role == Role.CULT_LEADER and running.game.is_cult_team(player):
            players.append(player)
        elif player.role == role:
            players.append(player)
    return sorted({player.user_id: player for player in players}.values(), key=lambda item: item.name.casefold())


def role_status_players(running: RunningGame, role: Role) -> list[Player]:
    if running.anonymous_enabled:
        return anonymous_role_status_players(running, role)
    return sorted(role_chat_players(running.game, role), key=lambda item: item.name.casefold())


def mafia_night_target_status_text(running: RunningGame) -> str:
    if running.game.phase != Phase.NIGHT:
        return ""
    actors = [
        actor
        for actor in running.game.night_action_actors()
        if actor.role == Role.MAFIA
    ]
    if not actors:
        return ""

    lines = ["마피아 처치 선택 현황"]
    for actor in sorted(actors, key=lambda item: status_display_name(running, item).casefold()):
        target_id = running.game.mafia_display_targets.get(actor.user_id) or running.game.mafia_targets.get(actor.user_id)
        target = running.game.get_player(target_id) if target_id else None
        target_name = status_display_name(running, target) if target else "미선택"
        lines.append(f"- {status_display_name(running, actor)} → {target_name}")
    return "\n".join(lines)


def role_channel_status_text(running: RunningGame, role: Role) -> str:
    players = role_status_players(running, role)
    if not players:
        text = "현재 생존: 없음"
    else:
        names = ", ".join(status_display_name(running, player) for player in players)
        text = f"현재 생존: {names}"
    if role == Role.MAFIA:
        mafia_status = mafia_night_target_status_text(running)
        if mafia_status:
            text = f"{text}\n\n{mafia_status}"
    return text


def anonymous_role_status_text(running: RunningGame, role: Role) -> str:
    return role_channel_status_text(running, role)


async def upsert_anonymous_role_status_message(
    channel: discord.TextChannel,
    running: RunningGame,
    role: Role,
    key: Role | tuple[int, Role],
) -> None:
    status_id = (
        running.anonymous_role_status_message_ids.get(key)
        if isinstance(key, Role)
        else running.anonymous_role_input_status_message_ids.get(key)
    )
    embed = make_embed(
        anonymous_role_status_text(running, role),
        title=f"{role.value} 채팅 현황",
        color=SUCCESS_EMBED_COLOR,
    )
    if status_id:
        with suppress(discord.DiscordException):
            message = await channel.fetch_message(status_id)
            await message.edit(embed=embed)
            return
    with suppress(discord.DiscordException):
        message = await channel.send(embed=embed)
        if isinstance(key, Role):
            running.anonymous_role_status_message_ids[key] = message.id
        else:
            running.anonymous_role_input_status_message_ids[key] = message.id


async def sync_anonymous_role_statuses(
    guild: discord.Guild,
    running: RunningGame,
    *,
    update_messages: bool = True,
) -> None:
    if not running.anonymous_enabled:
        return
    for role in PRIVATE_CHAT_ROLES:
        if not should_create_role_chat(running.game, role):
            continue
        topic = f"{role.value} 익명 채팅 | {anonymous_role_status_text(running, role)}"
        for (user_id, input_role), input_id in list(running.anonymous_role_input_channel_ids.items()):
            if input_role != role:
                continue
            input_channel = guild.get_channel(input_id)
            if isinstance(input_channel, discord.TextChannel):
                with suppress(discord.DiscordException):
                    await input_channel.edit(topic=topic[:1024])
                if update_messages:
                    await upsert_anonymous_role_status_message(
                        input_channel,
                        running,
                        role,
                        (user_id, input_role),
                    )


async def upsert_private_role_status_message(
    guild: discord.Guild,
    running: RunningGame,
    role: Role,
) -> None:
    if running.anonymous_enabled:
        return
    channel_id = running.private_channel_ids.get(role)
    channel = guild.get_channel(channel_id) if channel_id else None
    if not isinstance(channel, discord.TextChannel):
        return
    embed = make_embed(
        role_channel_status_text(running, role),
        title=f"{role.value} 채팅 현황",
        color=SUCCESS_EMBED_COLOR,
    )
    status_id = running.private_role_status_message_ids.get(role)
    if status_id:
        with suppress(discord.DiscordException):
            message = await channel.fetch_message(status_id)
            await message.edit(embed=embed)
            return
    with suppress(discord.DiscordException):
        message = await channel.send(embed=embed)
        running.private_role_status_message_ids[role] = message.id


async def sync_role_status_message(
    guild: discord.Guild,
    running: RunningGame,
    role: Role,
) -> None:
    if running.anonymous_enabled:
        await sync_anonymous_role_statuses(guild, running)
        return
    await upsert_private_role_status_message(guild, running, role)


def should_create_role_chat(game: MafiaGame, role: Role) -> bool:
    if role == Role.MAFIA:
        return any(player.role == Role.MAFIA for player in game.players) or any(
            player.role in MAFIA_SPECIAL_ROLES for player in game.players
        )
    return any(player.role == role for player in game.players)


async def set_anonymous_role_access(
    guild: discord.Guild,
    running: RunningGame,
    role: Role,
    player: Player,
    *,
    can_access: bool,
    reason: str,
) -> None:
    member = await get_guild_member(guild, player.user_id)
    if not member:
        return

    input_id = running.anonymous_role_input_channel_ids.get((player.user_id, role))
    input_channel = guild.get_channel(input_id) if input_id else None
    if can_access and not isinstance(input_channel, discord.TextChannel):
        source_channel = guild.get_channel(running.channel_id)
        category = source_channel_category(source_channel) if source_channel else None
        alias = running.anonymous_aliases.get(player.user_id, str(player.user_id))
        input_overwrites = anonymous_base_overwrites(
            guild,
            participant_can_view=False,
            participant_can_chat=False,
            default_can_view=False,
            default_can_chat=False,
        )
        input_overwrites[member] = anonymous_input_overwrite(True, True)
        input_channel = await create_text_channel_safe(
            guild,
            name=f"{sanitize_channel_part(alias)}-{sanitize_channel_part(role.value)}-채팅",
            overwrites=input_overwrites,
            category=category,
            reason="마피아 게임 익명 역할 입력 채널 생성",
            slowmode_delay=0,
        )
        if isinstance(input_channel, discord.TextChannel):
            running.anonymous_role_input_channel_ids[(player.user_id, role)] = input_channel.id
            running.anonymous_role_input_channels[input_channel.id] = (player.user_id, role)
            await send_embed(
                input_channel,
                f"{role.value} 역할 개인 채팅 채널입니다.\n"
                "여기에 쓰면 같은 역할의 개인 채팅방에 익명으로 전달됩니다.\n"
                "이 채널 하나에서 역할 대화와 밤 행동을 처리하세요.",
                title="익명 역할 입력",
                color=SUCCESS_EMBED_COLOR,
            )
    elif isinstance(input_channel, discord.TextChannel):
        if input_channel.slowmode_delay:
            with suppress(discord.DiscordException):
                await input_channel.edit(
                    slowmode_delay=0,
                    reason="마피아 게임 역할 채팅방 슬로우모드 해제",
                )
        with suppress(discord.DiscordException):
            await set_permissions_if_changed(
                input_channel,
                member,
                overwrite=anonymous_input_overwrite(can_access, can_access),
                reason=reason,
                running=running,
            )


async def set_anonymous_role_view_only(
    guild: discord.Guild,
    running: RunningGame,
    role: Role,
    player: Player,
    *,
    can_view: bool,
    reason: str,
) -> None:
    member = await get_guild_member(guild, player.user_id)
    if not member:
        return

    input_id = running.anonymous_role_input_channel_ids.get((player.user_id, role))
    input_channel = guild.get_channel(input_id) if input_id else None
    if can_view and not isinstance(input_channel, discord.TextChannel):
        source_channel = guild.get_channel(running.channel_id)
        category = source_channel_category(source_channel) if source_channel else None
        alias = running.anonymous_aliases.get(player.user_id, str(player.user_id))
        input_overwrites = anonymous_base_overwrites(
            guild,
            participant_can_view=False,
            participant_can_chat=False,
            default_can_view=False,
            default_can_chat=False,
        )
        input_overwrites[member] = anonymous_input_overwrite(True, False)
        input_channel = await create_text_channel_safe(
            guild,
            name=f"{sanitize_channel_part(alias)}-{sanitize_channel_part(role.value)}-채팅",
            overwrites=input_overwrites,
            category=category,
            reason="마피아 게임 익명 역할 보기 전용 채널 생성",
            slowmode_delay=0,
        )
        if isinstance(input_channel, discord.TextChannel):
            running.anonymous_role_input_channel_ids[(player.user_id, role)] = input_channel.id
            running.anonymous_role_input_channels[input_channel.id] = (player.user_id, role)
            await send_embed(
                input_channel,
                f"{role.value} 역할 보기 전용 채널입니다.\n"
                "이 채널에서 역할 대화를 확인할 수 있습니다.",
                title="익명 역할 채팅",
                color=SUCCESS_EMBED_COLOR,
            )
    elif isinstance(input_channel, discord.TextChannel):
        if input_channel.slowmode_delay:
            with suppress(discord.DiscordException):
                await input_channel.edit(
                    slowmode_delay=0,
                    reason="마피아 게임 역할 채팅방 슬로우모드 해제",
                )
        with suppress(discord.DiscordException):
            await set_permissions_if_changed(
                input_channel,
                member,
                overwrite=anonymous_input_overwrite(can_view, False),
                reason=reason,
                running=running,
            )


async def create_anonymous_role_channels(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    failed_roles: list[str] = []
    for role in PRIVATE_CHAT_ROLES:
        if not should_create_role_chat(running.game, role):
            continue
        before_count = len(running.anonymous_role_input_channel_ids)
        for player in role_chat_players(running.game, role):
            await set_anonymous_role_access(
                guild,
                running,
                role,
                player,
                can_access=True,
                reason="마피아 게임 익명 역할 채팅 권한 부여",
            )
        if len(running.anonymous_role_input_channel_ids) == before_count and role_chat_players(running.game, role):
            failed_roles.append(role.value)
    await sync_anonymous_role_statuses(guild, running, update_messages=False)
    if failed_roles:
        await send_embed(
            channel,
            "익명 역할 개인 채팅방 생성 실패: " + ", ".join(failed_roles),
            color=ERROR_EMBED_COLOR,
        )


async def create_private_role_channels(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    if running.anonymous_enabled:
        await create_anonymous_role_channels(guild, channel, running)
        return

    category = source_channel_category(channel)
    failed_roles: list[str] = []
    participant_role = get_participant_role(guild)
    manager_role = discord.utils.get(guild.roles, name=config.manager_role)
    bot_member = guild.me or (guild.get_member(bot.user.id) if bot.user else None)

    for role in PRIVATE_CHAT_ROLES:
        players = [player for player in running.game.players if player.role == role]
        should_create = bool(players)
        if role == Role.MAFIA:
            should_create = should_create or any(
                player.role in MAFIA_SPECIAL_ROLES for player in running.game.players
            )
        if not should_create:
            continue

        overwrites: dict[discord.abc.Snowflake, discord.PermissionOverwrite] = {
            guild.default_role: private_channel_overwrite(False),
        }
        if participant_role:
            overwrites[participant_role] = private_channel_overwrite(False)
        add_spectator_overwrite(guild, overwrites)
        if manager_role:
            overwrites[manager_role] = private_channel_overwrite(False)
        if bot_member:
            overwrites[bot_member] = private_channel_overwrite(True)

        for player in players:
            member = await get_guild_member(guild, player.user_id)
            if member:
                can_open = role != Role.LOVER or lover_chat_is_open(running.game)
                overwrites[member] = private_channel_overwrite(can_open)

        try:
            private_channel = await guild.create_text_channel(
                name=PRIVATE_CHANNEL_NAMES[role],
                overwrites=overwrites,
                category=category,
                reason="마피아 게임 역할별 비공개 채팅방 생성",
                slowmode_delay=0,
            )
        except discord.DiscordException:
            try:
                private_channel = await guild.create_text_channel(
                    name=PRIVATE_CHANNEL_NAMES[role],
                    overwrites=overwrites,
                    reason="마피아 게임 역할별 비공개 채팅방 생성",
                    slowmode_delay=0,
                )
            except discord.DiscordException:
                failed_roles.append(role.value)
                continue

        running.private_channel_ids[role] = private_channel.id
        await send_embed(
            private_channel,
            f"{role.value} 전용 비공개 채팅방입니다. "
            f"살아있는 {role.value}만 볼 수 있습니다.\n\n"
            f"{special_role_rule_text(role)}",
            title="역할 비공개 채널",
            color=SUCCESS_EMBED_COLOR,
        )
        await upsert_private_role_status_message(guild, running, role)

    if failed_roles:
        await send_embed(
            channel,
            "역할별 비공개 채널 생성에 실패했습니다: "
            + ", ".join(failed_roles)
            + "\n봇에게 채널 관리 권한이 있는지 확인하세요.",
            color=ERROR_EMBED_COLOR,
        )


def shaman_chat_status_text(running: RunningGame) -> str:
    if running.anonymous_enabled:
        return (
            "사망자와 영매가 접신하는 채팅입니다.\n"
            "영매는 이 채널만 볼 수 있으며, 밤에만 말할 수 있습니다.\n"
            "익명 모드에서는 각자의 영매 개인 채널을 사용하세요."
        )
    return (
        "사망자와 영매가 접신하는 채팅입니다.\n"
        "영매는 이 채널만 볼 수 있으며, 밤에만 말할 수 있습니다."
    )


async def upsert_shaman_chat_status(guild: discord.Guild, running: RunningGame) -> None:
    if running.shaman_channel_id is None:
        return
    channel = guild.get_channel(running.shaman_channel_id)
    if not isinstance(channel, discord.TextChannel):
        return
    embed = make_embed(
        shaman_chat_status_text(running),
        title="영매 채팅 상태",
        color=SUCCESS_EMBED_COLOR,
    )
    if running.shaman_status_message_id:
        with suppress(discord.DiscordException):
            message = await channel.fetch_message(running.shaman_status_message_id)
            await message.edit(embed=embed)
            return
    with suppress(discord.DiscordException):
        message = await channel.send(embed=embed)
        running.shaman_status_message_id = message.id


async def create_shaman_chat_channel(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    if not any(player.role == Role.SHAMAN for player in running.game.players):
        return
    category = source_channel_category(channel)
    participant_role = get_participant_role(guild)
    dead_role = get_dead_player_role(guild)
    manager_role = discord.utils.get(guild.roles, name=config.manager_role)
    bot_member = guild.me or (guild.get_member(bot.user.id) if bot.user else None)

    overwrites: dict[discord.abc.Snowflake, discord.PermissionOverwrite] = {
        guild.default_role: dead_channel_overwrite(False, False),
    }
    if participant_role:
        overwrites[participant_role] = dead_channel_overwrite(False, False)
    if dead_role:
        overwrites[dead_role] = dead_channel_overwrite(True, not running.anonymous_enabled)
    add_spectator_overwrite(guild, overwrites)
    if manager_role:
        overwrites[manager_role] = dead_channel_overwrite(False, False)
    if bot_member:
        overwrites[bot_member] = dead_channel_overwrite(True, True)

    for player in running.game.alive_players():
        if player.role != Role.SHAMAN:
            continue
        member = await get_guild_member(guild, player.user_id)
        if member:
            overwrites[member] = dead_channel_overwrite(True, False)

    shaman_channel = await create_text_channel_safe(
        guild,
        name=SHAMAN_CHAT_CHANNEL_NAME,
        overwrites=overwrites,
        category=category,
        reason="마피아 게임 영매 채팅방 생성",
    )
    if not isinstance(shaman_channel, discord.TextChannel):
        await send_embed(
            channel,
            "영매 채팅방 생성에 실패했습니다. 봇에게 채널 관리 권한이 있는지 확인하세요.",
            color=ERROR_EMBED_COLOR,
        )
        return

    running.shaman_channel_id = shaman_channel.id
    await send_embed(
        shaman_channel,
        "영매와 사망자가 접신하는 채팅방입니다.\n"
        "사망자는 이곳에서 대화할 수 있고, 영매는 밤에만 말할 수 있습니다.\n"
        "영매는 사망자 채팅방을 볼 수 없습니다.",
        title="영매 채팅방",
        color=SUCCESS_EMBED_COLOR,
    )
    if running.anonymous_enabled:
        for player in running.game.alive_players():
            if player.role != Role.SHAMAN:
                continue
            await set_anonymous_shaman_input_access(
                guild,
                running,
                player,
                can_view=True,
                can_chat=False,
                reason="마피아 게임 영매 익명 채팅 채널 생성",
            )
    await upsert_shaman_chat_status(guild, running)


async def create_frog_chat_channel(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    if not any(player.role == Role.WITCH for player in running.game.players):
        return
    category = source_channel_category(channel)
    participant_role = get_participant_role(guild)
    manager_role = discord.utils.get(guild.roles, name=config.manager_role)
    bot_member = guild.me or (guild.get_member(bot.user.id) if bot.user else None)

    overwrites: dict[discord.abc.Snowflake, discord.PermissionOverwrite] = {
        guild.default_role: dead_channel_overwrite(False, False),
    }
    if participant_role:
        overwrites[participant_role] = dead_channel_overwrite(False, False)
    add_spectator_overwrite(guild, overwrites)
    if manager_role:
        overwrites[manager_role] = dead_channel_overwrite(False, False)
    if bot_member:
        overwrites[bot_member] = dead_channel_overwrite(True, True)

    try:
        frog_channel = await guild.create_text_channel(
            name=FROG_CHAT_CHANNEL_NAME,
            overwrites=overwrites,
            category=category,
            reason="마피아 게임 개구리 채팅방 생성",
        )
    except discord.DiscordException:
        try:
            frog_channel = await guild.create_text_channel(
                name=FROG_CHAT_CHANNEL_NAME,
                overwrites=overwrites,
                reason="마피아 게임 개구리 채팅방 생성",
            )
        except discord.DiscordException:
            await send_embed(
                channel,
                "개구리 채팅방 생성에 실패했습니다. 봇에게 채널 관리 권한이 있는지 확인하세요.",
                color=ERROR_EMBED_COLOR,
            )
            return

    running.frog_channel_id = frog_channel.id
    await send_embed(
        frog_channel,
        "개구리 전용 채팅방입니다.\n"
        "저주에 걸린 참가자가 이곳에 쓴 말은 게임 채널에 개굴 소리로 전달됩니다.",
        title="개구리 채팅방",
        color=SUCCESS_EMBED_COLOR,
    )


async def set_dead_channel_member_access(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
    *,
    can_view: bool,
    can_chat: bool,
    reason: str,
) -> None:
    await set_anonymous_dead_input_access(
        guild,
        running,
        player,
        can_view=can_view,
        can_chat=can_chat,
        reason=reason,
    )
    if running.anonymous_enabled and not player.alive:
        await set_anonymous_general_input_access(
            guild,
            running,
            player,
            can_chat=False,
            reason="마피아 게임 사망으로 일반 익명 채팅 권한 제거",
        )


async def set_shaman_channel_member_access(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
    *,
    can_view: bool,
    can_chat: bool,
    reason: str,
) -> None:
    if running.shaman_channel_id is None:
        return
    channel = guild.get_channel(running.shaman_channel_id)
    if not isinstance(channel, discord.TextChannel):
        running.shaman_channel_id = None
        return
    member = await get_guild_member(guild, player.user_id)
    if not member:
        return
    dead_role = get_dead_player_role(guild)
    should_set_member = (
        dead_role is None
        or player.role == Role.SHAMAN
        or cached_channel_overwrite(channel, member, running) is not None
        or not can_view
        or not can_chat
    )
    if should_set_member:
        with suppress(discord.DiscordException):
            await set_permissions_if_changed(
                channel,
                member,
                overwrite=dead_channel_overwrite(can_view, can_chat and not running.anonymous_enabled and not running.game.is_madam_seduced(player)),
                reason=reason,
                running=running,
            )
    if running.anonymous_enabled:
        await set_anonymous_shaman_input_access(
            guild,
            running,
            player,
            can_view=can_view,
            can_chat=can_chat and not running.game.is_madam_seduced(player),
            reason=reason,
        )
    await upsert_shaman_chat_status(guild, running)


async def set_frog_channel_member_access(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
    *,
    can_view: bool,
    can_chat: bool,
    reason: str,
) -> None:
    if running.frog_channel_id is None:
        return
    channel = guild.get_channel(running.frog_channel_id)
    if not isinstance(channel, discord.TextChannel):
        running.frog_channel_id = None
        return
    member = await get_guild_member(guild, player.user_id)
    if not member:
        return
    try:
        await set_permissions_if_changed(
            channel,
            member,
            overwrite=dead_channel_overwrite(can_view, can_chat),
            reason=reason,
            running=running,
        )
    except discord.DiscordException:
        return


async def set_frog_game_channel_permission(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
    *,
    can_chat: bool,
    reason: str,
) -> None:
    channel = guild.get_channel(running.channel_id)
    if not isinstance(channel, discord.abc.Messageable) or not supports_member_overwrites(channel):
        return
    member = await get_guild_member(guild, player.user_id)
    if not member:
        return
    if member.id not in running.frog_game_channel_overwrites:
        running.frog_game_channel_overwrites[member.id] = clone_overwrite(channel.overwrites.get(member))
    overwrite = channel.overwrites_for(member)
    set_chat_values(overwrite, can_chat)
    try:
        await set_permissions_if_changed(channel, member, overwrite=overwrite, reason=reason, running=running)
    except discord.DiscordException:
        return


async def restore_frog_game_channel_permission(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
) -> None:
    channel = guild.get_channel(running.channel_id)
    if not isinstance(channel, discord.abc.Messageable) or not supports_member_overwrites(channel):
        running.frog_game_channel_overwrites.pop(player.user_id, None)
        return
    member = await get_guild_member(guild, player.user_id)
    if not member:
        running.frog_game_channel_overwrites.pop(player.user_id, None)
        return
    original = running.frog_game_channel_overwrites.pop(member.id, None)
    try:
        await set_permissions_if_changed(
            channel,
            member,
            overwrite=clone_overwrite(original),
            reason="마피아 게임 개구리 저주 종료로 채팅 권한 복구",
            running=running,
        )
    except discord.DiscordException:
        return


async def sync_shaman_channel_permissions(
    guild: discord.Guild,
    running: RunningGame,
    *,
    can_chat: bool,
) -> None:
    for player in running.game.alive_players():
        if player.role != Role.SHAMAN:
            continue
        await set_shaman_channel_member_access(
            guild,
            running,
            player,
            can_view=True,
            can_chat=can_chat,
            reason="마피아 게임 영매 접신 권한 갱신",
        )
    await upsert_shaman_chat_status(guild, running)


async def disable_anonymous_channels_for_player(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
) -> None:
    member = await get_guild_member(guild, player.user_id)
    if not member:
        return
    input_id = running.anonymous_input_channel_ids.get(player.user_id)
    input_channel = guild.get_channel(input_id) if input_id else None
    if isinstance(input_channel, discord.TextChannel):
        with suppress(discord.DiscordException):
            await set_permissions_if_changed(
                input_channel,
                member,
                overwrite=anonymous_input_overwrite(True, False),
                reason="마피아 게임 익명 채팅 권한 제거",
                running=running,
            )
    can_dead_chat = (not player.alive) and player.user_id not in running.game.purified_dead_ids
    await set_anonymous_dead_input_access(
        guild,
        running,
        player,
        can_view=can_dead_chat,
        can_chat=can_dead_chat,
        reason="마피아 게임 사망자 익명 채팅 권한 갱신",
    )
    can_shaman_view = (
        can_dead_chat
        or (player.alive and player.role == Role.SHAMAN and not running.game.is_frog(player))
    )
    await set_anonymous_shaman_input_access(
        guild,
        running,
        player,
        can_view=can_shaman_view,
        can_chat=can_use_anonymous_shaman_chat(running, player),
        reason="마피아 게임 영매 익명 채팅 권한 갱신",
    )
    for role in PRIVATE_CHAT_ROLES:
        await set_anonymous_role_access(
            guild,
            running,
            role,
            player,
            can_access=False,
            reason="마피아 게임 익명 역할 채팅 권한 제거",
        )
    await sync_anonymous_role_statuses(guild, running)


async def disable_private_role_channel_for_player(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
) -> None:
    if running.anonymous_enabled:
        await disable_anonymous_channels_for_player(guild, running, player)
        return

    member = await get_guild_member(guild, player.user_id)
    if not member:
        return

    for channel in private_role_channels(guild, running):
        if cached_channel_overwrite(channel, member, running) is None:
            continue
        try:
            await set_permissions_if_changed(
                channel,
                member,
                overwrite=private_channel_overwrite(False),
                reason="마피아 게임 역할 채팅방 권한 제거",
                running=running,
            )
        except discord.DiscordException:
            continue
    for role in PRIVATE_CHAT_ROLES:
        await upsert_private_role_status_message(guild, running, role)


def private_role_channels(
    guild: discord.Guild,
    running: RunningGame,
) -> list[discord.TextChannel]:
    channels_by_id: dict[int, discord.TextChannel] = {}
    private_channel_names = set(PRIVATE_CHANNEL_NAMES.values())

    for role, channel_id in list(running.private_channel_ids.items()):
        channel = guild.get_channel(channel_id)
        if isinstance(channel, discord.TextChannel):
            channels_by_id[channel.id] = channel
        else:
            running.private_channel_ids.pop(role, None)

    for channel in guild.text_channels:
        if channel.name in private_channel_names:
            channels_by_id[channel.id] = channel

    return list(channels_by_id.values())


async def sync_dead_players_private_role_channels(
    guild: discord.Guild,
    running: RunningGame,
) -> None:
    for dead_player in running.game.dead_players():
        await disable_private_role_channel_for_player(guild, running, dead_player)


async def add_player_to_private_role_channel(
    guild: discord.Guild,
    running: RunningGame,
    channel_role: Role,
    player: Player,
) -> None:
    if running.anonymous_enabled:
        can_access = (
            player.alive
            and not running.game.is_frog(player)
            and not running.game.is_madam_seduced(player)
        )
        await set_anonymous_role_access(
            guild,
            running,
            channel_role,
            player,
            can_access=can_access,
            reason="마피아 게임 익명 역할 채팅 권한 부여",
        )
        await sync_anonymous_role_statuses(guild, running)
        return

    if not player.alive:
        await disable_private_role_channel_for_player(guild, running, player)
        return

    channel_id = running.private_channel_ids.get(channel_role)
    if not channel_id:
        return
    channel = guild.get_channel(channel_id)
    if not isinstance(channel, discord.TextChannel):
        return
    member = await get_guild_member(guild, player.user_id)
    if not member:
        return
    can_chat = not running.game.is_madam_seduced(player)
    try:
        await set_permissions_if_changed(
            channel,
            member,
            overwrite=private_channel_overwrite(can_chat),
            reason="마피아 게임 접선으로 비공개 채널 권한 부여",
            running=running,
        )
    except discord.DiscordException:
        return
    await upsert_private_role_status_message(guild, running, channel_role)


async def set_player_private_channel_access(
    guild: discord.Guild,
    running: RunningGame,
    channel_role: Role,
    player: Player,
    *,
    can_chat: bool,
    reason: str,
) -> None:
    if running.anonymous_enabled:
        can_chat = can_chat and not running.game.is_madam_seduced(player)
        await set_anonymous_role_access(
            guild,
            running,
            channel_role,
            player,
            can_access=can_chat,
            reason=reason,
        )
        await sync_anonymous_role_statuses(guild, running)
        return

    channel_id = running.private_channel_ids.get(channel_role)
    if not channel_id:
        return
    channel = guild.get_channel(channel_id)
    if not isinstance(channel, discord.TextChannel):
        return
    member = await get_guild_member(guild, player.user_id)
    if not member:
        return
    can_chat = can_chat and not running.game.is_madam_seduced(player)
    try:
        await set_permissions_if_changed(
            channel,
            member,
            overwrite=private_channel_overwrite(can_chat),
            reason=reason,
            running=running,
        )
    except discord.DiscordException:
        return


async def sync_lover_chat_access(
    guild: discord.Guild,
    running: RunningGame,
    *,
    reason: str,
) -> None:
    if not any(player.role == Role.LOVER for player in running.game.players):
        return
    can_open = lover_chat_is_open(running.game)
    for player in running.game.players:
        if player.role != Role.LOVER:
            continue
        await set_player_private_channel_access(
            guild,
            running,
            Role.LOVER,
            player,
            can_chat=can_open and player.alive and not running.game.is_frog(player),
            reason=reason,
        )


async def sync_cult_team_channel_access(
    guild: discord.Guild,
    running: RunningGame,
) -> None:
    if not any(player.role in {Role.CULT_LEADER, Role.FANATIC} for player in running.game.players):
        return

    role = Role.CULT_LEADER
    reason = "마피아 게임 교주팀 채팅 권한 갱신"
    if running.anonymous_enabled:
        for player in running.game.players:
            can_view = (
                player.alive
                and not running.game.is_frog(player)
                and running.game.is_cult_team(player)
            )
            if player.alive and can_view and player.role == Role.CULT_LEADER and not running.game.is_madam_seduced(player):
                await set_anonymous_role_access(
                    guild,
                    running,
                    role,
                    player,
                    can_access=True,
                    reason=reason,
                )
            else:
                await set_anonymous_role_view_only(
                    guild,
                    running,
                    role,
                    player,
                    can_view=can_view,
                    reason=reason,
                )
        await sync_anonymous_role_statuses(guild, running)
        return

    channel_id = running.private_channel_ids.get(role)
    channel = guild.get_channel(channel_id) if channel_id else None
    if not isinstance(channel, discord.TextChannel):
        return

    for player in running.game.players:
        member = await get_guild_member(guild, player.user_id)
        if not member:
            continue
        can_view = (
            player.alive
            and not running.game.is_frog(player)
            and running.game.is_cult_team(player)
        )
        can_chat = player.alive and can_view and player.role == Role.CULT_LEADER and not running.game.is_madam_seduced(player)
        with suppress(discord.DiscordException):
            await set_permissions_if_changed(
                channel,
                member,
                overwrite=dead_channel_overwrite(can_view, can_chat),
                reason=reason,
                running=running,
            )


async def refresh_player_private_channel_access(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
) -> None:
    if not player.alive or running.game.is_frog(player):
        await disable_private_role_channel_for_player(guild, running, player)
        return
    if running.anonymous_enabled:
        member = await get_guild_member(guild, player.user_id)
        input_id = running.anonymous_input_channel_ids.get(player.user_id)
        input_channel = guild.get_channel(input_id) if input_id else None
        if member and isinstance(input_channel, discord.TextChannel):
            with suppress(discord.DiscordException):
                await set_permissions_if_changed(
                    input_channel,
                    member,
                    overwrite=anonymous_input_overwrite(
                        True,
                        running.game.phase == Phase.DAY
                        and not running.game.is_frog(player)
                        and not running.game.is_madam_seduced(player),
                    ),
                    reason="마피아 게임 익명 채팅 권한 복구",
                    running=running,
                )
    if player.role in PRIVATE_CHAT_ROLES:
        await add_player_to_private_role_channel(guild, running, player.role, player)
    if running.game.is_known_mafia_team(player):
        await add_player_to_private_role_channel(guild, running, Role.MAFIA, player)


async def delete_private_role_channels(guild: discord.Guild, running: RunningGame) -> None:
    for role, channel_id in list(running.private_channel_ids.items()):
        channel = guild.get_channel(channel_id)
        if channel:
            try:
                await channel.delete(reason="마피아 게임 종료로 역할별 비공개 채널 삭제")
            except discord.DiscordException:
                continue
        running.private_channel_ids.pop(role, None)


async def delete_memo_channels(guild: discord.Guild, running: RunningGame) -> None:
    for user_id, channel_id in list(running.memo_channel_ids.items()):
        channel = guild.get_channel(channel_id)
        if channel:
            with suppress(discord.DiscordException):
                await channel.delete(reason="마피아 게임 종료로 개인 메모 채널 삭제")
        running.memo_channel_ids.pop(user_id, None)
    running.memos.clear()


async def delete_anonymous_chat_channels(guild: discord.Guild, running: RunningGame) -> None:
    channel_ids: set[int] = set()
    channel_ids.update(running.anonymous_input_channel_ids.values())
    channel_ids.update(running.anonymous_dead_input_channel_ids.values())
    channel_ids.update(running.anonymous_shaman_input_channel_ids.values())
    channel_ids.update(running.anonymous_role_input_channel_ids.values())

    for channel_id in channel_ids:
        channel = guild.get_channel(channel_id)
        if channel:
            with suppress(discord.DiscordException):
                await channel.delete(reason="마피아 게임 종료로 익명 채팅방 삭제")

    running.anonymous_input_channel_ids.clear()
    running.anonymous_input_channel_owners.clear()
    running.anonymous_dead_input_channel_ids.clear()
    running.anonymous_dead_input_channel_owners.clear()
    running.anonymous_shaman_input_channel_ids.clear()
    running.anonymous_shaman_input_channel_owners.clear()
    running.anonymous_role_input_channel_ids.clear()
    running.anonymous_role_input_channels.clear()
    running.anonymous_role_status_message_ids.clear()
    running.anonymous_role_input_status_message_ids.clear()
    running.anonymous_aliases.clear()
    running.anonymous_original_names.clear()
    running.anonymous_webhook_urls.clear()


async def cleanup_old_dead_chat_channels(guild: discord.Guild) -> None:
    for channel in guild.text_channels:
        if channel.name != OLD_DEAD_CHAT_CHANNEL_NAME:
            continue
        with suppress(discord.DiscordException):
            await channel.delete(reason="마피아 게임 공용 사망자 채팅방 미사용으로 삭제")


async def delete_shaman_chat_channel(guild: discord.Guild, running: RunningGame) -> None:
    if running.shaman_channel_id is None:
        return
    channel = guild.get_channel(running.shaman_channel_id)
    if channel:
        try:
            await channel.delete(reason="마피아 게임 종료로 영매 채팅방 삭제")
        except discord.DiscordException:
            return
    running.shaman_channel_id = None
    running.shaman_status_message_id = None


async def delete_frog_chat_channel(guild: discord.Guild, running: RunningGame) -> None:
    if running.frog_channel_id is None:
        return
    channel = guild.get_channel(running.frog_channel_id)
    if channel:
        try:
            await channel.delete(reason="마피아 게임 종료로 개구리 채팅방 삭제")
        except discord.DiscordException:
            return
    running.frog_channel_id = None


async def warm_anonymous_startup_resources(guild_id: int, running: RunningGame) -> None:
    await asyncio.sleep(5)
    guild = bot.get_guild(guild_id)
    if not guild or games.get(running.guild_id) is not running or not running.anonymous_enabled:
        return

    await sync_anonymous_role_statuses(guild, running)

    channel_ids: list[int] = []
    channel_ids.extend(running.anonymous_input_channel_ids.values())
    channel_ids.extend(running.anonymous_dead_input_channel_ids.values())
    channel_ids.extend(running.anonymous_shaman_input_channel_ids.values())
    channel_ids.extend(running.anonymous_role_input_channel_ids.values())
    seen: set[int] = set()
    for channel_id in channel_ids:
        if channel_id in seen:
            continue
        seen.add(channel_id)
        if games.get(running.guild_id) is not running or not running.anonymous_enabled:
            return
        channel = guild.get_channel(channel_id)
        if isinstance(channel, discord.TextChannel):
            await prepare_anonymous_webhook(channel, running)
            await asyncio.sleep(0.05)


async def restore_all_frog_game_channel_permissions(
    guild: discord.Guild,
    running: RunningGame,
) -> None:
    for user_id in list(running.frog_game_channel_overwrites):
        player = running.game.get_player(user_id)
        if player:
            await restore_frog_game_channel_permission(guild, running, player)
        else:
            running.frog_game_channel_overwrites.pop(user_id, None)


async def cleanup_game(guild: discord.Guild, running: RunningGame) -> None:
    await restore_all_frog_game_channel_permissions(guild, running)
    await restore_member_channel_chat(guild, running)
    await restore_madam_seduction_permissions(guild, running)
    await restore_original_game_channel_for_anonymous(guild, running)
    await restore_game_channel_chat(guild, running)
    await restore_channel_slowmode(guild, running)
    await remove_game_participant_roles(guild, running)
    await remove_game_dead_player_roles(guild, running)
    await remove_game_spectator_roles(guild, running)
    await delete_private_role_channels(guild, running)
    await delete_memo_channels(guild, running)
    await delete_anonymous_chat_channels(guild, running)
    await delete_shaman_chat_channel(guild, running)
    await delete_frog_chat_channel(guild, running)

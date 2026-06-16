from __future__ import annotations

from bot import *  # noqa: F401,F403


__all__ = (
    'game_loop',
    'apply_day_channel_state',
    'run_vote_phase',
    'set_final_defense_mode',
    'send_hacker_day_actions',
    'send_vigilante_day_actions',
    'send_psychologist_day_actions',
    'send_thief_vote_actions',
    'run_day_discussion',
    'send_roles',
    'restore_frogs_for_new_night',
    'apply_frog_permissions',
    'remove_frog_permissions',
    'restore_revived_player_roles',
    'sync_scientist_mafia_permissions',
    'apply_timed_night_state',
    'timed_night_events_have_work',
    'announce_timed_night_events',
    'trigger_timed_night_events',
    'wait_for_night_actions',
    'run_night',
    'announce_police_result',
    'announce_hacker_results',
    'announce_vigilante_investigation_results',
    'send_police_result_message',
    'announce_night_private_results',
    'announce_public_police_status',
    'announce_morning_mafia_count',
    'night_targets',
    'announce_winner',
    'announce_final_roles',
    'final_role_reveal_text',
)


async def game_loop(guild: discord.Guild, running: RunningGame) -> None:
    channel = guild.get_channel(running.channel_id)
    if not isinstance(channel, discord.abc.Messageable):
        games.pop(running.guild_id, None)
        return

    try:
        original_channel = channel
        await create_anonymous_chat_channels(guild, original_channel, running)
        channel = original_channel
        await set_spectator_game_channel_access(
            guild,
            original_channel,
            running,
            "마피아 게임 관전자 게임 채널 열람 권한 설정",
        )
        await cleanup_old_dead_chat_channels(guild)
        await hide_original_game_channel_for_anonymous(guild, original_channel, running)
        await create_private_role_channels(guild, channel, running)
        await create_memo_channels(guild, channel, running)
        await sync_cult_team_channel_access(guild, running)
        await create_shaman_chat_channel(guild, channel, running)
        await create_frog_chat_channel(guild, channel, running)
        await send_game_embed(
            guild,
            channel,
            running,
            public_game_settings_text(running.game, "게임 방 설정입니다."),
            title="방 설정",
            broadcast=not running.anonymous_enabled,
        )
        await send_game_embed(
            guild,
            channel,
            running,
            game_rule_text(running.game, running.reveal_death_roles),
            title="게임 설명",
            broadcast=not running.anonymous_enabled,
        )
        await send_roles(guild, running)
        await send_game_embed(
            guild,
            channel,
            running,
            "역할 배정이 끝났습니다. 각자 비밀 메시지와 역할별 비공개 채널을 확인하세요.",
            title="역할 배정 완료",
            color=SUCCESS_EMBED_COLOR,
            broadcast=not running.anonymous_enabled,
        )
        if running.anonymous_enabled:
            asyncio.create_task(warm_anonymous_startup_resources(guild.id, running))
        await upsert_game_status(guild, running)

        while running.game.phase != Phase.ENDED:
            await run_night(guild, channel, running)
            if await announce_winner(channel, running):
                break

            day_result = await run_day_discussion(channel, running)
            if day_result == "stop":
                break

            await run_vote_phase(guild, channel, running)

            if await announce_winner(channel, running):
                break
    except asyncio.CancelledError:
        return
    except Exception as error:
        print(f"Game loop error: {error!r}")
        with suppress(discord.DiscordException):
            await send_embed(
                original_channel,
                "게임 시작 중 오류가 발생해 자동 정리했습니다.\n"
                f"오류: `{type(error).__name__}: {error}`",
                title="게임 시작 오류",
                color=ERROR_EMBED_COLOR,
            )
    finally:
        await cleanup_game(guild, running)
        if games.get(running.guild_id) is running:
            games.pop(running.guild_id, None)


async def apply_day_channel_state(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    running.day_chat_open = True
    await set_game_channel_chat(
        guild,
        channel,
        running,
        participants_can_chat=True,
        reason="마피아 게임 낮 토론 시작",
    )
    await sync_madam_seduction_permissions(
        guild,
        running,
        reason="마피아 게임 낮 시작으로 마담 유혹 권한 갱신",
    )
    await set_channel_slowmode(
        channel,
        running,
        config.chat_slowmode_seconds,
        "마피아 게임 낮 토론 슬로우모드 적용",
    )
    await upsert_game_status(guild, running)


async def run_vote_phase(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    running.day_chat_open = False
    running.game.start_vote()
    await upsert_game_status(guild, running)
    running.vote_complete_event.clear()
    alive = running.game.alive_players()
    await set_game_channel_chat(
        guild,
        channel,
        running,
        participants_can_chat=False,
        reason="마피아 게임 투표 시작",
    )
    await send_game_embed(
        guild,
        channel,
        running,
        f"지목 투표를 시작합니다. {config.vote_seconds}초 안에 최후변론에 세울 사람을 선택하세요.\n"
        "투표 중에는 게임 채널 채팅이 비활성화됩니다.\n"
        "생존자가 모두 투표하면 남은 시간을 기다리지 않고 바로 정산합니다.",
        view=DayVoteView(running.guild_id, alive),
        title="지목 투표 시작",
    )
    await send_thief_vote_actions(channel, running)
    await wait_for_event_or_timeout(running.vote_complete_event, config.vote_seconds)

    vote_result = running.game.resolve_nomination_vote()
    await handle_madam_seduction_result(guild, running, vote_result)
    vote_summary = anonymous_vote_summary(running.game, vote_result)
    blocked_notice = ""
    if vote_result.blocked_voters:
        blocked_names = ", ".join(player.name for player in vote_result.blocked_voters)
        blocked_notice = f"\n\n공갈로 투표권을 잃은 참가자: {blocked_names}"
    nominee = vote_result.executed
    if not nominee:
        if vote_result.tied:
            message = "투표가 동률이라 최후변론 대상이 없습니다."
        elif vote_result.skipped:
            message = "스킵이 최다 득표하여 최후변론 대상이 없습니다."
        else:
            message = "투표가 없어 최후변론 대상이 없습니다."
        await send_game_embed(
            guild,
            channel,
            running,
            f"{message}{blocked_notice}\n\n익명 투표 집계\n{vote_summary}",
            title="지목 투표 결과",
        )
        return

    await send_game_embed(
        guild,
        channel,
        running,
        f"지목 투표 결과, {nominee.name} 님이 최후변론 대상이 되었습니다.{blocked_notice}\n\n익명 투표 집계\n{vote_summary}",
        title="지목 투표 결과",
    )
    await set_final_defense_mode(guild, channel, running, nominee)
    await send_game_embed(
        guild,
        channel,
        running,
        f"{nominee.name} 님의 최후변론 시간입니다. 20초 동안 지목된 사람만 말할 수 있습니다.\n"
        "이 시간 동안 슬로우모드는 해제됩니다.",
        title="최후변론",
    )
    await asyncio.sleep(20)
    await restore_member_channel_chat(guild, running)

    running.game.start_confirmation_vote()
    await upsert_game_status(guild, running)
    running.confirm_complete_event.clear()
    await send_game_embed(
        guild,
        channel,
        running,
        f"{nominee.name} 님 처형 여부를 찬반투표합니다. {CONFIRM_VOTE_SECONDS}초 안에 선택하세요.\n"
        "찬성이 반대보다 많으면 처형됩니다.",
        view=ConfirmVoteView(running.guild_id),
        title="찬반투표",
    )
    await wait_for_event_or_timeout(running.confirm_complete_event, CONFIRM_VOTE_SECONDS)
    confirm_result = running.game.resolve_confirmation_vote(nominee.user_id)
    await set_channel_slowmode(
        channel,
        running,
        config.chat_slowmode_seconds,
        "마피아 게임 찬반투표 종료 후 슬로우모드 복구",
    )

    counts = confirm_result.vote_counts
    summary = f"찬성 {counts.get(True, 0)}표 / 반대 {counts.get(False, 0)}표"
    judge_notice = ""
    if confirm_result.decided_by_judge and confirm_result.judge:
        if confirm_result.judge_choice is None:
            judge_choice = "미투표(처형 없음)"
        else:
            judge_choice = "찬성" if confirm_result.judge_choice else "반대"
        judge_notice = (
            f"\n\n[판사 {confirm_result.judge.name}님이 투표 결과를 정했습니다]\n"
            f"판사의 선택: {judge_choice}"
        )
    if confirm_result.blocked_by_politician:
        await send_game_embed(
            guild,
            channel,
            running,
            f"찬반투표 결과, {nominee.name} 님은 **정치인** 입니다.\n"
            "[정치인은 투표로 죽지 않습니다]\n\n"
            f"{nominee.name} 님은 처형되지 않고 밤으로 넘어갑니다."
            f"{judge_notice}\n\n"
            f"찬반투표 집계\n{summary}",
            title="찬반투표 결과",
            color=WARNING_EMBED_COLOR,
        )
    elif confirm_result.executed:
        killed_players = [confirm_result.executed, *confirm_result.extra_killed]
        killed_lines: list[str] = []
        for killed in killed_players:
            removed_role = await remove_participant_role_from_dead(guild, running, killed)
            added_dead_role = await add_dead_player_role(guild, killed)
            await set_dead_channel_member_access(
                guild,
                running,
                killed,
                can_view=True,
                can_chat=killed.user_id not in running.game.purified_dead_ids,
                reason="마피아 게임 사망자 채팅방 권한 부여",
            )
            await set_shaman_channel_member_access(
                guild,
                running,
                killed,
                can_view=True,
                can_chat=killed.user_id not in running.game.purified_dead_ids,
                reason="마피아 게임 영매 채팅방 권한 부여",
            )
            await remove_frog_permissions(guild, running, killed)
            await disable_private_role_channel_for_player(guild, running, killed)
            if killed.role == Role.SCIENTIST and killed.user_id in running.game.scientist_contacted:
                await set_player_private_channel_access(
                    guild,
                    running,
                    Role.MAFIA,
                    killed,
                    can_chat=True,
                    reason="마피아 게임 과학자 유착으로 마피아 채널 권한 부여",
                )
            line = f"- {killed.name}: {death_role_text(running, killed)}"
            if removed_role:
                line += f" '{config.participant_role}' 역할을 제거했습니다."
            else:
                line += (
                    f" '{config.participant_role}' 역할 제거에 실패했습니다. "
                    "봇의 역할 관리 권한과 역할 순서를 확인하세요."
                )
            if added_dead_role is True:
                line += f" '{DEAD_PLAYER_ROLE}' 역할을 부여했습니다."
            elif added_dead_role is None:
                line += f" '{DEAD_PLAYER_ROLE}' 역할을 찾지 못했습니다."
            else:
                line += f" '{DEAD_PLAYER_ROLE}' 역할 부여에 실패했습니다."
            killed_lines.append(line)
        await sync_dead_players_private_role_channels(guild, running)
        await sync_cult_team_channel_access(guild, running)
        await sync_lover_chat_access(guild, running, reason="마피아 게임 투표 사망으로 연인 채팅 권한 갱신")
        await upsert_game_status(guild, running)

        message = f"찬반투표 결과, {confirm_result.executed.name} 님이 처형되었습니다."
        if confirm_result.extra_killed:
            if confirm_result.executed.role == Role.TERRORIST:
                message += "\n테러리스트의 [산화]가 발동해 지목 중이던 적 팀도 함께 사망했습니다."
            else:
                message += "\n처형 대상이 지목하고 있던 시민팀이 아닌 대상도 함께 사망했습니다."
        await send_game_embed(
            guild,
            channel,
            running,
            f"{message}\n\n사망자\n"
            + "\n".join(killed_lines)
            + f"{judge_notice}\n\n찬반투표 집계\n{summary}",
            title="찬반투표 결과",
            include_dead=True,
        )
    elif confirm_result.tied:
        await send_game_embed(
            guild,
            channel,
            running,
            f"찬반투표가 동률이라 처형하지 않습니다.{judge_notice}\n\n찬반투표 집계\n{summary}",
            title="찬반투표 결과",
        )
    else:
        reject_message = (
            "판사의 선택으로 처형하지 않습니다."
            if confirm_result.decided_by_judge
            else "반대가 많아 처형하지 않습니다."
        )
        await send_game_embed(
            guild,
            channel,
            running,
            f"{reject_message}{judge_notice}\n\n찬반투표 집계\n{summary}",
            title="찬반투표 결과",
        )


async def set_final_defense_mode(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
    nominee: Player,
) -> None:
    running.final_defense_user_id = nominee.user_id
    running.day_chat_open = False
    await set_game_channel_chat(
        guild,
        channel,
        running,
        participants_can_chat=False,
        reason="마피아 게임 최후변론 시작",
    )
    await set_channel_slowmode(channel, running, 0, "마피아 게임 최후변론 슬로우모드 해제")
    if not running.game.is_frog(nominee) and not running.game.is_madam_seduced(nominee):
        await set_member_chat_permission(
            guild,
            channel,
            running,
            nominee,
            True,
            "마피아 게임 최후변론 대상 발언 재허용",
        )
        if running.anonymous_enabled:
            await set_anonymous_general_input_access(
                guild,
                running,
                nominee,
                can_chat=True,
                reason="마피아 게임 익명 최후변론 대상 발언 재허용",
            )


async def send_hacker_day_actions(
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    guild = bot.get_guild(running.guild_id)
    if not guild:
        return
    failed_names: list[str] = []
    for actor in running.game.hacker_day_actors():
        targets = [
            player
            for player in sorted(running.game.alive_players(), key=lambda item: item.name.casefold())
            if player.user_id != actor.user_id
        ]
        sent = await send_player_secret(
            guild,
            running,
            actor,
            "해커 낮 행동을 선택하세요.\n"
            "해킹은 1회용입니다. 선택한 대상의 직업은 밤이 시작될 때 비밀 메시지로 전달됩니다.\n"
            "해킹 사용 후 자신에게 쓰이는 능력은 해킹 대상에게 우회됩니다.",
            HackerDayActionView(running.guild_id, actor, targets),
        )
        if not sent:
            failed_names.append(actor.name)
    if failed_names:
        await send_embed(
            channel,
            "해커 낮 행동 DM을 보낼 수 없는 참가자: " + ", ".join(failed_names),
            color=ERROR_EMBED_COLOR,
        )


async def send_vigilante_day_actions(
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    guild = bot.get_guild(running.guild_id)
    if not guild:
        return
    failed_names: list[str] = []
    for actor in running.game.vigilante_day_actors():
        targets = [
            player
            for player in sorted(running.game.alive_players(), key=lambda item: item.name.casefold())
            if player.user_id != actor.user_id
        ]
        sent = await send_player_secret(
            guild,
            running,
            actor,
            "자경단원 낮 행동을 선택하세요.\n"
            "숙청 조사는 1회용입니다. 밤이 시작될 때 대상이 마피아팀인지 비밀 메시지로 전달됩니다.\n"
            "숙청 처형은 조사와 별개로 밤에 한 번 시도할 수 있고, 마피아팀이 아니어도 기회가 소진됩니다.",
            VigilanteDayActionView(running.guild_id, actor, targets),
        )
        if not sent:
            failed_names.append(actor.name)
    if failed_names:
        await send_embed(
            channel,
            "자경단원 낮 행동 DM을 보낼 수 없는 참가자: " + ", ".join(failed_names),
            color=ERROR_EMBED_COLOR,
        )


async def send_psychologist_day_actions(
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    guild = bot.get_guild(running.guild_id)
    if not guild:
        return
    failed_names: list[str] = []
    for actor in running.game.psychologist_day_actors():
        targets = [
            player
            for player in sorted(running.game.alive_players(), key=lambda item: item.name.casefold())
            if player.user_id != actor.user_id
        ]
        sent = await send_player_secret(
            guild,
            running,
            actor,
            "심리학자 낮 행동을 선택하세요.\n"
            "자신을 제외한 생존자 2명을 선택하면 두 사람이 같은 팀인지 즉시 확인합니다.",
            PsychologistDayActionView(running.guild_id, actor, targets),
        )
        if not sent:
            failed_names.append(actor.name)
    if failed_names:
        await send_embed(
            channel,
            "심리학자 낮 행동 선택지를 보낼 수 없는 참가자: " + ", ".join(failed_names),
            color=ERROR_EMBED_COLOR,
        )


async def send_thief_vote_actions(
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    guild = bot.get_guild(running.guild_id)
    if not guild:
        return
    failed_names: list[str] = []
    for actor in running.game.thief_vote_actors():
        targets = [
            player
            for player in sorted(running.game.alive_players(), key=lambda item: item.name.casefold())
            if player.user_id != actor.user_id
        ]
        sent = await send_player_secret(
            guild,
            running,
            actor,
            "도둑 투표 시간 행동을 선택하세요.\n"
            "하루에 한 번 플레이어 한 명의 직업 능력을 훔쳐 다음 밤까지 사용할 수 있습니다.",
            ThiefVoteActionView(running.guild_id, actor, targets),
        )
        if not sent:
            failed_names.append(actor.name)
    if failed_names:
        await send_embed(
            channel,
            "도둑 도벽 선택지를 보낼 수 없는 참가자: " + ", ".join(failed_names),
            color=ERROR_EMBED_COLOR,
        )


async def run_day_discussion(
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> DayDiscussionResult:
    guild = bot.get_guild(running.guild_id)
    running.day_vote_event.clear()
    discussion_seconds = config.discussion_seconds
    extension_used = False
    discussion_time = duration_text(discussion_seconds)
    alive_user_ids = {player.user_id for player in running.game.alive_players()}
    vote_view = DaySkipToVoteView(running.guild_id, alive_user_ids)
    await send_hacker_day_actions(channel, running)
    await send_vigilante_day_actions(channel, running)
    await send_psychologist_day_actions(channel, running)
    day_message_text = (
        f"{running.game.day_number}일차 낮입니다. {discussion_time} 동안 자유롭게 토론하세요.\n"
        "생존자 과반이 `바로 투표`를 누르면 토론과 연장을 끝내고 바로 지목 투표로 넘어갑니다.\n"
        f"시간이 지나면 {DAY_EXTENSION_VOTE_SECONDS}초 동안 1분 연장 투표가 열립니다. "
        "생존자 과반수가 연장을 누르면 1분 연장되고, 연장은 낮마다 1번만 가능합니다. "
        "과반수가 모이지 않으면 바로 투표로 넘어갑니다.\n"
        f"{running.game.public_status()}"
    )
    if guild:
        day_message = await send_game_embed(
            guild,
            channel,
            running,
            day_message_text,
            view=vote_view,
            title="낮 토론",
        )
    else:
        day_message = await send_embed(channel, day_message_text, view=vote_view, title="낮 토론")
    if guild:
        await apply_day_channel_state(guild, channel, running)

    while running.game.phase == Phase.DAY and games.get(running.guild_id) is running:
        if await wait_for_day_vote_or_timeout(running, discussion_seconds):
            await disable_message_view(day_message, vote_view)
            return "vote"
        if running.game.phase == Phase.ENDED or games.get(running.guild_id) is not running:
            await disable_message_view(day_message, vote_view)
            return "stop"

        if extension_used:
            if guild:
                await send_game_embed(
                    guild,
                    channel,
                    running,
                    "연장된 토론 시간이 종료되었습니다.\n"
                    "토론 연장은 낮마다 1번만 가능하므로 바로 지목 투표로 넘어갑니다.",
                    title="낮 토론 종료",
                )
            else:
                await send_embed(
                    channel,
                    "연장된 토론 시간이 종료되었습니다.\n"
                    "토론 연장은 낮마다 1번만 가능하므로 바로 지목 투표로 넘어갑니다.",
                    title="낮 토론 종료",
                )
            await disable_message_view(day_message, vote_view)
            return "vote"

        alive_user_ids = {player.user_id for player in running.game.alive_players()}
        extension_view = DayExtensionVoteView(running.guild_id, alive_user_ids)
        extension_message_text = (
            f"{duration_text(discussion_seconds)} 토론 시간이 지났습니다.\n"
            f"{DAY_EXTENSION_VOTE_SECONDS}초 안에 생존자 과반수"
            f"({extension_view.required_votes}/{len(alive_user_ids)}명)가 `1분 연장`을 누르면 "
            "낮 토론을 1분 연장합니다.\n"
            "과반수가 모이지 않으면 바로 투표로 넘어갑니다."
        )
        if guild:
            vote_message = await send_game_embed(
                guild,
                channel,
                running,
                extension_message_text,
                view=extension_view,
                title="낮 토론 연장 투표",
            )
        else:
            vote_message = await send_embed(
                channel,
                extension_message_text,
                view=extension_view,
                title="낮 토론 연장 투표",
            )
        skipped_to_vote = await wait_for_day_vote_or_view(running, extension_view)
        extension_view.accepting = False
        disable_view_items(extension_view)

        if skipped_to_vote:
            try:
                await vote_message.edit(
                    embed=make_embed(
                        "생존자 과반수가 바로 투표를 선택해 연장 투표를 종료합니다.\n"
                        "바로 지목 투표로 넘어갑니다.",
                        title="바로 투표",
                        color=SUCCESS_EMBED_COLOR,
                    ),
                    view=extension_view,
                )
            except discord.DiscordException:
                pass
            await disable_message_view(day_message, vote_view)
            return "vote"

        if running.game.phase == Phase.ENDED or games.get(running.guild_id) is not running:
            await disable_message_view(day_message, vote_view)
            return "stop"
        if extension_view.extended:
            extension_used = True
            discussion_seconds = DISCUSSION_EXTENSION_SECONDS
            continue

        try:
            await vote_message.edit(
                embed=make_embed(
                    f"{DAY_EXTENSION_VOTE_SECONDS}초 동안 1분 연장 투표가 과반수에 도달하지 못했습니다. "
                    f"({len(extension_view.voter_ids)}/{extension_view.required_votes}명)\n"
                    "바로 투표로 넘어갑니다.",
                    title="낮 토론 종료",
                ),
                view=extension_view,
            )
        except discord.DiscordException:
            pass
        await disable_message_view(day_message, vote_view)
        return "vote"

    await disable_message_view(day_message, vote_view)
    return "stop"


async def send_roles(guild: discord.Guild, running: RunningGame) -> None:
    channel = guild.get_channel(running.channel_id)
    semaphore = asyncio.Semaphore(8)

    async def send_one(player: Player) -> str | None:
        async with semaphore:
            anonymous_notice = ""
            if running.anonymous_enabled:
                alias = running.anonymous_aliases.get(player.user_id, "익명")
                anonymous_notice = (
                    f"\n\n익명 이름: **{alias}**\n"
                    "채팅은 서버에 생성된 본인 익명 입력 채널에서만 진행하세요."
                )
            sent = await send_player_secret(
                guild,
                running,
                player,
                f"{role_message(running.game, player)}\n\n"
                f"방 설정\n{public_game_settings_text(running.game, '현재 게임 설정입니다.')}\n\n"
                f"게임 설명\n{game_rule_text(running.game, running.reveal_death_roles)}\n\n"
                "본인 역할 설명은 `/마피아능력`, 전체 역할 설명은 `/역할설명`으로 다시 확인할 수 있습니다."
                f"{anonymous_notice}",
            )
            return None if sent else player.name

    results = await asyncio.gather(
        *(send_one(player) for player in running.game.players),
        return_exceptions=True,
    )
    failed_names = [
        result
        for result in results
        if isinstance(result, str)
    ]

    if failed_names and isinstance(channel, discord.abc.Messageable):
        await send_embed(
            channel,
            "비밀 메시지를 보낼 수 없는 참가자: " + ", ".join(failed_names),
            color=ERROR_EMBED_COLOR,
        )


async def restore_frogs_for_new_night(guild: discord.Guild, running: RunningGame) -> None:
    restored = False
    for player in running.game.restore_frogs():
        restored = True
        await set_frog_channel_member_access(
            guild,
            running,
            player,
            can_view=False,
            can_chat=False,
            reason="마피아 게임 개구리 저주 종료",
        )
        await restore_frog_game_channel_permission(guild, running, player)
        await refresh_player_private_channel_access(guild, running, player)
    if restored:
        await sync_cult_team_channel_access(guild, running)
        await sync_shaman_channel_permissions(guild, running, can_chat=running.game.phase == Phase.NIGHT)


async def apply_frog_permissions(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
) -> None:
    await set_frog_channel_member_access(
        guild,
        running,
        player,
        can_view=True,
        can_chat=True,
        reason="마피아 게임 마녀 저주로 개구리 채팅방 권한 부여",
    )
    await set_frog_game_channel_permission(
        guild,
        running,
        player,
        can_chat=False,
        reason="마피아 게임 마녀 저주로 게임 채널 발언 제한",
    )
    if player.role == Role.SHAMAN:
        await set_shaman_channel_member_access(
            guild,
            running,
            player,
            can_view=True,
            can_chat=False,
            reason="마피아 게임 마녀 저주로 영매 채팅 권한 제거",
        )
    await disable_private_role_channel_for_player(guild, running, player)


async def remove_frog_permissions(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
) -> None:
    await set_frog_channel_member_access(
        guild,
        running,
        player,
        can_view=False,
        can_chat=False,
        reason="마피아 게임 개구리 상태 종료",
    )
    await restore_frog_game_channel_permission(guild, running, player)


async def restore_revived_player_roles(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
    *,
    reason_prefix: str = "마피아 게임 부활",
) -> None:
    participant_role = get_participant_role(guild)
    member = await get_guild_member(guild, player.user_id)
    if member and participant_role and participant_role not in member.roles:
        try:
            await member.add_roles(participant_role, reason=f"{reason_prefix}로 참가자 역할 복구")
        except discord.DiscordException:
            pass
    await remove_dead_player_roles_from_ids(
        guild,
        {player.user_id},
        f"{reason_prefix}로 사망자 역할 제거",
    )
    await set_dead_channel_member_access(
        guild,
        running,
        player,
        can_view=False,
        can_chat=False,
        reason=f"{reason_prefix}로 사망자 채팅방 권한 제거",
    )
    await set_shaman_channel_member_access(
        guild,
        running,
        player,
        can_view=False,
        can_chat=False,
        reason=f"{reason_prefix}로 영매 채팅방 권한 제거",
    )
    await refresh_player_private_channel_access(guild, running, player)
    await upsert_game_status(guild, running)


async def sync_scientist_mafia_permissions(
    guild: discord.Guild,
    running: RunningGame,
) -> None:
    for player in running.game.players:
        if player.role != Role.SCIENTIST or player.user_id not in running.game.scientist_contacted:
            continue
        if player.alive:
            await refresh_player_private_channel_access(guild, running, player)
        elif player.user_id in running.game.scientist_pending_revive_ids:
            await set_player_private_channel_access(
                guild,
                running,
                Role.MAFIA,
                player,
                can_chat=True,
                reason="마피아 게임 과학자 유착으로 마피아 채널 권한 부여",
            )


def apply_timed_night_state(running: RunningGame) -> TimedNightEvents:
    cursed_players, witch_contacts = running.game.apply_witch_curses()
    cult_bell_count = running.game.consume_cult_bells()
    revived_players = running.game.revive_pending_scientists()
    return TimedNightEvents(
        cursed_players=cursed_players,
        witch_contacts=witch_contacts,
        cult_bell_count=cult_bell_count,
        revived_players=revived_players,
    )


def timed_night_events_have_work(events: TimedNightEvents) -> bool:
    return bool(
        events.cursed_players
        or events.witch_contacts
        or events.cult_bell_count
        or events.revived_players
    )


async def announce_timed_night_events(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
    events: TimedNightEvents,
) -> None:
    for player in events.cursed_players:
        await apply_frog_permissions(guild, running, player)
    for user_id in events.witch_contacts:
        player = running.game.get_player(user_id)
        if player:
            await add_player_to_private_role_channel(guild, running, Role.MAFIA, player)
            await send_player_secret(guild, running, player, "저주 대상이 마피아라 마피아와 접선했습니다.")
    if events.cursed_players:
        await send_game_embed(
            guild,
            channel,
            running,
            "마녀의 저주가 발동했습니다.\n누군가 다음 밤까지 개구리가 되었습니다.",
            title="마녀 저주",
            color=WARNING_EMBED_COLOR,
        )
    if events.cult_bell_count:
        await announce_cult_bells_now(running, events.cult_bell_count)
    for player in events.revived_players:
        await restore_revived_player_roles(guild, running, player)
    if events.revived_players:
        await sync_cult_team_channel_access(guild, running)
        await send_game_embed(
            guild,
            channel,
            running,
            "\n".join(f"[과학자 {player.name}님이 부활했습니다.]" for player in events.revived_players),
            title="과학자 부활",
            color=SUCCESS_EMBED_COLOR,
        )


def trigger_timed_night_events(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    events = apply_timed_night_state(running)
    if not timed_night_events_have_work(events):
        return
    create_logged_background_task(
        announce_timed_night_events(guild, channel, running, events),
        "timed night events",
    )


async def wait_for_night_actions(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    running.night_timed_events_due = config.night_seconds <= 10
    if config.night_seconds <= 10:
        await wait_for_event_or_timeout(running.night_complete_event, config.night_seconds)
        trigger_timed_night_events(guild, channel, running)
        return

    await wait_for_event_or_timeout(running.night_complete_event, config.night_seconds - 10)
    if running.night_complete_event.is_set():
        return
    if running.game.phase == Phase.NIGHT and games.get(running.guild_id) is running:
        await send_game_embed(
            guild,
            channel,
            running,
            "밤 시간이 10초 남았습니다. 아직 행동하지 않았다면 지금 선택하세요.",
            title="밤 10초 전",
        )
        running.night_timed_events_due = True
        trigger_timed_night_events(guild, channel, running)
    await wait_for_event_or_timeout(running.night_complete_event, 10)


async def run_night(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    running.game.phase = Phase.NIGHT
    running.day_chat_open = False
    running.game.police_result_announced = False
    await upsert_game_status(guild, running)
    running.night_complete_event.clear()
    running.night_timed_events_due = config.night_seconds <= 10
    await restore_frogs_for_new_night(guild, running)
    await sync_lover_chat_access(guild, running, reason="마피아 게임 밤 시작으로 연인 채팅 권한 갱신")
    await announce_hacker_results(guild, running)
    await announce_vigilante_investigation_results(guild, running)
    await sync_scientist_mafia_permissions(guild, running)
    for user_id in running.game.ensure_godfather_auto_contact():
        player = running.game.get_player(user_id)
        if player:
            await add_player_to_private_role_channel(guild, running, Role.MAFIA, player)
            await send_player_secret(
                guild,
                running,
                player,
                "세 번째 밤이 되어 마피아 팀과 자동 접선했습니다. 이제 마피아 비밀방을 볼 수 있고 밤마다 확정 처치 대상을 지목합니다.",
            )
    police_can_act = any(actor.role == Role.POLICE for actor in running.game.night_action_actors())
    await set_game_channel_chat(
        guild,
        channel,
        running,
        participants_can_chat=False,
        reason="마피아 게임 밤 시작",
    )
    await sync_shaman_channel_permissions(guild, running, can_chat=True)
    await send_game_embed(
        guild,
        channel,
        running,
        f"밤이 되었습니다. {config.night_seconds}초 동안 게임 채널 채팅이 비활성화됩니다.\n"
        "밤 행동이 있는 역할은 본인 익명 채널 또는 DM에서 선택합니다.\n"
        "행동 가능한 역할이 모두 선택하면 남은 시간을 기다리지 않고 바로 아침으로 넘어갑니다.",
        title="밤",
    )
    if running.night_timed_events_due:
        trigger_timed_night_events(guild, channel, running)
    if has_changeable_mafia_action(running):
        await sync_role_status_message(guild, running, Role.MAFIA)

    failed_names: list[str] = []
    for actor in running.game.night_action_actors():
        effective_role = running.game.thief_night_role(actor) if actor.role == Role.THIEF else actor.role
        if effective_role == Role.CONTRACTOR:
            contract_targets = sorted(
                running.game.contractor_contract_targets(actor),
                key=lambda player: player.name.casefold(),
            )
            sent = await send_player_secret(
                guild,
                running,
                actor,
                "청부업자 밤 행동을 선택하세요.\n"
                "두 명과 각 직업을 추측합니다. 둘 중 한 명이라도 마피아를 정확히 맞히면 접선합니다.\n"
                "첫날 밤에는 사용할 수 없고, 수사직과 직업이 공개된 사람은 대상에서 제외됩니다.",
                ContractorContractView(running.guild_id, actor, contract_targets),
            )
            if not sent:
                failed_names.append(actor.name)
            continue
        targets = night_targets(running.game, actor)
        if targets:
            sent = await send_player_secret(
                guild,
                running,
                actor,
                f"{(effective_role or actor.role).value} 밤 행동을 선택하세요.",
                NightActionView(running.guild_id, actor, targets, effective_role),
            )
            if not sent:
                failed_names.append(actor.name)

    if failed_names:
        await send_game_embed(
            guild,
            channel,
            running,
            "밤 행동 선택지를 보낼 수 없는 참가자: " + ", ".join(failed_names),
            color=ERROR_EMBED_COLOR,
        )

    if should_finish_night_early(running):
        running.night_complete_event.set()
    await wait_for_night_actions(guild, channel, running)
    running.night_timed_events_due = True
    trigger_timed_night_events(guild, channel, running)
    result = running.game.resolve_night()
    await sync_lover_chat_access(guild, running, reason="마피아 게임 낮 시작으로 연인 채팅 권한 갱신")
    await sync_shaman_channel_permissions(guild, running, can_chat=False)
    await announce_night_private_results(guild, running, result)
    mafia_contact_user_ids = [
        *result.spy_contacts,
        *result.contractor_contacts,
        *result.witch_contacts,
    ]
    for user_id in result.nurse_contacts:
        player = running.game.get_player(user_id)
        if player:
            await add_player_to_private_role_channel(guild, running, Role.DOCTOR, player)
    await sync_cult_team_channel_access(guild, running)
    await sync_dead_players_private_role_channels(guild, running)
    for revived in result.priest_revives:
        await restore_revived_player_roles(
            guild,
            running,
            revived,
            reason_prefix="마피아 게임 성직자 소생",
        )
    if result.priest_revives:
        await sync_cult_team_channel_access(guild, running)
        await sync_lover_chat_access(guild, running, reason="마피아 게임 성직자 소생으로 연인 채팅 권한 갱신")

    doctor_saved = (
        result.mafia_target is not None
        and result.protected is not None
        and result.mafia_target.user_id == result.protected.user_id
        and result.mafia_target not in result.killed_players
        and not result.lover_sacrifices
    )
    scientist_mafia_channel_players: list[Player] = []
    if result.killed_players:
        killed_lines: list[str] = []
        for killed in result.killed_players:
            removed_role = await remove_participant_role_from_dead(guild, running, killed)
            added_dead_role = await add_dead_player_role(guild, killed)
            await set_dead_channel_member_access(
                guild,
                running,
                killed,
                can_view=True,
                can_chat=killed.user_id not in running.game.purified_dead_ids,
                reason="마피아 게임 사망자 채팅방 권한 부여",
            )
            await set_shaman_channel_member_access(
                guild,
                running,
                killed,
                can_view=True,
                can_chat=killed.user_id not in running.game.purified_dead_ids,
                reason="마피아 게임 영매 채팅방 권한 부여",
            )
            await remove_frog_permissions(guild, running, killed)
            await disable_private_role_channel_for_player(guild, running, killed)
            if killed.role == Role.SCIENTIST and killed.user_id in running.game.scientist_contacted:
                scientist_mafia_channel_players.append(killed)
            if killed in result.contractor_kills:
                line = (
                    f"- {killed.name} 님이 청부업자에게 정체를 들켜 암살 당했습니다. "
                    f"{death_role_text(running, killed)}"
                )
            elif killed in result.vigilante_kills:
                line = (
                    f"- {killed.name} 님이 자경단원에게 숙청당했습니다. "
                    f"{death_role_text(running, killed)}"
                )
            else:
                line = f"- {killed.name}: {death_role_text(running, killed)}"
            if removed_role:
                line += f" '{config.participant_role}' 역할을 제거했습니다."
            else:
                line += f" '{config.participant_role}' 역할 제거에 실패했습니다."
            if added_dead_role is True:
                line += f" '{DEAD_PLAYER_ROLE}' 역할을 부여했습니다."
            elif added_dead_role is None:
                line += f" '{DEAD_PLAYER_ROLE}' 역할을 찾지 못했습니다."
            else:
                line += f" '{DEAD_PLAYER_ROLE}' 역할 부여에 실패했습니다."
            killed_lines.append(line)
        await sync_dead_players_private_role_channels(guild, running)
        await sync_cult_team_channel_access(guild, running)
        message = (
            "아침이 밝았습니다. 밤 사이 사망자가 발생했습니다.\n"
            + "\n".join(killed_lines)
        )
        if result.lover_sacrifices:
            lover_lines = [
                f"- {savior.name}님이 연인 {saved.name}님을 살리고 대신 마피아에게 살해 당했습니다!"
                for savior, saved in result.lover_sacrifices
            ]
            message += "\n\n연인 희생\n" + "\n".join(lover_lines)
        if result.terrorist_retaliations:
            retaliation_lines = [
                f"- {terrorist.name} 님이 지목 중이던 {target.name} 님도 함께 사망했습니다."
                for terrorist, target in result.terrorist_retaliations
            ]
            message += "\n\n지목 반격\n" + "\n".join(retaliation_lines)
        await send_game_embed(guild, channel, running, message, title="밤 결과", include_dead=True)
    elif doctor_saved:
        saved_player = result.protected
        await send_game_embed(
            guild,
            channel,
            running,
            f"아침이 밝았습니다. **{saved_player.name}**님이 의사의 치료로 살아났습니다.",
            title="밤 결과",
            color=SUCCESS_EMBED_COLOR,
            include_dead=True,
        )
    else:
        await send_game_embed(
            guild,
            channel,
            running,
            "아침이 밝았습니다. 아무도 사망하지 않았습니다.",
            title="밤 결과",
            include_dead=True,
        )
    for user_id in mafia_contact_user_ids:
        player = running.game.get_player(user_id)
        if player and player.alive:
            await add_player_to_private_role_channel(guild, running, Role.MAFIA, player)
    for scientist in scientist_mafia_channel_players:
        await set_player_private_channel_access(
            guild,
            running,
            Role.MAFIA,
            scientist,
            can_chat=True,
            reason="마피아 게임 과학자 유착으로 마피아 채널 권한 부여",
        )
    if result.killed_players and doctor_saved and result.protected:
        await send_game_embed(
            guild,
            channel,
            running,
            f"**{result.protected.name}**님이 의사의 치료로 살아났습니다.",
            title="의사 치료",
            color=SUCCESS_EMBED_COLOR,
            include_dead=True,
        )
    if result.soldier_blocks:
        await send_game_embed(
            guild,
            channel,
            running,
            "\n".join(
                f"군인 **{soldier.name}**님이 마피아의 공격을 버텨냈습니다!"
                for soldier in result.soldier_blocks
            ),
            title="군인 방탄",
            color=WARNING_EMBED_COLOR,
            include_dead=True,
        )
    if result.priest_revives:
        await send_game_embed(
            guild,
            channel,
            running,
            "\n".join(f"[{player.name}님이 부활하셨습니다]" for player in result.priest_revives),
            title="성직자 소생",
            color=SUCCESS_EMBED_COLOR,
            include_dead=True,
        )
    if result.reporter_results:
        await send_game_embed(
            guild,
            channel,
            running,
            "\n".join(result.reporter_results.values()),
            title="기자 특종",
            color=SUCCESS_EMBED_COLOR,
            include_dead=True,
        )
    if result.cult_bells:
        await send_game_embed(
            guild,
            channel,
            running,
            "\n".join("교주의 종소리가 울렸습니다." for _ in range(result.cult_bells)),
            title="교주 포교",
            color=WARNING_EMBED_COLOR,
            include_dead=True,
        )
    await announce_police_result(guild, running, result)
    await announce_public_police_status(guild, channel, running, police_can_act, result)
    await announce_morning_mafia_count(guild, channel, running)
    await upsert_game_status(guild, running)


async def announce_police_result(
    guild: discord.Guild,
    running: RunningGame,
    result: NightResult,
) -> None:
    if running.game.police_result_announced:
        return
    alive_police = [
        player for player in running.game.alive_players() if player.role == Role.POLICE
    ]
    if not alive_police:
        return

    if result.police_target is None:
        message = "경찰 조사 대상이 과반을 넘지 못해 이번 밤 조사 결과가 없습니다."
    else:
        result_text = "마피아입니다" if result.police_target_is_mafia else "마피아가 아닙니다"
        message = f"조사 결과: {result.police_target.name} 님은 **{result_text}**."

    running.game.mark_police_result_announced()
    await send_police_result_message(guild, running, message)


async def announce_hacker_results(
    guild: discord.Guild,
    running: RunningGame,
) -> None:
    for user_id, message in running.game.consume_hacker_results().items():
        player = running.game.get_player(user_id)
        if player:
            await send_player_secret(guild, running, player, message)


async def announce_vigilante_investigation_results(
    guild: discord.Guild,
    running: RunningGame,
) -> None:
    for user_id, message in running.game.consume_vigilante_results().items():
        player = running.game.get_player(user_id)
        if player:
            await send_player_secret(guild, running, player, message)


async def send_police_result_message(
    guild: discord.Guild,
    running: RunningGame,
    message: str,
    *,
    exclude_user_ids: set[int] | None = None,
) -> None:
    excluded = exclude_user_ids or set()
    alive_police = [
        player
        for player in running.game.alive_players()
        if player.role == Role.POLICE and player.user_id not in excluded
    ]
    for player in alive_police:
        await send_player_secret(guild, running, player, message)


async def announce_night_private_results(
    guild: discord.Guild,
    running: RunningGame,
    result: NightResult,
) -> None:
    for user_id, message in {
        **result.detective_results,
        **result.shaman_results,
        **result.priest_results,
        **result.agent_results,
        **result.spy_results,
        **result.contractor_results,
        **result.godfather_results,
        **result.vigilante_results,
        **result.nurse_results,
        **result.gangster_results,
        **result.cult_results,
        **result.fanatic_results,
    }.items():
        player = running.game.get_player(user_id)
        if player:
            await send_player_secret(guild, running, player, message)

    for user_id in result.shaman_purifications:
        player = running.game.get_player(user_id)
        if player:
            await set_dead_channel_member_access(
                guild,
                running,
                player,
                can_view=True,
                can_chat=False,
                reason="마피아 게임 영매 성불로 사망자 채팅 금지",
            )
            await set_shaman_channel_member_access(
                guild,
                running,
                player,
                can_view=True,
                can_chat=False,
                reason="마피아 게임 영매 성불로 영매 채팅 금지",
            )
            if running.anonymous_enabled:
                await set_anonymous_general_input_access(
                    guild,
                    running,
                    player,
                    can_chat=False,
                    reason="마피아 게임 영매 성불로 익명 사망자 채팅 금지",
                )

    for user_id, inherited_role in result.graverobber_results.items():
        player = running.game.get_player(user_id)
        if player and inherited_role in PRIVATE_CHAT_ROLES:
            await add_player_to_private_role_channel(guild, running, inherited_role, player)
        if player and inherited_role == Role.SHAMAN:
            await set_shaman_channel_member_access(
                guild,
                running,
                player,
                can_view=True,
                can_chat=running.game.phase == Phase.NIGHT,
                reason="마피아 게임 도굴꾼 영매 계승으로 영매 채팅방 권한 부여",
            )
        if player:
            await send_player_secret(
                guild,
                running,
                player,
                f"도굴꾼 능력으로 **{inherited_role.value}** 직업을 이어받았습니다.",
            )

    for soldier in result.soldier_blocks:
        await send_player_secret(
            guild,
            running,
            soldier,
            "방탄으로 마피아 공격을 한 차례 막았습니다. 누가 공격했는지는 알 수 없습니다.",
        )

    for user_id in result.fanatic_inherits:
        player = running.game.get_player(user_id)
        if player:
            await send_player_secret(guild, running, player, "교주가 사망해 광신도가 교주의 능력을 물려받았습니다.")


async def announce_public_police_status(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
    police_can_act: bool,
    result: NightResult,
) -> None:
    if not running.reveal_public_police_status:
        return
    if not police_can_act:
        return

    if result.police_target is None:
        message = "경찰 조사는 성공하지 못했습니다. 대상이 과반을 넘지 못했거나 선택이 완료되지 않았습니다."
        color = WARNING_EMBED_COLOR
    elif result.police_target_is_mafia:
        message = "경찰이 마피아를 발견했습니다. 자세한 조사 결과는 경찰 비공개 채널로 전달됩니다."
        color = SUCCESS_EMBED_COLOR
    else:
        message = "경찰이 마피아를 발견하지 못했습니다. 자세한 조사 결과는 경찰 비공개 채널로 전달됩니다."
        color = WARNING_EMBED_COLOR
    await send_game_embed(
        guild,
        channel,
        running,
        message,
        title="경찰 조사 결과 공개",
        color=color,
        include_dead=True,
    )


async def announce_morning_mafia_count(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    if not running.reveal_morning_mafia_count:
        return

    await send_game_embed(
        guild,
        channel,
        running,
        f"현재 생존 마피아: **{len(running.game.alive_known_mafia_team())}명**",
        title="아침 마피아 현황",
        include_dead=True,
    )


def night_targets(game: MafiaGame, actor: Player) -> list[Player]:
    alive = sorted(game.alive_players(), key=lambda player: player.name.casefold())
    effective_role = game.thief_night_role(actor) if actor.role == Role.THIEF else actor.role
    if effective_role is None:
        return []
    if effective_role == Role.MAFIA:
        return [player for player in alive if game.can_mafia_attack(player, actor.user_id)]
    if effective_role == Role.DOCTOR:
        return alive
    if effective_role == Role.NURSE:
        if actor.user_id in game.nurse_contacted:
            return alive if not game.alive_role_count(Role.DOCTOR) else []
        return [player for player in alive if player.user_id != actor.user_id]
    if effective_role == Role.SHAMAN:
        return sorted(game.unpurified_dead_players(), key=lambda player: player.name.casefold())
    if effective_role == Role.PRIEST:
        return sorted(game.unpurified_dead_players(), key=lambda player: player.name.casefold())
    if effective_role == Role.CULT_LEADER:
        return [
            player
            for player in alive
            if player.user_id != actor.user_id and not game.is_cult_team(player)
        ]
    if effective_role in {
        Role.POLICE,
        Role.REPORTER,
        Role.DETECTIVE,
        Role.SPY,
        Role.WITCH,
        Role.GODFATHER,
        Role.TERRORIST,
        Role.FANATIC,
        Role.GANGSTER,
    }:
        return [player for player in alive if player.user_id != actor.user_id]
    if effective_role == Role.VIGILANTE:
        return sorted(game.vigilante_execution_targets(actor), key=lambda player: player.name.casefold())
    if effective_role == Role.CONTRACTOR:
        return sorted(game.contractor_contract_targets(actor), key=lambda player: player.name.casefold())
    return []


async def announce_winner(channel: discord.abc.Messageable, running: RunningGame) -> bool:
    winner = running.game.winner()
    if not winner:
        return False

    running.game.phase = Phase.ENDED
    guild = bot.get_guild(running.guild_id)
    if guild:
        await upsert_game_status(guild, running)
    if winner == Winner.MAFIA:
        winner_text = "마피아 승리!"
    elif winner == Winner.JOKER:
        winner_text = "조커 승리!"
    elif winner == Winner.CULT:
        winner_text = "교주팀 승리!"
    else:
        winner_text = "시민 승리!"
    await announce_final_roles(channel, running, winner_text)
    record_game_stats(running, winner)
    return True


async def announce_final_roles(
    channel: discord.abc.Messageable,
    running: RunningGame,
    result_text: str,
) -> None:
    elapsed_seconds = max(0, int(time.monotonic() - running.started_at))
    message = (
        f"{result_text}\n"
        f"플레이 시간: **{play_duration_text(elapsed_seconds)}**\n\n"
        f"최종 역할 공개\n{final_role_reveal_text(running)}"
    )
    guild = bot.get_guild(running.guild_id)
    if guild:
        await send_game_embed(
            guild,
            channel,
            running,
            message,
            title="게임 종료",
            color=SUCCESS_EMBED_COLOR,
            include_dead=True,
        )
        original_channel = guild.get_channel(running.channel_id)
        current_channel_id = getattr(channel, "id", None)
        if (
            isinstance(original_channel, discord.abc.Messageable)
            and getattr(original_channel, "id", None) != current_channel_id
        ):
            await send_embed(
                original_channel,
                message,
                title="게임 종료",
                color=SUCCESS_EMBED_COLOR,
            )
        return

    await send_embed(
        channel,
        message,
        title="게임 종료",
        color=SUCCESS_EMBED_COLOR,
    )


def final_role_reveal_text(running: RunningGame) -> str:
    def final_team(player: Player) -> str:
        if running.game.is_cult_team(player):
            return "교주팀"
        if running.game.is_mafia_team(player):
            return "마피아팀"
        if player.role == Role.JOKER:
            return "중립"
        return "시민팀"

    def role_detail(player: Player) -> str:
        state = "" if player.alive else " (사망)"
        return f"{player.role.value}{state} / 최종 진영: {final_team(player)}"

    if not running.anonymous_enabled:
        return "\n".join(
            f"- {player.name}: {role_detail(player)}"
            for player in sorted(running.game.players, key=lambda item: item.name.casefold())
        )

    lines: list[str] = []
    for player in sorted(
        running.game.players,
        key=lambda item: running.anonymous_aliases.get(item.user_id, item.name).casefold(),
    ):
        alias = running.anonymous_aliases.get(player.user_id, "익명")
        real_name = original_player_name(running, player)
        lines.append(f"- {alias} = {real_name}: {role_detail(player)}")
    return "\n".join(lines)

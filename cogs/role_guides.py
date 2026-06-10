from __future__ import annotations

from bot import *  # noqa: F401,F403


__all__ = (
    'role_team_text',
    'role_goal_text',
    'ability_lines',
    'role_rule_lines',
    'role_guide_value',
    'personal_role_status',
    'role_message',
    'game_rule_text',
    'death_role_text',
    'make_role_guide_embed',
    'make_role_guide_embeds',
    'anonymous_vote_summary',
)


def role_team_text(role: Role) -> str:
    return ROLE_TEAM_TEXT.get(role, "시민팀")


def role_goal_text(role: Role) -> str:
    return ROLE_GOAL_TEXT.get(role, "시민팀을 도와 모든 마피아를 제거하세요.")


def ability_lines(role: Role) -> list[str]:
    abilities = ROLE_ABILITY_TEXTS.get(role, ROLE_ABILITY_TEXTS[Role.CITIZEN])
    return [f"`[{name}]` {description}" for name, description in abilities]


def role_rule_lines(role: Role) -> list[str]:
    return [f"- {line}" for line in ROLE_RULE_TEXTS.get(role, ())]


def role_guide_value(role: Role) -> str:
    lines = [
        f"**진영** {role_team_text(role)}",
        f"**목표** {role_goal_text(role)}",
        "**능력**",
        *ability_lines(role),
    ]
    rules = role_rule_lines(role)
    if rules:
        lines.extend(["**판정/주의**", *rules])
    return "\n".join(lines)


def personal_role_status(game: MafiaGame, player: Player) -> list[str]:
    if player.role == Role.MAFIA:
        teammates = ", ".join(
            teammate.name
            for teammate in sorted(game.players, key=lambda item: item.name.casefold())
            if game.is_known_mafia_team(teammate)
        )
        return [f"현재 알고 있는 마피아팀: {teammates or '없음'}"]
    if player.role == Role.SPY:
        contacted = player.user_id in game.spy_contacted
        return [
            "접선 상태: 완료" if contacted else "접선 상태: 미접선",
            "미접선 중에는 마피아 비밀방, 경찰 마피아 판정, 생존 마피아 수에 포함되지 않습니다.",
        ]
    if player.role == Role.CONTRACTOR:
        contacted = player.user_id in game.contractor_contacted
        return [
            "접선 상태: 완료" if contacted else "접선 상태: 미접선",
            "청부는 두 번째 밤부터 두 명의 직업을 추측하는 방식으로만 사용할 수 있습니다.",
            "추측 중 일반 마피아를 마피아로 정확히 맞히면 접선합니다.",
        ]
    if player.role == Role.WITCH:
        contacted = player.user_id in game.witch_contacted
        return [
            "접선 상태: 완료" if contacted else "접선 상태: 미접선",
            "저주는 밤 10초 전부터 적용됩니다. 10초 미만에 선택하면 바로 적용됩니다.",
        ]
    if player.role == Role.SCIENTIST:
        contacted = player.user_id in game.scientist_contacted
        revived = player.user_id in game.scientist_revive_used_ids
        return [
            "접선 상태: 완료" if contacted else "접선 상태: 사망 전까지 미접선",
            "재생 사용: 사용함" if revived else "재생 사용: 미사용",
        ]
    if player.role == Role.MADAM:
        contacted = player.user_id in game.madam_contacted
        seduced = [
            target.name
            for target in sorted(game.players, key=lambda item: item.name.casefold())
            if target.user_id in game.madam_seduced_ids
        ]
        return [
            "접선 상태: 완료" if contacted else "접선 상태: 미접선",
            f"유혹 대상: {', '.join(seduced) if seduced else '없음'}",
        ]
    if player.role == Role.GODFATHER:
        contacted = player.user_id in game.godfather_contacted
        return [
            "접선 상태: 완료" if contacted else "접선 상태: 세 번째 밤 전까지 미접선",
            "접선 후부터 마피아 비밀방에 입장하고 말살을 사용할 수 있습니다.",
        ]
    if player.role == Role.NURSE:
        contacted = player.user_id in game.nurse_contacted
        doctor_alive = game.alive_role_count(Role.DOCTOR) > 0
        return [
            "의사 접선: 완료" if contacted else "의사 접선: 미접선",
            "치료 승계: 가능" if contacted and not doctor_alive else "치료 승계: 불가",
        ]
    if player.role == Role.PRIEST:
        revivable = ", ".join(
            target.name for target in sorted(game.unpurified_dead_players(), key=lambda item: item.name.casefold())
        )
        return [
            f"소생 사용: {'사용함' if player.user_id in game.priest_used_ids else '미사용'}",
            f"소생 가능 사망자: {revivable or '없음'}",
        ]
    if player.role == Role.CULT_LEADER:
        culted = [
            target.name
            for target in sorted(game.players, key=lambda item: item.name.casefold())
            if target.user_id in game.culted_ids and target.user_id != player.user_id
        ]
        return [f"포교 대상: {', '.join(culted) if culted else '없음'}"]
    if player.role == Role.FANATIC:
        return ["포교 상태: 완료" if player.user_id in game.culted_ids else "포교 상태: 미포교"]
    if player.role == Role.VIGILANTE:
        known = [
            target.name
            for target in sorted(game.players, key=lambda item: item.name.casefold())
            if target.user_id in game.vigilante_known_enemy_ids.get(player.user_id, set())
        ]
        return [
            f"조사 사용: {'사용함' if player.user_id in game.vigilante_investigation_used_ids else '미사용'}",
            f"처형 사용: {'사용함' if player.user_id in game.vigilante_execution_used_ids else '미사용'}",
            f"숙청 가능 확정 대상: {', '.join(known) if known else '없음'}",
        ]
    return []


def role_message(game: MafiaGame, player: Player) -> str:
    lines = [
        f"당신의 역할은 **{player.role.value}** 입니다.",
        f"진영: **{role_team_text(player.role)}**",
        f"목표: {role_goal_text(player.role)}",
        "",
        "능력",
        *ability_lines(player.role),
    ]
    personal = personal_role_status(game, player)
    if personal:
        lines.extend(["", "개인 상태", *personal])
    rules = role_rule_lines(player.role)
    if rules:
        lines.extend(["", "판정/주의", *rules])
    return "\n".join(lines)


def game_rule_text(game: MafiaGame, reveal_death_roles: bool) -> str:
    death_rule = (
        "사망자의 직업은 즉시 공개됩니다."
        if reveal_death_roles
        else "사망자의 직업은 즉시 공개되지 않습니다."
    )
    return (
        f"{public_role_count_text(game)}\n\n"
        "게임은 밤과 낮을 반복합니다.\n"
        "- 역할 설명: 전체 역할 설명은 `/역할설명`, 본인 역할 설명은 `/마피아능력`으로 확인할 수 있습니다.\n"
        "- 밤: 게임 채널 채팅과 반응이 비활성화되고, 밤 행동이 있는 역할은 DM으로 행동합니다.\n"
        "- 낮: 생존자는 자유롭게 토론합니다. 생존자 과반이 `바로 투표`를 누르면 토론을 끝내고 지목 투표로 넘어갑니다. 시간이 끝나면 생존자 과반으로 1분 연장을 정할 수 있고, 연장은 낮마다 1번만 가능합니다.\n"
        f"- 마피아 수 공개: 아침 생존 마피아 수는 {'공개됩니다' if config.reveal_morning_mafia_count else '공개되지 않습니다'}.\n"
        "- 투표: 생존자는 최후변론에 세울 사람 또는 스킵을 선택합니다. 지목자는 20초 동안 혼자 최후변론을 하고, 이후 찬반투표 과반 결과를 따릅니다.\n"
        f"- 경찰 공개: 조사 성공 여부는 {'공개됩니다' if config.reveal_public_police_status else '공개되지 않습니다'}. 실제 조사 결과는 경찰에게만 전달됩니다.\n"
        f"- 채팅: 낮 토론 슬로우모드는 {config.chat_slowmode_seconds}초이며 최후변론 중에는 해제됩니다.\n"
        f"- 사망자: {death_rule} 게임 채널 채팅/반응 권한은 제거되고 '{DEAD_PLAYER_ROLE}' 역할이 부여됩니다.\n\n"
        "승리 조건\n"
        "- 시민 진영: 모든 마피아를 제거하면 승리합니다.\n"
        "- 마피아 진영: 생존 마피아 수가 나머지 생존자 수 이상이면 승리합니다.\n"
        "- 교주팀: 교주팀 생존자가 비교주팀 생존자 이상이면 승리합니다.\n"
        "- 조커: 낮 투표로 처형되면 즉시 단독 승리합니다."
    )


def death_role_text(running: RunningGame, player: Player) -> str:
    if running.reveal_death_roles:
        return f"직업은 **{player.role.value}** 입니다."
    return "직업은 공개되지 않습니다."


ROLE_GUIDE_ENTRIES: tuple[tuple[Role, str, str], ...] = tuple(
    (role, role.value, role_guide_value(role)) for role in ROLE_GUIDE_ORDER
)
ROLE_GUIDE_SECTIONS: tuple[tuple[str, str], ...] = tuple(
    (role_name, guide) for _role, role_name, guide in ROLE_GUIDE_ENTRIES
)


def make_role_guide_embed(
    game: MafiaGame | None = None,
    *,
    player: Player | None = None,
    title: str = "역할 안내",
) -> discord.Embed:
    if player:
        personal_text = role_message(game, player) if game else f"당신의 역할은 **{player.role.value}** 입니다."
        description = (
            f"{personal_text}\n\n"
            "전체 역할 설명은 `/역할설명`으로 확인할 수 있습니다."
        )
        return make_embed(description, title=title)
    else:
        description = "역할별 능력과 이 봇의 실제 판정 안내입니다. 게임 중 본인 역할은 `/마피아능력`으로 다시 확인할 수 있습니다."

    embed = make_embed(description, title=title)
    embed.add_field(name="공통 판정", value=ROLE_GUIDE_COMMON_TEXT, inline=False)
    for role_name, guide in ROLE_GUIDE_SECTIONS:
        embed.add_field(name=role_name, value=guide, inline=False)
    return embed


def make_role_guide_embeds(
    game: MafiaGame | None = None,
    *,
    player: Player | None = None,
    title: str = "역할 안내",
) -> list[discord.Embed]:
    if player:
        return [make_role_guide_embed(game, player=player, title=title)]

    embeds: list[discord.Embed] = []
    groups: tuple[tuple[str, tuple[tuple[str, str], ...]], ...] = (
        (
            "시민 역할",
            tuple(
                (role_name, guide)
                for role, role_name, guide in ROLE_GUIDE_ENTRIES
                if role_team_text(role).startswith("시민팀")
            ),
        ),
        (
            "마피아 역할",
            tuple(
                (role_name, guide)
                for role, role_name, guide in ROLE_GUIDE_ENTRIES
                if role_team_text(role).startswith("마피아팀")
            ),
        ),
        (
            "중립 역할",
            tuple(
                (role_name, guide)
                for role, role_name, guide in ROLE_GUIDE_ENTRIES
                if role_team_text(role) == "중립"
            ),
        ),
        (
            "교주팀 역할",
            tuple(
                (role_name, guide)
                for role, role_name, guide in ROLE_GUIDE_ENTRIES
                if role_team_text(role).startswith("교주팀")
            ),
        ),
    )

    for group_name, sections in groups:
        group_chunks: list[list[tuple[str, str]]] = []
        current: list[tuple[str, str]] = []
        current_size = len(title) + len(group_name) + 200
        for role_name, guide in sections:
            entry_size = len(role_name) + len(guide) + 8
            if current and (current_size + entry_size > 5200 or len(current) >= 6):
                group_chunks.append(current)
                current = []
                current_size = len(title) + len(group_name) + 200
            current.append((role_name, guide))
            current_size += entry_size
        if current:
            group_chunks.append(current)

        for index, chunk in enumerate(group_chunks, start=1):
            suffix = f" {index}/{len(group_chunks)}" if len(group_chunks) > 1 else ""
            embed = make_embed(
                f"{group_name} 설명입니다.",
                title=f"{title} - {group_name}{suffix}",
            )
            if not embeds:
                embed.add_field(name="공통 판정", value=ROLE_GUIDE_COMMON_TEXT, inline=False)
            for role_name, guide in chunk:
                embed.add_field(name=role_name, value=guide, inline=False)
            embeds.append(embed)
    return embeds


def anonymous_vote_summary(game: MafiaGame, result: VoteResult) -> str:
    if not result.vote_counts:
        return "투표 없음"

    rows: list[tuple[str, int]] = []
    for target_id, count in result.vote_counts.items():
        if target_id is None:
            name = "스킵"
        else:
            player = game.get_player(target_id)
            name = player.name if player else str(target_id)
        rows.append((name, count))

    rows.sort(key=lambda item: (-item[1], item[0].casefold()))
    return "\n".join(f"- {name}: {count}표" for name, count in rows)

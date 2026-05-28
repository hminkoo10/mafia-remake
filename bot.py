from __future__ import annotations

import asyncio
from dataclasses import dataclass, field
import json
import os
from pathlib import Path

import discord
from discord import app_commands
from discord.ext import commands
from dotenv import load_dotenv

from game import MafiaGame, NightResult, Phase, Player, Role, VoteResult, Winner


BASE_DIR = Path(__file__).resolve().parent


@dataclass
class BotConfig:
    participant_role: str
    manager_role: str
    default_mafia_count: int
    default_doctor_count: int
    default_police_count: int
    default_joker_count: int
    night_seconds: int
    discussion_seconds: int
    vote_seconds: int
    chat_slowmode_seconds: int = 3
    reveal_death_roles: bool = False
    citizen_special_count: int = 0
    mafia_special_count: int = 0
    neutral_special_count: int = 1
    enable_detective: bool = True
    enable_graverobber: bool = True
    enable_spy: bool = True
    enable_godfather: bool = True
    enable_joker: bool = True
    enable_politician: bool = True
    enable_terrorist: bool = True


@dataclass
class RunningGame:
    guild_id: int
    channel_id: int
    game: MafiaGame
    reveal_death_roles: bool = False
    task: asyncio.Task[None] | None = None
    night_complete_event: asyncio.Event = field(default_factory=asyncio.Event)
    vote_complete_event: asyncio.Event = field(default_factory=asyncio.Event)
    confirm_complete_event: asyncio.Event = field(default_factory=asyncio.Event)
    game_channel_overwrites: dict[int, discord.PermissionOverwrite | None] = field(default_factory=dict)
    member_channel_overwrites: dict[int, discord.PermissionOverwrite | None] = field(default_factory=dict)
    original_slowmode_delay: int | None = None
    participant_user_ids: set[int] = field(default_factory=set)
    private_channel_ids: dict[Role, int] = field(default_factory=dict)


RECRUITMENT_SECONDS = 60
DAY_EXTENSION_VOTE_SECONDS = 10
DISCUSSION_EXTENSION_SECONDS = 60
GAME_NOTIFICATION_ROLE = "게임알림"
PRIVATE_CHAT_ROLES = (Role.MAFIA, Role.POLICE, Role.DOCTOR)
PRIVATE_CHANNEL_NAMES = {
    Role.MAFIA: "마피아-비밀방",
    Role.POLICE: "경찰-비밀방",
    Role.DOCTOR: "의사-비밀방",
}
BASE_ROLE_ORDER = (Role.MAFIA, Role.DOCTOR, Role.POLICE)
CITIZEN_SPECIAL_ROLES = (Role.DETECTIVE, Role.GRAVEROBBER, Role.POLITICIAN, Role.TERRORIST)
MAFIA_SPECIAL_ROLES = (Role.SPY, Role.GODFATHER)
NEUTRAL_SPECIAL_ROLES = (Role.JOKER,)
PUBLIC_MAFIA_SPECIAL_ROLES = (Role.SPY, Role.GODFATHER)
PUBLIC_CITIZEN_SPECIAL_ROLES = (
    Role.DETECTIVE,
    Role.GRAVEROBBER,
    Role.POLITICIAN,
    Role.TERRORIST,
)
PUBLIC_NEUTRAL_SPECIAL_ROLES = (Role.JOKER,)
DEFAULT_EMBED_COLOR = discord.Color.gold()
ERROR_EMBED_COLOR = discord.Color.red()
SUCCESS_EMBED_COLOR = discord.Color.green()
WARNING_EMBED_COLOR = discord.Color.orange()


def load_config() -> BotConfig:
    with (BASE_DIR / "config.json").open("r", encoding="utf-8") as file:
        data = json.load(file)
    return BotConfig(
        participant_role=str(data["participant_role"]),
        manager_role=str(data["manager_role"]),
        default_mafia_count=int(data["default_mafia_count"]),
        default_doctor_count=int(data["default_doctor_count"]),
        default_police_count=int(data["default_police_count"]),
        default_joker_count=int(data.get("default_joker_count", 1)),
        night_seconds=int(data["night_seconds"]),
        discussion_seconds=int(data.get("discussion_seconds", 60)),
        vote_seconds=int(data["vote_seconds"]),
        chat_slowmode_seconds=int(data.get("chat_slowmode_seconds", 3)),
        reveal_death_roles=bool(data.get("reveal_death_roles", False)),
        citizen_special_count=int(data.get("citizen_special_count", 0)),
        mafia_special_count=int(data.get("mafia_special_count", 0)),
        neutral_special_count=int(data.get("neutral_special_count", data.get("default_joker_count", 1))),
        enable_detective=bool(data.get("enable_detective", True)),
        enable_graverobber=bool(data.get("enable_graverobber", True)),
        enable_spy=bool(data.get("enable_spy", True)),
        enable_godfather=bool(data.get("enable_godfather", True)),
        enable_joker=bool(data.get("enable_joker", True)),
        enable_politician=bool(data.get("enable_politician", True)),
        enable_terrorist=bool(data.get("enable_terrorist", True)),
    )


config = load_config()
games: dict[int, RunningGame] = {}
recruiting_guilds: set[int] = set()


def save_config() -> None:
    with (BASE_DIR / "config.json").open("w", encoding="utf-8") as file:
        json.dump(config.__dict__, file, ensure_ascii=False, indent=2)
        file.write("\n")


def member_has_role(member: discord.Member, role_name: str) -> bool:
    return any(role.name == role_name for role in member.roles)


def require_manager(interaction: discord.Interaction) -> discord.Member:
    if not isinstance(interaction.user, discord.Member):
        raise app_commands.CheckFailure("서버 안에서만 사용할 수 있습니다.")
    if not member_has_role(interaction.user, config.manager_role):
        raise app_commands.CheckFailure(
            f"'{config.manager_role}' 역할을 가진 사람만 사용할 수 있습니다."
        )
    return interaction.user


def display_name(member: discord.Member) -> str:
    return member.nick or member.global_name or member.name


def duration_text(seconds: int) -> str:
    if seconds % 60 == 0:
        return f"{seconds // 60}분"
    return f"{seconds}초"


def special_role_rule_text(role: Role) -> str:
    action = {
        Role.MAFIA: "공격",
        Role.DOCTOR: "보호",
        Role.POLICE: "조사",
    }.get(role, "행동")
    return (
        f"{role.value}가 여러 명이면 같은 대상이 살아있는 {role.value} 인원의 "
        f"과반 초과를 받아야 {action}이 행사됩니다.\n"
        "동률이거나 과반에 못 미치면 그 밤 행동은 행사되지 않습니다."
    )


def selected_role_counts(
    special_roles: list[Role] | None = None,
) -> dict[Role, int]:
    selected_special_roles = special_roles or []
    mafia_special_count = sum(1 for role in selected_special_roles if role in MAFIA_SPECIAL_ROLES)
    if mafia_special_count > config.default_mafia_count:
        raise ValueError(
            "마피아 특수룰 수는 전체 마피아 수보다 많을 수 없습니다. "
            f"현재 마피아 {config.default_mafia_count}명, 마피아 특수 {mafia_special_count}명입니다."
        )
    role_counts = {
        Role.MAFIA: config.default_mafia_count - mafia_special_count,
        Role.DOCTOR: config.default_doctor_count,
        Role.POLICE: config.default_police_count,
    }
    for role in selected_special_roles:
        role_counts[role] = role_counts.get(role, 0) + 1
    return role_counts


def enabled_special_roles(pool: tuple[Role, ...]) -> list[Role]:
    enabled = {
        Role.DETECTIVE: config.enable_detective,
        Role.GRAVEROBBER: config.enable_graverobber,
        Role.SPY: config.enable_spy,
        Role.GODFATHER: config.enable_godfather,
        Role.JOKER: config.enable_joker,
        Role.POLITICIAN: config.enable_politician,
        Role.TERRORIST: config.enable_terrorist,
    }
    return [role for role in pool if enabled[role]]


def choose_special_roles() -> list[Role]:
    selected: list[Role] = []
    for pool, count in (
        (CITIZEN_SPECIAL_ROLES, config.citizen_special_count),
        (MAFIA_SPECIAL_ROLES, config.mafia_special_count),
        (NEUTRAL_SPECIAL_ROLES, config.neutral_special_count),
    ):
        candidates = enabled_special_roles(pool)
        if count > len(candidates):
            raise ValueError(
                f"{', '.join(role.value for role in pool)} 중 활성화된 역할보다 선택할 특수룰 수가 많습니다."
            )
        selected.extend(random_sample_roles(candidates, count))
    return selected


def random_sample_roles(candidates: list[Role], count: int) -> list[Role]:
    import random

    return random.sample(candidates, count) if count > 0 else []


def minimum_player_count(role_counts: dict[Role, int]) -> int:
    special_count = sum(role_counts.values())
    mafia_count = (
        role_counts.get(Role.MAFIA, 0)
        + role_counts.get(Role.SPY, 0)
        + role_counts.get(Role.GODFATHER, 0)
    )
    return max(3, special_count, mafia_count * 2 + 1)


def ordered_role_counts(role_counts: dict[Role, int]) -> list[tuple[Role, int]]:
    order = (
        Role.MAFIA,
        Role.SPY,
        Role.GODFATHER,
        Role.DOCTOR,
        Role.POLICE,
        Role.DETECTIVE,
        Role.GRAVEROBBER,
        Role.POLITICIAN,
        Role.TERRORIST,
        Role.JOKER,
    )
    return [(role, role_counts.get(role, 0)) for role in order if role_counts.get(role, 0) > 0]


def count_role_group(role_counts: dict[Role, int], roles: tuple[Role, ...]) -> int:
    return sum(role_counts.get(role, 0) for role in roles)


def public_role_count_text_from_counts(
    role_counts: dict[Role, int],
    total_players: int | None = None,
) -> str:
    mafia_special = count_role_group(role_counts, PUBLIC_MAFIA_SPECIAL_ROLES)
    mafia_total = role_counts.get(Role.MAFIA, 0) + mafia_special
    doctor_total = role_counts.get(Role.DOCTOR, 0)
    police_total = role_counts.get(Role.POLICE, 0)
    citizen_special = count_role_group(role_counts, PUBLIC_CITIZEN_SPECIAL_ROLES)
    neutral_special = count_role_group(role_counts, PUBLIC_NEUTRAL_SPECIAL_ROLES)

    if total_players is None:
        citizen_text = f"시민 변동(중 특수 {citizen_special}명)"
    else:
        citizen_total = max(0, total_players - mafia_total - doctor_total - police_total - neutral_special)
        citizen_text = f"시민 {citizen_total}명(중 특수 {citizen_special}명)"

    parts = [
        f"마피아 {mafia_total}명(중 특수 {mafia_special}명)",
        f"의사 {doctor_total}명",
        f"경찰 {police_total}명",
        citizen_text,
    ]
    if neutral_special > 0:
        parts.append(f"중립 특수 {neutral_special}명")
    return ", ".join(parts)


def public_role_count_text(game: MafiaGame) -> str:
    role_counts: dict[Role, int] = {}
    for player in game.players:
        role_counts[player.role] = role_counts.get(player.role, 0) + 1
    return "역할 구성: " + public_role_count_text_from_counts(role_counts, len(game.players))


def public_game_settings_text(game: MafiaGame, prefix: str = "게임 방 설정입니다.") -> str:
    return (
        f"{prefix}\n"
        f"{public_role_count_text(game)}\n"
        f"사망 시 직업 공개: {'공개' if config.reveal_death_roles else '비공개'}\n"
        f"채팅 슬로우모드: {config.chat_slowmode_seconds}초"
    )


def current_settings_text(prefix: str = "마피아 설정을 저장했습니다.") -> str:
    enabled = [
        role.value
        for role in (
            Role.DETECTIVE,
            Role.GRAVEROBBER,
            Role.SPY,
            Role.GODFATHER,
            Role.JOKER,
            Role.POLITICIAN,
            Role.TERRORIST,
        )
        if {
            Role.DETECTIVE: config.enable_detective,
            Role.GRAVEROBBER: config.enable_graverobber,
            Role.SPY: config.enable_spy,
            Role.GODFATHER: config.enable_godfather,
            Role.JOKER: config.enable_joker,
            Role.POLITICIAN: config.enable_politician,
            Role.TERRORIST: config.enable_terrorist,
        }[role]
    ]
    return (
        f"{prefix}\n"
        f"기본 직업: 마피아 {config.default_mafia_count}명, "
        f"의사 {config.default_doctor_count}명, 경찰 {config.default_police_count}명\n"
        f"특수룰 수: 시민 {config.citizen_special_count}개, "
        f"마피아 {config.mafia_special_count}개, 중립 {config.neutral_special_count}개\n"
        f"활성 특수룰: {', '.join(enabled) if enabled else '없음'}\n"
        f"채팅 슬로우모드: {config.chat_slowmode_seconds}초\n"
        f"사망 시 직업 공개: {'공개' if config.reveal_death_roles else '비공개'}"
    )


def disable_view_items(view: discord.ui.View | None) -> None:
    if not view:
        return
    for item in view.children:
        if hasattr(item, "disabled"):
            item.disabled = True


def make_embed(
    message: str,
    *,
    title: str = "마피아 게임",
    color: discord.Color = DEFAULT_EMBED_COLOR,
) -> discord.Embed:
    lines = message.splitlines()
    if lines:
        first_line = lines[0] if "**" in lines[0] else f"**{lines[0]}**"
        description = "\n".join([first_line, *lines[1:]])
    else:
        description = message

    embed = discord.Embed(
        title=f"[마피아] {title}",
        description=description,
        color=color,
    )
    embed.set_author(name="마피아 게임 알림")
    embed.set_footer(text="마피아 게임 진행 메시지")
    return embed


async def send_embed(
    channel: discord.abc.Messageable,
    message: str,
    *,
    view: discord.ui.View | None = None,
    title: str = "마피아 게임",
    color: discord.Color = DEFAULT_EMBED_COLOR,
) -> discord.Message:
    return await channel.send(embed=make_embed(message, title=title, color=color), view=view)


async def send_private(
    member: discord.Member,
    message: str,
    view: discord.ui.View | None = None,
) -> bool:
    try:
        await member.send(embed=make_embed(message, title="비밀 메시지"), view=view)
        return True
    except discord.Forbidden:
        return False


async def send_interaction_reply(
    interaction: discord.Interaction,
    message: str,
    *,
    private: bool,
) -> None:
    ephemeral = private and interaction.guild is not None
    embed = make_embed(
        message,
        color=ERROR_EMBED_COLOR if private else DEFAULT_EMBED_COLOR,
    )
    if interaction.response.is_done():
        await interaction.followup.send(embed=embed, ephemeral=ephemeral)
    else:
        await interaction.response.send_message(embed=embed, ephemeral=ephemeral)


async def wait_for_event_or_timeout(event: asyncio.Event, seconds: int) -> None:
    try:
        await asyncio.wait_for(event.wait(), timeout=seconds)
    except asyncio.TimeoutError:
        return


class JoinGameView(discord.ui.View):
    def __init__(
        self,
        guild_id: int,
        host_user_id: int,
        participant_role_id: int,
        role_counts: dict[Role, int],
        reveal_death_roles: bool,
    ) -> None:
        super().__init__(timeout=RECRUITMENT_SECONDS + 5)
        self.guild_id = guild_id
        self.host_user_id = host_user_id
        self.participant_role_id = participant_role_id
        self.role_counts = role_counts
        self.reveal_death_roles = reveal_death_roles
        self.minimum_players = minimum_player_count(role_counts)
        self.joined_ids: set[int] = set()
        self.joined_names: dict[int, str] = {}
        self.accepting = True
        self.started = False
        self.cancelled = False
        self.done = asyncio.Event()
        self.lock = asyncio.Lock()
        self.message: discord.Message | None = None

    def participant_text(self) -> str:
        if not self.joined_names:
            return "아직 참가자가 없습니다."
        names = sorted(self.joined_names.values(), key=str.casefold)
        return "\n".join(f"{index}. {name}" for index, name in enumerate(names, start=1))

    def role_count_text(self) -> str:
        return public_role_count_text_from_counts(self.role_counts)

    def minimum_status_text(self) -> str:
        shortage = self.minimum_players - len(self.joined_ids)
        if shortage <= 0:
            return f"최소 시작 인원 **{self.minimum_players}명** 충족"
        return f"최소 시작 인원 **{self.minimum_players}명**까지 **{shortage}명** 더 필요"

    def embed(
        self,
        status: str,
        *,
        title: str = "참가자 모집",
        color: discord.Color = SUCCESS_EMBED_COLOR,
    ) -> discord.Embed:
        return make_embed(
            f"최대 {duration_text(RECRUITMENT_SECONDS)} 동안 참가자를 모집합니다.\n"
            "참가 버튼을 누르면 게임 참가자로 등록되고, "
            f"'{config.participant_role}' 역할이 부여됩니다.\n"
            "주최자는 `시작` 버튼으로 즉시 시작하거나 `취소` 버튼으로 모집을 취소할 수 있습니다.\n\n"
            f"역할 구성: {self.role_count_text()}\n"
            f"사망 시 직업 공개: {'공개' if self.reveal_death_roles else '비공개'}\n"
            f"{self.minimum_status_text()}\n\n"
            f"현재 참가자 **{len(self.joined_ids)}명**\n"
            f"{self.participant_text()}\n\n"
            f"{status}",
            title=title,
            color=color,
        )

    async def refresh_message(
        self,
        status: str = "모집 중입니다.",
        *,
        title: str = "참가자 모집",
        color: discord.Color = SUCCESS_EMBED_COLOR,
    ) -> None:
        if not self.message:
            return
        try:
            await self.message.edit(embed=self.embed(status, title=title, color=color), view=self)
        except discord.DiscordException:
            return

    async def finish_from_host(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
        *,
        cancelled: bool,
    ) -> None:
        if interaction.user.id != self.host_user_id:
            await send_interaction_reply(
                interaction,
                "게임을 모집한 주최자만 사용할 수 있습니다.",
                private=True,
            )
            return

        async with self.lock:
            if not self.accepting:
                await send_interaction_reply(interaction, "참가자 모집이 이미 종료되었습니다.", private=True)
                return
            if not cancelled and len(self.joined_ids) < self.minimum_players:
                await send_interaction_reply(
                    interaction,
                    f"아직 시작할 수 없습니다. 최소 {self.minimum_players}명이 필요합니다. "
                    f"현재 {len(self.joined_ids)}명입니다.",
                    private=True,
                )
                return

            self.accepting = False
            self.cancelled = cancelled
            self.started = not cancelled
            disable_view_items(self)
            button.label = "취소 완료" if cancelled else "시작 확정"
            status = "주최자가 참가자 모집을 취소했습니다." if cancelled else "주최자가 게임 시작을 눌렀습니다."
            title = "참가자 모집 취소" if cancelled else "참가자 모집 종료"
            color = ERROR_EMBED_COLOR if cancelled else SUCCESS_EMBED_COLOR
            await interaction.response.edit_message(
                content=None,
                embed=self.embed(status, title=title, color=color),
                view=self,
            )
            self.done.set()
            self.stop()

    @discord.ui.button(label="참가", style=discord.ButtonStyle.success)
    async def join_game(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
    ) -> None:
        async with self.lock:
            if not self.accepting:
                await send_interaction_reply(interaction, "참가자 모집이 종료되었습니다.", private=True)
                return
            if interaction.guild_id != self.guild_id or not interaction.guild:
                await send_interaction_reply(interaction, "이 모집에는 참가할 수 없습니다.", private=True)
                return
            if not isinstance(interaction.user, discord.Member) or interaction.user.bot:
                await send_interaction_reply(interaction, "서버 멤버만 참가할 수 있습니다.", private=True)
                return

            participant_role = interaction.guild.get_role(self.participant_role_id)
            if not participant_role:
                await send_interaction_reply(
                    interaction,
                    f"'{config.participant_role}' 역할을 찾을 수 없습니다.",
                    private=True,
                )
                return
            if interaction.user.id in self.joined_ids:
                await send_interaction_reply(interaction, "이미 참가했습니다.", private=True)
                return

            try:
                if participant_role not in interaction.user.roles:
                    await interaction.user.add_roles(
                        participant_role,
                        reason="마피아 게임 참가 신청",
                    )
            except discord.DiscordException:
                await send_interaction_reply(
                    interaction,
                    f"'{config.participant_role}' 역할 부여에 실패했습니다. "
                    "봇에게 역할 관리 권한이 있고, 봇 역할이 참가자 역할보다 위에 있는지 확인하세요.",
                    private=True,
                )
                return

            self.joined_ids.add(interaction.user.id)
            self.joined_names[interaction.user.id] = display_name(interaction.user)
            await send_interaction_reply(
                interaction,
                f"참가 완료! '{config.participant_role}' 역할을 부여했습니다.",
                private=True,
            )
            await self.refresh_message()

    @discord.ui.button(label="시작", style=discord.ButtonStyle.primary)
    async def start_now(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
    ) -> None:
        await self.finish_from_host(interaction, button, cancelled=False)

    @discord.ui.button(label="취소", style=discord.ButtonStyle.danger)
    async def cancel_recruitment(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
    ) -> None:
        await self.finish_from_host(interaction, button, cancelled=True)


class NightActionSelect(discord.ui.Select[discord.ui.View]):
    def __init__(self, guild_id: int, actor_id: int, role: Role, targets: list[Player]) -> None:
        options = [
            discord.SelectOption(label=target.name[:100], value=str(target.user_id))
            for target in targets[:25]
        ]
        placeholder = {
            Role.MAFIA: "공격할 대상을 선택하세요",
            Role.DOCTOR: "보호할 대상을 선택하세요",
            Role.POLICE: "조사할 대상을 선택하세요",
            Role.DETECTIVE: "추적할 대상을 선택하세요",
            Role.SPY: "첩보할 대상을 선택하세요",
            Role.GODFATHER: "확정 처치할 대상을 선택하세요",
            Role.TERRORIST: "지목할 대상을 선택하세요",
        }[role]
        super().__init__(placeholder=placeholder, min_values=1, max_values=1, options=options)
        self.guild_id = guild_id
        self.actor_id = actor_id

    async def callback(self, interaction: discord.Interaction) -> None:
        if interaction.user.id != self.actor_id:
            await send_interaction_reply(
                interaction,
                "본인에게 온 선택지만 사용할 수 있습니다.",
                private=True,
            )
            return

        running = games.get(self.guild_id)
        if not running:
            await send_interaction_reply(interaction, "진행 중인 게임이 없습니다.", private=True)
            return

        try:
            result = running.game.submit_night_action(self.actor_id, int(self.values[0]))
        except ValueError as error:
            await send_interaction_reply(interaction, str(error), private=True)
            return

        disable_view_items(self.view)
        actor = running.game.get_player(self.actor_id)
        if actor and actor.role == Role.SPY and running.game.spy_can_use_bonus_action(self.actor_id):
            await interaction.response.edit_message(
                content=None,
                embed=make_embed(
                    f"{result}\n\n추가 첩보를 한 번 더 사용할 수 있습니다.",
                    title="접선 성공",
                    color=SUCCESS_EMBED_COLOR,
                ),
                view=NightActionView(self.guild_id, actor, night_targets(running.game, actor)),
            )
            return

        if running.game.all_night_actions_submitted():
            running.night_complete_event.set()
        await interaction.response.edit_message(
            content=None,
            embed=make_embed(result, title="밤 행동 완료", color=SUCCESS_EMBED_COLOR),
            view=self.view,
        )


class NightActionView(discord.ui.View):
    def __init__(self, guild_id: int, actor: Player, targets: list[Player]) -> None:
        super().__init__(timeout=None)
        self.add_item(NightActionSelect(guild_id, actor.user_id, actor.role, targets))


class DayVoteSelect(discord.ui.Select[discord.ui.View]):
    def __init__(self, guild_id: int, targets: list[Player]) -> None:
        options = [
            discord.SelectOption(label=target.name[:100], value=str(target.user_id))
            for target in targets[:24]
        ]
        options.append(discord.SelectOption(label="스킵", value="skip"))
        super().__init__(
            placeholder="처형할 대상 또는 스킵을 선택하세요",
            min_values=1,
            max_values=1,
            options=options,
        )
        self.guild_id = guild_id

    async def callback(self, interaction: discord.Interaction) -> None:
        running = games.get(self.guild_id)
        if not running:
            await send_interaction_reply(interaction, "진행 중인 게임이 없습니다.", private=True)
            return
        try:
            target_id = None if self.values[0] == "skip" else int(self.values[0])
            result = running.game.submit_day_vote(interaction.user.id, target_id)
        except ValueError as error:
            await send_interaction_reply(interaction, str(error), private=True)
            return
        if running.game.all_day_votes_submitted():
            running.vote_complete_event.set()
        await send_interaction_reply(interaction, result, private=True)


class DayVoteView(discord.ui.View):
    def __init__(self, guild_id: int, targets: list[Player]) -> None:
        super().__init__(timeout=None)
        self.add_item(DayVoteSelect(guild_id, targets))


class ConfirmVoteView(discord.ui.View):
    def __init__(self, guild_id: int) -> None:
        super().__init__(timeout=None)
        self.guild_id = guild_id

    async def vote(self, interaction: discord.Interaction, approve: bool) -> None:
        running = games.get(self.guild_id)
        if not running:
            await send_interaction_reply(interaction, "진행 중인 게임이 없습니다.", private=True)
            return
        try:
            result = running.game.submit_confirmation_vote(interaction.user.id, approve)
        except ValueError as error:
            await send_interaction_reply(interaction, str(error), private=True)
            return
        if running.game.all_confirm_votes_submitted():
            running.confirm_complete_event.set()
        await send_interaction_reply(interaction, result, private=True)

    @discord.ui.button(label="찬성", style=discord.ButtonStyle.success)
    async def approve(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
    ) -> None:
        await self.vote(interaction, True)

    @discord.ui.button(label="반대", style=discord.ButtonStyle.danger)
    async def reject(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
    ) -> None:
        await self.vote(interaction, False)


class DayExtensionVoteView(discord.ui.View):
    def __init__(self, guild_id: int, alive_user_ids: set[int]) -> None:
        super().__init__(timeout=DAY_EXTENSION_VOTE_SECONDS)
        self.guild_id = guild_id
        self.alive_user_ids = alive_user_ids
        self.voter_ids: set[int] = set()
        self.required_votes = (len(alive_user_ids) // 2) + 1
        self.extended = False
        self.accepting = True

    @discord.ui.button(label="1분 연장", style=discord.ButtonStyle.secondary)
    async def extend_discussion(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
    ) -> None:
        if not self.accepting:
            await send_interaction_reply(interaction, "연장 투표가 종료되었습니다.", private=True)
            return
        if interaction.user.id not in self.alive_user_ids:
            await send_interaction_reply(
                interaction,
                "생존 중인 참가자만 연장 투표를 할 수 있습니다.",
                private=True,
            )
            return
        running = games.get(self.guild_id)
        if not running or running.game.phase != Phase.DAY:
            await send_interaction_reply(
                interaction,
                "지금 진행 중인 낮 토론이 없습니다.",
                private=True,
            )
            return
        if interaction.user.id in self.voter_ids:
            await send_interaction_reply(
                interaction,
                f"이미 1분 연장에 투표했습니다. 현재 {len(self.voter_ids)}/{self.required_votes}명",
                private=True,
            )
            return

        self.voter_ids.add(interaction.user.id)
        if len(self.voter_ids) < self.required_votes:
            await send_interaction_reply(
                interaction,
                f"1분 연장에 투표했습니다. 현재 {len(self.voter_ids)}/{self.required_votes}명",
                private=True,
            )
            return

        self.extended = True
        self.accepting = False
        disable_view_items(self)
        button.label = "연장 확정"
        await interaction.response.edit_message(
            content=None,
            embed=make_embed(
                f"생존자 과반수가 1분 연장을 선택했습니다. "
                f"({len(self.voter_ids)}/{len(self.alive_user_ids)}명)\n"
                "낮 토론을 1분 연장합니다.",
                title="낮 토론 연장",
                color=SUCCESS_EMBED_COLOR,
            ),
            view=self,
        )
        self.stop()


class MafiaBot(commands.Bot):
    async def setup_hook(self) -> None:
        synced = await self.tree.sync()
        print(f"Synced {len(synced)} slash command(s).")


intents = discord.Intents.default()
intents.members = True
bot = MafiaBot(command_prefix="!", intents=intents)


@bot.tree.command(name="마피아설정", description="마피아 게임 기본 설정을 변경합니다.")
@app_commands.describe(
    mafia="마피아 수",
    doctor="의사 수",
    police="경찰 수",
    citizen_special="시민 특수룰 수",
    mafia_special="마피아 특수룰 수",
    neutral_special="중립 특수룰 수",
    slowmode="낮 채팅 슬로우모드 초. 기본 3초",
    death_role_reveal="사망 시 직업 공개 여부",
    detective="사립탐정 활성화 여부",
    graverobber="도굴꾼 활성화 여부",
    spy="스파이 활성화 여부",
    godfather="대부 활성화 여부",
    joker="조커 활성화 여부",
    politician="정치인 활성화 여부",
    terrorist="테러리스트 활성화 여부",
)
async def configure_game(
    interaction: discord.Interaction,
    mafia: int | None = None,
    doctor: int | None = None,
    police: int | None = None,
    citizen_special: int | None = None,
    mafia_special: int | None = None,
    neutral_special: int | None = None,
    slowmode: int | None = None,
    death_role_reveal: bool | None = None,
    detective: bool | None = None,
    graverobber: bool | None = None,
    spy: bool | None = None,
    godfather: bool | None = None,
    joker: bool | None = None,
    politician: bool | None = None,
    terrorist: bool | None = None,
) -> None:
    require_manager(interaction)
    updates: dict[str, int | bool] = {}
    int_updates = {
        "default_mafia_count": mafia,
        "default_doctor_count": doctor,
        "default_police_count": police,
        "citizen_special_count": citizen_special,
        "mafia_special_count": mafia_special,
        "neutral_special_count": neutral_special,
        "chat_slowmode_seconds": slowmode,
    }
    for key, value in int_updates.items():
        if value is None:
            continue
        if value < 0:
            await send_interaction_reply(interaction, "설정 값은 0 이상이어야 합니다.", private=True)
            return
        updates[key] = value
    if mafia is not None and mafia < 1:
        await send_interaction_reply(interaction, "마피아는 최소 1명이어야 합니다.", private=True)
        return

    bool_updates = {
        "reveal_death_roles": death_role_reveal,
        "enable_detective": detective,
        "enable_graverobber": graverobber,
        "enable_spy": spy,
        "enable_godfather": godfather,
        "enable_joker": joker,
        "enable_politician": politician,
        "enable_terrorist": terrorist,
    }
    for key, value in bool_updates.items():
        if value is not None:
            updates[key] = value

    previous = {key: getattr(config, key) for key in updates}
    for key, value in updates.items():
        setattr(config, key, value)

    try:
        selected_role_counts(choose_special_roles())
    except ValueError as error:
        for key, value in previous.items():
            setattr(config, key, value)
        await send_interaction_reply(interaction, str(error), private=True)
        return

    save_config()
    await send_interaction_reply(interaction, current_settings_text(), private=False)


@bot.tree.command(name="마피아시작", description="저장된 설정대로 마피아 게임 참가자를 모집하고 시작합니다.")
async def start_game(
    interaction: discord.Interaction,
) -> None:
    if not interaction.guild or interaction.guild_id is None or interaction.channel_id is None:
        await send_interaction_reply(interaction, "서버 채널에서만 사용할 수 있습니다.", private=True)
        return
    if interaction.guild_id in games:
        await send_interaction_reply(interaction, "이미 진행 중인 게임이 있습니다.", private=True)
        return
    if interaction.guild_id in recruiting_guilds:
        await send_interaction_reply(interaction, "이미 참가자를 모집 중입니다.", private=True)
        return

    participant_role = discord.utils.get(interaction.guild.roles, name=config.participant_role)
    if not participant_role:
        await send_interaction_reply(
            interaction,
            f"'{config.participant_role}' 역할을 찾을 수 없습니다.",
            private=True,
        )
        return

    await interaction.response.defer(thinking=True)
    recruiting_guilds.add(interaction.guild.id)
    try:
        try:
            special_roles = choose_special_roles()
            role_counts = selected_role_counts(special_roles)
        except ValueError as error:
            await interaction.followup.send(
                embed=make_embed(str(error), color=ERROR_EMBED_COLOR),
                ephemeral=True,
            )
            return
        clear_failed = await clear_existing_participant_roles(interaction.guild, participant_role)
        join_view = JoinGameView(
            interaction.guild.id,
            interaction.user.id,
            participant_role.id,
            role_counts,
            config.reveal_death_roles,
        )
        notification_role = discord.utils.get(interaction.guild.roles, name=GAME_NOTIFICATION_ROLE)
        recruit_message = await interaction.followup.send(
            content=notification_role.mention if notification_role else None,
            embed=join_view.embed("모집 중입니다."),
            view=join_view,
            allowed_mentions=discord.AllowedMentions(roles=True),
            wait=True,
        )
        join_view.message = recruit_message

        try:
            await asyncio.wait_for(join_view.done.wait(), timeout=RECRUITMENT_SECONDS)
        except asyncio.TimeoutError:
            pass

        if join_view.accepting:
            async with join_view.lock:
                if join_view.accepting:
                    join_view.accepting = False
                    disable_view_items(join_view)
                    if len(join_view.joined_ids) < join_view.minimum_players:
                        join_view.cancelled = True
                        await join_view.refresh_message(
                            "최소 시작 인원에 도달하지 못해 모집이 자동 취소되었습니다.",
                            title="참가자 모집 취소",
                            color=ERROR_EMBED_COLOR,
                        )
                    else:
                        await join_view.refresh_message(
                            "최대 모집 시간이 지나 자동으로 마감되었습니다. 게임을 시작합니다.",
                            title="참가자 모집 종료",
                        )
                    join_view.stop()

        if join_view.cancelled:
            await remove_participant_roles_from_ids(
                interaction.guild,
                join_view.joined_ids,
                "마피아 게임 참가 모집 취소로 참가자 역할 제거",
            )
            await interaction.followup.send(
                embed=make_embed(
                    "참가자 모집이 취소되었습니다. 참가자 역할을 회수했습니다.",
                    title="참가자 모집 취소",
                    color=ERROR_EMBED_COLOR,
                )
            )
            return

        participants = await collect_joined_participants(interaction.guild, join_view.joined_ids)
        player_data = [(member.id, display_name(member)) for member in participants]

        try:
            game = MafiaGame(
                players=player_data,
                mafia_count=role_counts[Role.MAFIA],
                doctor_count=role_counts[Role.DOCTOR],
                police_count=role_counts[Role.POLICE],
                joker_count=0,
                special_roles=special_roles,
            )
        except ValueError as error:
            await remove_participant_roles_from_ids(
                interaction.guild,
                join_view.joined_ids,
                "마피아 게임 시작 실패로 참가자 역할 제거",
            )
            await interaction.followup.send(
                embed=make_embed(str(error), color=ERROR_EMBED_COLOR),
                ephemeral=True,
            )
            return

        running = RunningGame(
            guild_id=interaction.guild.id,
            channel_id=interaction.channel_id,
            game=game,
            reveal_death_roles=config.reveal_death_roles,
            participant_user_ids=set(join_view.joined_ids),
        )
        games[interaction.guild.id] = running
        running.task = asyncio.create_task(game_loop(interaction.guild, running))

        warning = ""
        if clear_failed:
            warning = (
                "\n\n기존 참가자 역할을 제거하지 못한 유저: "
                + ", ".join(clear_failed)
                + "\n봇 역할 관리 권한과 역할 순서를 확인하세요."
            )
        await interaction.followup.send(
            embed=make_embed(
                "게임을 시작합니다. "
                f"참가자 {len(game.players)}명에게 역할을 DM으로 보냅니다.\n"
                f"{public_role_count_text(game)}"
                f"\n사망 시 직업 공개: {'공개' if config.reveal_death_roles else '비공개'}"
                f"\n채팅 슬로우모드: {config.chat_slowmode_seconds}초"
                f"{warning}",
                title="게임 시작",
                color=SUCCESS_EMBED_COLOR,
            )
        )
    finally:
        recruiting_guilds.discard(interaction.guild.id)


@bot.tree.command(name="마피아중지", description="진행 중인 마피아 게임을 중지합니다.")
async def stop_game(interaction: discord.Interaction) -> None:
    require_manager(interaction)
    if not interaction.guild or not interaction.guild_id:
        await send_interaction_reply(interaction, "서버에서만 사용할 수 있습니다.", private=True)
        return

    running = games.pop(interaction.guild_id, None)
    if not running:
        await send_interaction_reply(interaction, "진행 중인 게임이 없습니다.", private=True)
        return

    await interaction.response.defer(thinking=True)
    running.game.phase = Phase.ENDED
    channel = interaction.guild.get_channel(running.channel_id)
    if isinstance(channel, discord.abc.Messageable):
        await announce_final_roles(channel, running, "관리자가 게임을 중지했습니다.")

    if running.task:
        running.task.cancel()
        try:
            await running.task
        except asyncio.CancelledError:
            pass
        except Exception as error:
            print(f"Game task error during stop: {error!r}")
            await cleanup_game(interaction.guild, running)
    else:
        await cleanup_game(interaction.guild, running)

    await interaction.followup.send(
        embed=make_embed("게임을 중지했습니다.", title="게임 중지", color=SUCCESS_EMBED_COLOR)
    )


@bot.tree.command(name="마피아상태", description="진행 중인 마피아 게임 상태를 확인합니다.")
async def show_status(interaction: discord.Interaction) -> None:
    require_manager(interaction)
    if not interaction.guild_id:
        await send_interaction_reply(interaction, "서버에서만 사용할 수 있습니다.", private=True)
        return

    running = games.get(interaction.guild_id)
    if not running:
        await send_interaction_reply(interaction, "진행 중인 게임이 없습니다.", private=True)
        return
    await interaction.response.send_message(
        embed=make_embed(running.game.public_status(), title="게임 상태"),
        ephemeral=True,
    )


@bot.tree.command(name="마피아능력", description="배정받은 역할과 능력 설명을 다시 확인합니다.")
async def show_abilities(interaction: discord.Interaction) -> None:
    if not interaction.guild_id:
        await send_interaction_reply(interaction, "서버에서만 사용할 수 있습니다.", private=True)
        return

    running = games.get(interaction.guild_id)
    if not running:
        await send_interaction_reply(interaction, "진행 중인 게임이 없습니다.", private=True)
        return

    player = running.game.get_player(interaction.user.id)
    if not player:
        await send_interaction_reply(interaction, "현재 게임 참가자만 능력 설명을 확인할 수 있습니다.", private=True)
        return

    await interaction.response.send_message(
        embed=make_role_guide_embed(running.game, player=player, title="능력 설명"),
        ephemeral=True,
    )


@bot.tree.command(name="역할안내", description="마피아 게임 역할 안내를 공지용 임베드로 보냅니다.")
async def announce_role_guide(interaction: discord.Interaction) -> None:
    await interaction.response.send_message(
        embed=make_role_guide_embed(title="역할 안내"),
    )


@configure_game.error
@start_game.error
@stop_game.error
@show_status.error
@show_abilities.error
@announce_role_guide.error
async def command_error(
    interaction: discord.Interaction,
    error: app_commands.AppCommandError,
) -> None:
    root_error = getattr(error, "original", error)
    if isinstance(root_error, app_commands.CheckFailure | ValueError):
        message = str(root_error)
    else:
        print(f"Command error: {root_error!r}")
        message = "명령을 실행하는 중 오류가 발생했습니다."

    if interaction.response.is_done():
        await interaction.followup.send(
            embed=make_embed(message, color=ERROR_EMBED_COLOR),
            ephemeral=True,
        )
    else:
        await interaction.response.send_message(
            embed=make_embed(message, color=ERROR_EMBED_COLOR),
            ephemeral=True,
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


def supports_member_overwrites(channel: discord.abc.Messageable) -> bool:
    return all(
        hasattr(channel, attribute)
        for attribute in ("overwrites", "overwrites_for", "set_permissions")
    )


def get_participant_role(guild: discord.Guild) -> discord.Role | None:
    return discord.utils.get(guild.roles, name=config.participant_role)


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

        overwrite = channel.overwrites_for(target)
        set_chat_values(overwrite, can_send)
        try:
            await channel.set_permissions(
                target,
                overwrite=overwrite,
                reason=reason,
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
    channel = guild.get_channel(running.channel_id)
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
            await channel.set_permissions(
                target,
                overwrite=clone_overwrite(original),
                reason="마피아 게임 종료로 게임 채널 권한 복구",
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
        await channel.set_permissions(member, overwrite=overwrite, reason=reason)
    except discord.DiscordException:
        await send_embed(channel, "최후변론 채팅 권한 변경에 실패했습니다.", color=ERROR_EMBED_COLOR)


async def restore_member_channel_chat(guild: discord.Guild, running: RunningGame) -> None:
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
            await channel.set_permissions(
                member,
                overwrite=clone_overwrite(original),
                reason="마피아 게임 최후변론 채팅 권한 복구",
            )
        except discord.DiscordException:
            pass
        running.member_channel_overwrites.pop(user_id, None)


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
    )


async def create_private_role_channels(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
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
        if manager_role:
            overwrites[manager_role] = private_channel_overwrite(False)
        if bot_member:
            overwrites[bot_member] = private_channel_overwrite(True)

        for player in players:
            member = await get_guild_member(guild, player.user_id)
            if member:
                overwrites[member] = private_channel_overwrite(True)

        try:
            private_channel = await guild.create_text_channel(
                name=PRIVATE_CHANNEL_NAMES[role],
                overwrites=overwrites,
                category=category,
                reason="마피아 게임 역할별 비공개 채팅방 생성",
            )
        except discord.DiscordException:
            try:
                private_channel = await guild.create_text_channel(
                    name=PRIVATE_CHANNEL_NAMES[role],
                    overwrites=overwrites,
                    reason="마피아 게임 역할별 비공개 채팅방 생성",
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

    if failed_roles:
        await send_embed(
            channel,
            "역할별 비공개 채널 생성에 실패했습니다: "
            + ", ".join(failed_roles)
            + "\n봇에게 채널 관리 권한이 있는지 확인하세요.",
            color=ERROR_EMBED_COLOR,
        )


async def disable_private_role_channel_for_player(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
) -> None:
    channel_roles = {player.role}
    if running.game.is_mafia_team(player):
        channel_roles.add(Role.MAFIA)

    member = await get_guild_member(guild, player.user_id)
    if not member:
        return

    for channel_role in channel_roles:
        channel_id = running.private_channel_ids.get(channel_role)
        if not channel_id:
            continue

        channel = guild.get_channel(channel_id)
        if not isinstance(channel, discord.TextChannel):
            running.private_channel_ids.pop(channel_role, None)
            continue

        try:
            await channel.set_permissions(
                member,
                overwrite=private_channel_overwrite(False),
                reason="마피아 게임 사망자 역할 채팅방 권한 제거",
            )
        except discord.DiscordException:
            continue


async def add_player_to_private_role_channel(
    guild: discord.Guild,
    running: RunningGame,
    channel_role: Role,
    player: Player,
) -> None:
    channel_id = running.private_channel_ids.get(channel_role)
    if not channel_id:
        return
    channel = guild.get_channel(channel_id)
    if not isinstance(channel, discord.TextChannel):
        return
    member = await get_guild_member(guild, player.user_id)
    if not member:
        return
    try:
        await channel.set_permissions(
            member,
            overwrite=private_channel_overwrite(True),
            reason="마피아 게임 접선으로 비공개 채널 권한 부여",
        )
    except discord.DiscordException:
        return


async def delete_private_role_channels(guild: discord.Guild, running: RunningGame) -> None:
    for role, channel_id in list(running.private_channel_ids.items()):
        channel = guild.get_channel(channel_id)
        if channel:
            try:
                await channel.delete(reason="마피아 게임 종료로 역할별 비공개 채널 삭제")
            except discord.DiscordException:
                continue
        running.private_channel_ids.pop(role, None)


async def cleanup_game(guild: discord.Guild, running: RunningGame) -> None:
    await restore_member_channel_chat(guild, running)
    await restore_game_channel_chat(guild, running)
    await restore_channel_slowmode(guild, running)
    await remove_game_participant_roles(guild, running)
    await delete_private_role_channels(guild, running)


async def game_loop(guild: discord.Guild, running: RunningGame) -> None:
    channel = guild.get_channel(running.channel_id)
    if not isinstance(channel, discord.abc.Messageable):
        games.pop(running.guild_id, None)
        return

    try:
        await create_private_role_channels(guild, channel, running)
        await send_embed(channel, public_game_settings_text(running.game, "게임 방 설정입니다."), title="방 설정")
        await send_embed(channel, game_rule_text(running.game, running.reveal_death_roles), title="게임 설명")
        await channel.send(embed=make_role_guide_embed(running.game, title="역할 설명"))
        await send_roles(guild, running)
        await send_embed(
            channel,
            "역할 배정이 끝났습니다. 각자 DM과 역할별 비공개 채널을 확인하세요.",
            title="역할 배정 완료",
            color=SUCCESS_EMBED_COLOR,
        )

        while running.game.phase != Phase.ENDED:
            await run_night(guild, channel, running)
            if await announce_winner(channel, running):
                break

            await set_game_channel_chat(
                guild,
                channel,
                running,
                participants_can_chat=True,
                reason="마피아 게임 낮 토론 시작",
            )
            await set_channel_slowmode(
                channel,
                running,
                config.chat_slowmode_seconds,
                "마피아 게임 낮 토론 슬로우모드 적용",
            )
            should_vote = await run_day_discussion(channel, running)
            if not should_vote:
                break

            await run_vote_phase(guild, channel, running)

            if await announce_winner(channel, running):
                break
    except asyncio.CancelledError:
        return
    finally:
        await cleanup_game(guild, running)
        if games.get(running.guild_id) is running:
            games.pop(running.guild_id, None)


async def run_vote_phase(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    running.game.start_vote()
    running.vote_complete_event.clear()
    alive = running.game.alive_players()
    await set_game_channel_chat(
        guild,
        channel,
        running,
        participants_can_chat=False,
        reason="마피아 게임 투표 시작",
    )
    await send_embed(
        channel,
        f"지목 투표를 시작합니다. {config.vote_seconds}초 안에 최후변론에 세울 사람을 선택하세요.\n"
        "투표 중에는 게임 채널 채팅이 비활성화됩니다.\n"
        "생존자가 모두 투표하면 남은 시간을 기다리지 않고 바로 정산합니다.",
        view=DayVoteView(running.guild_id, alive),
        title="지목 투표 시작",
    )
    await wait_for_event_or_timeout(running.vote_complete_event, config.vote_seconds)

    vote_result = running.game.resolve_nomination_vote()
    vote_summary = anonymous_vote_summary(running.game, vote_result)
    nominee = vote_result.executed
    if not nominee:
        if vote_result.tied:
            message = "투표가 동률이라 최후변론 대상이 없습니다."
        elif vote_result.skipped:
            message = "스킵이 최다 득표하여 최후변론 대상이 없습니다."
        else:
            message = "투표가 없어 최후변론 대상이 없습니다."
        await send_embed(channel, f"{message}\n\n익명 투표 집계\n{vote_summary}", title="지목 투표 결과")
        return

    await send_embed(
        channel,
        f"지목 투표 결과, {nominee.name} 님이 최후변론 대상이 되었습니다.\n\n익명 투표 집계\n{vote_summary}",
        title="지목 투표 결과",
    )
    await set_final_defense_mode(guild, channel, running, nominee)
    await send_embed(
        channel,
        f"{nominee.name} 님의 최후변론 시간입니다. 20초 동안 지목된 사람만 말할 수 있습니다.\n"
        "이 시간 동안 슬로우모드는 해제됩니다.",
        title="최후변론",
    )
    await asyncio.sleep(20)
    await restore_member_channel_chat(guild, running)

    running.game.start_confirmation_vote()
    running.confirm_complete_event.clear()
    await send_embed(
        channel,
        f"{nominee.name} 님 처형 여부를 찬반투표합니다. {config.vote_seconds}초 안에 선택하세요.\n"
        "찬성이 반대보다 많으면 처형됩니다.",
        view=ConfirmVoteView(running.guild_id),
        title="찬반투표",
    )
    await wait_for_event_or_timeout(running.confirm_complete_event, config.vote_seconds)
    confirm_result = running.game.resolve_confirmation_vote(nominee.user_id)
    await set_channel_slowmode(
        channel,
        running,
        config.chat_slowmode_seconds,
        "마피아 게임 찬반투표 종료 후 슬로우모드 복구",
    )

    counts = confirm_result.vote_counts
    summary = f"찬성 {counts.get(True, 0)}표 / 반대 {counts.get(False, 0)}표"
    if confirm_result.blocked_by_politician:
        await send_embed(
            channel,
            f"찬반투표 결과, {nominee.name} 님은 **정치인** 입니다.\n"
            "[정치인은 투표로 죽지 않습니다]\n\n"
            f"{nominee.name} 님은 처형되지 않고 밤으로 넘어갑니다.\n\n"
            f"찬반투표 집계\n{summary}",
            title="찬반투표 결과",
            color=WARNING_EMBED_COLOR,
        )
    elif confirm_result.executed:
        killed_players = [confirm_result.executed, *confirm_result.extra_killed]
        killed_lines: list[str] = []
        for killed in killed_players:
            removed_role = await remove_participant_role_from_dead(guild, running, killed)
            await disable_private_role_channel_for_player(guild, running, killed)
            line = f"- {killed.name}: {death_role_text(running, killed)}"
            if removed_role:
                line += f" '{config.participant_role}' 역할을 제거했습니다."
            else:
                line += (
                    f" '{config.participant_role}' 역할 제거에 실패했습니다. "
                    "봇의 역할 관리 권한과 역할 순서를 확인하세요."
                )
            killed_lines.append(line)

        message = f"찬반투표 결과, {confirm_result.executed.name} 님이 처형되었습니다."
        if confirm_result.extra_killed:
            message += "\n처형 대상이 지목하고 있던 시민팀이 아닌 대상도 함께 사망했습니다."
        await send_embed(
            channel,
            f"{message}\n\n사망자\n" + "\n".join(killed_lines) + f"\n\n찬반투표 집계\n{summary}",
            title="찬반투표 결과",
        )
    elif confirm_result.tied:
        await send_embed(
            channel,
            f"찬반투표가 동률이라 처형하지 않습니다.\n\n찬반투표 집계\n{summary}",
            title="찬반투표 결과",
        )
    else:
        await send_embed(
            channel,
            f"반대가 많아 처형하지 않습니다.\n\n찬반투표 집계\n{summary}",
            title="찬반투표 결과",
        )


async def set_final_defense_mode(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
    nominee: Player,
) -> None:
    await set_game_channel_chat(
        guild,
        channel,
        running,
        participants_can_chat=False,
        reason="마피아 게임 최후변론 시작",
    )
    await set_member_chat_permission(
        guild,
        channel,
        running,
        nominee,
        True,
        "마피아 게임 최후변론 대상 발언 허용",
    )
    await set_channel_slowmode(channel, running, 0, "마피아 게임 최후변론 슬로우모드 해제")


async def run_day_discussion(
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> bool:
    discussion_seconds = config.discussion_seconds
    discussion_time = duration_text(discussion_seconds)
    await send_embed(
        channel,
        f"{running.game.day_number}일차 낮입니다. {discussion_time} 동안 자유롭게 토론하세요.\n"
        f"시간이 지나면 {DAY_EXTENSION_VOTE_SECONDS}초 동안 1분 연장 투표가 열립니다. "
        "생존자 과반수가 연장을 누르면 1분 연장되고, 아니면 바로 투표로 넘어갑니다.\n"
        f"{running.game.public_status()}",
        title="낮 토론",
    )

    while running.game.phase == Phase.DAY and games.get(running.guild_id) is running:
        await asyncio.sleep(discussion_seconds)
        if running.game.phase == Phase.ENDED or games.get(running.guild_id) is not running:
            return False

        alive_user_ids = {player.user_id for player in running.game.alive_players()}
        extension_view = DayExtensionVoteView(running.guild_id, alive_user_ids)
        vote_message = await send_embed(
            channel,
            f"{duration_text(discussion_seconds)} 토론 시간이 지났습니다.\n"
            f"{DAY_EXTENSION_VOTE_SECONDS}초 안에 생존자 과반수"
            f"({extension_view.required_votes}/{len(alive_user_ids)}명)가 `1분 연장`을 누르면 "
            "낮 토론을 1분 연장합니다.\n"
            "과반수가 모이지 않으면 바로 투표로 넘어갑니다.",
            view=extension_view,
            title="낮 토론 연장 투표",
        )
        await extension_view.wait()
        extension_view.accepting = False
        disable_view_items(extension_view)

        if running.game.phase == Phase.ENDED or games.get(running.guild_id) is not running:
            return False
        if extension_view.extended:
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
        return True

    return False


async def send_roles(guild: discord.Guild, running: RunningGame) -> None:
    channel = guild.get_channel(running.channel_id)
    failed_names: list[str] = []
    for player in running.game.players:
        member = await get_guild_member(guild, player.user_id)
        if not member:
            continue
        sent = await send_private(
            member,
            f"{role_message(running.game, player)}\n\n"
            f"방 설정\n{public_game_settings_text(running.game, '현재 게임 설정입니다.')}\n\n"
            "전체 역할 설명은 다음 임베드 또는 `/마피아능력` 명령어로 다시 확인할 수 있습니다.",
        )
        if sent:
            try:
                await member.send(embed=make_role_guide_embed(running.game, title="전체 역할 설명"))
            except discord.DiscordException:
                sent = False
        if not sent:
            failed_names.append(player.name)

    if failed_names and isinstance(channel, discord.abc.Messageable):
        await send_embed(
            channel,
            "DM을 보낼 수 없는 참가자: " + ", ".join(failed_names),
            color=ERROR_EMBED_COLOR,
        )


async def wait_for_night_actions(
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    if config.night_seconds <= 10:
        await wait_for_event_or_timeout(running.night_complete_event, config.night_seconds)
        return

    await wait_for_event_or_timeout(running.night_complete_event, config.night_seconds - 10)
    if running.night_complete_event.is_set():
        return
    if running.game.phase == Phase.NIGHT and games.get(running.guild_id) is running:
        await send_embed(
            channel,
            "밤 시간이 10초 남았습니다. 아직 행동하지 않았다면 지금 선택하세요.",
            title="밤 10초 전",
        )
    await wait_for_event_or_timeout(running.night_complete_event, 10)


async def run_night(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    running.game.phase = Phase.NIGHT
    running.night_complete_event.clear()
    for user_id in running.game.ensure_godfather_auto_contact():
        player = running.game.get_player(user_id)
        if player:
            await add_player_to_private_role_channel(guild, running, Role.MAFIA, player)
            member = await get_guild_member(guild, user_id)
            if member:
                await send_private(member, "세 번째 밤이 되어 마피아 팀과 자동 접선했습니다. 이제 마피아 비밀방을 볼 수 있고 밤마다 확정 처치 대상을 지목합니다.")
    police_can_act = any(actor.role == Role.POLICE for actor in running.game.night_action_actors())
    await set_game_channel_chat(
        guild,
        channel,
        running,
        participants_can_chat=False,
        reason="마피아 게임 밤 시작",
    )
    await send_embed(
        channel,
        f"밤이 되었습니다. {config.night_seconds}초 동안 게임 채널 채팅이 비활성화됩니다.\n"
        "밤 행동이 있는 역할에게 DM이 전송됩니다.\n"
        "행동 가능한 역할이 모두 선택하면 남은 시간을 기다리지 않고 바로 아침으로 넘어갑니다.",
        title="밤",
    )

    failed_names: list[str] = []
    for actor in running.game.night_action_actors():
        member = await get_guild_member(guild, actor.user_id)
        if not member:
            continue
        targets = night_targets(running.game, actor)
        if targets:
            sent = await send_private(
                member,
                f"{actor.role.value} 밤 행동을 선택하세요.",
                NightActionView(running.guild_id, actor, targets),
            )
            if not sent:
                failed_names.append(actor.name)

    if failed_names:
        await send_embed(
            channel,
            "밤 행동 DM을 보낼 수 없는 참가자: " + ", ".join(failed_names),
            color=ERROR_EMBED_COLOR,
        )

    if running.game.all_night_actions_submitted():
        running.night_complete_event.set()
    await wait_for_night_actions(channel, running)
    result = running.game.resolve_night()
    await announce_night_private_results(guild, running, result)
    for user_id in result.spy_contacts:
        player = running.game.get_player(user_id)
        if player:
            await add_player_to_private_role_channel(guild, running, Role.MAFIA, player)

    if result.killed_players:
        killed_lines: list[str] = []
        for killed in result.killed_players:
            removed_role = await remove_participant_role_from_dead(guild, running, killed)
            await disable_private_role_channel_for_player(guild, running, killed)
            line = f"- {killed.name}: {death_role_text(running, killed)}"
            if removed_role:
                line += f" '{config.participant_role}' 역할을 제거했습니다."
            else:
                line += f" '{config.participant_role}' 역할 제거에 실패했습니다."
            killed_lines.append(line)
        message = (
            "아침이 밝았습니다. 밤 사이 사망자가 발생했습니다.\n"
            + "\n".join(killed_lines)
        )
        if result.terrorist_retaliations:
            retaliation_lines = [
                f"- {terrorist.name} 님이 지목 중이던 {target.name} 님도 함께 사망했습니다."
                for terrorist, target in result.terrorist_retaliations
            ]
            message += "\n\n지목 반격\n" + "\n".join(retaliation_lines)
        await send_embed(channel, message, title="밤 결과")
    elif result.mafia_target and result.protected:
        await send_embed(
            channel,
            "아침이 밝았습니다. 의사의 보호로 아무도 사망하지 않았습니다.",
            title="밤 결과",
        )
    else:
        await send_embed(
            channel,
            "아침이 밝았습니다. 아무도 사망하지 않았습니다.",
            title="밤 결과",
        )
    await announce_police_result(guild, running, result)
    await announce_public_police_status(channel, police_can_act, result)
    await announce_morning_mafia_count(channel, running)


async def announce_police_result(
    guild: discord.Guild,
    running: RunningGame,
    result: NightResult,
) -> None:
    if not any(player.role == Role.POLICE for player in running.game.players):
        return

    if result.police_target is None:
        message = "경찰 조사 대상이 과반을 넘지 못해 이번 밤 조사 결과가 없습니다."
    else:
        result_text = "마피아입니다" if result.police_target_is_mafia else "마피아가 아닙니다"
        message = f"조사 결과: {result.police_target.name} 님은 **{result_text}**."

    channel = guild.get_channel(running.private_channel_ids.get(Role.POLICE, 0))
    if isinstance(channel, discord.abc.Messageable):
        await send_embed(channel, message, title="경찰 조사 결과")
        return

    for player in running.game.alive_players():
        if player.role != Role.POLICE:
            continue
        member = await get_guild_member(guild, player.user_id)
        if member:
            await send_private(member, message)


async def announce_night_private_results(
    guild: discord.Guild,
    running: RunningGame,
    result: NightResult,
) -> None:
    for user_id, message in {
        **result.detective_results,
        **result.spy_results,
        **result.godfather_results,
    }.items():
        member = await get_guild_member(guild, user_id)
        if member:
            await send_private(member, message)

    for user_id, inherited_role in result.graverobber_results.items():
        player = running.game.get_player(user_id)
        if player and inherited_role in PRIVATE_CHAT_ROLES:
            await add_player_to_private_role_channel(guild, running, inherited_role, player)
        member = await get_guild_member(guild, user_id)
        if member:
            await send_private(member, f"도굴꾼 능력으로 **{inherited_role.value}** 직업을 이어받았습니다.")


async def announce_public_police_status(
    channel: discord.abc.Messageable,
    police_can_act: bool,
    result: NightResult,
) -> None:
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
    await send_embed(channel, message, title="경찰 조사 결과 공개", color=color)


async def announce_morning_mafia_count(
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    await send_embed(
        channel,
        f"현재 생존 마피아 팀: **{len(running.game.alive_mafia_team())}명**",
        title="아침 마피아 현황",
    )


def night_targets(game: MafiaGame, actor: Player) -> list[Player]:
    alive = sorted(game.alive_players(), key=lambda player: player.name.casefold())
    if actor.role == Role.MAFIA:
        return [player for player in alive if not game.is_mafia_team(player)]
    if actor.role == Role.DOCTOR:
        return alive
    if actor.role in {Role.POLICE, Role.DETECTIVE, Role.SPY, Role.GODFATHER, Role.TERRORIST}:
        return [player for player in alive if player.user_id != actor.user_id]
    return []


async def announce_winner(channel: discord.abc.Messageable, running: RunningGame) -> bool:
    winner = running.game.winner()
    if not winner:
        return False

    running.game.phase = Phase.ENDED
    if winner == Winner.MAFIA:
        winner_text = "마피아 승리!"
    elif winner == Winner.JOKER:
        winner_text = "조커 승리!"
    else:
        winner_text = "시민 승리!"
    await announce_final_roles(channel, running, winner_text)
    return True


async def announce_final_roles(
    channel: discord.abc.Messageable,
    running: RunningGame,
    result_text: str,
) -> None:
    await send_embed(
        channel,
        f"{result_text}\n\n최종 역할 공개\n{running.game.reveal_roles()}",
        title="게임 종료",
        color=SUCCESS_EMBED_COLOR,
    )


def role_message(game: MafiaGame, player: Player) -> str:
    lines = [f"당신의 역할은 **{player.role.value}** 입니다."]

    if player.role == Role.MAFIA:
        teammates = ", ".join(
            teammate.name
            for teammate in game.players
            if teammate.role == Role.MAFIA
        )
        lines.extend(
            [
                f"마피아 팀: {teammates}",
                "밤마다 제거할 대상을 선택하세요.",
                special_role_rule_text(Role.MAFIA),
            ]
        )
    elif player.role == Role.DOCTOR:
        lines.extend(
            [
                "기본 능력",
                "[치료] 밤마다 한 사람을 지목하여 대상이 마피아에게 공격받을 경우, 대상을 치료한다.",
                special_role_rule_text(Role.DOCTOR),
            ]
        )
    elif player.role == Role.POLICE:
        lines.extend(
            [
                "기본 능력",
                "[수색] 밤이 되면 플레이어 한 명을 선택해 마피아 여부를 알아낼 수 있다.",
                special_role_rule_text(Role.POLICE),
            ]
        )
    elif player.role == Role.DETECTIVE:
        lines.extend(
            [
                "기본 능력",
                "[추리] 밤에 한 사람을 조사하여 그 사람이 밤마다 능력 사용 대상으로 누굴 선택하는지 알 수 있다.",
            ]
        )
    elif player.role == Role.SPY:
        lines.extend(
            [
                "기본 능력",
                "[첩보] 밤마다 플레이어 한 명을 선택하여 직업을 알아낼 수 있다.",
                "[접선] 밤에 선택한 플레이어가 마피아라면 접선하며, 한 번 더 능력을 사용할 수 있다.",
            ]
        )
    elif player.role == Role.GODFATHER:
        lines.append("세 번째 밤에 마피아 팀과 자동 접선합니다. 접선 후 밤마다 지목한 사람을 보호 여부와 관계없이 확정 처치합니다.")
    elif player.role == Role.GRAVEROBBER:
        lines.extend(
            [
                "기본 능력",
                "[도굴] 첫날 마피아에게 살해당한 플레이어의 직업을 얻는다.",
                "[약탈] 도굴에 성공한 경우, 도굴당한 플레이어를 시민 또는 악인으로 만든다.",
            ]
        )
    elif player.role == Role.JOKER:
        lines.append("투표로 처형되면 즉시 단독 승리합니다. 밤에 사망하면 승리하지 못합니다.")
    elif player.role == Role.POLITICIAN:
        lines.extend(
            [
                "기본 능력",
                "[처세] 플레이어간의 투표로 처형당하지 않는다.",
                "[논객] 정치인의 투표권은 두 표로 인정된다.",
            ]
        )
    elif player.role == Role.TERRORIST:
        lines.extend(
            [
                "기본 능력",
                "[자폭] 마피아를 지목하고 있는 상태에서 마피아에게 처형당할 때 대상과 함께 사망한다.",
                "[산화] 투표로 인해 처형될 때 적 팀을 지목했다면 대상과 함께 사망한다.",
            ]
        )
    else:
        lines.append("낮 토론과 투표로 마피아를 찾아내세요.")

    return "\n".join(lines)


def role_count_text(game: MafiaGame) -> str:
    return public_role_count_text(game)


def game_rule_text(game: MafiaGame, reveal_death_roles: bool) -> str:
    death_rule = (
        "사망자의 직업은 즉시 공개됩니다."
        if reveal_death_roles
        else "사망자의 직업은 즉시 공개되지 않습니다."
    )
    return (
        f"{role_count_text(game)}\n\n"
        "게임은 밤과 낮을 반복합니다.\n"
        "- 능력 설명: 역할 배정 후 `/마피아능력` 명령어로 언제든 다시 확인할 수 있습니다.\n"
        "- 밤: 게임 채널 채팅과 반응이 비활성화되고, 밤 행동이 있는 역할은 DM으로 행동합니다.\n"
        "- 낮: 생존자는 자유롭게 토론합니다. 시간이 끝나면 생존자 과반으로 1분 연장을 정할 수 있습니다.\n"
        "- 투표: 생존자는 최후변론에 세울 사람 또는 스킵을 선택합니다. 지목자는 20초 동안 혼자 최후변론을 하고, 이후 찬반투표 과반 결과를 따릅니다.\n"
        "- 정치인: 투표권은 2표이며, 찬반투표에서 처형이 확정되어도 죽지 않고 직업이 공개된 뒤 밤으로 넘어갑니다.\n"
        "- 테러리스트: 밤에 지목한 대상에 따라 밤 사망 또는 투표 처형 시 함께 사망시킬 수 있습니다.\n"
        f"- 채팅: 낮 토론 슬로우모드는 {config.chat_slowmode_seconds}초이며 최후변론 중에는 해제됩니다.\n"
        f"- 사망자: {death_rule} 게임 채널 채팅/반응 권한은 제거됩니다.\n\n"
        "승리 조건\n"
        "- 시민 진영: 모든 마피아를 제거하면 승리합니다.\n"
        "- 마피아 진영: 생존 마피아 수가 나머지 생존자 수 이상이면 승리합니다.\n"
        "- 조커: 낮 투표로 처형되면 즉시 단독 승리합니다."
    )


def death_role_text(running: RunningGame, player: Player) -> str:
    if running.reveal_death_roles:
        return f"직업은 **{player.role.value}** 입니다."
    return "직업은 공개되지 않습니다."


ROLE_GUIDE_SECTIONS: tuple[tuple[str, str], ...] = (
    (
        "마피아",
        "**팀:** 마피아\n"
        "**밤 행동:** 제거 대상을 선택합니다.\n"
        "마피아가 여러 명이면 같은 대상이 과반 초과를 받아야 공격이 실행됩니다.",
    ),
    (
        "경찰",
        "**기본 능력**\n"
        "`[수색]` 밤이 되면 플레이어 한 명을 선택해 마피아 여부를 알아낼 수 있습니다.",
    ),
    (
        "의사",
        "**기본 능력**\n"
        "`[치료]` 밤마다 한 사람을 지목합니다. 대상이 마피아에게 공격받으면 치료합니다.",
    ),
    (
        "사립탐정",
        "**기본 능력**\n"
        "`[추리]` 밤에 한 사람을 조사해 그 사람이 능력 대상으로 누구를 선택했는지 알아냅니다.",
    ),
    (
        "도굴꾼",
        "**기본 능력**\n"
        "`[도굴]` 첫날 마피아에게 살해당한 플레이어의 직업을 얻습니다.\n"
        "`[약탈]` 도굴에 성공하면 도굴당한 플레이어를 시민 또는 악인으로 만듭니다.",
    ),
    (
        "정치인",
        "**기본 능력**\n"
        "`[처세]` 플레이어 투표로 처형당하지 않습니다.\n"
        "`[논객]` 정치인의 투표권은 2표로 인정됩니다.",
    ),
    (
        "테러리스트",
        "**기본 능력**\n"
        "`[자폭]` 마피아를 지목한 상태에서 마피아에게 처형당하면 대상과 함께 사망합니다.\n"
        "`[산화]` 투표로 처형될 때 적 팀을 지목했다면 대상과 함께 사망합니다.",
    ),
    (
        "스파이",
        "**팀:** 마피아\n"
        "**기본 능력**\n"
        "`[첩보]` 밤마다 플레이어 한 명을 선택해 직업을 알아냅니다.\n"
        "`[접선]` 선택한 플레이어가 마피아라면 접선하고, 한 번 더 능력을 사용할 수 있습니다.",
    ),
    (
        "대부",
        "**팀:** 마피아\n"
        "세 번째 밤에 마피아 팀과 자동 접선합니다.\n"
        "접선 후에는 지목한 사람을 보호 여부와 관계없이 확정 처치합니다.",
    ),
    (
        "조커",
        "**팀:** 중립\n"
        "밤 행동은 없습니다.\n"
        "낮 투표로 처형되면 단독 승리하고, 밤에 사망하면 승리하지 못합니다.",
    ),
    (
        "시민",
        "**팀:** 시민\n"
        "별도 밤 행동은 없습니다.\n"
        "낮 토론과 투표로 마피아를 찾아내야 합니다.",
    ),
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
            "아래에서 각 역할의 능력을 확인할 수 있습니다."
        )
    else:
        description = "역할별 능력 안내입니다. 게임 중에는 `/마피아능력`으로 다시 확인할 수 있습니다."

    embed = make_embed(description, title=title)
    for role_name, guide in ROLE_GUIDE_SECTIONS:
        embed.add_field(name=role_name, value=guide, inline=False)
    return embed


def role_guide_text(game: MafiaGame | None = None) -> str:
    return "\n\n".join(f"{role_name}\n{guide}" for role_name, guide in ROLE_GUIDE_SECTIONS)


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


def count_role(game: MafiaGame, role: Role) -> int:
    return sum(1 for player in game.players if player.role == role)


def main() -> None:
    load_dotenv(BASE_DIR / ".env")
    token = os.getenv("DISCORD_TOKEN")
    if not token:
        raise RuntimeError(".env 파일에 DISCORD_TOKEN을 설정하세요.")
    bot.run(token)


if __name__ == "__main__":
    main()

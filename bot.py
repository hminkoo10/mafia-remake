from __future__ import annotations

import asyncio
from contextlib import suppress
from dataclasses import dataclass, field
import json
import os
from pathlib import Path
import time
from typing import Literal

import discord
from discord import app_commands
from discord.ext import commands
from dotenv import load_dotenv

from game import MafiaGame, NightResult, Phase, Player, Role, VoteResult, Winner


BASE_DIR = Path(__file__).resolve().parent
STATS_FILE = BASE_DIR / "stats.json"


@dataclass
class BotConfig:
    participant_role: str
    manager_role: str
    default_mafia_count: int
    default_doctor_count: int
    default_police_count: int
    default_joker_count: int
    max_player_count: int
    night_seconds: int
    discussion_seconds: int
    vote_seconds: int
    chat_slowmode_seconds: int = 3
    reveal_death_roles: bool = False
    reveal_public_police_status: bool = True
    reveal_morning_mafia_count: bool = True
    citizen_special_count: int = 0
    mafia_special_count: int = 0
    neutral_special_count: int = 1
    enable_detective: bool = True
    enable_graverobber: bool = True
    enable_spy: bool = True
    enable_contractor: bool = True
    enable_witch: bool = True
    enable_scientist: bool = True
    enable_madam: bool = True
    enable_godfather: bool = True
    enable_joker: bool = True
    enable_politician: bool = True
    enable_judge: bool = True
    enable_reporter: bool = True
    enable_hacker: bool = True
    enable_terrorist: bool = True
    enable_lover: bool = True
    enable_shaman: bool = True
    enable_priest: bool = True
    enable_soldier: bool = True
    enable_nurse: bool = True
    enable_cult_team: bool = False
    use_agent: bool = False
    use_vigilante: bool = False
    anonymous_mode: bool = False
    anonymous_name_mode: str = "animal"
    game_enabled: bool = True
    blacklist_user_ids: list[int] = field(default_factory=list)


@dataclass
class RunningGame:
    guild_id: int
    channel_id: int
    game: MafiaGame
    reveal_death_roles: bool = False
    reveal_public_police_status: bool = True
    reveal_morning_mafia_count: bool = True
    task: asyncio.Task[None] | None = None
    night_complete_event: asyncio.Event = field(default_factory=asyncio.Event)
    vote_complete_event: asyncio.Event = field(default_factory=asyncio.Event)
    confirm_complete_event: asyncio.Event = field(default_factory=asyncio.Event)
    day_vote_event: asyncio.Event = field(default_factory=asyncio.Event)
    game_channel_overwrites: dict[int, discord.PermissionOverwrite | None] = field(default_factory=dict)
    member_channel_overwrites: dict[int, discord.PermissionOverwrite | None] = field(default_factory=dict)
    madam_seduction_channel_overwrites: dict[int, discord.PermissionOverwrite | None] = field(default_factory=dict)
    original_slowmode_delay: int | None = None
    original_slowmode_channel_id: int | None = None
    participant_user_ids: set[int] = field(default_factory=set)
    private_channel_ids: dict[Role, int] = field(default_factory=dict)
    memo_channel_ids: dict[int, int] = field(default_factory=dict)
    memos: dict[int, dict[int, list[str]]] = field(default_factory=dict)
    spectator_user_ids: set[int] = field(default_factory=set)
    game_status_message_id: int | None = None
    dead_channel_id: int | None = None
    dead_status_message_id: int | None = None
    shaman_channel_id: int | None = None
    shaman_status_message_id: int | None = None
    frog_channel_id: int | None = None
    frog_game_channel_overwrites: dict[int, discord.PermissionOverwrite | None] = field(default_factory=dict)
    night_timed_events_due: bool = False
    anonymous_enabled: bool = False
    anonymous_public_channel_id: int | None = None
    anonymous_input_channel_ids: dict[int, int] = field(default_factory=dict)
    anonymous_input_channel_owners: dict[int, int] = field(default_factory=dict)
    anonymous_dead_input_channel_ids: dict[int, int] = field(default_factory=dict)
    anonymous_dead_input_channel_owners: dict[int, int] = field(default_factory=dict)
    anonymous_shaman_input_channel_ids: dict[int, int] = field(default_factory=dict)
    anonymous_shaman_input_channel_owners: dict[int, int] = field(default_factory=dict)
    anonymous_role_input_channel_ids: dict[tuple[int, Role], int] = field(default_factory=dict)
    anonymous_role_input_channels: dict[int, tuple[int, Role]] = field(default_factory=dict)
    anonymous_role_status_message_ids: dict[Role, int] = field(default_factory=dict)
    anonymous_role_input_status_message_ids: dict[tuple[int, Role], int] = field(default_factory=dict)
    anonymous_aliases: dict[int, str] = field(default_factory=dict)
    anonymous_original_names: dict[int, str] = field(default_factory=dict)
    anonymous_webhook_urls: dict[int, str] = field(default_factory=dict)
    anonymous_original_channel_overwrites: dict[int, discord.PermissionOverwrite | None] = field(default_factory=dict)
    permission_overwrite_cache: dict[tuple[int, int], discord.PermissionOverwrite | None] = field(default_factory=dict)
    final_defense_user_id: int | None = None
    initial_roles: dict[int, Role] = field(default_factory=dict)
    stats_recorded: bool = False
    started_at: float = field(default_factory=time.monotonic)


RECRUITMENT_SECONDS = 60
MAX_GAME_PLAYERS = 24
DAY_EXTENSION_VOTE_SECONDS = 10
DISCUSSION_EXTENSION_SECONDS = 60
CONFIRM_VOTE_SECONDS = 15
GAME_NOTIFICATION_ROLE = "게임알림"
DEAD_PLAYER_ROLE = "사망자"
SPECTATOR_ROLE = "관전자"
DEAD_CHAT_CHANNEL_NAME = "사망자-채팅방"
SHAMAN_CHAT_CHANNEL_NAME = "영매-채팅방"
FROG_CHAT_CHANNEL_NAME = "개구리-채팅방"
ANONYMOUS_PUBLIC_CHANNEL_NAME = "익명-전체채팅"
PRIVATE_CHAT_ROLES = (
    Role.MAFIA,
    Role.POLICE,
    Role.AGENT,
    Role.VIGILANTE,
    Role.DOCTOR,
    Role.CULT_LEADER,
    Role.LOVER,
)
PRIVATE_CHANNEL_NAMES = {
    Role.MAFIA: "마피아-비밀방",
    Role.POLICE: "경찰-비밀방",
    Role.AGENT: "요원-비밀방",
    Role.VIGILANTE: "자경단원-비밀방",
    Role.DOCTOR: "의사-비밀방",
    Role.CULT_LEADER: "교주-비밀방",
    Role.LOVER: "연인-비밀방",
}
BASE_ROLE_ORDER = (Role.MAFIA, Role.DOCTOR, Role.POLICE)
CITIZEN_SPECIAL_ROLES = (
    Role.DETECTIVE,
    Role.SHAMAN,
    Role.PRIEST,
    Role.GRAVEROBBER,
    Role.POLITICIAN,
    Role.JUDGE,
    Role.REPORTER,
    Role.HACKER,
    Role.TERRORIST,
    Role.LOVER,
    Role.SOLDIER,
    Role.NURSE,
)
MAFIA_SPECIAL_ROLES = (Role.SPY, Role.CONTRACTOR, Role.WITCH, Role.SCIENTIST, Role.MADAM, Role.GODFATHER)
NEUTRAL_SPECIAL_ROLES = (Role.JOKER,)
PUBLIC_MAFIA_SPECIAL_ROLES = (Role.SPY, Role.CONTRACTOR, Role.WITCH, Role.SCIENTIST, Role.MADAM, Role.GODFATHER)
PUBLIC_CITIZEN_SPECIAL_ROLES = (
    Role.DETECTIVE,
    Role.SHAMAN,
    Role.PRIEST,
    Role.GRAVEROBBER,
    Role.POLITICIAN,
    Role.JUDGE,
    Role.REPORTER,
    Role.HACKER,
    Role.TERRORIST,
    Role.LOVER,
    Role.SOLDIER,
    Role.NURSE,
    Role.FANATIC,
)
PUBLIC_NEUTRAL_SPECIAL_ROLES = (Role.JOKER,)
PUBLIC_CULT_SPECIAL_ROLES = (Role.CULT_LEADER,)
INVESTIGATION_ROLES = (Role.POLICE, Role.AGENT, Role.VIGILANTE)
CONTRACTOR_GUESS_ROLES = (
    Role.DOCTOR,
    Role.WITCH,
    Role.SCIENTIST,
    Role.MADAM,
    Role.DETECTIVE,
    Role.SHAMAN,
    Role.PRIEST,
    Role.GRAVEROBBER,
    Role.POLITICIAN,
    Role.JUDGE,
    Role.REPORTER,
    Role.HACKER,
    Role.TERRORIST,
    Role.LOVER,
    Role.SOLDIER,
    Role.NURSE,
    Role.CULT_LEADER,
    Role.FANATIC,
    Role.JOKER,
    Role.CITIZEN,
)
DEFAULT_EMBED_COLOR = discord.Color.gold()
ERROR_EMBED_COLOR = discord.Color.red()
SUCCESS_EMBED_COLOR = discord.Color.green()
WARNING_EMBED_COLOR = discord.Color.orange()
DayDiscussionResult = Literal["vote", "stop"]

ANIMAL_ALIASES = (
    "사자", "호랑이", "고양이", "강아지", "토끼", "판다", "곰", "여우", "늑대", "돼지",
    "원숭이", "코끼리", "기린", "펭귄", "오리", "병아리", "부엉이", "독수리", "거북이", "돌고래",
    "상어", "고래", "악어", "뱀", "나비", "벌", "개미", "달팽이", "문어", "물고기",
    "게", "새우", "오징어", "말", "얼룩말", "소", "양", "염소", "닭", "쥐",
    "햄스터", "사슴", "라마", "캥거루", "하마", "코뿔소", "박쥐", "고슴도치", "수달", "비버",
    "너구리", "스컹크", "공작", "앵무새", "백조", "플라밍고", "칠면조", "고릴라", "오랑우탄", "물개",
)

ANIMAL_EMOJI_CODES = {
    "사자": "1f981", "호랑이": "1f42f", "고양이": "1f431", "강아지": "1f436", "토끼": "1f430",
    "판다": "1f43c", "곰": "1f43b", "여우": "1f98a", "늑대": "1f43a", "돼지": "1f437",
    "원숭이": "1f435", "코끼리": "1f418", "기린": "1f992", "펭귄": "1f427", "오리": "1f986",
    "병아리": "1f424", "부엉이": "1f989", "독수리": "1f985", "거북이": "1f422", "돌고래": "1f42c",
    "상어": "1f988", "고래": "1f433", "악어": "1f40a", "뱀": "1f40d", "나비": "1f98b",
    "벌": "1f41d", "개미": "1f41c", "달팽이": "1f40c", "문어": "1f419", "물고기": "1f41f",
    "게": "1f980", "새우": "1f990", "오징어": "1f991", "말": "1f434", "얼룩말": "1f993",
    "소": "1f42e", "양": "1f411", "염소": "1f410", "닭": "1f414", "쥐": "1f42d",
    "햄스터": "1f439", "사슴": "1f98c", "라마": "1f999", "캥거루": "1f998", "하마": "1f99b",
    "코뿔소": "1f98f", "박쥐": "1f987", "고슴도치": "1f994", "수달": "1f9a6", "비버": "1f9ab",
    "너구리": "1f99d", "스컹크": "1f9a8", "공작": "1f99a", "앵무새": "1f99c", "백조": "1f9a2",
    "플라밍고": "1f9a9", "칠면조": "1f983", "고릴라": "1f98d", "오랑우탄": "1f9a7", "물개": "1f9ad",
}

NUMBER_AVATAR_COLORS = (
    "e11d48", "2563eb", "16a34a", "f59e0b", "7c3aed", "0891b2", "db2777", "65a30d",
    "dc2626", "4f46e5", "0f766e", "ea580c", "9333ea", "0284c7", "ca8a04", "be123c",
    "1d4ed8", "15803d", "b45309", "6d28d9", "0369a1", "a21caf", "047857", "c2410c",
)


def parse_user_id_list(values: object) -> list[int]:
    if not isinstance(values, list):
        return []
    user_ids: set[int] = set()
    for value in values:
        with suppress(TypeError, ValueError):
            user_ids.add(int(value))
    return sorted(user_ids)


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
        max_player_count=int(data.get("max_player_count", 0)),
        night_seconds=int(data["night_seconds"]),
        discussion_seconds=int(data.get("discussion_seconds", 60)),
        vote_seconds=int(data["vote_seconds"]),
        chat_slowmode_seconds=int(data.get("chat_slowmode_seconds", 3)),
        reveal_death_roles=bool(data.get("reveal_death_roles", False)),
        reveal_public_police_status=bool(data.get("reveal_public_police_status", True)),
        reveal_morning_mafia_count=bool(data.get("reveal_morning_mafia_count", True)),
        citizen_special_count=int(data.get("citizen_special_count", 0)),
        mafia_special_count=int(data.get("mafia_special_count", 0)),
        neutral_special_count=int(data.get("neutral_special_count", data.get("default_joker_count", 1))),
        enable_detective=bool(data.get("enable_detective", True)),
        enable_graverobber=bool(data.get("enable_graverobber", True)),
        enable_spy=bool(data.get("enable_spy", True)),
        enable_contractor=bool(data.get("enable_contractor", True)),
        enable_witch=bool(data.get("enable_witch", True)),
        enable_scientist=bool(data.get("enable_scientist", True)),
        enable_madam=bool(data.get("enable_madam", True)),
        enable_godfather=bool(data.get("enable_godfather", True)),
        enable_joker=bool(data.get("enable_joker", True)),
        enable_politician=bool(data.get("enable_politician", True)),
        enable_judge=bool(data.get("enable_judge", True)),
        enable_reporter=bool(data.get("enable_reporter", True)),
        enable_hacker=bool(data.get("enable_hacker", True)),
        enable_terrorist=bool(data.get("enable_terrorist", True)),
        enable_lover=bool(data.get("enable_lover", True)),
        enable_shaman=bool(data.get("enable_shaman", True)),
        enable_priest=bool(data.get("enable_priest", True)),
        enable_soldier=bool(data.get("enable_soldier", True)),
        enable_nurse=bool(data.get("enable_nurse", True)),
        enable_cult_team=bool(data.get("enable_cult_team", False)),
        use_agent=bool(data.get("use_agent", False)),
        use_vigilante=bool(data.get("use_vigilante", False)),
        anonymous_mode=bool(data.get("anonymous_mode", False)),
        anonymous_name_mode=str(data.get("anonymous_name_mode", "animal")),
        game_enabled=bool(data.get("game_enabled", True)),
        blacklist_user_ids=parse_user_id_list(data.get("blacklist_user_ids", [])),
    )


config = load_config()
games: dict[int, RunningGame] = {}
recruiting_guilds: set[int] = set()
known_presence_statuses: dict[tuple[int, int], discord.Status | str] = {}


def reload_config() -> None:
    global config
    config = load_config()


def save_config() -> None:
    with (BASE_DIR / "config.json").open("w", encoding="utf-8") as file:
        json.dump(config.__dict__, file, ensure_ascii=False, indent=2)
        file.write("\n")


def remember_member_presence(guild_id: int, member: discord.Member) -> None:
    raw_status = getattr(member, "raw_status", "offline")
    if raw_status != "offline":
        known_presence_statuses[(guild_id, member.id)] = member.status


def remember_presence_update(before: discord.Member, after: discord.Member) -> None:
    known_presence_statuses[(after.guild.id, after.id)] = after.status


def member_is_confirmed_offline(guild_id: int, member: discord.Member) -> bool:
    guild = bot.get_guild(guild_id)
    cached_member = guild.get_member(member.id) if guild else None
    for candidate in (cached_member, member):
        if not isinstance(candidate, discord.Member):
            continue
        raw_status = getattr(candidate, "raw_status", "offline")
        if raw_status != "offline":
            known_presence_statuses[(guild_id, candidate.id)] = candidate.status
            return False

    status = known_presence_statuses.get((guild_id, member.id))
    return status in {discord.Status.offline, "offline"}


def load_stats() -> dict:
    if not STATS_FILE.exists():
        return {"users": {}}
    try:
        with STATS_FILE.open("r", encoding="utf-8") as file:
            data = json.load(file)
    except (OSError, json.JSONDecodeError):
        return {"users": {}}
    if not isinstance(data, dict):
        return {"users": {}}
    if not isinstance(data.get("users"), dict):
        data["users"] = {}
    return data


def save_stats(stats: dict) -> None:
    with STATS_FILE.open("w", encoding="utf-8") as file:
        json.dump(stats, file, ensure_ascii=False, indent=2)
        file.write("\n")


def default_player_stats(name: str) -> dict:
    return {
        "name": name,
        "games": 0,
        "wins": 0,
        "losses": 0,
        "mafia_team_games": 0,
        "play_seconds": 0,
        "roles": {},
    }


def ensure_player_stats(stats: dict, user_id: int, name: str) -> dict:
    users = stats.setdefault("users", {})
    key = str(user_id)
    entry = users.get(key)
    if not isinstance(entry, dict):
        entry = default_player_stats(name)
        users[key] = entry
    entry["name"] = name
    entry.setdefault("games", 0)
    entry.setdefault("wins", 0)
    entry.setdefault("losses", 0)
    entry.setdefault("mafia_team_games", 0)
    entry.setdefault("play_seconds", 0)
    entry.setdefault("roles", {})
    return entry


def initial_role_for_stats(running: RunningGame, player: Player) -> Role:
    return running.initial_roles.get(player.user_id, player.role)


def is_mafia_team_role(role: Role) -> bool:
    return role in {Role.MAFIA, Role.SPY, Role.CONTRACTOR, Role.WITCH, Role.SCIENTIST, Role.GODFATHER, Role.VILLAIN}


def player_won_game(game: MafiaGame, player: Player, winner: Winner) -> bool:
    if winner == Winner.MAFIA:
        return game.is_mafia_team(player)
    if winner == Winner.CULT:
        return game.is_cult_team(player)
    if winner == Winner.JOKER:
        joker_winner_id = getattr(game, "joker_winner_id", None)
        return player.user_id == joker_winner_id or (joker_winner_id is None and player.role == Role.JOKER)
    return game.is_citizen_team(player)


def record_game_stats(running: RunningGame, winner: Winner) -> None:
    if running.stats_recorded:
        return
    stats = load_stats()
    elapsed_seconds = max(0, int(time.monotonic() - running.started_at))
    for player in running.game.players:
        name = original_player_name(running, player) if running.anonymous_enabled else player.name
        entry = ensure_player_stats(stats, player.user_id, name)
        entry["games"] = int(entry.get("games", 0)) + 1
        entry["play_seconds"] = int(entry.get("play_seconds", 0)) + elapsed_seconds
        role = initial_role_for_stats(running, player)
        roles = entry.setdefault("roles", {})
        roles[role.value] = int(roles.get(role.value, 0)) + 1
        if is_mafia_team_role(role):
            entry["mafia_team_games"] = int(entry.get("mafia_team_games", 0)) + 1
        if player_won_game(running.game, player, winner):
            entry["wins"] = int(entry.get("wins", 0)) + 1
        else:
            entry["losses"] = int(entry.get("losses", 0)) + 1
    save_stats(stats)
    running.stats_recorded = True


def win_rate_text(wins: int, games: int) -> str:
    if games <= 0:
        return "0.0%"
    return f"{wins / games * 100:.1f}%"


def role_stats_text(entry: dict) -> str:
    roles = entry.get("roles", {})
    if not isinstance(roles, dict) or not roles:
        return "없음"
    ordered_roles = {role.value: index for index, role in enumerate(ROLE_GUIDE_ORDER)}
    items = sorted(
        roles.items(),
        key=lambda item: (-int(item[1]), ordered_roles.get(item[0], 999), item[0]),
    )
    return ", ".join(f"{role} {count}회" for role, count in items)


def personal_stats_text(user_id: int, fallback_name: str) -> str:
    stats = load_stats()
    entry = stats.get("users", {}).get(str(user_id))
    if not isinstance(entry, dict):
        return "아직 기록된 게임 전적이 없습니다."
    games = int(entry.get("games", 0))
    wins = int(entry.get("wins", 0))
    losses = int(entry.get("losses", 0))
    mafia_games = int(entry.get("mafia_team_games", 0))
    play_seconds = int(entry.get("play_seconds", 0))
    name = str(entry.get("name") or fallback_name)
    return (
        f"{name}님의 전적\n"
        f"전체 게임: **{games}판**\n"
        f"승리/패배: **{wins}승 {losses}패**\n"
        f"승률: **{win_rate_text(wins, games)}**\n"
        f"마피아팀 플레이: **{mafia_games}회**\n"
        f"플레이 시간: **{play_duration_text(play_seconds)}**\n\n"
        f"역할별 플레이\n{role_stats_text(entry)}"
    )


def leaderboard_value(entry: dict, metric: str) -> float:
    games = int(entry.get("games", 0))
    wins = int(entry.get("wins", 0))
    if metric == "winrate":
        return wins / games if games else 0.0
    if metric == "games":
        return float(games)
    if metric == "mafia":
        return float(entry.get("mafia_team_games", 0))
    if metric == "playtime":
        return float(entry.get("play_seconds", 0))
    return float(wins)


def leaderboard_text(metric: str) -> str:
    stats = load_stats()
    users = stats.get("users", {})
    if not isinstance(users, dict) or not users:
        return "아직 기록된 게임 전적이 없습니다."
    entries = [
        (user_id, entry)
        for user_id, entry in users.items()
        if isinstance(entry, dict) and int(entry.get("games", 0)) > 0
    ]
    if not entries:
        return "아직 기록된 게임 전적이 없습니다."
    metric_names = {
        "wins": "승리수",
        "winrate": "승률",
        "games": "판수",
        "mafia": "마피아팀 플레이",
        "playtime": "플레이 시간",
    }
    entries.sort(
        key=lambda item: (
            -leaderboard_value(item[1], metric),
            -int(item[1].get("wins", 0)),
            -int(item[1].get("games", 0)),
            str(item[1].get("name", "")),
        )
    )
    lines = [f"기준: **{metric_names.get(metric, '승리수')}**"]
    for rank, (_user_id, entry) in enumerate(entries[:10], start=1):
        games = int(entry.get("games", 0))
        wins = int(entry.get("wins", 0))
        losses = int(entry.get("losses", 0))
        mafia_games = int(entry.get("mafia_team_games", 0))
        play_seconds = int(entry.get("play_seconds", 0))
        lines.append(
            f"{rank}. **{entry.get('name', '알 수 없음')}** - "
            f"{wins}승 {losses}패 / {games}판 / 승률 {win_rate_text(wins, games)} / "
            f"마피아팀 {mafia_games}회 / 플레이 {play_duration_text(play_seconds)}"
        )
    return "\n".join(lines)


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


def blacklist_user_ids() -> set[int]:
    return set(parse_user_id_list(config.blacklist_user_ids))


def is_blacklisted(user_id: int) -> bool:
    return user_id in blacklist_user_ids()


def set_blacklist_status(user_id: int, blacklisted: bool) -> bool:
    user_ids = blacklist_user_ids()
    was_blacklisted = user_id in user_ids
    if blacklisted:
        user_ids.add(user_id)
    else:
        user_ids.discard(user_id)
    config.blacklist_user_ids = sorted(user_ids)
    return was_blacklisted != blacklisted


def duration_text(seconds: int) -> str:
    if seconds % 60 == 0:
        return f"{seconds // 60}분"
    return f"{seconds}초"


def play_duration_text(seconds: int) -> str:
    minutes = seconds // 60
    if minutes <= 0:
        return "1분 미만"
    return f"{minutes}분"


def special_role_rule_text(role: Role) -> str:
    if role == Role.LOVER:
        return (
            "연인은 두 명이 함께 배정됩니다.\n"
            "연인 대화방은 밤에만 열리며, 두 연인이 모두 생존 중일 때 사용할 수 있습니다."
        )
    action = {
        Role.MAFIA: "공격",
        Role.DOCTOR: "보호",
        Role.POLICE: "조사",
        Role.AGENT: "공작",
        Role.VIGILANTE: "숙청",
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
    if config.default_mafia_count - mafia_special_count < 1:
        raise ValueError(
            "접선 전 특수 마피아만으로는 게임을 진행할 수 없습니다. "
            "일반 마피아가 최소 1명 필요합니다."
        )
    investigation_role = random_investigation_role()
    role_counts = {
        Role.MAFIA: config.default_mafia_count - mafia_special_count,
        Role.DOCTOR: config.default_doctor_count,
    }
    if config.default_police_count > 0:
        role_counts[investigation_role] = config.default_police_count
    for role in selected_special_roles:
        role_counts[role] = role_counts.get(role, 0) + special_role_player_count(role)
    if config.enable_cult_team:
        role_counts[Role.CULT_LEADER] = role_counts.get(Role.CULT_LEADER, 0) + 1
        role_counts[Role.FANATIC] = role_counts.get(Role.FANATIC, 0) + 1
    return role_counts


def enabled_special_roles(pool: tuple[Role, ...]) -> list[Role]:
    enabled = {
        Role.DETECTIVE: config.enable_detective,
        Role.SHAMAN: config.enable_shaman,
        Role.GRAVEROBBER: config.enable_graverobber,
        Role.SPY: config.enable_spy,
        Role.CONTRACTOR: config.enable_contractor,
        Role.WITCH: config.enable_witch,
        Role.SCIENTIST: config.enable_scientist,
        Role.MADAM: config.enable_madam,
        Role.GODFATHER: config.enable_godfather,
        Role.JOKER: config.enable_joker,
        Role.POLITICIAN: config.enable_politician,
        Role.JUDGE: config.enable_judge,
        Role.REPORTER: config.enable_reporter,
        Role.HACKER: config.enable_hacker,
        Role.TERRORIST: config.enable_terrorist,
        Role.LOVER: config.enable_lover,
        Role.PRIEST: config.enable_priest,
        Role.SOLDIER: config.enable_soldier,
        Role.NURSE: config.enable_nurse,
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


def special_role_player_count(role: Role) -> int:
    return 2 if role == Role.LOVER else 1


def expand_special_roles_for_game(roles: list[Role]) -> list[Role]:
    expanded: list[Role] = []
    for role in roles:
        expanded.extend([role] * special_role_player_count(role))
    return expanded


def random_sample_roles(candidates: list[Role], count: int) -> list[Role]:
    import random

    return random.SystemRandom().sample(candidates, count) if count > 0 else []


def investigation_role_candidates() -> list[Role]:
    candidates = [Role.POLICE]
    if config.use_agent:
        candidates.append(Role.AGENT)
    if config.use_vigilante:
        candidates.append(Role.VIGILANTE)
    return candidates


def random_investigation_role() -> Role:
    import random

    return random.SystemRandom().choice(investigation_role_candidates())


def minimum_player_count(role_counts: dict[Role, int]) -> int:
    special_count = sum(role_counts.values())
    mafia_count = (
        role_counts.get(Role.MAFIA, 0)
        + role_counts.get(Role.SPY, 0)
        + role_counts.get(Role.CONTRACTOR, 0)
        + role_counts.get(Role.MADAM, 0)
        + role_counts.get(Role.GODFATHER, 0)
    )
    return max(3, special_count, mafia_count * 2 + 1)


def effective_max_player_count() -> int:
    if config.max_player_count <= 0:
        return MAX_GAME_PLAYERS
    return min(config.max_player_count, MAX_GAME_PLAYERS)


def max_player_setting_text() -> str:
    if config.max_player_count <= 0:
        return f"제한 없음(봇 최대 {MAX_GAME_PLAYERS}명)"
    return f"{effective_max_player_count()}명"


def validate_max_player_count(role_counts: dict[Role, int], max_players: int) -> None:
    if max_players < 0:
        raise ValueError("최대 인원은 0 이상이어야 합니다. 0은 제한 없음입니다.")
    if max_players > MAX_GAME_PLAYERS:
        raise ValueError(f"최대 인원은 {MAX_GAME_PLAYERS}명 이하로 설정해야 합니다.")
    minimum_players = minimum_player_count(role_counts)
    if max_players and max_players < minimum_players:
        raise ValueError(
            f"현재 설정의 최소 시작 인원은 {minimum_players}명이라 "
            f"최대 인원을 {max_players}명으로 설정할 수 없습니다."
        )


def ordered_role_counts(role_counts: dict[Role, int]) -> list[tuple[Role, int]]:
    order = (
        Role.MAFIA,
        Role.SPY,
        Role.CONTRACTOR,
        Role.WITCH,
        Role.SCIENTIST,
        Role.MADAM,
        Role.GODFATHER,
        Role.DOCTOR,
        Role.POLICE,
        Role.AGENT,
        Role.VIGILANTE,
        Role.DETECTIVE,
        Role.SHAMAN,
        Role.PRIEST,
        Role.GRAVEROBBER,
        Role.POLITICIAN,
        Role.JUDGE,
        Role.REPORTER,
        Role.HACKER,
        Role.TERRORIST,
        Role.SOLDIER,
        Role.NURSE,
        Role.CULT_LEADER,
        Role.FANATIC,
        Role.JOKER,
    )
    return [(role, role_counts.get(role, 0)) for role in order if role_counts.get(role, 0) > 0]


def count_role_group(role_counts: dict[Role, int], roles: tuple[Role, ...]) -> int:
    return sum(role_counts.get(role, 0) for role in roles)


def investigation_candidates_text() -> str:
    return ", ".join(role.value for role in investigation_role_candidates())


def public_role_count_text_from_counts(
    role_counts: dict[Role, int],
    total_players: int | None = None,
) -> str:
    mafia_special = count_role_group(role_counts, PUBLIC_MAFIA_SPECIAL_ROLES)
    mafia_total = role_counts.get(Role.MAFIA, 0) + mafia_special
    doctor_total = role_counts.get(Role.DOCTOR, 0)
    police_total = role_counts.get(Role.POLICE, 0)
    agent_total = role_counts.get(Role.AGENT, 0)
    vigilante_total = role_counts.get(Role.VIGILANTE, 0)
    citizen_special = count_role_group(role_counts, PUBLIC_CITIZEN_SPECIAL_ROLES)
    neutral_special = count_role_group(role_counts, PUBLIC_NEUTRAL_SPECIAL_ROLES)
    cult_total = count_role_group(role_counts, PUBLIC_CULT_SPECIAL_ROLES)

    if total_players is None:
        citizen_text = f"시민 변동(중 특수 {citizen_special}명)"
    else:
        citizen_total = max(
            0,
            total_players
            - mafia_total
            - doctor_total
            - police_total
            - agent_total
            - vigilante_total
            - neutral_special
            - cult_total,
        )
        citizen_text = f"시민 {citizen_total}명(중 특수 {citizen_special}명)"

    investigation_total = police_total + agent_total + vigilante_total
    investigation_text = f"수사직 {investigation_total}명"
    parts = [
        f"마피아 {mafia_total}명(중 특수 {mafia_special}명)",
        f"의사 {doctor_total}명",
        investigation_text,
        citizen_text,
    ]
    if neutral_special > 0:
        parts.append(f"중립 특수 {neutral_special}명")
    if cult_total > 0:
        parts.append(f"교주팀 {cult_total}명")
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
        f"최대 참가 인원: {max_player_setting_text()}\n"
        f"교주팀: {'켜짐 - 교주 1명, 광신도 1명 필수 배정' if config.enable_cult_team else '꺼짐'}\n"
        f"사망 시 직업 공개: {'공개' if config.reveal_death_roles else '비공개'}\n"
        f"경찰 조사 성공 여부 공개: {'공개' if config.reveal_public_police_status else '비공개'}\n"
        f"아침 생존 마피아 수 공개: {'공개' if config.reveal_morning_mafia_count else '비공개'}\n"
        f"채팅 슬로우모드: {config.chat_slowmode_seconds}초\n"
        f"익명 채팅: {'켜짐' if config.anonymous_mode else '꺼짐'}"
        f"{f' ({anonymous_name_mode_text()})' if config.anonymous_mode else ''}"
    )


def normalized_anonymous_name_mode() -> str:
    return config.anonymous_name_mode if config.anonymous_name_mode in {"animal", "number"} else "animal"


def anonymous_name_mode_text() -> str:
    return "숫자 이름" if normalized_anonymous_name_mode() == "number" else "동물 이름"


def current_settings_text(prefix: str = "마피아 설정을 저장했습니다.") -> str:
    enabled = [
        role.value
        for role in (
            Role.DETECTIVE,
            Role.SHAMAN,
            Role.GRAVEROBBER,
            Role.SPY,
            Role.CONTRACTOR,
            Role.WITCH,
            Role.SCIENTIST,
            Role.MADAM,
            Role.GODFATHER,
            Role.JOKER,
            Role.POLITICIAN,
            Role.JUDGE,
            Role.REPORTER,
            Role.HACKER,
            Role.TERRORIST,
            Role.LOVER,
            Role.PRIEST,
            Role.SOLDIER,
            Role.NURSE,
            Role.CULT_LEADER,
            Role.FANATIC,
        )
        if {
            Role.DETECTIVE: config.enable_detective,
            Role.SHAMAN: config.enable_shaman,
            Role.GRAVEROBBER: config.enable_graverobber,
            Role.SPY: config.enable_spy,
            Role.CONTRACTOR: config.enable_contractor,
            Role.WITCH: config.enable_witch,
            Role.SCIENTIST: config.enable_scientist,
            Role.MADAM: config.enable_madam,
            Role.GODFATHER: config.enable_godfather,
            Role.JOKER: config.enable_joker,
            Role.POLITICIAN: config.enable_politician,
            Role.JUDGE: config.enable_judge,
            Role.REPORTER: config.enable_reporter,
            Role.HACKER: config.enable_hacker,
            Role.TERRORIST: config.enable_terrorist,
            Role.LOVER: config.enable_lover,
            Role.PRIEST: config.enable_priest,
            Role.SOLDIER: config.enable_soldier,
            Role.NURSE: config.enable_nurse,
            Role.CULT_LEADER: config.enable_cult_team,
            Role.FANATIC: config.enable_cult_team,
        }[role]
    ]
    return (
        f"{prefix}\n"
        f"게임 상태: {'활성화' if config.game_enabled else '비활성화'}\n"
        f"기본 직업: 마피아 {config.default_mafia_count}명, "
        f"의사 {config.default_doctor_count}명, "
        f"수사직 {config.default_police_count}명\n"
        f"최대 참가 인원: {max_player_setting_text()}\n"
        f"특수룰 수: 시민 {config.citizen_special_count}개, "
        f"마피아 {config.mafia_special_count}개, 중립 {config.neutral_special_count}개\n"
        f"활성 특수룰: {', '.join(enabled) if enabled else '없음'}\n"
        f"수사직 후보: {investigation_candidates_text()}\n"
        f"교주팀: {'켜짐 - 교주 1명, 광신도 1명 필수 배정' if config.enable_cult_team else '꺼짐'}\n"
        f"채팅 슬로우모드: {config.chat_slowmode_seconds}초\n"
        f"사망 시 직업 공개: {'공개' if config.reveal_death_roles else '비공개'}\n"
        f"경찰 조사 성공 여부 공개: {'공개' if config.reveal_public_police_status else '비공개'}\n"
        f"아침 생존 마피아 수 공개: {'공개' if config.reveal_morning_mafia_count else '비공개'}\n"
        f"익명 채팅: {'켜짐' if config.anonymous_mode else '꺼짐'}\n"
        f"익명 이름 방식: {anonymous_name_mode_text()}"
    )


def disable_view_items(view: discord.ui.View | None) -> None:
    if not view:
        return
    for item in view.children:
        if hasattr(item, "disabled"):
            item.disabled = True


def target_select_label(target: Player, actor_id: int) -> str:
    label = f"{target.name}(나)" if target.user_id == actor_id else target.name
    return label[:100]


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
        with suppress(discord.HTTPException, discord.NotFound):
            await interaction.followup.send(embed=embed, ephemeral=ephemeral)
    else:
        try:
            await interaction.response.send_message(embed=embed, ephemeral=ephemeral)
        except discord.NotFound:
            with suppress(discord.HTTPException, discord.NotFound):
                await interaction.followup.send(embed=embed, ephemeral=ephemeral)


def status_display_name(running: RunningGame, player: Player) -> str:
    if running.anonymous_enabled:
        return running.anonymous_aliases.get(player.user_id, player.name)
    return player.name


def status_player_list(running: RunningGame, players: list[Player]) -> str:
    if not players:
        return "없음"
    sorted_players = sorted(
        players,
        key=lambda player: status_display_name(running, player).casefold(),
    )
    names = [status_display_name(running, player) for player in sorted_players]
    shown = names[:40]
    suffix = f" 외 {len(names) - len(shown)}명" if len(names) > len(shown) else ""
    return ", ".join(shown) + suffix


def game_status_text(running: RunningGame) -> str:
    alive = running.game.alive_players()
    dead = running.game.dead_players()
    return (
        f"{running.game.day_number}일차 / 현재 단계: {running.game.phase.value}\n"
        f"생존자 **{len(alive)}명** / 사망자 **{len(dead)}명**\n\n"
        f"생존자 목록\n{status_player_list(running, alive)}\n\n"
        f"사망자 목록\n{status_player_list(running, dead)}"
    )


def private_role_status_players(running: RunningGame, player: Player) -> tuple[str, list[Player]]:
    game = running.game
    if game.is_cult_team(player):
        return "내 교주팀", [target for target in game.players if game.is_cult_team(target)]
    if game.is_known_mafia_team(player):
        return "내 마피아팀", [target for target in game.players if game.is_known_mafia_team(target)]
    return f"내 역할({player.role.value})", [target for target in game.players if target.role == player.role]


def command_status_text(running: RunningGame, requester_id: int) -> str:
    message = game_status_text(running)
    player = running.game.get_player(requester_id)
    if not running.anonymous_enabled or not player:
        return message

    label, same_group = private_role_status_players(running, player)
    alive = [target for target in same_group if target.alive]
    dead = [target for target in same_group if not target.alive]
    return (
        f"{message}\n\n"
        f"{label} 현황\n"
        f"생존 **{len(alive)}명** / 사망 **{len(dead)}명**\n"
        f"생존: {status_player_list(running, alive)}\n"
        f"사망: {status_player_list(running, dead)}"
    )


def memo_target_name(running: RunningGame, player: Player) -> str:
    return status_display_name(running, player)


def memo_chunks(running: RunningGame, target: Player, memos: list[str]) -> list[str]:
    target_name = memo_target_name(running, target)
    header = f"{target_name} 님에 대한 메모"
    if not memos:
        return [f"{header}\n저장된 메모가 없습니다."]

    chunks: list[str] = []
    current = header
    for index, memo in enumerate(memos, start=1):
        line = f"{index}. {memo}"
        if len(current) + len(line) + 1 > 3500:
            chunks.append(current)
            current = f"{header} (계속)\n{line}"
        else:
            current += "\n" + line
    chunks.append(current)
    return chunks


async def upsert_game_status(guild: discord.Guild, running: RunningGame) -> None:
    channel = guild.get_channel(running.channel_id)
    if not isinstance(channel, discord.TextChannel):
        return

    embed = make_embed(game_status_text(running), title="게임 현황", color=SUCCESS_EMBED_COLOR)
    if running.game_status_message_id:
        try:
            message = await channel.fetch_message(running.game_status_message_id)
            await message.edit(embed=embed)
            return
        except discord.DiscordException:
            pass

    with suppress(discord.DiscordException):
        message = await channel.send(embed=embed)
        running.game_status_message_id = message.id


async def wait_for_event_or_timeout(event: asyncio.Event, seconds: int) -> None:
    try:
        await asyncio.wait_for(event.wait(), timeout=seconds)
    except asyncio.TimeoutError:
        return


async def wait_for_day_vote_or_timeout(running: RunningGame, seconds: int) -> bool:
    if running.day_vote_event.is_set():
        return True

    vote_task = asyncio.create_task(running.day_vote_event.wait())
    timeout_task = asyncio.create_task(asyncio.sleep(seconds))
    _done, pending = await asyncio.wait(
        {vote_task, timeout_task},
        return_when=asyncio.FIRST_COMPLETED,
    )
    for task in pending:
        task.cancel()
    if pending:
        await asyncio.gather(*pending, return_exceptions=True)
    return running.day_vote_event.is_set()


async def wait_for_day_vote_or_view(running: RunningGame, view: discord.ui.View) -> bool:
    if running.day_vote_event.is_set():
        return True

    vote_task = asyncio.create_task(running.day_vote_event.wait())
    view_task = asyncio.create_task(view.wait())
    _done, pending = await asyncio.wait(
        {vote_task, view_task},
        return_when=asyncio.FIRST_COMPLETED,
    )
    for task in pending:
        task.cancel()
    if pending:
        await asyncio.gather(*pending, return_exceptions=True)
    return running.day_vote_event.is_set()


async def disable_message_view(message: discord.Message, view: discord.ui.View) -> None:
    disable_view_items(view)
    try:
        await message.edit(view=view)
    except discord.DiscordException:
        pass


class JoinGameView(discord.ui.View):
    def __init__(
        self,
        guild_id: int,
        host_user_id: int,
        participant_role_id: int,
        role_counts: dict[Role, int],
        reveal_death_roles: bool,
        reveal_public_police_status: bool,
        reveal_morning_mafia_count: bool,
        max_players: int,
    ) -> None:
        super().__init__(timeout=RECRUITMENT_SECONDS + 5)
        self.guild_id = guild_id
        self.host_user_id = host_user_id
        self.participant_role_id = participant_role_id
        self.role_counts = role_counts
        self.reveal_death_roles = reveal_death_roles
        self.reveal_public_police_status = reveal_public_police_status
        self.reveal_morning_mafia_count = reveal_morning_mafia_count
        self.max_players = max_players
        self.minimum_players = minimum_player_count(role_counts)
        self.joined_ids: set[int] = set()
        self.joined_names: dict[int, str] = {}
        self.spectator_ids: set[int] = set()
        self.spectator_names: dict[int, str] = {}
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

    def spectator_text(self) -> str:
        if not self.spectator_names:
            return "아직 관전자가 없습니다."
        names = sorted(self.spectator_names.values(), key=str.casefold)
        return "\n".join(f"{index}. {name}" for index, name in enumerate(names, start=1))

    def role_count_text(self) -> str:
        return public_role_count_text_from_counts(self.role_counts)

    def minimum_status_text(self) -> str:
        shortage = self.minimum_players - len(self.joined_ids)
        if shortage <= 0:
            return f"최소 시작 인원 **{self.minimum_players}명** 충족"
        return f"최소 시작 인원 **{self.minimum_players}명**까지 **{shortage}명** 더 필요"

    def capacity_status_text(self) -> str:
        remaining = self.max_players - len(self.joined_ids)
        if remaining <= 0:
            return f"최대 참가 인원 **{self.max_players}명** 도달"
        return f"최대 참가 인원 **{self.max_players}명**까지 **{remaining}명** 더 참가 가능"

    def joined_count_text(self) -> str:
        return f"현재 참가자 **{len(self.joined_ids)}/{self.max_players}명**"

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
            f"관전 버튼을 누르면 '{SPECTATOR_ROLE}' 역할이 부여되고 게임 채널을 읽을 수 있습니다.\n"
            "주최자는 `시작` 버튼으로 즉시 시작하거나 `취소` 버튼으로 모집을 취소할 수 있습니다.\n\n"
            f"역할 구성: {self.role_count_text()}\n"
            f"사망 시 직업 공개: {'공개' if self.reveal_death_roles else '비공개'}\n"
            f"경찰 조사 성공 여부 공개: {'공개' if self.reveal_public_police_status else '비공개'}\n"
            f"아침 생존 마피아 수 공개: {'공개' if self.reveal_morning_mafia_count else '비공개'}\n"
            f"{self.minimum_status_text()}\n\n"
            f"{self.capacity_status_text()}\n\n"
            f"{self.joined_count_text()}\n"
            f"{self.participant_text()}\n\n"
            f"현재 관전자 **{len(self.spectator_ids)}명**\n"
            f"{self.spectator_text()}\n\n"
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
        if not interaction.response.is_done():
            with suppress(discord.HTTPException, discord.NotFound):
                await interaction.response.defer(ephemeral=True, thinking=True)
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
            if is_blacklisted(interaction.user.id):
                await send_interaction_reply(
                    interaction,
                    "블랙리스트에 등록된 유저는 참가할 수 없습니다.",
                    private=True,
                )
                return
            if member_is_confirmed_offline(interaction.guild_id, interaction.user):
                await send_interaction_reply(
                    interaction,
                    "오프라인으로 표시된 유저는 참가할 수 없습니다.",
                    private=True,
                )
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
            if interaction.user.id in self.spectator_ids:
                await send_interaction_reply(interaction, "이미 관전자로 등록되어 있습니다.", private=True)
                return
            if len(self.joined_ids) >= self.max_players:
                await send_interaction_reply(
                    interaction,
                    f"최대 참가 인원 {self.max_players}명에 도달해 더 이상 참가할 수 없습니다.",
                    private=True,
                )
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

    @discord.ui.button(label="관전", style=discord.ButtonStyle.secondary)
    async def spectate_game(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
    ) -> None:
        if not interaction.response.is_done():
            with suppress(discord.HTTPException, discord.NotFound):
                await interaction.response.defer(ephemeral=True, thinking=True)
        async with self.lock:
            if not self.accepting:
                await send_interaction_reply(interaction, "참가자 모집이 종료되었습니다.", private=True)
                return
            if interaction.guild_id != self.guild_id or not interaction.guild:
                await send_interaction_reply(interaction, "이 모집에는 관전할 수 없습니다.", private=True)
                return
            if not isinstance(interaction.user, discord.Member) or interaction.user.bot:
                await send_interaction_reply(interaction, "서버 멤버만 관전할 수 있습니다.", private=True)
                return
            if is_blacklisted(interaction.user.id):
                await send_interaction_reply(
                    interaction,
                    "블랙리스트에 등록된 유저는 관전할 수 없습니다.",
                    private=True,
                )
                return
            if interaction.user.id in self.joined_ids:
                await send_interaction_reply(interaction, "이미 참가자로 등록되어 있습니다.", private=True)
                return
            if interaction.user.id in self.spectator_ids:
                await send_interaction_reply(interaction, "이미 관전자로 등록되어 있습니다.", private=True)
                return

            spectator_role = await ensure_spectator_role(interaction.guild)
            if not spectator_role:
                await send_interaction_reply(
                    interaction,
                    f"'{SPECTATOR_ROLE}' 역할을 만들거나 찾을 수 없습니다. "
                    "봇에게 역할 관리 권한이 있고, 봇 역할이 관전자 역할보다 위에 있는지 확인하세요.",
                    private=True,
                )
                return

            try:
                if spectator_role not in interaction.user.roles:
                    await interaction.user.add_roles(
                        spectator_role,
                        reason="마피아 게임 관전 신청",
                    )
            except discord.DiscordException:
                await send_interaction_reply(
                    interaction,
                    f"'{SPECTATOR_ROLE}' 역할 부여에 실패했습니다. "
                    "봇에게 역할 관리 권한이 있고, 봇 역할이 관전자 역할보다 위에 있는지 확인하세요.",
                    private=True,
                )
                return

            self.spectator_ids.add(interaction.user.id)
            self.spectator_names[interaction.user.id] = display_name(interaction.user)
            await send_interaction_reply(
                interaction,
                f"관전 등록 완료! '{SPECTATOR_ROLE}' 역할을 부여했습니다.",
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
            discord.SelectOption(label=target_select_label(target, actor_id), value=str(target.user_id))
            for target in targets[:25]
        ]
        if role == Role.REPORTER:
            options.append(discord.SelectOption(label="사용 안함", value="skip"))
        placeholder = {
            Role.MAFIA: "공격할 대상을 선택하세요",
            Role.DOCTOR: "보호할 대상을 선택하세요",
            Role.NURSE: "처방/치료 대상을 선택하세요",
            Role.POLICE: "조사할 대상을 선택하세요",
            Role.VIGILANTE: "숙청할 대상을 선택하세요",
            Role.REPORTER: "특종을 낼 대상 또는 사용 안함을 선택하세요",
            Role.DETECTIVE: "추적할 대상을 선택하세요",
            Role.SHAMAN: "성불할 사망자를 선택하세요",
            Role.PRIEST: "소생할 사망자를 선택하세요",
            Role.SPY: "첩보할 대상을 선택하세요",
            Role.WITCH: "저주할 대상을 선택하세요",
            Role.GODFATHER: "확정 처치할 대상을 선택하세요",
            Role.TERRORIST: "지목할 대상을 선택하세요",
            Role.CULT_LEADER: "포교할 대상을 선택하세요",
            Role.FANATIC: "추종할 대상을 선택하세요",
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
            target_id = None if self.values[0] == "skip" else int(self.values[0])
            result = running.game.submit_night_action(self.actor_id, target_id)
            cult_bell_count = running.game.consume_cult_bells()
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

        if actor and actor.role == Role.WITCH and running.night_timed_events_due:
            guild = bot.get_guild(running.guild_id)
            channel = guild.get_channel(running.channel_id) if guild else None
            if guild and isinstance(channel, discord.abc.Messageable):
                await apply_timed_night_events(guild, channel, running)
        await interaction.response.edit_message(
            content=None,
            embed=make_embed(result, title="밤 행동 완료", color=SUCCESS_EMBED_COLOR),
            view=self.view,
        )
        if cult_bell_count:
            await announce_cult_bells_now(running, cult_bell_count)
        if running.game.all_night_actions_submitted():
            running.night_complete_event.set()


class NightActionView(discord.ui.View):
    def __init__(self, guild_id: int, actor: Player, targets: list[Player]) -> None:
        super().__init__(timeout=None)
        self.add_item(NightActionSelect(guild_id, actor.user_id, actor.role, targets))


class HackerDayActionSelect(discord.ui.Select[discord.ui.View]):
    def __init__(self, guild_id: int, actor_id: int, targets: list[Player]) -> None:
        options = [
            discord.SelectOption(label=target_select_label(target, actor_id), value=str(target.user_id))
            for target in targets[:25]
        ]
        super().__init__(
            placeholder="해킹할 대상을 선택하세요",
            min_values=1,
            max_values=1,
            options=options,
        )
        self.guild_id = guild_id
        self.actor_id = actor_id

    async def callback(self, interaction: discord.Interaction) -> None:
        if interaction.user.id != self.actor_id:
            await send_interaction_reply(interaction, "본인에게 온 선택지만 사용할 수 있습니다.", private=True)
            return

        running = games.get(self.guild_id)
        if not running:
            await send_interaction_reply(interaction, "진행 중인 게임이 없습니다.", private=True)
            return

        try:
            result = running.game.submit_hacker_action(self.actor_id, int(self.values[0]))
        except ValueError as error:
            await send_interaction_reply(interaction, str(error), private=True)
            return

        disable_view_items(self.view)
        await interaction.response.edit_message(
            content=None,
            embed=make_embed(
                f"{result}\n밤이 시작될 때 대상의 직업을 확인합니다.",
                title="해킹 완료",
                color=SUCCESS_EMBED_COLOR,
            ),
            view=self.view,
        )


class HackerDayActionView(discord.ui.View):
    def __init__(self, guild_id: int, actor: Player, targets: list[Player]) -> None:
        super().__init__(timeout=None)
        self.add_item(HackerDayActionSelect(guild_id, actor.user_id, targets))


class VigilanteDayActionSelect(discord.ui.Select[discord.ui.View]):
    def __init__(self, guild_id: int, actor_id: int, targets: list[Player]) -> None:
        options = [
            discord.SelectOption(label=target_select_label(target, actor_id), value=str(target.user_id))
            for target in targets[:25]
        ]
        super().__init__(
            placeholder="숙청 조사 대상을 선택하세요",
            min_values=1,
            max_values=1,
            options=options,
        )
        self.guild_id = guild_id
        self.actor_id = actor_id

    async def callback(self, interaction: discord.Interaction) -> None:
        if interaction.user.id != self.actor_id:
            await send_interaction_reply(interaction, "본인에게 온 선택지만 사용할 수 있습니다.", private=True)
            return

        running = games.get(self.guild_id)
        if not running:
            await send_interaction_reply(interaction, "진행 중인 게임이 없습니다.", private=True)
            return

        try:
            result = running.game.submit_vigilante_investigation(self.actor_id, int(self.values[0]))
        except ValueError as error:
            await send_interaction_reply(interaction, str(error), private=True)
            return

        disable_view_items(self.view)
        await interaction.response.edit_message(
            content=None,
            embed=make_embed(
                f"{result}\n밤이 시작될 때 대상이 마피아팀인지 확인합니다.",
                title="숙청 조사 완료",
                color=SUCCESS_EMBED_COLOR,
            ),
            view=self.view,
        )


class VigilanteDayActionView(discord.ui.View):
    def __init__(self, guild_id: int, actor: Player, targets: list[Player]) -> None:
        super().__init__(timeout=None)
        self.add_item(VigilanteDayActionSelect(guild_id, actor.user_id, targets))


class ContractorContactSelect(discord.ui.Select[discord.ui.View]):
    def __init__(self, guild_id: int, actor_id: int, targets: list[Player]) -> None:
        options = [
            discord.SelectOption(label=target_select_label(target, actor_id), value=str(target.user_id))
            for target in targets[:25]
        ]
        super().__init__(
            placeholder="동업할 대상을 선택하세요",
            min_values=1,
            max_values=1,
            options=options,
        )
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
            result = running.game.submit_contractor_contact(self.actor_id, int(self.values[0]))
        except ValueError as error:
            await send_interaction_reply(interaction, str(error), private=True)
            return

        disable_view_items(self.view)
        if running.game.all_night_actions_submitted():
            running.night_complete_event.set()
        await interaction.response.edit_message(
            content=None,
            embed=make_embed(result, title="밤 행동 완료", color=SUCCESS_EMBED_COLOR),
            view=self.view,
        )


class ContractorContactView(discord.ui.View):
    def __init__(self, guild_id: int, actor: Player, targets: list[Player]) -> None:
        super().__init__(timeout=None)
        self.add_item(ContractorContactSelect(guild_id, actor.user_id, targets))


class ContractorTargetSelect(discord.ui.Select[discord.ui.View]):
    def __init__(self, parent_view: "ContractorContractView", slot: int, targets: list[Player]) -> None:
        options = [
            discord.SelectOption(
                label=target_select_label(target, parent_view.actor.user_id),
                value=str(target.user_id),
            )
            for target in targets[:25]
        ]
        super().__init__(
            placeholder=f"{slot + 1}번째 청부 대상",
            min_values=1,
            max_values=1,
            options=options,
            row=slot * 2,
        )
        self.parent_view = parent_view
        self.slot = slot

    async def callback(self, interaction: discord.Interaction) -> None:
        if interaction.user.id != self.parent_view.actor.user_id:
            await send_interaction_reply(interaction, "본인에게 온 선택지만 사용할 수 있습니다.", private=True)
            return
        self.parent_view.target_ids[self.slot] = int(self.values[0])
        await interaction.response.defer()


class ContractorRoleSelect(discord.ui.Select[discord.ui.View]):
    def __init__(self, parent_view: "ContractorContractView", slot: int) -> None:
        options = [
            discord.SelectOption(label=role.value, value=role.name)
            for role in CONTRACTOR_GUESS_ROLES
        ]
        super().__init__(
            placeholder=f"{slot + 1}번째 대상 직업 추측",
            min_values=1,
            max_values=1,
            options=options,
            row=slot * 2 + 1,
        )
        self.parent_view = parent_view
        self.slot = slot

    async def callback(self, interaction: discord.Interaction) -> None:
        if interaction.user.id != self.parent_view.actor.user_id:
            await send_interaction_reply(interaction, "본인에게 온 선택지만 사용할 수 있습니다.", private=True)
            return
        self.parent_view.guessed_roles[self.slot] = Role[self.values[0]]
        await interaction.response.defer()


class ContractorContractView(discord.ui.View):
    def __init__(self, guild_id: int, actor: Player, targets: list[Player]) -> None:
        super().__init__(timeout=None)
        self.guild_id = guild_id
        self.actor = actor
        self.target_ids: list[int | None] = [None, None]
        self.guessed_roles: list[Role | None] = [None, None]
        self.add_item(ContractorTargetSelect(self, 0, targets))
        self.add_item(ContractorRoleSelect(self, 0))
        self.add_item(ContractorTargetSelect(self, 1, targets))
        self.add_item(ContractorRoleSelect(self, 1))

    @discord.ui.button(label="청부 확정", style=discord.ButtonStyle.danger, row=4)
    async def submit_contract(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
    ) -> None:
        if interaction.user.id != self.actor.user_id:
            await send_interaction_reply(interaction, "본인에게 온 선택지만 사용할 수 있습니다.", private=True)
            return
        if any(target_id is None for target_id in self.target_ids) or any(
            role is None for role in self.guessed_roles
        ):
            await send_interaction_reply(interaction, "청부 대상 2명과 각 대상의 직업을 모두 선택하세요.", private=True)
            return

        running = games.get(self.guild_id)
        if not running:
            await send_interaction_reply(interaction, "진행 중인 게임이 없습니다.", private=True)
            return

        try:
            result = running.game.submit_contractor_contract(
                self.actor.user_id,
                int(self.target_ids[0]),
                self.guessed_roles[0],
                int(self.target_ids[1]),
                self.guessed_roles[1],
            )
        except ValueError as error:
            await send_interaction_reply(interaction, str(error), private=True)
            return

        disable_view_items(self)
        if running.game.all_night_actions_submitted():
            running.night_complete_event.set()
        await interaction.response.edit_message(
            content=None,
            embed=make_embed(result, title="밤 행동 완료", color=SUCCESS_EMBED_COLOR),
            view=self,
        )


class ContractorActionModeView(discord.ui.View):
    def __init__(
        self,
        guild_id: int,
        actor: Player,
        contact_targets: list[Player],
        contract_targets: list[Player],
        *,
        can_contact: bool,
        can_contract: bool,
    ) -> None:
        super().__init__(timeout=None)
        self.guild_id = guild_id
        self.actor = actor
        self.contact_targets = contact_targets
        self.contract_targets = contract_targets
        for item in self.children:
            if isinstance(item, discord.ui.Button):
                if item.custom_id == "contractor_contact":
                    item.disabled = not can_contact
                if item.custom_id == "contractor_contract":
                    item.disabled = not can_contract

    @discord.ui.button(
        label="동업",
        style=discord.ButtonStyle.secondary,
        custom_id="contractor_contact",
    )
    async def choose_contact(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
    ) -> None:
        if interaction.user.id != self.actor.user_id:
            await send_interaction_reply(interaction, "본인에게 온 선택지만 사용할 수 있습니다.", private=True)
            return
        await interaction.response.edit_message(
            embed=make_embed(
                "마피아라고 생각하는 사람을 한 명 선택하세요.\n대상이 일반 마피아라면 접선합니다.",
                title="청부업자 동업",
            ),
            view=ContractorContactView(self.guild_id, self.actor, self.contact_targets),
        )

    @discord.ui.button(
        label="청부",
        style=discord.ButtonStyle.danger,
        custom_id="contractor_contract",
    )
    async def choose_contract(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
    ) -> None:
        if interaction.user.id != self.actor.user_id:
            await send_interaction_reply(interaction, "본인에게 온 선택지만 사용할 수 있습니다.", private=True)
            return
        await interaction.response.edit_message(
            embed=make_embed(
                "직업이 공개되지 않은 생존자 두 명과 각 대상의 직업을 선택하세요.\n"
                "두 명의 직업을 모두 맞히면 밤이 끝날 때 둘 다 암살됩니다.",
                title="청부업자 청부",
            ),
            view=ContractorContractView(self.guild_id, self.actor, self.contract_targets),
        )


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


class DaySkipToVoteView(discord.ui.View):
    def __init__(self, guild_id: int, alive_user_ids: set[int]) -> None:
        super().__init__(timeout=None)
        self.guild_id = guild_id
        self.alive_user_ids = alive_user_ids
        self.voter_ids: set[int] = set()
        self.required_votes = (len(alive_user_ids) // 2) + 1
        self.accepting = True

    @discord.ui.button(label="바로 투표", style=discord.ButtonStyle.primary)
    async def skip_to_vote(
        self,
        interaction: discord.Interaction,
        button: discord.ui.Button[discord.ui.View],
    ) -> None:
        if not self.accepting:
            await send_interaction_reply(interaction, "바로 투표 선택이 종료되었습니다.", private=True)
            return
        if interaction.user.id not in self.alive_user_ids:
            await send_interaction_reply(
                interaction,
                "생존 중인 참가자만 바로 투표를 선택할 수 있습니다.",
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
                f"이미 바로 투표에 동의했습니다. 현재 {len(self.voter_ids)}/{self.required_votes}명",
                private=True,
            )
            return

        self.voter_ids.add(interaction.user.id)
        if len(self.voter_ids) < self.required_votes:
            await send_interaction_reply(
                interaction,
                f"바로 투표에 동의했습니다. 현재 {len(self.voter_ids)}/{self.required_votes}명",
                private=True,
            )
            return

        self.accepting = False
        disable_view_items(self)
        button.label = "투표 확정"
        await interaction.response.edit_message(
            content=None,
            embed=make_embed(
                f"생존자 과반수가 바로 투표를 선택했습니다. "
                f"({len(self.voter_ids)}/{len(self.alive_user_ids)}명)\n"
                "토론을 끝내고 바로 지목 투표로 넘어갑니다.",
                title="바로 투표",
                color=SUCCESS_EMBED_COLOR,
            ),
            view=self,
        )
        running.day_vote_event.set()
        self.stop()


class MafiaBot(commands.Bot):
    async def setup_hook(self) -> None:
        synced = await self.tree.sync()
        print(f"Synced {len(synced)} slash command(s).")

    async def on_ready(self) -> None:
        for guild in self.guilds:
            for member in guild.members:
                remember_member_presence(guild.id, member)

    async def on_presence_update(self, before: discord.Member, after: discord.Member) -> None:
        remember_presence_update(before, after)


intents = discord.Intents.default()
intents.members = True
intents.message_content = True
intents.presences = True
bot = MafiaBot(command_prefix="!", intents=intents)


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
    )


def can_use_anonymous_role_chat(running: RunningGame, player: Player, role: Role) -> bool:
    if running.game.is_frog(player):
        return False
    if running.game.is_madam_seduced(player) and role != Role.MAFIA:
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


async def send_anonymous_dead_log(
    guild: discord.Guild,
    running: RunningGame,
    *,
    player: Player,
    body: str,
) -> None:
    channel = guild.get_channel(running.dead_channel_id) if running.dead_channel_id else None
    if not isinstance(channel, discord.TextChannel):
        return
    alias = anonymous_dead_sender_label(running, player)
    await send_anonymous_text(
        channel,
        "[사망자 로그]",
        f"{alias} - {body}",
        running=running,
    )


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
    await send_anonymous_dead_log(guild, running, player=sender, body=body)


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
        with suppress(discord.DiscordException):
            await message.delete()
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
            try:
                await message.delete()
            except discord.DiscordException:
                pass
            return
        if running.game.phase != Phase.DAY:
            try:
                await message.delete()
            except discord.DiscordException:
                pass
            return

        croak_count = max(1, len(message.content))
        game_channel = message.guild.get_channel(running.channel_id) if message.guild else None
        try:
            await message.delete()
        except discord.DiscordException:
            pass
        if message.guild:
            game_channel = game_display_channel(message.guild, running, game_channel)
        if isinstance(game_channel, discord.abc.Messageable):
            await send_embed(
                game_channel,
                f"개구리: {'개굴' * croak_count}",
                title="개구리 채팅",
            )
        return

    await bot.process_commands(message)


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
    police_status_reveal="낮에 경찰 조사 성공 여부 공개 여부",
    mafia_count_reveal="아침마다 생존 마피아 수 공개 여부",
    detective="사립탐정 활성화 여부",
    shaman="영매 활성화 여부",
    graverobber="도굴꾼 활성화 여부",
    spy="스파이 활성화 여부",
    contractor="청부업자 활성화 여부",
    witch="마녀 활성화 여부",
    scientist="과학자 활성화 여부",
    godfather="대부 활성화 여부",
    joker="조커 활성화 여부",
    politician="정치인 활성화 여부",
    judge="판사 활성화 여부",
    reporter="기자 활성화 여부",
    hacker="해커 활성화 여부",
    terrorist="테러리스트 활성화 여부",
    soldier="군인 활성화 여부",
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
    police_status_reveal: bool | None = None,
    mafia_count_reveal: bool | None = None,
    detective: bool | None = None,
    shaman: bool | None = None,
    graverobber: bool | None = None,
    spy: bool | None = None,
    contractor: bool | None = None,
    witch: bool | None = None,
    scientist: bool | None = None,
    godfather: bool | None = None,
    joker: bool | None = None,
    politician: bool | None = None,
    judge: bool | None = None,
    reporter: bool | None = None,
    hacker: bool | None = None,
    terrorist: bool | None = None,
    soldier: bool | None = None,
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
        "reveal_public_police_status": police_status_reveal,
        "reveal_morning_mafia_count": mafia_count_reveal,
        "enable_detective": detective,
        "enable_shaman": shaman,
        "enable_graverobber": graverobber,
        "enable_spy": spy,
        "enable_contractor": contractor,
        "enable_witch": witch,
        "enable_scientist": scientist,
        "enable_godfather": godfather,
        "enable_joker": joker,
        "enable_politician": politician,
        "enable_judge": judge,
        "enable_reporter": reporter,
        "enable_hacker": hacker,
        "enable_terrorist": terrorist,
        "enable_soldier": soldier,
    }
    for key, value in bool_updates.items():
        if value is not None:
            updates[key] = value

    previous = {key: getattr(config, key) for key in updates}
    for key, value in updates.items():
        setattr(config, key, value)

    try:
        role_counts = selected_role_counts(choose_special_roles())
        validate_max_player_count(role_counts, config.max_player_count)
    except ValueError as error:
        for key, value in previous.items():
            setattr(config, key, value)
        await send_interaction_reply(interaction, str(error), private=True)
        return

    save_config()
    await send_interaction_reply(interaction, current_settings_text(), private=False)


@bot.tree.command(name="마피아인원설정", description="마피아 게임 모집 최대 인원을 설정합니다.")
@app_commands.describe(max_players=f"최대 참가 인원. 0은 제한 없음(봇 최대 {MAX_GAME_PLAYERS}명)")
async def configure_player_limit(
    interaction: discord.Interaction,
    max_players: int,
) -> None:
    require_manager(interaction)
    try:
        role_counts = selected_role_counts(choose_special_roles())
        validate_max_player_count(role_counts, max_players)
    except ValueError as error:
        await send_interaction_reply(interaction, str(error), private=True)
        return

    config.max_player_count = max_players
    save_config()
    await send_interaction_reply(
        interaction,
        current_settings_text("마피아 인원 설정을 저장했습니다."),
        private=False,
    )


@bot.tree.command(name="마피아익명설정", description="마피아 게임 익명 채팅 사용 여부를 설정합니다.")
@app_commands.describe(enabled="익명 채팅 사용 여부", 이름방식="익명 이름을 동물로 할지 숫자로 할지 선택합니다.")
@app_commands.choices(
    이름방식=[
        app_commands.Choice(name="동물", value="animal"),
        app_commands.Choice(name="숫자", value="number"),
    ]
)
async def configure_anonymous_mode(
    interaction: discord.Interaction,
    enabled: bool,
    이름방식: str | None = None,
) -> None:
    require_manager(interaction)
    config.anonymous_mode = enabled
    if 이름방식 is not None:
        config.anonymous_name_mode = 이름방식
    save_config()
    await send_interaction_reply(
        interaction,
        current_settings_text("마피아 익명 설정을 저장했습니다."),
        private=False,
    )


@bot.tree.command(name="마피아추가설정", description="추가 역할 묶음을 설정합니다.")
@app_commands.describe(
    nurse="간호사 역할 활성화 여부",
    lover="연인 역할 활성화 여부. 선택되면 연인 2명이 함께 배정됩니다.",
    priest="성직자 역할 활성화 여부",
    madam="마담 역할 활성화 여부",
    cult_team="교주팀 활성화 여부. 켜면 교주와 광신도가 함께 배정됩니다.",
)
async def configure_extra_roles(
    interaction: discord.Interaction,
    nurse: bool | None = None,
    lover: bool | None = None,
    priest: bool | None = None,
    madam: bool | None = None,
    cult_team: bool | None = None,
) -> None:
    require_manager(interaction)
    updates: dict[str, bool] = {}
    if nurse is not None:
        updates["enable_nurse"] = nurse
    if lover is not None:
        updates["enable_lover"] = lover
    if priest is not None:
        updates["enable_priest"] = priest
    if madam is not None:
        updates["enable_madam"] = madam
    if cult_team is not None:
        updates["enable_cult_team"] = cult_team

    previous = {key: getattr(config, key) for key in updates}
    for key, value in updates.items():
        setattr(config, key, value)

    try:
        role_counts = selected_role_counts(choose_special_roles())
        validate_max_player_count(role_counts, config.max_player_count)
    except ValueError as error:
        for key, value in previous.items():
            setattr(config, key, value)
        await send_interaction_reply(interaction, str(error), private=True)
        return

    save_config()
    await send_interaction_reply(
        interaction,
        current_settings_text("마피아 추가 설정을 저장했습니다."),
        private=False,
    )


@bot.tree.command(name="마피아수사설정", description="수사직 후보를 설정합니다.")
@app_commands.describe(
    agent="요원을 수사직 랜덤 후보에 포함할지 설정합니다.",
    vigilante="자경단원을 수사직 랜덤 후보에 포함할지 설정합니다.",
)
async def configure_investigation_role(
    interaction: discord.Interaction,
    agent: bool | None = None,
    vigilante: bool | None = None,
) -> None:
    require_manager(interaction)
    updates: dict[str, bool] = {}
    if agent is not None:
        updates["use_agent"] = agent
    if vigilante is not None:
        updates["use_vigilante"] = vigilante

    previous = {key: getattr(config, key) for key in updates}
    for key, value in updates.items():
        setattr(config, key, value)

    try:
        role_counts = selected_role_counts(choose_special_roles())
        validate_max_player_count(role_counts, config.max_player_count)
    except ValueError as error:
        for key, value in previous.items():
            setattr(config, key, value)
        await send_interaction_reply(interaction, str(error), private=True)
        return

    save_config()
    await send_interaction_reply(
        interaction,
        current_settings_text("마피아 수사 설정을 저장했습니다."),
        private=False,
    )


@bot.tree.command(name="마피아시작", description="저장된 설정대로 마피아 게임 참가자를 모집하고 시작합니다.")
async def start_game(
    interaction: discord.Interaction,
) -> None:
    reload_config()
    if not interaction.guild or interaction.guild_id is None or interaction.channel_id is None:
        await send_interaction_reply(interaction, "서버 채널에서만 사용할 수 있습니다.", private=True)
        return
    if not config.game_enabled:
        await send_interaction_reply(interaction, "마피아 게임이 비활성화되어 있습니다.", private=True)
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
            validate_max_player_count(role_counts, config.max_player_count)
            fixed_special_roles: list[Role] = []
            if config.enable_cult_team:
                fixed_special_roles.extend([Role.CULT_LEADER, Role.FANATIC])
            game_special_roles = [*expand_special_roles_for_game(special_roles), *fixed_special_roles]
        except ValueError as error:
            await interaction.followup.send(
                embed=make_embed(str(error), color=ERROR_EMBED_COLOR),
                ephemeral=True,
            )
            return
        clear_failed = await clear_existing_participant_roles(interaction.guild, participant_role)
        spectator_clear_failed: list[str] = []
        spectator_role = get_spectator_role(interaction.guild)
        if spectator_role:
            spectator_clear_failed = await clear_existing_spectator_roles(interaction.guild, spectator_role)
        join_view = JoinGameView(
            interaction.guild.id,
            interaction.user.id,
            participant_role.id,
            role_counts,
            config.reveal_death_roles,
            config.reveal_public_police_status,
            config.reveal_morning_mafia_count,
            effective_max_player_count(),
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
            await remove_spectator_roles_from_ids(
                interaction.guild,
                join_view.spectator_ids,
                "마피아 게임 참가 모집 취소로 관전자 역할 제거",
            )
            await interaction.followup.send(
                embed=make_embed(
                    "참가자 모집이 취소되었습니다. 참가자/관전자 역할을 회수했습니다.",
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
                police_count=role_counts.get(Role.POLICE, 0),
                joker_count=0,
                special_roles=game_special_roles,
                agent_count=role_counts.get(Role.AGENT, 0),
                vigilante_count=role_counts.get(Role.VIGILANTE, 0),
            )
        except ValueError as error:
            await remove_participant_roles_from_ids(
                interaction.guild,
                join_view.joined_ids,
                "마피아 게임 시작 실패로 참가자 역할 제거",
            )
            await remove_spectator_roles_from_ids(
                interaction.guild,
                join_view.spectator_ids,
                "마피아 게임 시작 실패로 관전자 역할 제거",
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
            reveal_public_police_status=config.reveal_public_police_status,
            reveal_morning_mafia_count=config.reveal_morning_mafia_count,
            anonymous_enabled=config.anonymous_mode,
            participant_user_ids=set(join_view.joined_ids),
            spectator_user_ids=set(join_view.spectator_ids),
            initial_roles={player.user_id: player.role for player in game.players},
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
        if spectator_clear_failed:
            warning += (
                "\n\n기존 관전자 역할을 제거하지 못한 유저: "
                + ", ".join(spectator_clear_failed)
                + "\n봇 역할 관리 권한과 역할 순서를 확인하세요."
            )
        await interaction.followup.send(
            embed=make_embed(
                "게임을 시작합니다. "
                f"참가자 {len(game.players)}명에게 역할을 DM으로 보냅니다.\n"
                f"관전자: {len(join_view.spectator_ids)}명\n"
                f"{public_role_count_text(game)}"
                f"\n사망 시 직업 공개: {'공개' if config.reveal_death_roles else '비공개'}"
                f"\n경찰 조사 성공 여부 공개: {'공개' if config.reveal_public_police_status else '비공개'}"
                f"\n아침 생존 마피아 수 공개: {'공개' if config.reveal_morning_mafia_count else '비공개'}"
                f"\n교주팀: {'켜짐 - 교주 1명, 광신도 1명 필수 배정' if config.enable_cult_team else '꺼짐'}"
                f"\n채팅 슬로우모드: {config.chat_slowmode_seconds}초"
                f"\n최대 참가 인원: {max_player_setting_text()}"
                f"\n익명 채팅: {'켜짐' if config.anonymous_mode else '꺼짐'}"
                f"{f' ({anonymous_name_mode_text()})' if config.anonymous_mode else ''}"
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


@bot.tree.command(name="마피아비활성화", description="마피아 게임 시작을 비활성화합니다.")
async def disable_mafia_game(interaction: discord.Interaction) -> None:
    require_manager(interaction)
    config.game_enabled = False
    save_config()
    await send_interaction_reply(
        interaction,
        "마피아 게임을 비활성화했습니다. 새 게임을 시작할 수 없습니다.",
        private=False,
    )


@bot.tree.command(name="마피아활성화", description="마피아 게임 시작을 활성화합니다.")
async def enable_mafia_game(interaction: discord.Interaction) -> None:
    require_manager(interaction)
    config.game_enabled = True
    save_config()
    await send_interaction_reply(
        interaction,
        "마피아 게임을 활성화했습니다. 이제 새 게임을 시작할 수 있습니다.",
        private=False,
    )


@bot.tree.command(name="블랙리스트추가", description="마피아 게임 참가 블랙리스트에 유저를 추가합니다.")
@app_commands.describe(유저="블랙리스트에 추가할 유저")
async def add_to_blacklist(interaction: discord.Interaction, 유저: discord.Member) -> None:
    require_manager(interaction)
    if 유저.bot:
        await send_interaction_reply(interaction, "봇은 블랙리스트에 추가할 수 없습니다.", private=True)
        return

    changed = set_blacklist_status(유저.id, True)
    save_config()
    if changed:
        message = f"{display_name(유저)} 님을 블랙리스트에 추가했습니다. 이제 게임에 참가할 수 없습니다."
    else:
        message = f"{display_name(유저)} 님은 이미 블랙리스트에 있습니다."
    await send_interaction_reply(interaction, message, private=False)


@bot.tree.command(name="블랙리스트해제", description="마피아 게임 참가 블랙리스트에서 유저를 제거합니다.")
@app_commands.describe(유저="블랙리스트에서 해제할 유저")
async def remove_from_blacklist(interaction: discord.Interaction, 유저: discord.Member) -> None:
    require_manager(interaction)
    changed = set_blacklist_status(유저.id, False)
    save_config()
    if changed:
        message = f"{display_name(유저)} 님을 블랙리스트에서 해제했습니다. 이제 게임에 참가할 수 있습니다."
    else:
        message = f"{display_name(유저)} 님은 블랙리스트에 없습니다."
    await send_interaction_reply(interaction, message, private=False)


@bot.tree.command(name="블랙리스트목록", description="마피아 게임 참가 블랙리스트 목록을 확인합니다.")
async def show_blacklist(interaction: discord.Interaction) -> None:
    require_manager(interaction)
    user_ids = sorted(blacklist_user_ids())
    if not user_ids:
        await send_interaction_reply(interaction, "블랙리스트가 비어 있습니다.", private=True)
        return

    lines = []
    guild = interaction.guild
    for index, user_id in enumerate(user_ids[:50], start=1):
        member = guild.get_member(user_id) if guild else None
        name = display_name(member) if member else f"알 수 없음 ({user_id})"
        lines.append(f"{index}. {name} - `{user_id}`")
    if len(user_ids) > 50:
        lines.append(f"... 외 {len(user_ids) - 50}명")
    await interaction.response.send_message(
        embed=make_embed("\n".join(lines), title="블랙리스트"),
        ephemeral=True,
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


@bot.tree.command(name="상태", description="현재 마피아 게임 생존자와 사망자를 확인합니다.")
async def show_public_status(interaction: discord.Interaction) -> None:
    if not interaction.guild_id:
        await send_interaction_reply(interaction, "서버에서만 사용할 수 있습니다.", private=True)
        return

    running = games.get(interaction.guild_id)
    if not running:
        await send_interaction_reply(interaction, "진행 중인 게임이 없습니다.", private=True)
        return

    player = running.game.get_player(interaction.user.id)
    await interaction.response.send_message(
        embed=make_embed(command_status_text(running, interaction.user.id), title="게임 현황"),
        ephemeral=running.anonymous_enabled and player is not None,
    )


@bot.tree.command(name="메모", description="개인 메모 채널에 참가자별 메모를 저장하거나 조회합니다.")
@app_commands.describe(참가자="메모 대상 참가자", 메모내용="저장할 메모 내용. 비워두면 조회합니다.")
async def write_memo(
    interaction: discord.Interaction,
    참가자: discord.Member,
    메모내용: str | None = None,
) -> None:
    if not interaction.guild or not interaction.guild_id:
        await send_interaction_reply(interaction, "서버에서만 사용할 수 있습니다.", private=True)
        return

    running = games.get(interaction.guild_id)
    if not running:
        await send_interaction_reply(interaction, "진행 중인 게임이 없습니다.", private=True)
        return

    author = running.game.get_player(interaction.user.id)
    if not author:
        await send_interaction_reply(interaction, "현재 게임 참가자만 메모를 사용할 수 있습니다.", private=True)
        return

    target = running.game.get_player(참가자.id)
    if not target:
        await send_interaction_reply(interaction, "메모 대상은 현재 게임 참가자여야 합니다.", private=True)
        return

    memo_channel = await ensure_memo_channel(interaction.guild, running, author)
    if not memo_channel:
        await send_interaction_reply(interaction, "개인 메모 채널을 만들 수 없습니다.", private=True)
        return

    target_memos = running.memos.setdefault(author.user_id, {}).setdefault(target.user_id, [])
    content = (메모내용 or "").strip()
    if not content:
        chunks = memo_chunks(running, target, target_memos)
        await interaction.response.send_message(
            embed=make_embed(chunks[0], title="메모 조회"),
            ephemeral=True,
        )
        for chunk in chunks[1:]:
            await interaction.followup.send(
                embed=make_embed(chunk, title="메모 조회"),
                ephemeral=True,
            )
        return

    target_memos.append(content)
    memo_number = len(target_memos)
    target_name = memo_target_name(running, target)
    await send_embed(
        memo_channel,
        f"대상: {target_name}\n{memo_number}. {content}",
        title="메모 등록",
        color=SUCCESS_EMBED_COLOR,
    )
    await interaction.response.send_message(
        embed=make_embed(
            f"{target_name} 님에 대한 메모를 저장했습니다.",
            title="메모 등록",
            color=SUCCESS_EMBED_COLOR,
        ),
        ephemeral=True,
    )


@bot.tree.command(name="내정보", description="내 마피아 게임 전적을 확인합니다.")
async def show_my_info(interaction: discord.Interaction) -> None:
    fallback_name = (
        display_name(interaction.user)
        if isinstance(interaction.user, discord.Member)
        else interaction.user.name
    )
    await interaction.response.send_message(
        embed=make_embed(personal_stats_text(interaction.user.id, fallback_name), title="내정보"),
        ephemeral=True,
    )


@bot.tree.command(name="리더보드", description="마피아 게임 전적 순위를 확인합니다.")
@app_commands.describe(기준="순위를 세울 기준")
@app_commands.choices(
    기준=[
        app_commands.Choice(name="승리수", value="wins"),
        app_commands.Choice(name="승률", value="winrate"),
        app_commands.Choice(name="판수", value="games"),
        app_commands.Choice(name="마피아팀 횟수", value="mafia"),
        app_commands.Choice(name="플레이 시간", value="playtime"),
    ]
)
async def show_leaderboard(
    interaction: discord.Interaction,
    기준: str = "wins",
) -> None:
    await interaction.response.send_message(
        embed=make_embed(leaderboard_text(기준), title="리더보드"),
    )


@bot.tree.command(name="리더보드초기화", description="마피아 게임 전적과 리더보드를 초기화합니다.")
async def reset_leaderboard(interaction: discord.Interaction) -> None:
    require_manager(interaction)
    save_stats({"users": {}})
    await send_interaction_reply(
        interaction,
        "리더보드와 개인 전적을 초기화했습니다.",
        private=False,
    )


def find_role_by_name(name: str) -> Role | None:
    normalized = name.strip()
    if not normalized:
        return None
    for role in ROLE_GUIDE_ORDER:
        if role.value == normalized:
            return role
    lowered = normalized.casefold()
    for role in ROLE_GUIDE_ORDER:
        if role.value.casefold() == lowered:
            return role
    matches = [role for role in ROLE_GUIDE_ORDER if lowered in role.value.casefold()]
    return matches[0] if len(matches) == 1 else None


async def role_name_autocomplete(
    interaction: discord.Interaction,
    current: str,
) -> list[app_commands.Choice[str]]:
    query = current.strip().casefold()
    roles = [
        role
        for role in ROLE_GUIDE_ORDER
        if not query or query in role.value.casefold()
    ]
    return [
        app_commands.Choice(name=role.value, value=role.value)
        for role in roles[:25]
    ]


def term_primary_name(term: tuple[str, tuple[str, ...], str, str]) -> str:
    return term[1][0]


def term_field_value(term: tuple[str, tuple[str, ...], str, str]) -> str:
    _category, names, meaning, example = term
    aliases = ", ".join(names[1:])
    lines = [meaning]
    if aliases:
        lines.append(f"같은 말: {aliases}")
    if example:
        lines.append(f"예시: {example}")
    return "\n".join(lines)


def find_term_by_name(name: str) -> tuple[str, tuple[str, ...], str, str] | None:
    query = name.strip().casefold()
    if not query:
        return None
    for term in MAFIA_TERM_ENTRIES:
        if any(alias.casefold() == query for alias in term[1]):
            return term
    matches = [
        term
        for term in MAFIA_TERM_ENTRIES
        if any(query in alias.casefold() for alias in term[1])
    ]
    return matches[0] if len(matches) == 1 else None


async def term_name_autocomplete(
    interaction: discord.Interaction,
    current: str,
) -> list[app_commands.Choice[str]]:
    query = current.strip().casefold()
    terms = [
        term
        for term in MAFIA_TERM_ENTRIES
        if not query or any(query in alias.casefold() for alias in term[1])
    ]
    return [
        app_commands.Choice(name=term_primary_name(term), value=term_primary_name(term))
        for term in terms[:25]
    ]


def make_term_guide_embeds(title: str = "용어 설명") -> list[discord.Embed]:
    embeds: list[discord.Embed] = []
    grouped_terms: dict[str, list[tuple[str, tuple[str, ...], str, str]]] = {}
    for term in MAFIA_TERM_ENTRIES:
        grouped_terms.setdefault(term[0], []).append(term)

    for category, terms in grouped_terms.items():
        current_embed: discord.Embed | None = None
        current_size = 0
        for term in terms:
            field_name = term_primary_name(term)
            field_value = term_field_value(term)
            entry_size = len(field_name) + len(field_value) + 16
            if current_embed is None or len(current_embed.fields) >= 25 or current_size + entry_size > 5200:
                suffix = f" - {category}"
                current_embed = make_embed(
                    "마피아42 용어 문서를 참고해 이 봇 진행에 맞게 짧게 정리한 용어집입니다.",
                    title=f"{title}{suffix}",
                )
                embeds.append(current_embed)
                current_size = len(current_embed.title or "") + len(current_embed.description or "")
            current_embed.add_field(name=field_name, value=field_value, inline=False)
            current_size += entry_size

    return embeds


@bot.tree.command(name="용어정보", description="마피아 게임 용어 하나를 확인합니다.")
@app_commands.describe(용어="설명을 볼 용어")
@app_commands.autocomplete(용어=term_name_autocomplete)
async def show_term_info(interaction: discord.Interaction, 용어: str) -> None:
    term = find_term_by_name(용어)
    if not term:
        await send_interaction_reply(
            interaction,
            "용어를 찾을 수 없습니다. 자동완성 목록에서 선택하거나 정확한 용어를 입력하세요.",
            private=True,
        )
        return

    category, names, _meaning, _example = term
    await interaction.response.send_message(
        embed=make_embed(
            f"분류: {category}\n\n{term_field_value(term)}",
            title=f"용어정보 - {names[0]}",
            color=SUCCESS_EMBED_COLOR,
        )
    )


@bot.tree.command(name="용어설명", description="마피아 게임 용어 설명을 공지용 임베드로 보냅니다.")
async def show_term_descriptions(interaction: discord.Interaction) -> None:
    embeds = make_term_guide_embeds()
    await interaction.response.send_message(embed=embeds[0])
    for embed in embeds[1:]:
        await interaction.followup.send(embed=embed)


@bot.tree.command(name="직업정보", description="특정 직업의 설명을 확인합니다.")
@app_commands.describe(직업명="설명을 볼 직업 이름")
@app_commands.autocomplete(직업명=role_name_autocomplete)
async def show_role_info(interaction: discord.Interaction, 직업명: str) -> None:
    role = find_role_by_name(직업명)
    if not role:
        await send_interaction_reply(
            interaction,
            "직업을 찾을 수 없습니다. 자동완성 목록에서 선택하거나 정확한 직업명을 입력하세요.",
            private=True,
        )
        return

    await interaction.response.send_message(
        embed=make_embed(
            f"{role.value}\n{role_guide_value(role)}",
            title="직업정보",
            color=SUCCESS_EMBED_COLOR,
        )
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


@bot.tree.command(name="역할설명", description="마피아 게임 전체 역할 설명을 공지용 임베드로 보냅니다.")
async def show_role_descriptions(interaction: discord.Interaction) -> None:
    embeds = make_role_guide_embeds(title="역할 설명")
    await interaction.response.send_message(embed=embeds[0])
    for embed in embeds[1:]:
        await interaction.followup.send(embed=embed)


@configure_game.error
@configure_player_limit.error
@configure_extra_roles.error
@configure_investigation_role.error
@start_game.error
@stop_game.error
@disable_mafia_game.error
@enable_mafia_game.error
@add_to_blacklist.error
@remove_from_blacklist.error
@show_blacklist.error
@show_status.error
@show_public_status.error
@write_memo.error
@show_my_info.error
@show_leaderboard.error
@reset_leaderboard.error
@show_term_info.error
@show_term_descriptions.error
@show_role_info.error
@show_abilities.error
@show_role_descriptions.error
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

    error_embed = make_embed(message, color=ERROR_EMBED_COLOR)
    if interaction.response.is_done():
        with suppress(discord.HTTPException):
            await interaction.followup.send(embed=error_embed, ephemeral=True)
        return

    try:
        await interaction.response.send_message(embed=error_embed, ephemeral=True)
    except discord.HTTPException:
        with suppress(discord.HTTPException):
            await interaction.followup.send(embed=error_embed, ephemeral=True)


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
                if role == Role.MAFIA and running.game.is_known_mafia_team(player):
                    continue
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
    import random

    players = sorted(running.game.players, key=lambda player: player.user_id)
    if normalized_anonymous_name_mode() == "number":
        aliases = [f"{index}번" for index in range(1, len(players) + 1)]
    else:
        aliases = list(ANIMAL_ALIASES)
    random.SystemRandom().shuffle(aliases)
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
    running.anonymous_public_channel_id = None

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
        input_overwrites[member] = anonymous_input_overwrite(True, True)
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


def anonymous_public_channel(guild: discord.Guild, running: RunningGame) -> discord.TextChannel | None:
    if running.anonymous_public_channel_id is None:
        return None
    channel = guild.get_channel(running.anonymous_public_channel_id)
    return channel if isinstance(channel, discord.TextChannel) else None


def anonymous_personal_channel(
    guild: discord.Guild,
    running: RunningGame,
    player: Player,
) -> discord.TextChannel | None:
    channel_id = running.anonymous_input_channel_ids.get(player.user_id)
    channel = guild.get_channel(channel_id) if channel_id else None
    return channel if isinstance(channel, discord.TextChannel) else None


def game_display_channel(
    guild: discord.Guild,
    running: RunningGame,
    fallback: discord.abc.Messageable,
) -> discord.abc.Messageable:
    return anonymous_public_channel(guild, running) or fallback


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
    channel = game_display_channel(guild, running, channel)
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
            "마담에게 유혹당했습니다. 유혹 상태에서는 능력을 사용할 수 없고 말할 수 없습니다.\n"
            "마피아팀이라면 능력 사용은 가능하며, 접대가 성립해 마피아 비밀방에 합류합니다.",
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


def anonymous_role_status_text(running: RunningGame, role: Role) -> str:
    players = anonymous_role_status_players(running, role)
    if not players:
        return "현재 생존: 없음"
    names = ", ".join(running.anonymous_aliases.get(player.user_id, player.name) for player in players)
    return f"현재 생존: {names}"


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

    if failed_roles:
        await send_embed(
            channel,
            "역할별 비공개 채널 생성에 실패했습니다: "
            + ", ".join(failed_roles)
            + "\n봇에게 채널 관리 권한이 있는지 확인하세요.",
            color=ERROR_EMBED_COLOR,
        )


async def create_dead_chat_channel(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    category = source_channel_category(channel)
    participant_role = get_participant_role(guild)
    dead_role = get_dead_player_role(guild)
    manager_role = discord.utils.get(guild.roles, name=config.manager_role)
    bot_member = guild.me or (guild.get_member(bot.user.id) if bot.user else None)

    overwrites: dict[discord.abc.Snowflake, discord.PermissionOverwrite] = {
        guild.default_role: dead_channel_overwrite(running.anonymous_enabled, False),
    }
    if participant_role:
        overwrites[participant_role] = dead_channel_overwrite(False, False)
    if dead_role:
        overwrites[dead_role] = dead_channel_overwrite(True, False)
    add_spectator_overwrite(guild, overwrites)
    if manager_role:
        overwrites[manager_role] = dead_channel_overwrite(False, False)
    if bot_member:
        overwrites[bot_member] = dead_channel_overwrite(True, True)

    try:
        dead_channel = await guild.create_text_channel(
            name=DEAD_CHAT_CHANNEL_NAME,
            overwrites=overwrites,
            category=category,
            reason="마피아 게임 사망자 채팅방 생성",
        )
    except discord.DiscordException:
        try:
            dead_channel = await guild.create_text_channel(
                name=DEAD_CHAT_CHANNEL_NAME,
                overwrites=overwrites,
                reason="마피아 게임 사망자 채팅방 생성",
            )
        except discord.DiscordException:
            await send_embed(
                channel,
                "사망자 채팅방 생성에 실패했습니다. 봇에게 채널 관리 권한이 있는지 확인하세요.",
                color=ERROR_EMBED_COLOR,
            )
            return

    running.dead_channel_id = dead_channel.id
    await send_embed(
        dead_channel,
        "사망자 전용 채팅방입니다.\n"
        "사망자 대화는 각자의 사망자 개인 채널끼리 전달됩니다.\n"
        "이 채널은 사망자 상태와 로그 확인용입니다.\n"
        + "성불된 사망자는 사망자 채팅을 할 수 없습니다.",
        title="사망자 채팅방",
        color=SUCCESS_EMBED_COLOR,
    )
    await upsert_dead_chat_status(guild, running)


def dead_chat_status_text(running: RunningGame) -> str:
    return (
        "사망자 채팅은 각자의 사망자 개인 채널로 전달됩니다.\n"
        "성불된 사망자는 채팅할 수 없습니다."
    )


async def upsert_dead_chat_status(guild: discord.Guild, running: RunningGame) -> None:
    if running.dead_channel_id is None:
        return
    channel = guild.get_channel(running.dead_channel_id)
    if not isinstance(channel, discord.TextChannel):
        return
    embed = make_embed(
        dead_chat_status_text(running),
        title="사망자 채팅 상태",
        color=SUCCESS_EMBED_COLOR,
    )
    if running.dead_status_message_id:
        with suppress(discord.DiscordException):
            message = await channel.fetch_message(running.dead_status_message_id)
            await message.edit(embed=embed)
            return
    with suppress(discord.DiscordException):
        message = await channel.send(embed=embed)
        running.dead_status_message_id = message.id


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
    if running.dead_channel_id is None:
        return
    channel = guild.get_channel(running.dead_channel_id)
    if not isinstance(channel, discord.TextChannel):
        running.dead_channel_id = None
        return
    member = await get_guild_member(guild, player.user_id)
    if not member:
        return
    dead_role = get_dead_player_role(guild)
    should_set_member = (
        dead_role is None
        or cached_channel_overwrite(channel, member, running) is not None
        or not can_view
        or not can_chat
    )
    if should_set_member:
        try:
            await set_permissions_if_changed(
                channel,
                member,
                overwrite=dead_channel_overwrite(can_view, False),
                reason=reason,
                running=running,
            )
        except discord.DiscordException:
            pass
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
    await upsert_dead_chat_status(guild, running)


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
            and not (running.game.is_madam_seduced(player) and channel_role != Role.MAFIA)
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
    can_chat = not (running.game.is_madam_seduced(player) and channel_role != Role.MAFIA)
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
        can_chat = can_chat and not (running.game.is_madam_seduced(player) and channel_role != Role.MAFIA)
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
    can_chat = can_chat and not (running.game.is_madam_seduced(player) and channel_role != Role.MAFIA)
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
    if running.anonymous_public_channel_id is not None:
        channel_ids.add(running.anonymous_public_channel_id)
    channel_ids.update(running.anonymous_input_channel_ids.values())
    channel_ids.update(running.anonymous_dead_input_channel_ids.values())
    channel_ids.update(running.anonymous_shaman_input_channel_ids.values())
    channel_ids.update(running.anonymous_role_input_channel_ids.values())

    for channel_id in channel_ids:
        channel = guild.get_channel(channel_id)
        if channel:
            with suppress(discord.DiscordException):
                await channel.delete(reason="마피아 게임 종료로 익명 채팅방 삭제")

    running.anonymous_public_channel_id = None
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


async def delete_dead_chat_channel(guild: discord.Guild, running: RunningGame) -> None:
    if running.dead_channel_id is None:
        return
    channel = guild.get_channel(running.dead_channel_id)
    if channel:
        try:
            await channel.delete(reason="마피아 게임 종료로 사망자 채팅방 삭제")
        except discord.DiscordException:
            return
    running.dead_channel_id = None
    running.dead_status_message_id = None


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
    await delete_dead_chat_channel(guild, running)
    await delete_shaman_chat_channel(guild, running)
    await delete_frog_chat_channel(guild, running)


async def game_loop(guild: discord.Guild, running: RunningGame) -> None:
    channel = guild.get_channel(running.channel_id)
    if not isinstance(channel, discord.abc.Messageable):
        games.pop(running.guild_id, None)
        return

    try:
        original_channel = channel
        await create_anonymous_chat_channels(guild, original_channel, running)
        channel = game_display_channel(guild, running, original_channel)
        await set_spectator_game_channel_access(
            guild,
            original_channel,
            running,
            "마피아 게임 관전자 게임 채널 열람 권한 설정",
        )
        await hide_original_game_channel_for_anonymous(guild, original_channel, running)
        await create_private_role_channels(guild, channel, running)
        await create_memo_channels(guild, channel, running)
        await sync_cult_team_channel_access(guild, running)
        await create_dead_chat_channel(guild, channel, running)
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


async def run_vote_phase(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
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
    await wait_for_event_or_timeout(running.vote_complete_event, config.vote_seconds)

    vote_result = running.game.resolve_nomination_vote()
    await handle_madam_seduction_result(guild, running, vote_result)
    vote_summary = anonymous_vote_summary(running.game, vote_result)
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
            f"{message}\n\n익명 투표 집계\n{vote_summary}",
            title="지목 투표 결과",
        )
        return

    await send_game_embed(
        guild,
        channel,
        running,
        f"지목 투표 결과, {nominee.name} 님이 최후변론 대상이 되었습니다.\n\n익명 투표 집계\n{vote_summary}",
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
    await set_game_channel_chat(
        guild,
        channel,
        running,
        participants_can_chat=False,
        reason="마피아 게임 최후변론 시작",
    )
    if not running.game.is_frog(nominee) and not running.game.is_madam_seduced(nominee):
        await set_member_chat_permission(
            guild,
            channel,
            running,
            nominee,
            True,
            "마피아 게임 최후변론 대상 발언 허용",
        )
    await set_channel_slowmode(channel, running, 0, "마피아 게임 최후변론 슬로우모드 해제")


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
            "숙청 처형은 밤에 확실하게 알고 있는 마피아팀에게 한 번만 시도할 수 있습니다.",
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


async def apply_timed_night_events(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    cursed_players, witch_contacts = running.game.apply_witch_curses()
    for player in cursed_players:
        await apply_frog_permissions(guild, running, player)
    cult_bell_count = running.game.consume_cult_bells()
    for user_id in witch_contacts:
        player = running.game.get_player(user_id)
        if player:
            await add_player_to_private_role_channel(guild, running, Role.MAFIA, player)
            await send_player_secret(guild, running, player, "저주 대상이 마피아라 마피아팀과 접선했습니다.")
    if cursed_players:
        await send_game_embed(
            guild,
            channel,
            running,
            "마녀의 저주가 발동했습니다.\n"
            "누군가 다음 밤까지 개구리가 되었습니다.",
            title="마녀 저주",
            color=WARNING_EMBED_COLOR,
        )
    if cult_bell_count:
        await announce_cult_bells_now(running, cult_bell_count)

    revived_players = running.game.revive_pending_scientists()
    for player in revived_players:
        await restore_revived_player_roles(guild, running, player)
    if revived_players:
        await sync_cult_team_channel_access(guild, running)
    if revived_players:
        await send_game_embed(
            guild,
            channel,
            running,
            "\n".join(f"[과학자 {player.name}님이 부활했습니다.]" for player in revived_players),
            title="과학자 부활",
            color=SUCCESS_EMBED_COLOR,
        )


async def wait_for_night_actions(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    running.night_timed_events_due = config.night_seconds <= 10
    if config.night_seconds <= 10:
        await wait_for_event_or_timeout(running.night_complete_event, config.night_seconds)
        await apply_timed_night_events(guild, channel, running)
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
        await apply_timed_night_events(guild, channel, running)
    await wait_for_event_or_timeout(running.night_complete_event, 10)


async def run_night(
    guild: discord.Guild,
    channel: discord.abc.Messageable,
    running: RunningGame,
) -> None:
    running.game.phase = Phase.NIGHT
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
        await apply_timed_night_events(guild, channel, running)

    failed_names: list[str] = []
    for actor in running.game.night_action_actors():
        if actor.role == Role.CONTRACTOR:
            contact_targets = [player for player in running.game.alive_players() if player.user_id != actor.user_id]
            contract_targets = sorted(
                running.game.contractor_contract_targets(actor),
                key=lambda player: player.name.casefold(),
            )
            view = ContractorActionModeView(
                running.guild_id,
                actor,
                sorted(contact_targets, key=lambda player: player.name.casefold()),
            contract_targets,
            can_contact=actor.user_id not in running.game.contractor_contacted,
            can_contract=running.game.contractor_can_use_contract(actor.user_id),
        )
            sent = await send_player_secret(
                guild,
                running,
                actor,
                "청부업자 밤 행동을 선택하세요.\n"
                "동업은 마피아를 지목하면 접선합니다.\n"
                "청부는 두 번째 밤부터 사용할 수 있고, 수사직과 직업이 공개된 사람은 대상에서 제외됩니다.",
                view,
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
                f"{actor.role.value} 밤 행동을 선택하세요.",
                NightActionView(running.guild_id, actor, targets),
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

    if running.game.all_night_actions_submitted():
        running.night_complete_event.set()
    await wait_for_night_actions(guild, channel, running)
    running.night_timed_events_due = True
    await apply_timed_night_events(guild, channel, running)
    result = running.game.resolve_night()
    await sync_lover_chat_access(guild, running, reason="마피아 게임 낮 시작으로 연인 채팅 권한 갱신")
    await sync_shaman_channel_permissions(guild, running, can_chat=False)
    await announce_night_private_results(guild, running, result)
    for user_id in result.spy_contacts:
        player = running.game.get_player(user_id)
        if player:
            await add_player_to_private_role_channel(guild, running, Role.MAFIA, player)
    for user_id in result.contractor_contacts:
        player = running.game.get_player(user_id)
        if player:
            await add_player_to_private_role_channel(guild, running, Role.MAFIA, player)
    for user_id in result.witch_contacts:
        player = running.game.get_player(user_id)
        if player:
            await add_player_to_private_role_channel(guild, running, Role.MAFIA, player)
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
                await set_player_private_channel_access(
                    guild,
                    running,
                    Role.MAFIA,
                    killed,
                    can_chat=True,
                    reason="마피아 게임 과학자 유착으로 마피아 채널 권한 부여",
                )
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

    for player in alive_police:
        await send_player_secret(guild, running, player, message)


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
    if actor.role == Role.MAFIA:
        return [player for player in alive if game.can_mafia_attack(player, actor.user_id)]
    if actor.role == Role.DOCTOR:
        return alive
    if actor.role == Role.NURSE:
        if actor.user_id in game.nurse_contacted:
            return alive if not game.alive_role_count(Role.DOCTOR) else []
        return [player for player in alive if player.user_id != actor.user_id]
    if actor.role == Role.SHAMAN:
        return sorted(game.unpurified_dead_players(), key=lambda player: player.name.casefold())
    if actor.role == Role.PRIEST:
        return sorted(game.unpurified_dead_players(), key=lambda player: player.name.casefold())
    if actor.role == Role.CULT_LEADER:
        return [
            player
            for player in alive
            if player.user_id != actor.user_id and not game.is_cult_team(player)
        ]
    if actor.role in {
        Role.POLICE,
        Role.REPORTER,
        Role.DETECTIVE,
        Role.SPY,
        Role.WITCH,
        Role.GODFATHER,
        Role.TERRORIST,
        Role.FANATIC,
    }:
        return [player for player in alive if player.user_id != actor.user_id]
    if actor.role == Role.VIGILANTE:
        return sorted(game.vigilante_execution_targets(actor), key=lambda player: player.name.casefold())
    if actor.role == Role.CONTRACTOR:
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


ROLE_GUIDE_ORDER = (
    Role.MAFIA,
    Role.POLICE,
    Role.AGENT,
    Role.VIGILANTE,
    Role.DOCTOR,
    Role.NURSE,
    Role.DETECTIVE,
    Role.SHAMAN,
    Role.PRIEST,
    Role.GRAVEROBBER,
    Role.POLITICIAN,
    Role.JUDGE,
    Role.REPORTER,
    Role.HACKER,
    Role.TERRORIST,
    Role.LOVER,
    Role.SOLDIER,
    Role.SPY,
    Role.CONTRACTOR,
    Role.WITCH,
    Role.SCIENTIST,
    Role.MADAM,
    Role.GODFATHER,
    Role.CULT_LEADER,
    Role.FANATIC,
    Role.JOKER,
    Role.CITIZEN,
)

ROLE_TEAM_TEXT = {
    Role.MAFIA: "마피아팀",
    Role.SPY: "마피아팀 특수",
    Role.CONTRACTOR: "마피아팀 특수",
    Role.WITCH: "마피아팀 특수",
    Role.SCIENTIST: "마피아팀 특수",
    Role.MADAM: "마피아팀 특수",
    Role.GODFATHER: "마피아팀 특수",
    Role.CULT_LEADER: "교주팀",
    Role.FANATIC: "시민팀 특수",
    Role.JOKER: "중립",
    Role.CITIZEN: "시민팀",
    Role.DOCTOR: "시민팀",
    Role.NURSE: "시민팀 특수",
    Role.POLICE: "시민팀",
    Role.AGENT: "시민팀",
    Role.VIGILANTE: "시민팀",
    Role.DETECTIVE: "시민팀 특수",
    Role.SHAMAN: "시민팀 특수",
    Role.PRIEST: "시민팀 특수",
    Role.GRAVEROBBER: "시민팀 특수",
    Role.POLITICIAN: "시민팀 특수",
    Role.JUDGE: "시민팀 특수",
    Role.REPORTER: "시민팀 특수",
    Role.HACKER: "시민팀 특수",
    Role.TERRORIST: "시민팀 특수",
    Role.LOVER: "시민팀 특수",
    Role.SOLDIER: "시민팀 특수",
    Role.VILLAIN: "마피아팀",
}

ROLE_GOAL_TEXT = {
    Role.MAFIA: "시민을 줄여 생존 마피아 수가 나머지 생존자 수 이상이 되게 하세요.",
    Role.SPY: "접선으로 마피아팀에 합류하고, 정보를 모아 시민팀을 무너뜨리세요.",
    Role.CONTRACTOR: "정체를 알아낸 시민을 암살하고, 마피아와 접선해 팀에 합류하세요.",
    Role.WITCH: "저주로 플레이어를 개구리로 만들어 행동과 발언을 막고 마피아와 접선하세요.",
    Role.SCIENTIST: "죽음을 이용해 마피아팀과 접선하고 다음 밤 부활하세요.",
    Role.MADAM: "낮 투표를 이용해 상대를 유혹하고 능력과 발언을 봉쇄하세요.",
    Role.GODFATHER: "세 번째 밤 이후 마피아팀에 합류해 확정 처치로 판을 끝내세요.",
    Role.CULT_LEADER: "홀수날 밤마다 포교를 늘려 교주팀이 생존자의 절반 이상이 되게 하세요.",
    Role.FANATIC: "포교 전에는 시민팀으로 추리하고, 포교 후에는 교주팀의 승리를 돕습니다.",
    Role.JOKER: "낮 투표와 찬반투표를 거쳐 처형되면 단독 승리합니다.",
    Role.CITIZEN: "토론과 투표로 모든 마피아를 제거하세요.",
    Role.DOCTOR: "마피아의 밤 공격을 막아 시민팀 생존자를 지키세요.",
    Role.NURSE: "의사를 찾아 접선하고, 의사 사망 후에는 치료 능력을 이어받아 시민팀을 지키세요.",
    Role.POLICE: "조사 결과로 마피아를 찾아 시민팀의 투표 방향을 잡으세요.",
    Role.AGENT: "매일 밤 도착하는 지령으로 시민팀 직업 정보를 확보하세요.",
    Role.VIGILANTE: "확실하게 알아낸 마피아팀을 밤에 직접 처형하세요.",
    Role.DETECTIVE: "밤 행동의 이동 경로를 추적해 거짓말을 잡아내세요.",
    Role.SHAMAN: "사망자의 말을 듣고 성불로 숨은 직업 정보를 확보하세요.",
    Role.PRIEST: "죽은 시민을 되살리고 교주의 포교를 신앙으로 막아내세요.",
    Role.GRAVEROBBER: "첫날 밤 사망자의 직업을 이어받아 변수 역할을 맡습니다.",
    Role.POLITICIAN: "강한 투표권과 처형 면역으로 낮 토론을 시민팀 쪽으로 끌어오세요.",
    Role.JUDGE: "찬반투표의 최종 결정을 장악해 시민팀 처형 흐름을 통제하세요.",
    Role.REPORTER: "단 한 번의 특종으로 숨은 직업을 공개해 시민팀에 정보를 주세요.",
    Role.HACKER: "낮 해킹으로 정보를 얻고, 자신에게 오는 능력을 다른 대상에게 돌리세요.",
    Role.TERRORIST: "위험한 대상을 지정해 자신이 죽을 때 함께 데려가세요.",
    Role.LOVER: "다른 연인과 정보를 공유하고, 마피아 공격에서 서로를 지키세요.",
    Role.SOLDIER: "마피아의 첫 공격을 버텨 살아남고 시민팀에 단서를 남기세요.",
    Role.VILLAIN: "마피아팀 승리를 도우세요.",
}

ROLE_ABILITY_TEXTS = {
    Role.MAFIA: (
        ("처형", "밤마다 제거할 대상을 선택합니다. 일반 마피아 표가 과반을 넘으면 공격이 실행됩니다."),
        ("비밀 회의", "마피아 비밀방에서 접선된 마피아팀과 밤 전략을 조율합니다."),
    ),
    Role.POLICE: (
        ("수색", "밤마다 한 명을 조사해 마피아인지 아닌지 확인합니다."),
    ),
    Role.AGENT: (
        ("공작", "밤이 끝날 때 살아있는 시민팀 중 아직 알아내지 않았고 공개되지 않은 한 명의 직업을 무작위로 전달받습니다."),
    ),
    Role.VIGILANTE: (
        ("숙청 조사 [1회용]", "게임 중 한 번, 낮에 한 명을 선택해 밤이 시작될 때 마피아팀 여부를 확인합니다."),
        ("숙청 처형 [1회용]", "밤에 확실하게 알고 있는 마피아팀을 한 명 선택해 처형합니다. 성공/실패와 관계없이 한 번만 시도할 수 있습니다."),
    ),
    Role.DOCTOR: (
        ("치료", "밤마다 한 명을 선택합니다. 대상이 일반 마피아에게 공격받으면 사망을 막습니다."),
    ),
    Role.NURSE: (
        ("처방", "밤마다 한 명을 선택해 의사인지 확인합니다. 의사라면 접선하고 의사 비밀방에 합류합니다."),
        ("보조 치료", "의사와 접선한 상태에서 의사의 치료는 대부 처치 같은 부가 처치를 무시하고 성공합니다."),
        ("승계 치료", "접선 후 의사가 모두 사망하면 간호사가 밤마다 한 명을 치료할 수 있습니다."),
    ),
    Role.DETECTIVE: (
        ("추리", "밤마다 한 명을 조사해 그 사람이 누구에게 능력을 사용했는지 확인합니다."),
    ),
    Role.SHAMAN: (
        ("접신", "영매 채팅방을 볼 수 있고, 밤에는 사망자와 대화할 수 있습니다."),
        ("성불", "밤마다 사망자 한 명을 선택해 직업을 알아내고 사망자 채팅을 금지합니다."),
    ),
    Role.PRIEST: (
        ("소생 [1회용]", "밤에 죽은 플레이어 한 명을 선택해 부활시킵니다. 다음날 낮에 부활 안내가 공개됩니다. 한 번만 사용할 수 있습니다."),
        ("신앙", "교주팀에게 포교당하지 않습니다. 포교 시도를 받으면 교주가 누구인지 알 수 있습니다."),
    ),
    Role.GRAVEROBBER: (
        ("도굴", "첫 번째 밤에 마피아팀에게 살해당한 사람의 직업을 얻습니다."),
        ("약탈", "도굴에 성공하면 도굴당한 사람은 시민 또는 악인으로 바뀝니다."),
    ),
    Role.POLITICIAN: (
        ("처세", "투표와 찬반투표로 처형이 확정되어도 죽지 않고 정체가 공개됩니다."),
        ("논객", "정치인의 낮 투표와 찬반투표는 2표로 계산됩니다."),
    ),
    Role.JUDGE: (
        ("선고", "찬반투표 결과가 자신의 선택과 다르면 정체를 드러내고 판사의 선택대로 처형 여부를 결정합니다."),
        ("권위", "정체를 드러낸 뒤부터 살아있는 동안 찬반투표 결과는 판사의 선택으로만 결정됩니다."),
    ),
    Role.REPORTER: (
        ("특종 [1회용]", "밤에 한 명을 취재해 직업을 알아내고, 아침에 모든 플레이어에게 기사로 공개합니다. 한 번만 사용할 수 있습니다."),
        ("엠바고", "첫 번째 낮에는 기사를 낼 수 없어 두 번째 밤부터 특종을 사용할 수 있습니다."),
    ),
    Role.HACKER: (
        ("해킹 [1회용]", "낮에 한 명을 선택합니다. 밤이 시작될 때 대상의 직업을 DM으로 확인합니다. 한 번만 사용할 수 있습니다."),
        ("프록시", "해킹 사용 후 자신에게 사용되는 능력은 해킹한 대상에게 우회되어 적용됩니다."),
    ),
    Role.TERRORIST: (
        ("지목", "밤마다 한 명을 지정합니다. 매일 밤 새 대상으로 바꿀 수 있습니다."),
        ("자폭", "마피아팀에게 처형당할 때, 지목 대상이 마피아팀이면 함께 사망합니다."),
        ("산화", "투표로 처형될 때, 지목 대상이 시민팀이 아니면 함께 사망합니다."),
    ),
    Role.LOVER: (
        ("연애", "밤에 다른 연인과 연인 대화방에서 서로 대화할 수 있습니다."),
        ("희생", "두 연인이 모두 생존 중일 때 한 연인이 마피아에게 지목되면, 다른 연인이 대신 사망합니다."),
    ),
    Role.SOLDIER: (
        ("방탄 [1회용]", "일반 마피아의 처치 대상이 되면 한 차례 사망하지 않고 버팁니다."),
    ),
    Role.SPY: (
        ("첩보", "밤마다 한 명을 선택해 정확한 직업을 확인합니다."),
        ("접선", "선택한 대상이 일반 마피아라면 접선하고, 그 밤에 첩보를 한 번 더 사용할 수 있습니다."),
    ),
    Role.CONTRACTOR: (
        ("동업", "밤마다 한 명을 지목합니다. 대상이 일반 마피아라면 접선합니다."),
        ("청부", "두 번째 밤부터 수사직과 직업이 공개된 사람을 제외한 생존자 두 명과 각 직업을 추측합니다. 둘 다 맞히면 둘 다 암살합니다."),
    ),
    Role.WITCH: (
        ("저주", "밤마다 한 명을 지목합니다. 밤 10초 전부터 다음 밤이 될 때까지 대상을 개구리로 만듭니다."),
        ("접선", "저주 대상이 일반 마피아라면 마피아팀과 접선합니다. 대상 마피아도 그 밤에는 개구리가 되어 행동할 수 없습니다."),
    ),
    Role.SCIENTIST: (
        ("재생 [1회용]", "어떤 방식으로든 사망하면 바로 다음 밤 10초 전에 한 번 부활합니다."),
        ("유착", "사망하면 마피아팀과 접선해 밤에 마피아 채널에서 대화할 수 있습니다."),
    ),
    Role.MADAM: (
        ("유혹", "낮 지목 투표에서 마담에게 투표한 플레이어를 유혹합니다. 유혹된 대상은 능력을 사용할 수 없고 말을 할 수 없습니다."),
        ("접대", "유혹한 대상이 마피아팀이면 서로의 존재를 알아차리고 밤에 마피아 비밀방에서 대화할 수 있습니다."),
    ),
    Role.GODFATHER: (
        ("배후", "세 번째 밤이 시작되면 마피아팀과 자동으로 접선합니다."),
        ("말살", "접선 후 밤마다 한 명을 선택해 의사 치료와 관계없이 확정 처치합니다."),
    ),
    Role.CULT_LEADER: (
        ("포교", "홀수날 밤마다 마피아가 아닌 플레이어 한 명을 포교합니다. 성공하면 모두에게 종소리 안내가 나갑니다."),
        ("숭배", "포교한 대상의 직업을 알 수 있고, 교주팀 채팅방에서 밤마다 일방적으로 말을 전달합니다."),
    ),
    Role.FANATIC: (
        ("추종", "밤마다 한 명을 골라 교주팀인지 확인합니다. 대상이 교주라면 자신이 포교됩니다."),
        ("재림", "포교된 상태에서 교주가 사망하면 광신도가 교주 능력을 물려받습니다."),
    ),
    Role.JOKER: (
        ("광대극", "밤 행동은 없습니다. 낮 처형을 유도하는 것이 핵심입니다."),
    ),
    Role.CITIZEN: (
        ("추리", "밤 행동은 없습니다. 낮 토론과 투표만으로 마피아를 찾아야 합니다."),
    ),
    Role.VILLAIN: (
        ("악인", "도굴당한 마피아팀 직업이 바뀐 상태입니다."),
    ),
}

ROLE_RULE_TEXTS = {
    Role.MAFIA: (
        "의사가 같은 대상을 치료하면 일반 마피아의 공격은 실패합니다.",
        "마피아는 본인을 포함한 모든 생존자를 공격 대상으로 고를 수 있습니다.",
        "일반 마피아가 모두 죽고 접선 전 특수 마피아만 남으면 시민팀이 승리합니다.",
    ),
    Role.POLICE: (
        "경찰이 여러 명이면 같은 대상이 살아있는 경찰 과반을 넘어야 조사됩니다.",
        "접선 전 스파이, 청부업자, 대부는 마피아가 아니라고 표시됩니다. 접선 후부터 마피아로 표시됩니다.",
    ),
    Role.AGENT: (
        "경찰과 요원은 한 게임에 함께 등장하지 않습니다.",
        "공개적으로 직업이 드러난 사람과 이미 공작으로 알아낸 사람은 지령 대상에서 제외됩니다.",
        "더 이상 알아낼 대상이 없으면 지령이 도착하지 않았다는 DM을 받습니다.",
    ),
    Role.VIGILANTE: (
        "경찰, 요원, 자경단원은 한 게임에 함께 등장하지 않습니다.",
        "낮 조사는 1회용입니다. 조사 결과가 마피아팀이면 그 대상은 숙청 가능한 대상으로 기억됩니다.",
        "기자 특종 등으로 게임 채널에 마피아팀 직업이 공개된 대상도 숙청할 수 있습니다.",
        "숙청 처형을 한 번이라도 시도하면 이후 낮 조사도 다시 사용할 수 없습니다.",
    ),
    Role.DOCTOR: (
        "의사가 여러 명이면 같은 대상이 살아있는 의사 과반을 넘어야 치료됩니다.",
        "대부의 말살은 치료로 막을 수 없습니다.",
    ),
    Role.NURSE: (
        "간호사는 의사와 접선하기 전에는 의사 여부만 확인합니다.",
        "의사와 접선하면 의사의 치료가 대부 말살 같은 부가 처치를 무시하고 성공합니다.",
        "접선한 뒤 살아있는 의사가 없으면 간호사가 치료 역할을 대신합니다.",
    ),
    Role.DETECTIVE: (
        "시민, 조커처럼 밤 행동이 없는 대상은 사용 안함으로 표시됩니다.",
        "마피아, 의사, 경찰처럼 과반이 필요한 행동은 실제로 성립한 대상 기준으로 보입니다.",
    ),
    Role.SHAMAN: (
        "영매는 사망자 채팅방을 볼 수 없고, 별도 영매 채팅방만 볼 수 있습니다.",
        "영매가 말할 수 있는 시간은 밤뿐입니다.",
        "성불 대상은 사망자 채팅방과 영매 채팅방에서 더 이상 채팅할 수 없습니다.",
        "성불은 이미 죽은 참가자에게만 사용할 수 있습니다.",
    ),
    Role.PRIEST: (
        "소생은 1회용입니다. 대상이 성불 상태거나 소생하는 밤에 성직자가 사망하면 부활은 실패합니다.",
        "성직자가 포교 시도를 받으면 포교되지 않고 교주가 누구인지 비밀 메시지로 알게 됩니다.",
        "단, 마녀 저주로 개구리가 된 상태라면 포교될 수 있으며 저주가 풀려도 교주팀 상태는 유지됩니다.",
    ),
    Role.GRAVEROBBER: (
        "첫 번째 밤에 사망자가 없거나 치료로 살아나면 시민이 됩니다.",
        "마피아팀 직업을 도굴하면 본인도 마피아팀 판정이 될 수 있습니다.",
    ),
    Role.POLITICIAN: (
        "최후변론과 찬반투표는 다른 플레이어와 똑같이 진행됩니다.",
        "처형이 확정되는 순간 죽지 않고 정치인임이 공개된 뒤 밤으로 넘어갑니다.",
    ),
    Role.JUDGE: (
        "선고가 발동하면 공개 메시지로 판사가 투표 결과를 정했다고 안내됩니다.",
        "정체를 드러낸 판사가 살아있으면 마피아팀이 인원 승리 조건을 달성해도 게임이 즉시 끝나지 않습니다.",
    ),
    Role.REPORTER: (
        "특종은 1회용이며, 사용 안함을 선택하면 능력을 소모하지 않습니다.",
        "특종 대상의 직업은 게임 채널에 공개되며 이후 공개된 직업으로 취급됩니다.",
    ),
    Role.HACKER: (
        "해킹 선택은 낮 토론 중 DM으로 진행됩니다.",
        "프록시는 밤 능력에 적용됩니다. 투표와 찬반투표는 능력이 아니므로 우회되지 않습니다.",
        "해킹 대상이 사망하면 프록시는 더 이상 우회되지 않습니다.",
    ),
    Role.TERRORIST: (
        "지목 대상은 밤마다 다시 선택해야 하며, 마지막으로 선택한 대상이 적용됩니다.",
        "투표 처형 때는 시민팀이 아닌 직업을 지목해야 함께 사망합니다.",
    ),
    Role.LOVER: (
        "연인은 선택되면 반드시 두 명이 함께 배정됩니다.",
        "연인 대화방은 밤에만 열리고, 두 연인이 모두 살아있을 때만 사용할 수 있습니다.",
        "희생이 발동하면 공격받은 연인은 살아남고 다른 연인이 사망하며 공개 메시지가 출력됩니다.",
    ),
    Role.SOLDIER: (
        "방탄은 한 번만 발동하며, 발동하면 본인 DM으로만 알림이 갑니다.",
        "누가 공격했는지는 알 수 없습니다.",
        "의사가 치료해서 공격이 막힌 경우에는 방탄을 소모하지 않습니다.",
        "대부의 말살은 방탄으로 막지 못합니다.",
    ),
    Role.SPY: (
        "접선 전에는 마피아 비밀방을 볼 수 없고, 일반 마피아도 스파이를 모릅니다.",
        "접선 전에는 경찰 조사에서 마피아가 아니라고 나오며 생존 마피아 수에도 포함되지 않습니다.",
        "접선 후에는 마피아 비밀방에 들어가고 생존 마피아 수에 포함됩니다.",
    ),
    Role.CONTRACTOR: (
        "접선 전에는 마피아 비밀방을 볼 수 없고, 일반 마피아도 청부업자를 모릅니다.",
        "접선 전에는 경찰 조사에서 마피아가 아니라고 나오며 생존 마피아 수에도 포함되지 않습니다.",
        "청부 대상 둘 중 한 명이라도 직업이 틀리거나 시민팀이 아니면 암살은 실패합니다.",
        "경찰, 요원, 자경단원은 청부 대상으로 고를 수 없습니다.",
        "군인 방탄, 정치인 처세처럼 게임 채널에 직업이 공개된 사람은 청부 대상으로 고를 수 없습니다.",
    ),
    Role.WITCH: (
        "접선 전에는 마피아 비밀방을 볼 수 없고, 일반 마피아도 마녀를 모릅니다.",
        "접선 전에는 경찰 조사에서 마피아가 아니라고 나오며 생존 마피아 수에도 포함되지 않습니다.",
        "개구리가 된 대상은 밤 행동을 할 수 없고 게임 채널에서 말할 수 없습니다.",
        "개구리 상태에서 직업을 직접 확인하는 능력은 원래 직업 대신 개구리로 표시됩니다.",
    ),
    Role.SCIENTIST: (
        "접선 전에는 마피아 비밀방을 볼 수 없고, 일반 마피아도 과학자를 모릅니다.",
        "사망하면 마피아팀과 접선하고 부활 전까지 시민 승리 판정을 막습니다.",
        "부활하면 과학자임이 공개되고 다시 생존자로 돌아옵니다.",
    ),
    Role.MADAM: (
        "접선 전에는 마피아 비밀방을 볼 수 없고, 일반 마피아도 마담을 모릅니다.",
        "접선 전에는 경찰 조사에서 마피아가 아니라고 나오며 생존 마피아 수에도 포함되지 않습니다.",
        "유혹은 마담에게 투표한 생존자에게 적용됩니다. 스킵 투표나 다른 사람 투표에는 적용되지 않습니다.",
        "유혹 상태는 마담이 살아있는 동안 유지됩니다. 마담이 죽어도 최소 다음 낮까지는 유지됩니다.",
        "유혹된 마피아팀은 밤 능력을 사용할 수 있지만, 접대가 성립해 마피아 비밀방에 합류합니다.",
    ),
    Role.GODFATHER: (
        "세 번째 밤 전에는 마피아 비밀방을 볼 수 없고 밤 행동도 없습니다.",
        "접선 전에는 경찰 조사에서 마피아가 아니라고 나오며 생존 마피아 수에도 포함되지 않습니다.",
        "접선 후에는 마피아 비밀방에 들어가고 말살을 사용할 수 있습니다.",
    ),
    Role.CULT_LEADER: (
        "포교는 홀수날 밤에만 가능합니다.",
        "마피아팀을 포교하면 실패하고 공개 안내는 나가지 않습니다.",
        "교주팀 채팅방은 교주만 말할 수 있고 포교 대상은 볼 수만 있습니다.",
        "교주팀 생존자가 비교주팀 생존자 이상이 되면 교주팀이 승리합니다.",
    ),
    Role.FANATIC: (
        "처음에는 시민팀 특수로 취급됩니다.",
        "추종 대상이 교주라면 광신도 본인이 포교됩니다.",
        "포교된 상태에서 교주가 죽으면 살아있는 광신도 한 명이 교주가 됩니다.",
    ),
    Role.JOKER: (
        "밤에 사망하거나 다른 방식으로 사망하면 승리하지 못합니다.",
        "최후변론 뒤 찬반투표로 처형이 확정되어야 승리합니다.",
    ),
    Role.CITIZEN: (
        "확정 정보가 없으므로 발언, 투표 흐름, 밤 결과를 종합해 판단해야 합니다.",
    ),
}


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
            "청부는 두 번째 밤부터 사용할 수 있습니다.",
            "미접선 중에는 마피아 비밀방, 경찰 마피아 판정, 생존 마피아 수에 포함되지 않습니다.",
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


ROLE_GUIDE_COMMON_TEXT = (
    "**게임 흐름** 밤 행동 후 낮 토론, 투표, 최후변론, 찬반투표 순서로 진행됩니다.\n"
    "**과반 행동** 같은 역할이 여러 명이면 같은 대상이 생존 인원의 과반을 넘어야 능력이 발동합니다.\n"
    "**사망자 채팅** 사망자는 전용 채널에서 대화할 수 있고, 성불되면 채팅할 수 없습니다.\n"
    "**숨은 마피아 특수** 스파이, 청부업자, 마녀, 과학자, 대부는 접선 전까지 마피아 비밀방, 경찰 마피아 판정, 생존 마피아 수에 포함되지 않습니다.\n"
    "**교주팀** 설정이 켜져 있으면 교주와 광신도가 함께 배정됩니다. 포교 대상은 교주팀 채팅방을 볼 수만 있습니다."
)

MAFIA_TERM_ENTRIES: tuple[tuple[str, tuple[str, ...], str, str], ...] = (
    ("기본", ("n픽", "픽"), "플레이어 위치나 번호를 부르는 말입니다.", "3픽 조사 = 3번 플레이어를 조사"),
    ("기본", ("직공", "ㅈㄱ"), "자기 직업을 공개한다는 뜻입니다.", "경찰 직공 ㄱ"),
    ("기본", ("조결",), "경찰, 사립탐정 등 조사 역할의 결과입니다.", "2픽 노맢 조결"),
    ("기본", ("퍼블",), "첫 번째 밤에 마피아 공격으로 죽은 사람입니다.", "퍼블이 경찰이면 퍼경"),
    ("기본", ("퍼경", "경퍼"), "첫날 밤에 수사직이 죽은 상황입니다.", "경찰이 안 나오면 퍼경 가능성 체크"),
    ("기본", ("연퍼",), "이전 판에 이어 또 첫날 죽은 사람을 가리킵니다.", "방마다 매너 기준이 다를 수 있음"),
    ("기본", ("아봉",), "말을 거의 하지 않는 상태입니다. 잠수와 달리 투표나 능력은 할 수 있습니다.", ""),
    ("기본", ("홀직", "홀경", "홀의"), "그 직업을 주장하는 사람이 한 명뿐인 상황입니다.", "홀경 = 경찰 주장 1명"),
    ("기본", ("맞직", "맞경", "맞의"), "같은 직업을 주장하는 사람이 둘 이상인 상황입니다.", "맞경이면 둘 중 하나가 거짓일 가능성 큼"),
    ("기본", ("쓰리직", "쓰리경", "쓰리의"), "같은 직업 주장자가 세 명인 상황입니다.", "쓰리경이면 보조나 마피아가 섞였을 가능성 큼"),
    ("기본", ("늦직", "눈치직"), "다른 사람이 직업을 밝힌 뒤 늦게 같은 직업으로 나온 사람입니다.", "늦경은 의심을 받기 쉬움"),
    ("기본", ("진직", "진경", "진의"), "맞직 중 진짜 직업인 사람입니다.", "진경을 살려야 함"),
    ("기본", ("짭직", "구라직", "짭경", "구라경"), "맞직 중 가짜 직업인 사람입니다.", "짭의가 달림"),
    ("기본", ("확직", "확경", "확의"), "직업이나 시민성이 거의 확정된 사람입니다.", "확직이 오더를 잡음"),
    ("기본", ("반확",), "완전 확정은 아니지만 시민 가능성이 높은 사람입니다.", "홀경이 반확으로 오더"),
    ("기본", ("무직", "백수"), "능력을 쓸 수 없거나 쓸 일이 사라진 직업 상태입니다.", "도굴 실패 도굴꾼은 사실상 무직"),
    ("진영/직업", ("시팀",), "시민팀입니다.", "시팀은 마피아 제거가 목표"),
    ("진영/직업", ("맢팀", "마피아팀"), "마피아팀입니다.", "접선한 보조도 맢팀으로 봄"),
    ("진영/직업", ("교팀", "교주팀"), "교주, 광신도, 포교된 사람을 포함한 교주팀입니다.", ""),
    ("진영/직업", ("중직",), "중요 직업입니다. 보통 수사직과 의사를 말합니다.", "경찰/요원/자경단원, 의사"),
    ("진영/직업", ("특직", "특"), "중직을 제외한 시민팀 특수 직업입니다.", "기자, 영매, 군인 등"),
    ("진영/직업", ("보조",), "마피아팀 특수 직업입니다.", "스파이, 마녀, 청부업자 등"),
    ("진영/직업", ("보광교",), "마피아를 제외한 악인 후보를 묶어 부르는 말입니다.", "보조/광신도/교주"),
    ("진영/직업", ("맢", "ㅁ"), "마피아의 줄임말입니다.", "2맢 남음"),
    ("진영/직업", ("맢킬",), "마피아의 밤 공격입니다.", "맢킬 대상 예측"),
    ("진영/직업", ("홀맢",), "동료 마피아가 죽고 혼자 남은 마피아입니다.", ""),
    ("진영/직업", ("짝맢", "팀맢"), "같은 팀 마피아입니다.", ""),
    ("조사/판정", ("경크",), "경찰 조사에서 마피아라고 나온 결과입니다.", "3픽 경크"),
    ("조사/판정", ("노맢",), "경찰 조사에서 마피아가 아니라고 나온 결과입니다.", "4픽 노맢"),
    ("조사/판정", ("맞경조사", "맞조"), "맞경 상대를 조사하는 행동입니다. 보통 정보 가치가 낮아 의심받기 쉽습니다.", ""),
    ("조사/판정", ("시조", "시체조사"), "그날 죽은 사람을 조사했다고 주장하는 것입니다.", "거짓 조결로 의심받기 쉬움"),
    ("조사/판정", ("자조", "자기조사"), "자기 자신을 조사했다는 뜻입니다.", ""),
    ("조사/판정", ("특경크", "특크"), "특직 주장자에게 마피아 판정을 내는 것입니다.", ""),
    ("조사/판정", ("팀경크", "팀크"), "마피아팀끼리 일부러 서로를 마피아라고 몰아 신뢰를 얻으려는 전략입니다.", ""),
    ("조사/판정", ("찍경크", "찍크"), "확실한 근거 없이 첫날 아무나 마피아라고 찍는 전략입니다.", ""),
    ("조사/판정", ("팀노맢",), "마피아가 같은 팀에게 마피아가 아니라고 결과를 내는 전략입니다.", ""),
    ("조사/판정", ("루트", "룻"), "사립탐정 추적을 의식해 밤 행동 대상을 규칙적으로 바꾸는 경로입니다.", "루트온 = 정한 루트대로 움직임"),
    ("투표/진행", ("오더",), "투표나 행동 방향을 정해 지시하는 것입니다.", "확직 오더 따르기"),
    ("투표/진행", ("대립",), "두 주장이나 두 사람이 서로 맞서는 구도입니다.", "맞직도 대립의 한 종류"),
    ("투표/진행", ("교환", "x교"), "죽은 사람과 산 사람 중 한 명 이상이 마피아라고 보고 산 사람을 처형하는 판단입니다.", "경교, 의교"),
    ("투표/진행", ("맞투",), "두 사람이 서로에게만 투표하게 하는 방식입니다.", "나머지는 스킵/무투"),
    ("투표/진행", ("추미", "추리미스"), "시민팀을 잘못 의심해 처형하거나 판을 망친 판단입니다.", "추미 나면 사과하는 편이 좋음"),
    ("투표/진행", ("역추리",), "일반적인 흐름과 반대로 판단하는 추리입니다.", "근거 없이 남발하면 위험"),
    ("투표/진행", ("3:3", "3ㄷ3"), "생존 구도가 시민팀 3명 대 마피아팀 3명에 가까운 위험 상황입니다.", "보통 마피아가 매우 유리"),
    ("투표/진행", ("n투찬", "n투반"), "n픽을 지목한 뒤 찬성/반대를 누르라는 짧은 오더입니다.", "5투찬 = 5픽 올리고 찬성"),
    ("투표/진행", ("포커싱", "경포", "특포"), "특정 직업군 안에서 처형 대상을 찾자는 흐름입니다.", "경포 = 경찰 주장자 중 처형"),
    ("전략/상황", ("지정힐",), "의사 후보들이 서로 다른 대상을 치료하게 해 진위를 가리는 방식입니다.", "1은 3힐, 5는 2힐"),
    ("전략/상황", ("힐배", "킬배"), "의사의 치료 성공 여부로 승패가 갈리는 상황입니다.", ""),
    ("전략/상황", ("홀경작",), "마피아가 경찰을 안 나와 홀경을 만들고 퍼블 경찰을 주장하는 전략입니다.", ""),
    ("전략/상황", ("홀의작",), "마피아가 의사 대립을 피해서 홀의를 만들거나 역이용하는 전략입니다.", ""),
    ("전략/상황", ("역홀작",), "자경단원 등이 숨어 있다가 경찰 사칭 마피아를 노리는 역전 전략입니다.", ""),
    ("전략/상황", ("위장",), "자기 직업이 아닌 다른 직업처럼 행동하는 것입니다.", "군인 위장, 기자 위장"),
    ("전략/상황", ("위칸",), "위장한 사람들이 동시에 진짜 직업을 밝히도록 카운트하는 것입니다.", ""),
    ("전략/상황", ("룻칸",), "루트 공개를 동시에 맞추기 위해 카운트하는 것입니다.", ""),
    ("전략/상황", ("노살",), "성직자가 특정 사망자를 살리지 말라는 의미로 쓰입니다.", "맞직을 살리지 말라는 오더"),
    ("전략/상황", ("고의시조", "고시"), "일부러 시체 조사 결과를 내는 전략입니다. 리스크가 큽니다.", ""),
    ("전략/상황", ("짜치",), "마피아팀이 미리 말을 맞춰 속이는 행동입니다.", ""),
    ("전략/상황", ("올직공",), "전원이 직업을 공개하는 진행입니다.", "청부 위험이 없을 때 고려"),
    ("기본", ("풍지",), "12인 방 등에서 직공을 뜻하는 말입니다.", "3풍지 = 3픽 직공"),
    ("기본", ("방매", "ㅂㅁ"), "방장이나 특정 참가자를 초반에 죽이거나 조사하지 말자는 매너 룰입니다.", ""),
    ("기본", ("노연퍼", "ㄴㅇㅍ"), "연속 퍼블 대상이 없는 상태입니다.", ""),
    ("기본", ("노연퍼고정", "노연고", "ㄴㅇㅍㄱㅈ", "ㄴㅇㄱ"), "연퍼를 챙기지 않기로 고정한다는 뜻입니다.", ""),
    ("기본", ("고퍼",), "특정 사람을 일부러 첫밤에 죽여달라는 고정 퍼블입니다.", ""),
    ("기본", ("자투", "ㅈㅌ"), "자기 자신에게 투표하는 것입니다. 하루를 넘기거나 인증용으로 쓰입니다.", ""),
    ("기본", ("무투", "ㅁㅌ"), "아무에게도 투표하지 않는 것입니다.", ""),
    ("기본", ("시무", "ㅅㅁ"), "시간 단축 후 무투표로 넘기자는 말입니다.", ""),
    ("기본", ("맢표",), "마피아팀이 몰래 던진 것으로 보이는 표입니다.", "처형 흐름과 다른 곳에 갑자기 표가 생김"),
    ("기본", ("몰투", "몰표"), "한 사람에게 표가 몰리는 상황입니다.", ""),
    ("기본", ("투갈", "표갈"), "투표가 갈려 최다 득표자가 여러 명이 되는 상황입니다.", ""),
    ("기본", ("물타기",), "뚜렷한 근거 없이 남의 투표 흐름에 따라가는 행동입니다.", ""),
    ("기본", ("잠수",), "말과 행동을 거의 하지 않는 상태나 그런 사람입니다.", ""),
    ("기본", ("묵언수행",), "채팅은 하지 않지만 투표나 능력은 사용하는 상태입니다.", ""),
    ("기본", ("시단", "ㅅㄷ"), "낮 시간을 줄이는 행동입니다.", ""),
    ("기본", ("시증", "ㅅㅈ"), "낮 시간을 늘리는 행동입니다.", ""),
    ("기본", ("칼시단",), "낮이 되자마자 시간을 줄이는 행동입니다.", ""),
    ("기본", ("늦시단",), "낮 시간이 어느 정도 지난 뒤 시간을 줄이는 행동입니다.", ""),
    ("기본", ("시단플",), "충분한 토론 없이 시간을 줄이는 플레이를 말합니다.", ""),
    ("기본", ("조밤",), "밤에 아무도 죽지 않거나 큰 결과가 없는 조용한 밤입니다.", ""),
    ("기본", ("고의조밤", "고조"), "마피아가 일부러 처형을 성립시키지 않아 만든 조밤입니다.", ""),
    ("기본", ("교밤",), "교주가 포교할 수 있는 밤입니다. 보통 홀수날 밤을 말합니다.", ""),
    ("기본", ("교종",), "교주의 포교 성공 안내, 즉 종소리 메시지를 말합니다.", ""),
    ("기본", ("물총",), "마피아 공격이 실패하거나 계속 빗나간 상황입니다.", ""),
    ("기본", ("자총",), "마피아가 자기 자신이나 팀을 죽이는 선택입니다.", ""),
    ("기본", ("도도",), "도굴꾼에게 특정 직업을 넘기려는 도굴 도박입니다.", ""),
    ("기본", ("밤챗",), "밤에만 가능한 비밀 대화입니다.", "마피아, 연인, 영매, 교주팀 등"),
    ("진영/직업", ("또맢", "연맢"), "전판에 이어 또 마피아가 된 상황입니다.", ""),
    ("진영/직업", ("은폐",), "원작 듀얼 능력 이름입니다. 마피아 관련 표현으로도 쓰입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("위선",), "원작 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("승부수", "승수"), "원작 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("수습",), "원작 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("퇴마",), "원작 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("무법", "무법자"), "원작 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("nㄱㅋ",), "n픽을 광클했다는 뜻입니다. 경크와 다릅니다.", "연인/원작 표현"),
    ("진영/직업", ("슾",), "스파이의 줄임말입니다.", ""),
    ("진영/직업", ("n긁슾",), "n픽을 조사한 스파이로 보인다는 말입니다.", ""),
    ("진영/직업", ("첫접슾",), "첫날 밤에 마피아를 찾아 바로 접선한 스파이입니다.", ""),
    ("진영/직업", ("자객",), "원작 스파이 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("미인계",), "원작 스파이 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("슾크",), "스파이가 교주팀 쪽 정보를 찾아 폭로하는 상황입니다.", "교주 모드 참고"),
    ("진영/직업", ("n접",), "n픽 마피아와 접선했다는 말입니다.", ""),
    ("진영/직업", ("짐인", "짐"), "원작 짐승인간의 줄임말입니다.", "원작 역할 참고"),
    ("진영/직업", ("짐인킬",), "짐승인간의 처치로 사망한 상황입니다.", "원작 역할 참고"),
    ("진영/직업", ("짐인판", "ㅈㅇㅍ"), "마피아가 짐승인간을 공격해 조밤이 난 판입니다.", "원작 역할 참고"),
    ("진영/직업", ("포효",), "원작 짐승인간 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("야만", "야만성"), "원작 짐승인간 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("n먹",), "n픽을 이용해 접선하자는 말입니다.", "원작 짐승인간 표현"),
    ("진영/직업", ("마담판",), "마담이 존재하거나 유혹을 받았음을 알리는 말입니다.", ""),
    ("진영/직업", ("현혹",), "원작 마담 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("데뷔",), "원작 마담 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("후계", "후계자"), "원작 도둑 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("조문",), "원작 도둑 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("개굴",), "마녀 저주로 개구리가 된 상태입니다.", ""),
    ("진영/직업", ("망마", "망각술"), "원작 망각술 능력을 가진 마녀를 말합니다.", "원작/듀얼 참고"),
    ("진영/직업", ("과자", "학자", "곽자"), "과학자의 줄임말입니다.", ""),
    ("진영/직업", ("과그로",), "과학자가 일부러 어그로를 끄는 행동입니다.", ""),
    ("진영/직업", ("분석", "최면"), "원작 과학자 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("분석투", "최면투"), "분석/최면 능력과 관련된 투표 표현입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("사기", "기꾼"), "원작 사기꾼의 줄임말입니다.", "원작 역할 참고"),
    ("진영/직업", ("청부", "ㅊㅂ"), "청부업자의 줄임말입니다.", ""),
    ("진영/직업", ("청부킬", "암살", "썰다"), "청부업자가 능력으로 대상을 제거하는 것입니다.", ""),
    ("진영/직업", ("직감",), "원작 청부업자 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("조사/판정", ("n노맢", "nㄴㅁ"), "n픽이 마피아가 아니라는 조사 결과입니다.", ""),
    ("조사/판정", ("n맢", "nㅁ"), "n픽이 마피아라는 조사 결과입니다.", ""),
    ("조사/판정", ("체나조사", "ㅊㄴ조사"), "나이트 말 움직임처럼 대상을 골라 조사했다는 표현입니다.", "원작 표현"),
    ("조사/판정", ("영장",), "원작 경찰 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("조사/판정", ("기밀",), "원작 경찰 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("조사/판정", ("도청",), "원작 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("조사/판정", ("부검",), "원작 경찰 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("조사/판정", ("랜조", "랜"), "기밀 능력 등으로 나온 랜덤 조사 결과입니다.", "원작/듀얼 참고"),
    ("조사/판정", ("직조", "직"), "랜덤이 아니라 직접 고른 조사 결과입니다.", ""),
    ("조사/판정", ("탐크",), "사립탐정 추적으로 마피아팀 단서를 잡은 상황입니다.", ""),
    ("조사/판정", ("해크",), "해커가 악인 쪽 직업을 알아낸 상황입니다.", ""),
    ("조사/판정", ("성크",), "성직자가 교주팀의 포교 시도를 받아 교주 정보를 얻은 상황입니다.", ""),
    ("조사/판정", ("광크",), "광신도가 마피아를 확인한 상황입니다.", "교주/원작 표현"),
    ("조사/판정", ("시체경크",), "죽은 사람에게 마피아 판정을 냈다는 주장입니다.", ""),
    ("조사/판정", ("보조경크", "보조경"), "마피아가 보조직업에게 마피아 판정을 낸 상황입니다.", ""),
    ("조사/판정", ("맞경노맢",), "맞경 중 한 명이 상대 맞경을 노맢으로 낸 상황입니다.", ""),
    ("진영/직업", ("자경",), "자경단원의 줄임말입니다.", ""),
    ("진영/직업", ("노손자경", "ㄴㅅㅈㄱ"), "첫날 능력을 쓰지 않은 자경단원입니다.", ""),
    ("진영/직업", ("n손자경", "nㅅㅈㄱ"), "n픽에게 능력을 쓴 자경단원입니다.", ""),
    ("진영/직업", ("n탕자경", "nㅌㅈㄱ"), "n픽에게 숙청/처형 능력을 쓴 자경단원입니다.", ""),
    ("진영/직업", ("캔디자경",), "4픽에게 능력을 쓴 자경단원을 장난스럽게 부르는 말입니다.", ""),
    ("진영/직업", ("자경킬",), "자경단원의 처형으로 사망한 상황입니다.", ""),
    ("진영/직업", ("결사", "결"), "원작 자경단원 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("의",), "의사의 줄임말입니다.", ""),
    ("진영/직업", ("자힐", "ㅈㅎ"), "의사가 자기 자신을 치료하는 것입니다.", ""),
    ("진영/직업", ("타힐", "ㅌㅎ"), "의사가 다른 사람을 치료하는 것입니다.", ""),
    ("진영/직업", ("센힐", "눈힐"), "의사가 눈치 있게 중요한 대상을 치료하는 것입니다.", ""),
    ("진영/직업", ("연퍼타힐", "ㅇㅍㅌㅎ"), "연퍼라서 다른 사람을 치료했다는 의사 주장입니다.", ""),
    ("진영/직업", ("검진타힐", "ㄱㅈㅌㅎ"), "검진 능력 때문에 타힐했다는 원작 의사 표현입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("n접의",), "n픽 간호사와 접선한 의사입니다.", ""),
    ("진영/직업", ("힐룻온", "ㅎㄹㅇ"), "사립탐정 추적을 의식해 치료 루트를 돌렸다는 말입니다.", ""),
    ("진영/직업", ("검진",), "원작 의사 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("박애",), "원작 의사 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("진정",), "원작 의사 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("군", "진군"), "군인 또는 진짜 군인을 뜻합니다.", ""),
    ("진영/직업", ("위군", "ㅇㄱ"), "군인인 척 위장하는 특직 표현입니다.", ""),
    ("진영/직업", ("군크",), "군인이 보조직업 등 단서를 잡은 상황입니다.", ""),
    ("진영/직업", ("군그로",), "군인이 방탄을 유도하려고 어그로를 끄는 행동입니다.", ""),
    ("진영/직업", ("정신", "정신력"), "원작 군인 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("불굴", "불"), "원작 군인 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("정", "정치"), "정치인의 줄임말입니다.", ""),
    ("진영/직업", ("정치인증", "정인"), "정치인의 투표 면역을 발동시켜 정치인임을 인증하는 것입니다.", ""),
    ("진영/직업", ("자투정인",), "자기투표로 정치인증을 하려는 행동입니다.", ""),
    ("진영/직업", ("독정",), "원작 독재 능력을 가진 정치인을 뜻합니다.", "원작/듀얼 참고"),
    ("진영/직업", ("영",), "영매 또는 원작 경찰 듀얼 능력 이름으로 쓰입니다. 문맥을 봐야 합니다.", ""),
    ("진영/직업", ("영매퍼직공", "영퍼직", "ㅇㅁㅍㅈㄱ"), "영매가 첫날 사망자의 직업을 묻거나 알리는 표현입니다.", ""),
    ("진영/직업", ("성결",), "영매 성불 결과입니다.", ""),
    ("진영/직업", ("칼성",), "밤이 되자마자 성불하는 것입니다.", ""),
    ("진영/직업", ("강령",), "원작 영매 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("연",), "연인의 줄임말입니다.", ""),
    ("진영/직업", ("연그로",), "연인이 일부러 다른 직업처럼 보이며 어그로를 끄는 행동입니다.", ""),
    ("진영/직업", ("암호",), "연인끼리 밤에 정해 낮에 서로를 증명하는 말입니다.", ""),
    ("진영/직업", ("원한",), "원작 연인 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("헌신",), "원작 연인 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("건", "건달"), "원작 건달의 줄임말입니다.", "원작 역할 참고"),
    ("진영/직업", ("무협", "노협", "무협건", "ㅁㅎㄱ"), "건달이 협박을 하지 않았다는 말입니다.", "원작 역할 참고"),
    ("진영/직업", ("첫협",), "첫날에 협박을 사용한 건달입니다.", "원작 역할 참고"),
    ("진영/직업", ("n협",), "n픽을 협박했다는 말입니다.", "원작 역할 참고"),
    ("진영/직업", ("갈취",), "원작 건달 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("길동무",), "원작 건달 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("기",), "기자의 줄임말입니다.", ""),
    ("진영/직업", ("취실",), "취재 대상 사망 등으로 특종이 실패한 상황입니다.", ""),
    ("진영/직업", ("속보",), "기자의 특종 공개를 말합니다.", ""),
    ("진영/직업", ("속기",), "원작 속보 능력 기자를 말합니다.", "원작/듀얼 참고"),
    ("진영/직업", ("부고",), "원작 기자 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("셀카",), "원작 기자 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("자찍",), "기자가 자기 자신을 취재하는 것입니다.", ""),
    ("진영/직업", ("기레기",), "정보 가치가 낮거나 불리한 취재를 한 기자를 비꼬는 말입니다.", ""),
    ("진영/직업", ("탐", "사탐"), "사립탐정의 줄임말입니다.", ""),
    ("진영/직업", ("n손m", "nㅅm"), "n픽이 m픽에게 능력을 사용했다는 탐정 결과입니다.", ""),
    ("진영/직업", ("n노손", "nㄴㅅ"), "n픽이 능력을 사용하지 않았다는 탐정 결과입니다.", ""),
    ("진영/직업", ("함정",), "원작 사립탐정 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("도굴무직", "도무", "ㄷㅁ"), "도굴꾼이 직업을 얻지 못한 상태입니다.", ""),
    ("진영/직업", ("도굴OO", "도O"), "도굴꾼이 이어받은 직업을 알리는 표현입니다.", "도경 = 경찰을 도굴"),
    ("진영/직업", ("계승",), "원작 도굴꾼 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("망령",), "원작 도굴꾼 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("테러", "ㅌㄹ"), "테러리스트의 줄임말입니다.", ""),
    ("진영/직업", ("n손테", "nㅅㅌ"), "테러리스트가 n픽을 지목했다는 말입니다.", ""),
    ("진영/직업", ("테펑",), "테러리스트 능력이 터져 함께 죽는 상황입니다.", ""),
    ("진영/직업", ("유폭",), "원작 테러리스트 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("섬광",), "원작 테러리스트 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("성직", "ㅅㅈ"), "성직자의 줄임말입니다.", ""),
    ("진영/직업", ("부실",), "부활 실패입니다.", "성불 대상 소생 실패 등"),
    ("진영/직업", ("구마",), "원작 성직자 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("구희", "구마희생"), "구마/희생 능력 조합을 줄여 부르는 말입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("술사", "마술"), "원작 마술사의 줄임말입니다.", "원작 역할 참고"),
    ("진영/직업", ("노트릭술사", "노트술", "ㄴㅌㄹㅅㅅ"), "아직 트릭을 걸지 않은 마술사입니다.", "원작 역할 참고"),
    ("진영/직업", ("n트릭", "n트술", "nㅌㅅ"), "n픽에게 트릭을 걸었다는 말입니다.", "원작 역할 참고"),
    ("진영/직업", ("트인",), "마술사 트릭 인증입니다.", "원작 역할 참고"),
    ("진영/직업", ("조수",), "원작 마술사 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("예자", "예언"), "원작 예언자의 줄임말입니다.", "원작 역할 참고"),
    ("진영/직업", ("도선예",), "도주/선각 능력 예언자를 줄여 부르는 말입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("판",), "판사의 줄임말입니다.", ""),
    ("진영/직업", ("판인",), "판사의 투표 판정으로 정체를 인증하는 것입니다.", ""),
    ("진영/직업", ("관권", "관판"), "원작 판사 듀얼 능력 또는 그 능력을 가진 판사입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("간호", "가노", "간", "ㄱㅎ"), "간호사의 줄임말입니다.", ""),
    ("진영/직업", ("n노의사", "nㄴㅇㅅ"), "n픽은 의사가 아니라는 간호사 결과입니다.", ""),
    ("진영/직업", ("n접간",), "n픽 의사와 접선한 간호사입니다.", ""),
    ("진영/직업", ("검시", "검간"), "원작 간호사 듀얼 능력 또는 그 능력을 가진 간호사입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("해", "햌"), "해커의 줄임말입니다.", ""),
    ("진영/직업", ("n해킹", "n햌"), "n픽을 해킹했다는 말입니다.", ""),
    ("진영/직업", ("해결",), "해킹 결과입니다.", ""),
    ("진영/직업", ("프록", "노프록"), "해커 프록시가 확인되었거나 없다는 말입니다.", ""),
    ("진영/직업", ("심리", "심"), "원작 심리학자의 줄임말입니다.", "원작 역할 참고"),
    ("진영/직업", ("nm같팀", "nm같"), "n픽과 m픽이 같은 팀이라는 결과입니다.", "원작 심리학자 참고"),
    ("진영/직업", ("nm다팀", "nmㄷㅌ"), "n픽과 m픽이 다른 팀이라는 결과입니다.", "원작 심리학자 참고"),
    ("진영/직업", ("프파",), "원작 프로파일링 능력을 가진 심리학자입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("n의뢰", "nㅇㄹ"), "n픽이 의뢰자라는 용병 표현입니다.", "원작 역할 참고"),
    ("진영/직업", ("홀수의뢰", "짝수의뢰", "홀의뢰", "짝의뢰"), "의뢰자가 홀수/짝수 픽에 있다는 용병 표현입니다.", "원작 역할 참고"),
    ("진영/직업", ("공무", "공"), "원작 공무원의 줄임말입니다.", "원작 역할 참고"),
    ("진영/직업", ("○판", "노○판"), "특정 직업이 있거나 없는 판을 말합니다.", "마담판, 노교판 등"),
    ("진영/직업", ("색출", "색공"), "원작 공무원 듀얼 능력 또는 그 능력을 가진 공무원입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("감사",), "원작 공무원 듀얼 능력 이름입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("비결",), "원작 비밀결사의 줄임말입니다.", "원작 역할 참고"),
    ("진영/직업", ("낮비결", "밤비결"), "낮/밤 비밀결사를 구분하는 표현입니다.", "원작 역할 참고"),
    ("진영/직업", ("파파",), "원작 파파라치의 줄임말입니다.", "원작 역할 참고"),
    ("진영/직업", ("노이슈", "노공유", "노정보"), "밤에 공유받은 정보가 없다는 파파라치 표현입니다.", "원작 역할 참고"),
    ("진영/직업", ("이슈옴", "공유옴"), "밤에 정보가 왔다는 파파라치 표현입니다.", "원작 역할 참고"),
    ("진영/직업", ("n이슈", "n직공유"), "n픽 관련 정보를 공유받았다는 말입니다.", "원작 역할 참고"),
    ("진영/직업", ("n초 이슈",), "몇 초에 이슈가 왔는지까지 말하는 파파라치 표현입니다.", "원작 역할 참고"),
    ("진영/직업", ("눈치파파", "눈파"), "눈치를 보다가 나온 파파라치라는 말입니다.", "원작 역할 참고"),
    ("진영/직업", ("교",), "교주의 줄임말입니다.", ""),
    ("진영/직업", ("광접교", "접교"), "광신도와 접선한 교주입니다.", ""),
    ("진영/직업", ("설파",), "원작 설파 능력을 가진 교주 또는 그로 인한 종소리입니다.", "원작/듀얼 참고"),
    ("진영/직업", ("광", "광신", "신도", "팡"), "광신도의 줄임말입니다.", ""),
    ("진영/직업", ("광접",), "광신도가 교주와 접선한 상태입니다.", ""),
    ("투표/진행", ("어필",), "자신이 시민팀임을 설득하는 발언과 행동입니다.", ""),
    ("투표/진행", ("판읽기",), "대립, 투표, 밤 결과를 보고 판 구도를 읽는 추리입니다.", ""),
    ("투표/진행", ("시민티", "마피아티", "맢티"), "발언이나 행동에서 시민/마피아처럼 보이는 느낌입니다.", ""),
    ("투표/진행", ("직멘", "직공멘트"), "직업을 공개할 때 쓰는 설명 문장입니다.", ""),
    ("투표/진행", ("고의대립", "고대"), "마피아팀끼리 일부러 대립을 만드는 전략입니다.", ""),
    ("투표/진행", ("팀구도",), "여러 명이 한 편처럼 묶여 보이는 구도입니다.", ""),
    ("투표/진행", ("노확유유",), "퍼블에게 확성/유언/유품 같은 공개 단서가 없는 상황입니다.", "원작 듀얼 참고"),
    ("투표/진행", ("특경", "특경크"), "특직 주장자에게 마피아 판정을 내는 것입니다.", ""),
    ("투표/진행", ("이중위장",), "위장 직업을 다시 다른 직업으로 푸는 복합 위장입니다.", ""),
    ("투표/진행", ("모밀나", "모밀N"), "모든 밀서는 나/N픽에게 보내라는 뜻입니다.", "원작 듀얼 참고"),
    ("사장/원작", ("돌림투", "돌투"), "예전 메타에서 표 없는 사람을 찾기 위해 투표를 돌리던 방식입니다.", ""),
    ("사장/원작", ("n초 자투",), "정해진 초에 동시에 자투하던 예전 보조 판별 방식입니다.", ""),
    ("사장/원작", ("연크",), "예전 연인 능력으로 마피아를 알아냈다는 표현입니다.", ""),
    ("사장/원작", ("연인퍼블", "연퍼(연인)"), "연인 희생 관련 첫밤 사망 표현입니다.", ""),
    ("사장/원작", ("특손",), "특직이 직공 대신 손을 들어 존재만 알리던 예전 문화입니다.", ""),
    ("사장/원작", ("투인", "퉆인"), "투표 순서나 투표 사실을 인증하던 예전 방식입니다.", ""),
    ("사장/원작", ("픽자", "역픽자"), "픽 순서대로 자투하던 예전 방식입니다.", ""),
    ("사장/원작", ("도둑고려", "도고", "도고시증"), "도둑을 의식해 채팅을 피하던 예전 메타입니다.", ""),
    ("사장/원작", ("광기작",), "확승 상황에서 보상을 위해 일부러 게임을 끌던 예전 플레이입니다.", ""),
    ("사장/원작", ("종전", "종후", "교종전", "교종후"), "교주 종소리 전/후를 구분하던 예전 심리학자 표현입니다.", ""),
    ("사장/원작", ("지령도도",), "예전 지령 정보를 보고 도굴 도박을 하던 전략입니다.", ""),
    ("플레이/매너", ("조결충",), "조사 결과만 보고 어필과 판읽기를 거의 보지 않는 사람을 비꼬는 말입니다.", ""),
    ("플레이/매너", ("보험충",), "자기 판단 없이 남에게 책임을 넘기려는 사람을 비꼬는 말입니다.", ""),
    ("플레이/매너", ("시단충",), "충분히 보지 않고 바로 시간을 줄이는 사람을 비꼬는 말입니다.", ""),
    ("플레이/매너", ("자힐충",), "상황과 무관하게 자기 치료만 고집하는 의사를 비꼬는 말입니다.", ""),
    ("플레이/매너", ("더티플",), "게임 외 요소나 비매너 협박으로 판을 흔드는 플레이입니다.", ""),
    ("플레이/매너", ("톡플",), "외부 메신저로 정보를 공유하는 부정 플레이입니다.", ""),
    ("플레이/매너", ("친플",), "친분을 이용해 게임 정보를 공유하거나 편을 드는 플레이입니다.", ""),
    ("플레이/매너", ("투폰",), "여러 기기나 계정으로 같은 판에 들어오는 부정 플레이입니다.", ""),
    ("플레이/매너", ("감플", "감정플"), "감정 때문에 게임 판단을 망치는 플레이입니다.", ""),
    ("플레이/매너", ("욕플",), "욕설 위주로 진행하는 플레이입니다.", ""),
    ("플레이/매너", ("엽플", "엽서플"), "엽서나 보상 등을 걸고 시민성을 주장하는 비매너 플레이입니다.", ""),
    ("플레이/매너", ("아봉플",), "말은 안 하지만 투표와 능력은 하는 플레이입니다.", ""),
    ("플레이/매너", ("찍플",), "근거 없이 찍어서 몰아가는 플레이입니다.", ""),
    ("플레이/매너", ("룰렛플", "사다리플"), "추리 대신 무작위 방식으로 처형 대상을 정하는 플레이입니다.", ""),
    ("플레이/매너", ("스킨플",), "스킨 정보를 근거로 시민/마피아를 판단하려는 플레이입니다.", ""),
    ("플레이/매너", ("스킨묘사플",), "자기 스킨을 묘사해 직업을 인증하려는 플레이입니다.", ""),
    ("플레이/매너", ("카드플", "덱플"), "덱이나 카드 구성을 근거로 추리하는 플레이입니다.", "원작 듀얼 참고"),
    ("플레이/매너", ("이모티콘플",), "이모티콘 반응 속도나 종류로 시민성을 주장하는 플레이입니다.", ""),
    ("플레이/매너", ("초성퀴즈", "초퀴"), "초성으로 정보를 숨겨 인증하려는 플레이입니다.", ""),
    ("플레이/매너", ("걸기플", "~걸기플"), "현실 물건이나 조건을 걸고 결백을 주장하는 비매너 표현입니다.", ""),
    ("플레이/매너", ("티어플",), "카드 티어나 숙련도를 근거로 믿어달라고 하는 플레이입니다.", "원작 듀얼 참고"),
    ("플레이/매너", ("보이스플",), "직업별 보이스 대사를 근거로 직업을 주장하는 플레이입니다.", "원작 참고"),
    ("플레이/매너", ("보석플",), "착용 보석을 근거로 직업을 추리하는 플레이입니다.", "원작 참고"),
    ("플레이/매너", ("마명",), "마이너스 명성을 뜻합니다. 보통 신뢰도가 낮은 유저로 취급됩니다.", "원작 시스템 참고"),
    ("아웃게임", ("자리", "ㅈㄹ"), "곧 들어올 사람이 있으니 자리를 비워달라는 방 밖 용어입니다.", ""),
    ("아웃게임", ("자첸",), "자리 체인지, 즉 자리 교체 요청입니다.", ""),
    ("아웃게임", ("○엽", "엽"), "엽서 아이템을 줄여 부르는 말입니다.", "고엽, 일엽 등"),
    ("아웃게임", ("엽교",), "엽서를 서로 교환하는 것입니다.", ""),
    ("아웃게임", ("무반",), "받은 엽서와 같은 종류를 무한 반사하겠다는 뜻입니다.", ""),
    ("아웃게임", ("우꽉",), "우체통이 꽉 찬 상태입니다.", ""),
    ("아웃게임", ("같종",), "같은 종류의 엽서가 이미 남아 있는 상태입니다.", ""),
    ("아웃게임", ("같쳌",), "같종 상태를 확인해 달라는 말입니다.", ""),
    ("아웃게임", ("같케",), "같종 상태를 다른 방식으로 케어해준다는 말입니다.", ""),
    ("아웃게임", ("같우대",), "같종, 우꽉, 대리 관련 조건을 묶어 부르는 말입니다.", ""),
    ("아웃게임", ("회재", "맞회재"), "엽서를 회수하고 다시 보내는 것입니다.", ""),
    ("아웃게임", ("받나",), "보상을 받고 방을 나가라는 뜻입니다.", ""),
    ("아웃게임", ("일괄",), "엽서 등을 한 번에 일괄 반사하겠다는 뜻입니다.", ""),
    ("아웃게임", ("큪", "일큪", "황큪"), "큐피트 아이템이나 커플 요청을 뜻합니다.", ""),
    ("아웃게임", ("획초",), "하루 획득량 초과 상태나 이를 노리는 방을 말합니다.", ""),
    ("아웃게임", ("접", "접선"), "친구 추가를 뜻하는 아웃게임 표현입니다. 인게임 접선과 문맥을 구분해야 합니다.", ""),
    ("아웃게임", ("접메", "접챗"), "친구끼리 할 수 있는 채팅을 말합니다.", ""),
    ("아웃게임", ("강퇴", "킥", "ㅋ"), "방에서 강제로 내보내는 것입니다.", ""),
    ("아웃게임", ("자킥", "ㅈㅋ"), "방장이 오래 시작하지 않아 자동으로 강퇴되는 상태입니다.", ""),
    ("아웃게임", ("방장잠수", "방잠"), "방장이 잠수한 상태입니다.", ""),
    ("아웃게임", ("경징", "경징낡", "경징권"), "경고/징벌류 마이너스 엽서를 묶어 부르는 말입니다.", ""),
    ("아웃게임", ("교류",), "엽서나 대리 목적의 최근 교류 조건입니다.", ""),
    ("아웃게임", ("명테",), "명성 테러입니다. 마이너스 엽서를 대량으로 보내는 행위입니다.", ""),
    ("아웃게임", ("경A징B",), "경고 엽서와 징벌 엽서에 각각 다른 보상을 주는 방제 표현입니다.", ""),
    ("아웃게임", ("옵패",), "패배 수가 승리 수보다 많은 전적 상태입니다.", ""),
    ("아웃게임", ("물컬",), "기본 물음표 컬렉션 상태를 말합니다.", ""),
    ("아웃게임", ("0승0패", "00"), "승패가 없는 관상용 또는 새 계정을 말합니다.", ""),
    ("아웃게임", ("출보", "접보", "길보"), "출석/접속/길드 보상을 줄여 부르는 말입니다.", ""),
)

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


def role_guide_text(game: MafiaGame | None = None) -> str:
    role_sections = "\n\n".join(f"{role_name}\n{guide}" for role_name, guide in ROLE_GUIDE_SECTIONS)
    return f"공통 판정\n{ROLE_GUIDE_COMMON_TEXT}\n\n{role_sections}"


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

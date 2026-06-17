from __future__ import annotations

import asyncio
from contextlib import suppress
from dataclasses import fields
import json
import math
import os
from pathlib import Path
import secrets
import sys
import time

import discord
from discord import app_commands
from discord.ext import commands
from dotenv import load_dotenv
import uvicorn

from game import MafiaGame
from game_models import NightResult, Phase, Player, Role, VoteResult, Winner
from bot_constants import (
    RECRUITMENT_SECONDS,
    MAX_GAME_PLAYERS,
    DAY_EXTENSION_VOTE_SECONDS,
    DISCUSSION_EXTENSION_SECONDS,
    CONFIRM_VOTE_SECONDS,
    GAME_NOTIFICATION_ROLE,
    DEAD_PLAYER_ROLE,
    SPECTATOR_ROLE,
    OLD_DEAD_CHAT_CHANNEL_NAME,
    SHAMAN_CHAT_CHANNEL_NAME,
    FROG_CHAT_CHANNEL_NAME,
    PRIVATE_CHAT_ROLES,
    PRIVATE_CHANNEL_NAMES,
    CITIZEN_SPECIAL_ROLES,
    MAFIA_SPECIAL_ROLES,
    NEUTRAL_SPECIAL_ROLES,
    PUBLIC_MAFIA_SPECIAL_ROLES,
    PUBLIC_CITIZEN_SPECIAL_ROLES,
    PUBLIC_NEUTRAL_SPECIAL_ROLES,
    PUBLIC_CULT_SPECIAL_ROLES,
    CONTRACTOR_GUESS_ROLES,
    DEFAULT_EMBED_COLOR,
    ERROR_EMBED_COLOR,
    SUCCESS_EMBED_COLOR,
    WARNING_EMBED_COLOR,
    DayDiscussionResult,
    ANIMAL_ALIASES,
    ANIMAL_EMOJI_CODES,
    NUMBER_AVATAR_COLORS,
)
from bot_state import BotConfig, RunningGame, TimedNightEvents
from role_data import (
    ROLE_GUIDE_ORDER,
    ROLE_TEAM_TEXT,
    ROLE_GOAL_TEXT,
    ROLE_ABILITY_TEXTS,
    ROLE_RULE_TEXTS,
    ROLE_GUIDE_COMMON_TEXT,
    MAFIA_TERM_ENTRIES,
)
from stats_store import (
    INITIAL_RATING,
    LEADERBOARD_METRIC_NAMES,
    default_player_stats,
    ensure_player_stats,
    initial_role_for_stats,
    is_mafia_team_role,
    leaderboard_entries,
    leaderboard_metric_name,
    leaderboard_text,
    leaderboard_value,
    load_stats,
    personal_stats_text,
    player_won_game,
    rating_log_text,
    record_game_stats,
    role_stats_text,
    save_stats,
    win_rate_text,
)
from time_text import duration_text, play_duration_text
import web_settings


sys.modules.setdefault("bot", sys.modules[__name__])

BASE_DIR = Path(__file__).resolve().parent
CONFIG_FILE = BASE_DIR / "config.json"
CONFIG_EXAMPLE_FILE = BASE_DIR / "config.example.json"
STATS_FILE = BASE_DIR / "stats.json"

# /마피아웹설정 명령어가 발급하는 1회용 설정 편집 링크 관련 상수.
WEB_SETTINGS_PATH = "/web-settings"
WEB_SETTINGS_SESSION_TTL_SECONDS = 600
WEB_SETTINGS_DEFAULT_HOST = "0.0.0.0"
WEB_SETTINGS_DEFAULT_PORT = 8800
BOT_STARTED_AT = time.monotonic()
WEB_LEADERBOARD_METRICS = ("rating", "wins", "winrate", "games", "mafia", "playtime")


def parse_user_id_list(values: object) -> list[int]:
    if not isinstance(values, list):
        return []
    user_ids: set[int] = set()
    for value in values:
        with suppress(TypeError, ValueError):
            user_ids.add(int(value))
    return sorted(user_ids)


def load_config() -> BotConfig:
    if not CONFIG_FILE.exists():
        if CONFIG_EXAMPLE_FILE.exists():
            CONFIG_FILE.write_text(CONFIG_EXAMPLE_FILE.read_text(encoding="utf-8"), encoding="utf-8")
        else:
            raise FileNotFoundError("config.json 파일이 없습니다. config.example.json을 복사해 config.json을 만들어 주세요.")

    with CONFIG_FILE.open("r", encoding="utf-8") as file:
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
        enable_gangster=bool(data.get("enable_gangster", True)),
        enable_prophet=bool(data.get("enable_prophet", True)),
        enable_psychologist=bool(data.get("enable_psychologist", True)),
        enable_thief=bool(data.get("enable_thief", True)),
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

def config_to_dict(value: BotConfig) -> dict[str, object]:
    return {item.name: getattr(value, item.name) for item in fields(BotConfig)}


def save_config() -> None:
    saved_data: dict[str, object] = {}
    with suppress(OSError, json.JSONDecodeError):
        with CONFIG_FILE.open("r", encoding="utf-8") as file:
            existing_data = json.load(file)
        if isinstance(existing_data, dict):
            saved_data.update(existing_data)

    saved_data.update(config_to_dict(config))
    temp_path = CONFIG_FILE.with_name(f"{CONFIG_FILE.name}.tmp")
    with temp_path.open("w", encoding="utf-8") as file:
        json.dump(saved_data, file, ensure_ascii=False, indent=2)
        file.write("\n")
    os.replace(temp_path, CONFIG_FILE)


# --- /마피아웹설정: 브라우저에서 설정을 편집할 수 있는 1회용 링크 -------------------

web_settings_sessions = web_settings.WebSettingsSessionStore(ttl_seconds=WEB_SETTINGS_SESSION_TTL_SECONDS)


def web_config_values() -> dict[str, object]:
    return {spec.name: getattr(config, spec.name) for spec in web_settings.EDITABLE_FIELDS}


def apply_web_config_updates(updates: dict[str, object]) -> str | None:
    """웹 폼에서 제출된 설정 값을 적용합니다.

    `/마피아설정` 명령어와 동일하게, 적용 후 특수 역할 조합이 모집 인원과
    맞는지 검증하고 실패하면 이전 값으로 되돌립니다. 성공하면 ``config.json``
    까지 저장하고 ``None`` 을, 실패하면 사용자에게 보여줄 오류 메시지를
    돌려줍니다.
    """

    if int(updates.get("default_mafia_count", config.default_mafia_count)) < 1:
        return "마피아는 최소 1명이어야 합니다."

    previous = {key: getattr(config, key) for key in updates}
    for key, value in updates.items():
        setattr(config, key, value)

    try:
        role_counts = selected_role_counts(choose_special_roles())
        validate_max_player_count(role_counts, config.max_player_count)
    except ValueError as error:
        for key, value in previous.items():
            setattr(config, key, value)
        return str(error)

    save_config()
    return None


def web_status_values() -> dict[str, object]:
    now = time.monotonic()
    bot_user = getattr(bot, "user", None)
    latency = bot.latency
    latency_ms = round(latency * 1000) if math.isfinite(latency) else 0
    guilds = list(getattr(bot, "guilds", []))
    running_games = []
    for guild_id, running in games.items():
        guild = bot.get_guild(guild_id)
        channel = guild.get_channel(running.channel_id) if guild else None
        alive_count = len(running.game.alive_players())
        dead_count = len(running.game.dead_players())
        running_games.append(
            {
                "guild_id": guild_id,
                "guild_name": guild.name if guild else str(guild_id),
                "channel_id": running.channel_id,
                "channel_name": f"#{channel.name}" if hasattr(channel, "name") else str(running.channel_id),
                "phase": running.game.phase.value,
                "day": f"{running.game.day_number}일차",
                "participant_count": len(running.game.players),
                "alive_count": alive_count,
                "dead_count": dead_count,
                "spectator_count": len(running.spectator_user_ids),
                "anonymous_enabled": running.anonymous_enabled,
                "elapsed": play_duration_text(int(now - running.started_at)),
            }
        )

    return {
        "bot": {
            "ready": bot.is_ready(),
            "name": bot_user.name if bot_user else "Dimi Check",
            "latency_ms": latency_ms,
            "guild_count": len(guilds),
            "user_count": sum(int(guild.member_count or 0) for guild in guilds),
            "uptime": play_duration_text(int(now - BOT_STARTED_AT)),
        },
        "games": sorted(running_games, key=lambda item: (str(item["guild_name"]), str(item["channel_name"]))),
        "recruiting_guild_count": len(recruiting_guilds),
        "settings": {
            "game_enabled": config.game_enabled,
            "max_player_count_text": max_player_setting_text(),
            "role_summary": (
                f"마피아 {config.default_mafia_count}명, "
                f"의사 {config.default_doctor_count}명, "
                f"수사직 {config.default_police_count}명"
            ),
            "special_summary": (
                f"시민 {config.citizen_special_count}개, "
                f"마피아 {config.mafia_special_count}개, "
                f"중립 {config.neutral_special_count}개"
            ),
            "anonymous_mode_text": (
                f"켜짐 ({anonymous_name_mode_text()})" if config.anonymous_mode else "꺼짐"
            ),
            "slowmode_text": f"{config.chat_slowmode_seconds}초",
            "cult_team_text": "켜짐" if config.enable_cult_team else "꺼짐",
        },
    }


def web_stats_summary() -> dict[str, object]:
    stats = load_stats()
    users = stats.get("users", {})
    if not isinstance(users, dict):
        users = {}
    entries = [entry for entry in users.values() if isinstance(entry, dict)]
    played_entries = [entry for entry in entries if int(entry.get("games", 0)) > 0]
    total_player_games = sum(int(entry.get("games", 0)) for entry in played_entries)
    total_wins = sum(int(entry.get("wins", 0)) for entry in played_entries)
    total_play_seconds = sum(int(entry.get("play_seconds", 0)) for entry in played_entries)
    ratings = [int(entry.get("rating", INITIAL_RATING)) for entry in played_entries]
    return {
        "registered_users": len(entries),
        "recorded_players": len(played_entries),
        "total_player_games": total_player_games,
        "total_wins": total_wins,
        "total_playtime": play_duration_text(total_play_seconds),
        "total_play_seconds": total_play_seconds,
        "average_rating": round(sum(ratings) / len(ratings)) if ratings else INITIAL_RATING,
    }


def web_leaderboard_values(metric: str = "rating", limit: int = 10) -> dict[str, object]:
    if metric not in WEB_LEADERBOARD_METRICS:
        metric = "rating"
    safe_limit = max(1, min(int(limit), 50))
    entries = []
    for rank, (user_id, entry) in enumerate(leaderboard_entries(metric, limit=safe_limit), start=1):
        games_count = int(entry.get("games", 0))
        wins = int(entry.get("wins", 0))
        losses = int(entry.get("losses", 0))
        mafia_games = int(entry.get("mafia_team_games", 0))
        play_seconds = int(entry.get("play_seconds", 0))
        rating = int(entry.get("rating", INITIAL_RATING))
        rating_peak = int(entry.get("rating_peak", INITIAL_RATING))
        rating_games = int(entry.get("rating_games", 0))
        entries.append(
            {
                "rank": rank,
                "user_id": user_id,
                "name": str(entry.get("name", "알 수 없음")),
                "games": games_count,
                "wins": wins,
                "losses": losses,
                "winrate": round(wins / games_count * 100, 1) if games_count else 0.0,
                "winrate_text": win_rate_text(wins, games_count),
                "mafia_team_games": mafia_games,
                "play_seconds": play_seconds,
                "playtime": play_duration_text(play_seconds),
                "rating": rating,
                "rating_peak": rating_peak,
                "rating_games": rating_games,
                "value": leaderboard_value(entry, metric),
            }
        )
    return {
        "metric": metric,
        "metric_name": leaderboard_metric_name(metric),
        "metrics": [
            {"key": key, "name": LEADERBOARD_METRIC_NAMES[key]}
            for key in WEB_LEADERBOARD_METRICS
        ],
        "limit": safe_limit,
        "entries": entries,
    }


web_settings_app = web_settings.create_app(
    sessions=web_settings_sessions,
    get_config_values=web_config_values,
    apply_config_updates=apply_web_config_updates,
    get_status_values=web_status_values,
    get_leaderboard_values=web_leaderboard_values,
    get_stats_summary=web_stats_summary,
    base_path=WEB_SETTINGS_PATH,
)


def web_settings_base_url() -> str:
    """사용자에게 보여줄 설정 페이지의 기본 URL을 계산합니다.

    `WEB_SETTINGS_BASE_URL` 환경 변수가 있으면 그대로 사용하고(리버스 프록시나
    도메인을 쓰는 경우), 없으면 호스트/포트로부터 추정합니다.
    """

    base_url = os.getenv("WEB_SETTINGS_BASE_URL")
    if base_url:
        return base_url.rstrip("/")
    host = os.getenv("WEB_SETTINGS_HOST", WEB_SETTINGS_DEFAULT_HOST)
    port = os.getenv("WEB_SETTINGS_PORT", str(WEB_SETTINGS_DEFAULT_PORT))
    display_host = "localhost" if host in ("0.0.0.0", "::") else host
    return f"http://{display_host}:{port}"


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
        Role.GANGSTER: config.enable_gangster,
        Role.PROPHET: config.enable_prophet,
        Role.PSYCHOLOGIST: config.enable_psychologist,
        Role.THIEF: config.enable_thief,
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
    return secrets.SystemRandom().sample(candidates, count) if count > 0 else []


def investigation_role_candidates() -> list[Role]:
    candidates = [Role.POLICE]
    if config.use_agent:
        candidates.append(Role.AGENT)
    if config.use_vigilante:
        candidates.append(Role.VIGILANTE)
    return candidates


def random_investigation_role() -> Role:
    return secrets.choice(investigation_role_candidates())


def minimum_player_count(role_counts: dict[Role, int]) -> int:
    special_count = sum(role_counts.values())
    mafia_count = (
        role_counts.get(Role.MAFIA, 0)
        + role_counts.get(Role.SPY, 0)
        + role_counts.get(Role.CONTRACTOR, 0)
        + role_counts.get(Role.THIEF, 0)
        + role_counts.get(Role.WITCH, 0)
        + role_counts.get(Role.SCIENTIST, 0)
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
            Role.THIEF,
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
            Role.GANGSTER,
            Role.PROPHET,
            Role.PSYCHOLOGIST,
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
            Role.GANGSTER: config.enable_gangster,
            Role.PROPHET: config.enable_prophet,
            Role.PSYCHOLOGIST: config.enable_psychologist,
            Role.THIEF: config.enable_thief,
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


def create_logged_background_task(coro, label: str) -> asyncio.Task:
    task = asyncio.create_task(coro)

    def log_task_error(done_task: asyncio.Task) -> None:
        error = None
        with suppress(asyncio.CancelledError):
            error = done_task.exception()
        if error:
            print(f"{label} error: {error!r}")

    task.add_done_callback(log_task_error)
    return task


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


class MafiaBot(commands.Bot):
    async def setup_hook(self) -> None:
        from cogs import load_all

        load_all()
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


def _share_cog_runtime(*modules: object) -> None:
    shared = {}
    for module in modules:
        for name in getattr(module, "__all__", ()):
            shared[name] = getattr(module, name)
    globals().update(shared)
    for module in modules:
        vars(module).update(shared)


def _load_cog_runtime() -> None:
    from cogs import anonymous_chat as anonymous_chat_cog
    from cogs import channel_runtime as channel_runtime_cog
    _share_cog_runtime(anonymous_chat_cog, channel_runtime_cog)

    from cogs import views as views_cog
    _share_cog_runtime(anonymous_chat_cog, channel_runtime_cog, views_cog)

    from cogs import role_guides as role_guides_cog
    _share_cog_runtime(anonymous_chat_cog, channel_runtime_cog, views_cog, role_guides_cog)

    from cogs import game_runner as game_runner_cog
    _share_cog_runtime(
        anonymous_chat_cog,
        channel_runtime_cog,
        views_cog,
        role_guides_cog,
        game_runner_cog,
    )


_load_cog_runtime()


async def run_bot_and_web_server(token: str) -> None:
    """디스코드 봇과 설정용 웹 서버(uvicorn)를 같은 이벤트 루프에서 함께 실행합니다."""

    host = os.getenv("WEB_SETTINGS_HOST", WEB_SETTINGS_DEFAULT_HOST)
    port = int(os.getenv("WEB_SETTINGS_PORT", str(WEB_SETTINGS_DEFAULT_PORT)))
    web_server = uvicorn.Server(
        uvicorn.Config(web_settings_app, host=host, port=port, log_level="warning")
    )

    async with bot:
        await asyncio.gather(
            bot.start(token),
            web_server.serve(),
        )


def main() -> None:
    load_dotenv(BASE_DIR / ".env")
    token = os.getenv("DISCORD_TOKEN")
    if not token:
        raise RuntimeError(".env 파일에 DISCORD_TOKEN을 설정하세요.")
    asyncio.run(run_bot_and_web_server(token))


if __name__ == "__main__":
    main()

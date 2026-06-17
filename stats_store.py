from __future__ import annotations

from datetime import datetime
import json
import os
from pathlib import Path
import time

from bot_state import RunningGame
from game import MafiaGame
from game_models import MAFIA_TEAM_ROLES, Player, Role, Winner
from role_data import ROLE_GUIDE_ORDER
from time_text import play_duration_text


BASE_DIR = Path(__file__).resolve().parent
STATS_FILE = BASE_DIR / "stats.json"
INITIAL_RATING = 1000
RATING_HISTORY_LIMIT = 20
RATING_DELTA_CAP = 50
ROLE_DELTA_CAP = 10
_STATS_CACHE: dict | None = None
_STATS_CACHE_SIGNATURE: tuple[int, int] | None = None
LEADERBOARD_METRIC_NAMES = {
    "wins": "승리수",
    "winrate": "승률",
    "games": "판수",
    "mafia": "마피아팀 플레이",
    "playtime": "게임시간",
    "rating": "레이팅",
}
ROLE_ORDER_INDEX = {role.value: index for index, role in enumerate(ROLE_GUIDE_ORDER)}


def original_stats_name(running: RunningGame, player: Player) -> str:
    return running.anonymous_original_names.get(player.user_id, player.name)


def stats_file_signature() -> tuple[int, int] | None:
    try:
        stat = STATS_FILE.stat()
    except OSError:
        return None
    return stat.st_mtime_ns, stat.st_size


def remember_stats_cache(stats: dict) -> None:
    global _STATS_CACHE, _STATS_CACHE_SIGNATURE
    _STATS_CACHE = stats
    _STATS_CACHE_SIGNATURE = stats_file_signature()


def load_stats() -> dict:
    signature = stats_file_signature()
    if _STATS_CACHE is not None and _STATS_CACHE_SIGNATURE == signature:
        return _STATS_CACHE
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
    changed = False
    for entry in data["users"].values():
        if not isinstance(entry, dict):
            continue
        if "play_seconds" not in entry:
            entry["play_seconds"] = 0
            changed = True
        if ensure_rating_fields(entry):
            changed = True
    if changed:
        save_stats(data)
    else:
        remember_stats_cache(data)
    return data


def save_stats(stats: dict) -> None:
    temp_path = STATS_FILE.with_name(f"{STATS_FILE.name}.tmp")
    with temp_path.open("w", encoding="utf-8") as file:
        json.dump(stats, file, ensure_ascii=False, indent=2)
        file.write("\n")
    os.replace(temp_path, STATS_FILE)
    remember_stats_cache(stats)


def default_player_stats(name: str) -> dict:
    return {
        "name": name,
        "games": 0,
        "wins": 0,
        "losses": 0,
        "mafia_team_games": 0,
        "play_seconds": 0,
        "rating": INITIAL_RATING,
        "rating_games": 0,
        "rating_peak": INITIAL_RATING,
        "rating_history": [],
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
    ensure_rating_fields(entry)
    entry.setdefault("roles", {})
    return entry


def ensure_rating_fields(entry: dict) -> bool:
    changed = False
    for key, default in (
        ("rating", INITIAL_RATING),
        ("rating_games", 0),
        ("rating_peak", INITIAL_RATING),
    ):
        if not isinstance(entry.get(key), int):
            entry[key] = default
            changed = True
    if not isinstance(entry.get("rating_history"), list):
        entry["rating_history"] = []
        changed = True
    return changed


def initial_role_for_stats(running: RunningGame, player: Player) -> Role:
    return running.initial_roles.get(player.user_id, player.role)


def is_mafia_team_role(role: Role) -> bool:
    return role in MAFIA_TEAM_ROLES


def player_won_game(game: MafiaGame, player: Player, winner: Winner) -> bool:
    if winner == Winner.MAFIA:
        return game.is_mafia_team(player)
    if winner == Winner.CULT:
        return game.is_cult_team(player)
    if winner == Winner.JOKER:
        joker_winner_id = getattr(game, "joker_winner_id", None)
        return player.user_id == joker_winner_id or (joker_winner_id is None and player.role == Role.JOKER)
    return game.is_citizen_team(player)


def clamp(value: int, low: int, high: int) -> int:
    return max(low, min(high, value))


def rating_k(entry: dict) -> int:
    rating_games = int(entry.get("rating_games", 0))
    if rating_games < 10:
        return 40
    if rating_games < 30:
        return 32
    return 24


def player_count_multiplier(player_count: int) -> float:
    if player_count <= 3:
        return 0.6
    if player_count <= 6:
        return 0.85
    if player_count <= 10:
        return 1.0
    return 1.1


def expected_score(player_rating: int, opponent_average: float) -> float:
    return 1 / (1 + 10 ** ((opponent_average - player_rating) / 400))


def rating_team_key(game: MafiaGame, player: Player) -> str:
    if player.role == Role.JOKER:
        return "joker"
    if game.is_cult_team(player):
        return "cult"
    if game.is_mafia_team(player):
        return "mafia"
    return "citizen"


def opponent_average_rating(
    game: MafiaGame,
    player: Player,
    ratings: dict[int, int],
    team_by_user_id: dict[int, str] | None = None,
) -> float:
    player_team = rating_team_key(game, player)
    if team_by_user_id is None:
        team_by_user_id = {item.user_id: rating_team_key(game, item) for item in game.players}
    if player_team == "citizen":
        candidates = [item.user_id for item in game.players if team_by_user_id[item.user_id] == "mafia"]
    elif player_team == "mafia":
        candidates = [item.user_id for item in game.players if team_by_user_id[item.user_id] == "citizen"]
    else:
        candidates = [item.user_id for item in game.players if team_by_user_id[item.user_id] != player_team]
    if not candidates:
        candidates = [item.user_id for item in game.players if item.user_id != player.user_id]
    if not candidates:
        return float(ratings.get(player.user_id, INITIAL_RATING))
    return sum(ratings.get(user_id, INITIAL_RATING) for user_id in candidates) / len(candidates)


def role_rating_adjustment(running: RunningGame, player: Player, role: Role, winner: Winner) -> tuple[int, list[str]]:
    game = running.game
    points: list[tuple[int, str]] = []

    def add(point: int, reason: str) -> None:
        points.append((point, reason))

    if role == Role.JOKER:
        if winner == Winner.JOKER and player_won_game(game, player, winner):
            add(10, "조커 투표 승리")
        else:
            add(-2, "조커 승리 실패")
    elif role == Role.AGENT:
        if game.agent_discovered_ids:
            add(3, "요원 공작 정보 확보")
        else:
            add(-1, "요원 공작 결과 없음")
    elif role == Role.VIGILANTE:
        used_any_vigilante_action = False
        if player.user_id in game.vigilante_execution_used_ids:
            used_any_vigilante_action = True
            add(5, "자경단원 숙청 사용")
        if player.user_id in game.vigilante_investigation_used_ids:
            used_any_vigilante_action = True
            add(2, "자경단원 조사 사용")
        if not used_any_vigilante_action:
            add(-2, "자경단원 능력 미사용")
    elif role == Role.REPORTER:
        if player.user_id in game.reporter_used_ids:
            add(4, "기자 특종 사용")
        else:
            add(-2, "기자 특종 미사용")
    elif role == Role.HACKER:
        if player.user_id in game.hacker_used_ids:
            add(3, "해커 해킹 사용")
        else:
            add(-2, "해커 해킹 미사용")
        if player.user_id in game.hacker_proxy_targets:
            add(2, "해커 프록시 설정")
    elif role == Role.SHAMAN:
        if game.purified_dead_ids:
            add(3, "영매 성불 정보 확보")
        else:
            add(-1, "영매 성불 미사용")
    elif role == Role.PRIEST:
        if player.user_id in game.priest_used_ids:
            add(5, "성직자 소생 사용")
        else:
            add(-2, "성직자 소생 미사용")
    elif role == Role.SOLDIER:
        if player.user_id in game.soldier_bulletproof_used:
            add(5, "군인 방탄 발동")
    elif role == Role.NURSE:
        if player.user_id in game.nurse_contacted:
            add(4, "간호사 의사 접선")
        else:
            add(-1, "간호사 접선 실패")
    elif role == Role.GANGSTER:
        if player.user_id in game.gangster_used_ids:
            add(3, "건달 공갈 성공")
        else:
            add(-1, "건달 공갈 미사용")
    elif role == Role.PROPHET:
        if player.alive and game.day_number >= 4:
            add(5, "예언자 계시 생존")
        else:
            add(-2, "예언자 계시 전 사망")
    elif role == Role.PSYCHOLOGIST:
        if player.user_id in game.psychologist_used_days:
            add(3, "심리학자 관찰 사용")
        else:
            add(-1, "심리학자 관찰 미사용")
    elif role == Role.GRAVEROBBER:
        if player.role != Role.GRAVEROBBER:
            add(4, f"도굴꾼 {player.role.value} 승계")
    elif role == Role.POLITICIAN:
        if player.user_id in game.publicly_revealed_ids:
            add(3, "정치인 처세 발동")
    elif role == Role.JUDGE:
        if player.user_id in game.revealed_judge_ids:
            add(4, "판사 선고 발동")
    elif role == Role.TERRORIST:
        target = game.get_player(game.terrorist_targets.get(player.user_id, 0))
        if target and not target.alive and not game.is_citizen_team(target):
            add(6, "테러리스트 적팀 동귀어진")
        elif player.user_id in game.terrorist_targets:
            add(2, "테러리스트 지목 유지")
        else:
            add(-2, "테러리스트 지목 미사용")
    elif role == Role.SPY:
        if player.user_id in game.spy_contacted:
            add(4, "스파이 마피아 접선")
        else:
            add(-2, "스파이 접선 실패")
    elif role == Role.CONTRACTOR:
        if player.user_id in game.contractor_contacted:
            add(4, "청부업자 동업 접선")
        elif game.day_number >= 2:
            add(-2, "청부업자 동업 실패")
    elif role == Role.THIEF:
        if player.user_id in game.thief_contacted:
            add(5, "도둑 교련 접선")
        elif player.user_id in game.thief_used_days:
            add(3, "도둑 도벽 사용")
        else:
            add(-2, "도둑 도벽 미사용")
    elif role == Role.WITCH:
        if player.user_id in game.witch_contacted:
            add(4, "마녀 접선")
        else:
            add(-1, "마녀 접선 실패")
    elif role == Role.SCIENTIST:
        if player.user_id in game.scientist_revive_used_ids:
            add(5, "과학자 재생 발동")
        elif player.user_id in game.scientist_contacted:
            add(3, "과학자 유착 발동")
    elif role == Role.MADAM:
        if player.user_id in game.madam_contacted:
            add(4, "마담 접대 성공")
        elif game.madam_seduced_ids:
            add(3, "마담 유혹 적용")
        else:
            add(-1, "마담 유혹 영향 없음")
    elif role == Role.GODFATHER:
        if player.user_id in game.godfather_contacted:
            add(4, "대부 접선")
    elif role == Role.CULT_LEADER:
        cult_count = sum(1 for item in game.players if item.user_id in game.culted_ids and item.user_id != player.user_id)
        if cult_count:
            add(min(8, cult_count * 3), "교주 포교 성공")
        else:
            add(-2, "교주 포교 실패")
    elif role == Role.FANATIC:
        if player.user_id in game.culted_ids:
            add(4, "광신도 교주 접촉")
        if player.role == Role.CULT_LEADER:
            add(5, "광신도 재림")

    role_delta = clamp(sum(point for point, _reason in points), -ROLE_DELTA_CAP, ROLE_DELTA_CAP)
    reasons = [reason for point, reason in points if point != 0]
    return role_delta, reasons


def rating_change_for_player(
    running: RunningGame,
    winner: Winner,
    player: Player,
    entry: dict,
    ratings: dict[int, int],
    team_by_user_id: dict[int, str] | None = None,
) -> dict:
    old_rating = ratings.get(player.user_id, INITIAL_RATING)
    score = 1.0 if player_won_game(running.game, player, winner) else 0.0
    opponent_average = opponent_average_rating(running.game, player, ratings, team_by_user_id)
    base_delta = rating_k(entry) * (score - expected_score(old_rating, opponent_average))
    team_delta = round(base_delta * player_count_multiplier(len(running.game.players)))
    role = initial_role_for_stats(running, player)
    role_delta, role_reasons = role_rating_adjustment(running, player, role, winner)
    final_delta = clamp(team_delta + role_delta, -RATING_DELTA_CAP, RATING_DELTA_CAP)
    if score == 0.0 and final_delta > 3:
        final_delta = 3
    return {
        "before": old_rating,
        "after": max(0, old_rating + final_delta),
        "delta": final_delta,
        "team_delta": team_delta,
        "role_delta": role_delta,
        "reasons": (["소속 진영 승리"] if score == 1.0 else ["소속 진영 패배"]) + role_reasons,
    }


def record_game_stats(running: RunningGame, winner: Winner) -> None:
    if running.stats_recorded:
        return
    stats = load_stats()
    elapsed_seconds = max(0, int(time.monotonic() - running.started_at))
    entries: dict[int, dict] = {}
    ratings: dict[int, int] = {}
    for player in running.game.players:
        name = original_stats_name(running, player) if running.anonymous_enabled else player.name
        entry = ensure_player_stats(stats, player.user_id, name)
        entries[player.user_id] = entry
        ratings[player.user_id] = int(entry.get("rating", INITIAL_RATING))
    team_by_user_id = {
        player.user_id: rating_team_key(running.game, player)
        for player in running.game.players
    }
    rating_changes = {
        player.user_id: rating_change_for_player(
            running,
            winner,
            player,
            entries[player.user_id],
            ratings,
            team_by_user_id,
        )
        for player in running.game.players
    }
    ended_at = datetime.now().astimezone().isoformat(timespec="seconds")
    for player in running.game.players:
        entry = entries[player.user_id]
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
        rating_change = rating_changes[player.user_id]
        entry["rating"] = rating_change["after"]
        entry["rating_games"] = int(entry.get("rating_games", 0)) + 1
        entry["rating_peak"] = max(int(entry.get("rating_peak", INITIAL_RATING)), int(entry["rating"]))
        history = entry.setdefault("rating_history", [])
        if isinstance(history, list):
            history.append(
                {
                    "ended_at": ended_at,
                    "before": rating_change["before"],
                    "after": rating_change["after"],
                    "delta": rating_change["delta"],
                    "team_delta": rating_change["team_delta"],
                    "role_delta": rating_change["role_delta"],
                    "role": role.value,
                    "team": rating_team_key(running.game, player),
                    "winner": winner.value,
                    "players": len(running.game.players),
                    "rating_reasons": rating_change["reasons"],
                }
            )
            del history[:-RATING_HISTORY_LIMIT]
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
    items = sorted(
        roles.items(),
        key=lambda item: (-int(item[1]), ROLE_ORDER_INDEX.get(item[0], 999), item[0]),
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
    rating = int(entry.get("rating", INITIAL_RATING))
    rating_games = int(entry.get("rating_games", 0))
    rating_peak = int(entry.get("rating_peak", INITIAL_RATING))
    name = str(entry.get("name") or fallback_name)
    return (
        f"{name}님의 전적\n"
        f"전체 게임: **{games}판**\n"
        f"승리/패배: **{wins}승 {losses}패**\n"
        f"승률: **{win_rate_text(wins, games)}**\n"
        f"마피아팀 플레이: **{mafia_games}회**\n"
        f"게임시간: **{play_duration_text(play_seconds)}**\n"
        f"레이팅: **{rating}점** (최고 {rating_peak}점, 반영 {rating_games}판)\n\n"
        f"역할별 플레이\n{role_stats_text(entry)}"
    )


def rating_log_text(user_id: int, fallback_name: str, limit: int = 10) -> str:
    stats = load_stats()
    entry = stats.get("users", {}).get(str(user_id))
    if not isinstance(entry, dict):
        return "아직 기록된 레이팅 로그가 없습니다."
    history = entry.get("rating_history", [])
    if not isinstance(history, list) or not history:
        return "아직 기록된 레이팅 로그가 없습니다."

    name = str(entry.get("name") or fallback_name)
    lines = [f"{name} 님의 최근 레이팅 로그"]
    for item in reversed(history[-limit:]):
        if not isinstance(item, dict):
            continue
        ended_at = str(item.get("ended_at", ""))
        try:
            ended_text = datetime.fromisoformat(ended_at).strftime("%m/%d %H:%M")
        except ValueError:
            ended_text = ended_at or "날짜 없음"
        before = int(item.get("before", INITIAL_RATING))
        after = int(item.get("after", before))
        delta = int(item.get("delta", after - before))
        team_delta = int(item.get("team_delta", 0))
        role_delta = int(item.get("role_delta", 0))
        sign = "+" if delta >= 0 else ""
        role = str(item.get("role", "직업 없음"))
        winner = str(item.get("winner", "승자 없음"))
        reasons = item.get("rating_reasons", [])
        reason_text = ", ".join(str(reason) for reason in reasons[:3]) if isinstance(reasons, list) else ""
        detail = f" / 팀 {team_delta:+d}, 직업 {role_delta:+d}"
        if reason_text:
            detail += f" / {reason_text}"
        lines.append(
            f"- {ended_text}: {before} -> {after} ({sign}{delta})"
            f" / {role} / 승자 {winner}{detail}"
        )
    return "\n".join(lines)


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
    if metric == "rating":
        return float(entry.get("rating", INITIAL_RATING))
    return float(wins)


def leaderboard_metric_name(metric: str) -> str:
    return LEADERBOARD_METRIC_NAMES.get(metric, LEADERBOARD_METRIC_NAMES["wins"])


def leaderboard_entries(metric: str, limit: int = 10) -> list[tuple[str, dict]]:
    stats = load_stats()
    users = stats.get("users", {})
    if not isinstance(users, dict) or not users:
        return []
    entries = [
        (user_id, entry)
        for user_id, entry in users.items()
        if isinstance(entry, dict) and int(entry.get("games", 0)) > 0
    ]
    entries.sort(
        key=lambda item: (
            -leaderboard_value(item[1], metric),
            -int(item[1].get("wins", 0)),
            -int(item[1].get("games", 0)),
            str(item[1].get("name", "")),
        )
    )
    return entries[:limit]


def leaderboard_text(metric: str) -> str:
    entries = leaderboard_entries(metric)
    if not entries:
        return "아직 기록된 게임 전적이 없습니다."
    lines = [f"기준: **{leaderboard_metric_name(metric)}**"]
    for rank, (_user_id, entry) in enumerate(entries[:10], start=1):
        games = int(entry.get("games", 0))
        wins = int(entry.get("wins", 0))
        losses = int(entry.get("losses", 0))
        mafia_games = int(entry.get("mafia_team_games", 0))
        play_seconds = int(entry.get("play_seconds", 0))
        rating = int(entry.get("rating", INITIAL_RATING))
        lines.append(
            f"{rank}. **{entry.get('name', '알 수 없음')}** - "
            f"{wins}승 {losses}패 / {games}판 / 승률 {win_rate_text(wins, games)} / "
            f"마피아팀 {mafia_games}회 / 게임시간 {play_duration_text(play_seconds)} / "
            f"레이팅 {rating}점"
        )
    return "\n".join(lines)

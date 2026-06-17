"""디스코드 봇과 같은 프로세스에서 띄우는 마피아 게임 설정용 웹 페이지.

`/마피아웹설정` 명령어로 발급한 1회용 세션 토큰을 가진 사람만 접속해
``config.json`` 설정 값을 브라우저에서 편집할 수 있도록 해 줍니다.

이 모듈은 discord 봇 쪽 상태(``config`` 전역, ``save_config`` 등)를 직접
참조하지 않고, 콜백을 주입받는 형태로 동작합니다. 그래야 ``bot.py`` 와의
순환 참조 없이 독립적으로 테스트할 수 있습니다.
"""

from __future__ import annotations

import html
import secrets
import time
from dataclasses import dataclass
from typing import Callable
from urllib.parse import parse_qs

from fastapi import FastAPI, Request
from fastapi.responses import HTMLResponse, JSONResponse

# 세션 토큰의 기본 유효 시간(초). 1회용이며, 사용하지 않아도 이 시간이 지나면 만료됩니다.
DEFAULT_SESSION_TTL_SECONDS = 600


@dataclass
class WebSettingsSession:
    guild_id: int
    user_id: int
    user_label: str
    expires_at: float
    used: bool = False

    def is_valid(self, *, now: float) -> bool:
        return not self.used and self.expires_at > now


class WebSettingsSessionStore:
    """1회용·짧은 만료 시간을 가진 세션 토큰을 관리합니다.

    - ``issue``: 새 토큰을 발급합니다 (예: 슬래시 명령어 실행 시).
    - ``peek``: 토큰이 아직 유효한지 확인하되 소비하지 않습니다 (폼을 보여줄 때).
    - ``consume``: 토큰을 1회 사용 처리하고 제거합니다 (설정 저장이 완료됐을 때).
    """

    def __init__(self, ttl_seconds: int = DEFAULT_SESSION_TTL_SECONDS) -> None:
        self._ttl_seconds = ttl_seconds
        self._sessions: dict[str, WebSettingsSession] = {}

    @property
    def ttl_seconds(self) -> int:
        return self._ttl_seconds

    def issue(self, *, guild_id: int, user_id: int, user_label: str) -> str:
        self._purge_expired()
        token = secrets.token_urlsafe(32)
        self._sessions[token] = WebSettingsSession(
            guild_id=guild_id,
            user_id=user_id,
            user_label=user_label,
            expires_at=time.monotonic() + self._ttl_seconds,
        )
        return token

    def peek(self, token: str) -> WebSettingsSession | None:
        self._purge_expired()
        session = self._sessions.get(token)
        if session is None or not session.is_valid(now=time.monotonic()):
            return None
        return session

    def consume(self, token: str) -> WebSettingsSession | None:
        session = self.peek(token)
        if session is None:
            return None
        session.used = True
        self._sessions.pop(token, None)
        return session

    def _purge_expired(self) -> None:
        now = time.monotonic()
        expired = [token for token, session in self._sessions.items() if not session.is_valid(now=now)]
        for token in expired:
            self._sessions.pop(token, None)


@dataclass(frozen=True)
class ConfigField:
    """웹 폼에 노출할 설정 항목 한 개를 나타냅니다."""

    name: str
    label: str
    kind: str  # "int" | "bool" | "str" | "int_list"
    min_value: int | None = None


# `/마피아설정`, `/마피아인원설정`, `/마피아익명설정` 등 기존 명령어가 다루는
# 항목을 한 화면에서 함께 편집할 수 있도록 모았습니다.
EDITABLE_FIELDS: tuple[ConfigField, ...] = (
    ConfigField("participant_role", "참가자 역할 이름", "str"),
    ConfigField("manager_role", "관리자 역할 이름", "str"),
    ConfigField("game_enabled", "게임 시작 활성화", "bool"),
    ConfigField("max_player_count", "모집 최대 인원 (0 = 제한 없음)", "int", min_value=0),
    ConfigField("night_seconds", "밤 진행 시간(초)", "int", min_value=1),
    ConfigField("discussion_seconds", "낮 토론 시간(초)", "int", min_value=1),
    ConfigField("vote_seconds", "투표 시간(초)", "int", min_value=1),
    ConfigField("chat_slowmode_seconds", "낮 채팅 슬로우모드(초)", "int", min_value=0),
    ConfigField("default_mafia_count", "기본 마피아 수", "int", min_value=1),
    ConfigField("default_doctor_count", "기본 의사 수", "int", min_value=0),
    ConfigField("default_police_count", "기본 경찰 수", "int", min_value=0),
    ConfigField("default_joker_count", "기본 조커 수", "int", min_value=0),
    ConfigField("citizen_special_count", "시민 특수룰 수", "int", min_value=0),
    ConfigField("mafia_special_count", "마피아 특수룰 수", "int", min_value=0),
    ConfigField("neutral_special_count", "중립 특수룰 수", "int", min_value=0),
    ConfigField("reveal_death_roles", "사망 시 직업 공개", "bool"),
    ConfigField("reveal_public_police_status", "경찰 조사 결과 공개", "bool"),
    ConfigField("reveal_morning_mafia_count", "아침마다 생존 마피아 수 공개", "bool"),
    ConfigField("anonymous_mode", "익명 채팅 모드 사용", "bool"),
    ConfigField("anonymous_name_mode", "익명 이름 모드 (animal / number)", "str"),
    ConfigField("use_agent", "요원 사용", "bool"),
    ConfigField("use_vigilante", "자경단원 사용", "bool"),
    ConfigField("enable_detective", "사립탐정 활성화", "bool"),
    ConfigField("enable_graverobber", "도굴꾼 활성화", "bool"),
    ConfigField("enable_spy", "스파이 활성화", "bool"),
    ConfigField("enable_contractor", "청부업자 활성화", "bool"),
    ConfigField("enable_witch", "마녀 활성화", "bool"),
    ConfigField("enable_scientist", "과학자 활성화", "bool"),
    ConfigField("enable_madam", "마담 활성화", "bool"),
    ConfigField("enable_godfather", "대부 활성화", "bool"),
    ConfigField("enable_joker", "조커 활성화", "bool"),
    ConfigField("enable_politician", "정치인 활성화", "bool"),
    ConfigField("enable_judge", "판사 활성화", "bool"),
    ConfigField("enable_reporter", "기자 활성화", "bool"),
    ConfigField("enable_hacker", "해커 활성화", "bool"),
    ConfigField("enable_terrorist", "테러리스트 활성화", "bool"),
    ConfigField("enable_lover", "연인 활성화", "bool"),
    ConfigField("enable_shaman", "영매 활성화", "bool"),
    ConfigField("enable_priest", "성직자 활성화", "bool"),
    ConfigField("enable_soldier", "군인 활성화", "bool"),
    ConfigField("enable_nurse", "간호사 활성화", "bool"),
    ConfigField("enable_gangster", "건달 활성화", "bool"),
    ConfigField("enable_prophet", "예언자 활성화", "bool"),
    ConfigField("enable_psychologist", "심리학자 활성화", "bool"),
    ConfigField("enable_thief", "도둑 활성화", "bool"),
    ConfigField("enable_cult_team", "교주/광신도 팀 활성화", "bool"),
    ConfigField("blacklist_user_ids", "블랙리스트 유저 ID 목록", "int_list"),
)


PAGE_STYLE = """
<style>
  :root { color-scheme: light dark; }
  body { font-family: -apple-system, "Segoe UI", "Apple SD Gothic Neo", sans-serif;
         max-width: 1120px; margin: 28px auto; padding: 0 16px; line-height: 1.5; }
  h1 { font-size: 1.5rem; margin-bottom: 8px; }
  h2 { font-size: 1.1rem; margin: 0 0 12px; }
  a { color: #5865F2; }
  .meta { color: #888; font-size: 0.9rem; margin-bottom: 24px; }
  .nav { display: flex; flex-wrap: wrap; gap: 10px; margin: 14px 0 20px; }
  .nav a { text-decoration: none; padding: 8px 12px; border: 1px solid #8884; border-radius: 8px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 12px; margin: 16px 0; }
  .split { display: grid; grid-template-columns: 1.1fr 0.9fr; gap: 16px; }
  .card { border: 1px solid #8884; border-radius: 8px; padding: 14px; background: #8881; }
  .card strong { display: block; font-size: 1.4rem; margin-top: 4px; }
  .panel { border: 1px solid #8884; border-radius: 8px; padding: 16px; margin: 16px 0; }
  .muted { color: #888; }
  .pill { display: inline-block; padding: 3px 9px; border: 1px solid #8884; border-radius: 999px; font-size: 0.86rem; color: #777; }
  .metric-tabs { display: flex; flex-wrap: wrap; gap: 8px; margin: 12px 0 18px; }
  .metric-tabs a { text-decoration: none; border: 1px solid #8884; border-radius: 999px; padding: 7px 12px; }
  .metric-tabs a.active { background: #5865F2; color: white; border-color: #5865F2; }
  .podium { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 12px; margin-bottom: 16px; }
  .podium-card { border: 1px solid #8884; border-radius: 8px; padding: 14px; background: #8881; }
  .podium-card .rank { font-size: 0.9rem; color: #888; }
  .podium-card .name { font-size: 1.15rem; font-weight: 800; margin: 6px 0; }
  .podium-card .rating { font-size: 1.45rem; font-weight: 800; }
  .endpoint { display: grid; grid-template-columns: minmax(210px, 0.7fr) 1fr; gap: 10px; padding: 10px 0; border-bottom: 1px solid #8883; }
  code { background: #8882; padding: 2px 6px; border-radius: 6px; }
  table { width: 100%; border-collapse: collapse; }
  th, td { padding: 8px 6px; border-bottom: 1px solid #8883; text-align: left; }
  td.num, th.num { text-align: right; }
  th { font-weight: 700; }
  pre { white-space: pre-wrap; word-break: keep-all; margin: 0; font-family: inherit; }
  fieldset { border: 1px solid #8884; border-radius: 8px; padding: 8px 16px; margin-bottom: 16px; }
  legend { padding: 0 6px; font-weight: 600; }
  .row { display: flex; align-items: center; justify-content: space-between;
         gap: 16px; padding: 8px 0; border-bottom: 1px solid #8882; }
  .row:last-child { border-bottom: none; }
  .row span { flex: 1; }
  input[type="text"], input[type="number"], textarea { padding: 6px 10px; border-radius: 6px;
         border: 1px solid #8886; min-width: 160px; font-size: 0.95rem; }
  textarea { min-height: 72px; width: min(360px, 100%); resize: vertical; }
  input[type="checkbox"] { width: 20px; height: 20px; }
  button { margin-top: 16px; padding: 10px 22px; border: none; border-radius: 8px;
           background: #5865F2; color: white; font-size: 1rem; cursor: pointer; }
  button:hover { background: #4752c4; }
  .message { padding: 12px 16px; border-radius: 8px; margin-bottom: 16px; }
  .message.error { background: #f8d7da; color: #842029; }
  .message.notice { background: #d1e7dd; color: #0f5132; }
  @media (max-width: 760px) {
    .split { grid-template-columns: 1fr; }
    .endpoint { grid-template-columns: 1fr; }
    table { font-size: 0.92rem; }
  }
</style>
"""


def _safe_text(value: object, fallback: str = "-") -> str:
    text = str(value if value is not None else fallback)
    return html.escape(text)


def _render_nav() -> str:
    return (
        '<nav class="nav">'
        '<a href="/">홈</a>'
        '<a href="/status">상태판</a>'
        '<a href="/leaderboard">리더보드</a>'
        '<a href="/api/docs">API 문서</a>'
        "</nav>"
    )


def _render_status_summary(status: dict[str, object]) -> str:
    bot_info = status.get("bot", {}) if isinstance(status.get("bot"), dict) else {}
    settings = status.get("settings", {}) if isinstance(status.get("settings"), dict) else {}
    games = status.get("games", []) if isinstance(status.get("games"), list) else []
    cards = [
        ("봇 상태", "온라인" if bot_info.get("ready") else "시작 중"),
        ("서버 수", bot_info.get("guild_count", 0)),
        ("진행 중 게임", len(games)),
        ("모집 중 서버", status.get("recruiting_guild_count", 0)),
        ("게임 시작", "활성화" if settings.get("game_enabled") else "비활성화"),
        ("업타임", bot_info.get("uptime", "-")),
    ]
    return (
        '<section class="grid">'
        + "".join(
            f'<div class="card"><span>{html.escape(label)}</span><strong>{_safe_text(value)}</strong></div>'
            for label, value in cards
        )
        + "</section>"
    )


def _render_settings_snapshot(status: dict[str, object]) -> str:
    settings = status.get("settings", {}) if isinstance(status.get("settings"), dict) else {}
    rows = [
        ("최대 인원", settings.get("max_player_count_text")),
        ("기본 구성", settings.get("role_summary")),
        ("특수룰 수", settings.get("special_summary")),
        ("익명 채팅", settings.get("anonymous_mode_text")),
        ("채팅 슬로우모드", settings.get("slowmode_text")),
        ("교주팀", settings.get("cult_team_text")),
    ]
    return (
        '<section class="panel"><h2>현재 주요 설정</h2><table><tbody>'
        + "".join(f"<tr><th>{html.escape(label)}</th><td>{_safe_text(value)}</td></tr>" for label, value in rows)
        + "</tbody></table></section>"
    )


def _render_games_table(status: dict[str, object]) -> str:
    games = status.get("games", []) if isinstance(status.get("games"), list) else []
    if not games:
        return '<section class="panel"><h2>진행 중 게임</h2><p class="muted">현재 진행 중인 게임이 없습니다.</p></section>'
    rows = []
    for item in games:
        if not isinstance(item, dict):
            continue
        rows.append(
            "<tr>"
            f"<td>{_safe_text(item.get('guild_name'))}</td>"
            f"<td>{_safe_text(item.get('channel_name'))}</td>"
            f"<td>{_safe_text(item.get('phase'))}</td>"
            f"<td>{_safe_text(item.get('day'))}</td>"
            f"<td>{_safe_text(item.get('alive_count'))}/{_safe_text(item.get('participant_count'))}</td>"
            f"<td>{_safe_text(item.get('dead_count'))}</td>"
            f"<td>{_safe_text(item.get('elapsed'))}</td>"
            "</tr>"
        )
    return (
        '<section class="panel"><h2>진행 중 게임</h2>'
        "<table><thead><tr><th>서버</th><th>채널</th><th>단계</th><th>일차</th>"
        "<th>생존/참가</th><th>사망</th><th>진행 시간</th></tr></thead><tbody>"
        + "".join(rows)
        + "</tbody></table></section>"
    )


def _leaderboard_entries(leaderboard: dict[str, object]) -> list[dict[str, object]]:
    entries = leaderboard.get("entries", []) if isinstance(leaderboard, dict) else []
    return [item for item in entries if isinstance(item, dict)]


def _render_metric_tabs(leaderboard: dict[str, object]) -> str:
    current = str(leaderboard.get("metric", "rating"))
    metrics = leaderboard.get("metrics", [])
    if not isinstance(metrics, list):
        return ""
    links = []
    for metric in metrics:
        if not isinstance(metric, dict):
            continue
        key = str(metric.get("key", "rating"))
        name = str(metric.get("name", key))
        class_attr = ' class="active"' if key == current else ""
        links.append(f'<a{class_attr} href="/leaderboard?metric={html.escape(key)}">{html.escape(name)}</a>')
    return '<div class="metric-tabs">' + "".join(links) + "</div>"


def _render_leaderboard_podium(leaderboard: dict[str, object]) -> str:
    entries = _leaderboard_entries(leaderboard)[:3]
    if not entries:
        return '<p class="muted">아직 기록된 게임 전적이 없습니다.</p>'
    cards = []
    for entry in entries:
        cards.append(
            '<div class="podium-card">'
            f'<div class="rank">#{_safe_text(entry.get("rank"))}</div>'
            f'<div class="name">{_safe_text(entry.get("name"))}</div>'
            f'<div class="rating">{_safe_text(entry.get("rating"))}점</div>'
            f'<div class="muted">{_safe_text(entry.get("wins"))}승 {_safe_text(entry.get("losses"))}패 · '
            f'승률 {_safe_text(entry.get("winrate_text"))}</div>'
            "</div>"
        )
    return '<div class="podium">' + "".join(cards) + "</div>"


def _render_leaderboard_table(leaderboard: dict[str, object]) -> str:
    entries = _leaderboard_entries(leaderboard)
    if not entries:
        return '<section class="panel"><h2>리더보드</h2><p class="muted">아직 기록된 게임 전적이 없습니다.</p></section>'
    rows = []
    for entry in entries:
        rows.append(
            "<tr>"
            f'<td class="num">{_safe_text(entry.get("rank"))}</td>'
            f"<td><strong>{_safe_text(entry.get('name'))}</strong></td>"
            f'<td class="num">{_safe_text(entry.get("rating"))}</td>'
            f'<td class="num">{_safe_text(entry.get("wins"))}승 {_safe_text(entry.get("losses"))}패</td>'
            f'<td class="num">{_safe_text(entry.get("winrate_text"))}</td>'
            f'<td class="num">{_safe_text(entry.get("games"))}</td>'
            f'<td class="num">{_safe_text(entry.get("mafia_team_games"))}</td>'
            f'<td class="num">{_safe_text(entry.get("playtime"))}</td>'
            "</tr>"
        )
    return (
        '<section class="panel"><h2>전체 순위</h2>'
        '<table><thead><tr><th class="num">순위</th><th>이름</th><th class="num">레이팅</th>'
        '<th class="num">승패</th><th class="num">승률</th><th class="num">판수</th>'
        '<th class="num">마피아팀</th><th class="num">게임시간</th></tr></thead><tbody>'
        + "".join(rows)
        + "</tbody></table></section>"
    )


def _render_stats_summary(stats: dict[str, object]) -> str:
    cards = [
        ("기록된 유저", stats.get("recorded_players", 0)),
        ("누적 플레이", stats.get("total_player_games", 0)),
        ("누적 시간", stats.get("total_playtime", "-")),
        ("평균 레이팅", stats.get("average_rating", "-")),
    ]
    return (
        '<section class="grid">'
        + "".join(
            f'<div class="card"><span>{html.escape(label)}</span><strong>{_safe_text(value)}</strong></div>'
            for label, value in cards
        )
        + "</section>"
    )


def _render_field_row(spec: ConfigField, value: object) -> str:
    field_id = f"field_{spec.name}"
    label = html.escape(spec.label)
    if spec.kind == "bool":
        checked = "checked" if value else ""
        return (
            f'<label class="row" for="{field_id}">'
            f"<span>{label}</span>"
            f'<input type="checkbox" id="{field_id}" name="{spec.name}" {checked}>'
            f"</label>"
        )
    if spec.kind == "int":
        min_attr = f' min="{spec.min_value}"' if spec.min_value is not None else ""
        return (
            f'<label class="row" for="{field_id}">'
            f"<span>{label}</span>"
            f'<input type="number" id="{field_id}" name="{spec.name}" '
            f'value="{html.escape(str(value))}"{min_attr} required>'
            f"</label>"
        )
    if spec.kind == "int_list":
        if isinstance(value, list):
            text_value = "\n".join(str(item) for item in value)
        else:
            text_value = str(value or "")
        return (
            f'<label class="row" for="{field_id}">'
            f"<span>{label}<br><small class=\"muted\">한 줄에 하나씩, 또는 쉼표/공백으로 구분</small></span>"
            f'<textarea id="{field_id}" name="{spec.name}">{html.escape(text_value)}</textarea>'
            f"</label>"
        )
    return (
        f'<label class="row" for="{field_id}">'
        f"<span>{label}</span>"
        f'<input type="text" id="{field_id}" name="{spec.name}" '
        f'value="{html.escape(str(value))}" required>'
        f"</label>"
    )


def _render_page(
    *,
    session: WebSettingsSession,
    action: str,
    values: dict[str, object],
    status: dict[str, object] | None = None,
    error: str | None = None,
    notice: str | None = None,
) -> str:
    message_html = ""
    if error:
        message_html = f'<p class="message error">⚠️ {html.escape(error)}</p>'
    elif notice:
        message_html = f'<p class="message notice">{html.escape(notice)}</p>'

    rows_html = "\n".join(_render_field_row(spec, values.get(spec.name)) for spec in EDITABLE_FIELDS)
    status_html = ""
    if status is not None:
        status_html = _render_status_summary(status) + _render_games_table(status)
    public_links = _render_nav()

    return f"""<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex, nofollow">
<title>마피아 게임 설정</title>
{PAGE_STYLE}
</head>
<body>
<h1>마피아 게임 관리 패널</h1>
<p class="meta">{html.escape(session.user_label)} 님 전용 1회용 링크입니다. 저장하면 이 링크는 더 이상 사용할 수 없습니다.</p>
{public_links}
{status_html}
{message_html}
<form method="post" action="{html.escape(action)}">
  <fieldset>
    <legend>설정 항목</legend>
    {rows_html}
  </fieldset>
  <button type="submit">저장하기</button>
</form>
</body>
</html>"""


def _base_html(title: str, body: str, *, auto_refresh: bool = False) -> str:
    refresh = '<meta http-equiv="refresh" content="20">' if auto_refresh else ""
    return f"""<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
{refresh}
<title>{html.escape(title)}</title>
{PAGE_STYLE}
</head>
<body>
<h1>{html.escape(title)}</h1>
{_render_nav()}
{body}
</body>
</html>"""


def _render_home_page(
    *,
    status: dict[str, object],
    leaderboard: dict[str, object],
    stats: dict[str, object],
) -> str:
    body = (
        '<p class="meta">봇 상태와 전적을 한눈에 보는 홈입니다. 상태 정보는 20초마다 자동 새로고침됩니다.</p>'
        + _render_status_summary(status)
        + '<div class="split">'
        + '<div>'
        + _render_games_table(status)
        + "</div>"
        + '<div class="panel"><h2>레이팅 TOP 3</h2>'
        + _render_leaderboard_podium(leaderboard)
        + '<p><a href="/leaderboard">전체 리더보드 보기</a></p></div>'
        + "</div>"
        + _render_stats_summary(stats)
    )
    return _base_html("마피아 봇 홈", body, auto_refresh=True)


def _render_status_page(status: dict[str, object]) -> str:
    body = (
        '<p class="meta">진행 중 게임, 서버 연결 상태, 주요 게임 설정만 보여줍니다. 20초마다 자동 새로고침됩니다.</p>'
        + _render_status_summary(status)
        + _render_settings_snapshot(status)
        + _render_games_table(status)
    )
    return _base_html("마피아 봇 상태판", body, auto_refresh=True)


def _render_leaderboard_page(leaderboard: dict[str, object], stats: dict[str, object]) -> str:
    metric_name = str(leaderboard.get("metric_name", "레이팅"))
    body = (
        f'<p class="meta">현재 기준: <span class="pill">{html.escape(metric_name)}</span></p>'
        + _render_metric_tabs(leaderboard)
        + _render_leaderboard_podium(leaderboard)
        + _render_leaderboard_table(leaderboard)
        + _render_stats_summary(stats)
    )
    return _base_html("마피아 리더보드", body)


def _render_api_docs_page() -> str:
    endpoints = [
        ("GET /health", "봇 웹 서버가 살아 있는지 확인합니다."),
        ("GET /api/status", "봇 연결 상태, 진행 중 게임, 공개 설정 요약을 반환합니다."),
        ("GET /api/games", "진행 중 게임 목록만 반환합니다."),
        ("GET /api/settings", "공개 가능한 게임 설정 요약을 반환합니다."),
        ("GET /api/stats", "전적 요약 정보를 반환합니다."),
        ("GET /api/leaderboard", "레이팅 기준 리더보드를 반환합니다."),
        ("GET /api/leaderboard/{metric}", "wins, winrate, games, mafia, playtime, rating 기준 리더보드를 반환합니다."),
    ]
    rows = "".join(
        f'<div class="endpoint"><code>{html.escape(path)}</code><span>{html.escape(desc)}</span></div>'
        for path, desc in endpoints
    )
    body = (
        '<p class="meta">웹 상태판에서 사용하는 공개 API입니다. 모든 응답은 JSON입니다.</p>'
        '<section class="panel"><h2>엔드포인트</h2>'
        + rows
        + '</section><section class="panel"><h2>예시</h2>'
        '<pre>GET /api/leaderboard/rating?limit=20\nGET /api/status</pre></section>'
    )
    return _base_html("마피아 봇 API 문서", body)


def _render_message_page(*, title: str, message: str) -> str:
    return f"""<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex, nofollow">
<title>{html.escape(title)}</title>
{PAGE_STYLE}
</head>
<body>
<h1>{html.escape(title)}</h1>
<p>{html.escape(message)}</p>
</body>
</html>"""


_EXPIRED_PAGE = _render_message_page(
    title="🔒 링크가 만료되었습니다",
    message="이 링크는 더 이상 유효하지 않습니다. 디스코드에서 /마피아웹설정 명령어를 다시 실행해 새 링크를 발급받으세요.",
)

_SAVED_PAGE = _render_message_page(
    title="✅ 설정을 저장했습니다",
    message="마피아 게임 설정이 반영되었습니다. 이 창은 닫으셔도 됩니다.",
)


def parse_form_updates(form: dict[str, object] | object) -> tuple[dict[str, object], str | None]:
    """제출된 폼 데이터를 ``EDITABLE_FIELDS`` 기준으로 검증하고 변환합니다.

    반환값은 ``(updates, error_message)`` 이며, 오류가 있으면 ``updates`` 는
    비어 있고 ``error_message`` 에 사용자에게 보여줄 메시지가 담깁니다.
    """

    updates: dict[str, object] = {}
    for spec in EDITABLE_FIELDS:
        if spec.kind == "bool":
            updates[spec.name] = spec.name in form
            continue

        raw_value = form.get(spec.name)
        if raw_value is None:
            return {}, f"'{spec.label}' 값이 비어 있습니다."
        text_value = str(raw_value).strip()
        if spec.kind == "int_list" and not text_value:
            updates[spec.name] = []
            continue
        if not text_value:
            return {}, f"'{spec.label}' 값이 비어 있습니다."

        if spec.kind == "int":
            try:
                parsed = int(text_value)
            except ValueError:
                return {}, f"'{spec.label}' 값은 숫자여야 합니다."
            if spec.min_value is not None and parsed < spec.min_value:
                return {}, f"'{spec.label}' 값은 {spec.min_value} 이상이어야 합니다."
            updates[spec.name] = parsed
        elif spec.kind == "int_list":
            normalized = text_value.replace(",", " ")
            values: list[int] = []
            for chunk in normalized.split():
                try:
                    values.append(int(chunk))
                except ValueError:
                    return {}, f"'{spec.label}'에는 숫자 ID만 입력할 수 있습니다."
            updates[spec.name] = sorted(set(values))
        else:
            updates[spec.name] = text_value

    return updates, None


async def read_urlencoded_form(request: Request) -> dict[str, str]:
    body = await request.body()
    text = body.decode("utf-8")
    parsed = parse_qs(text, keep_blank_values=True)
    return {key: values[-1] if values else "" for key, values in parsed.items()}


def create_app(
    *,
    sessions: WebSettingsSessionStore,
    get_config_values: Callable[[], dict[str, object]],
    apply_config_updates: Callable[[dict[str, object]], str | None],
    get_status_values: Callable[[], dict[str, object]] | None = None,
    get_leaderboard_values: Callable[[str, int], dict[str, object]] | None = None,
    get_stats_summary: Callable[[], dict[str, object]] | None = None,
    base_path: str = "/web-settings",
) -> FastAPI:
    """설정 편집용 FastAPI 앱을 만듭니다.

    Args:
        sessions: 토큰 발급/검증/소비를 담당하는 세션 저장소.
        get_config_values: 현재 설정 값을 ``{필드명: 값}`` 형태로 돌려주는 콜백.
        apply_config_updates: 새 값을 적용하는 콜백. 검증에 실패하면 오류
            메시지를 문자열로, 성공하면 ``None`` 을 돌려줘야 합니다. (성공 시
            설정 적용과 ``config.json`` 저장까지 책임집니다.)
        base_path: 라우트 경로 접두사 (``/web-settings/{token}``).
    """

    def status_values() -> dict[str, object]:
        if get_status_values is None:
            return {"bot": {"ready": False}, "games": [], "settings": {}, "recruiting_guild_count": 0}
        return get_status_values()

    def leaderboard_values(metric: str = "rating", limit: int = 10) -> dict[str, object]:
        if get_leaderboard_values is None:
            return {"metric": metric, "metric_name": metric, "metrics": [], "entries": [], "limit": limit}
        return get_leaderboard_values(metric, limit)

    def stats_summary() -> dict[str, object]:
        if get_stats_summary is None:
            return {}
        return get_stats_summary()

    app = FastAPI(title="마피아 게임 웹 관리", docs_url=None, redoc_url=None, openapi_url=None)
    route_path = f"{base_path}/{{token}}"

    @app.get("/", response_class=HTMLResponse)
    async def dashboard_home() -> HTMLResponse:
        return HTMLResponse(
            _render_home_page(
                status=status_values(),
                leaderboard=leaderboard_values("rating", 3),
                stats=stats_summary(),
            )
        )

    @app.get("/status", response_class=HTMLResponse)
    async def public_status_page() -> HTMLResponse:
        return HTMLResponse(_render_status_page(status_values()))

    @app.get("/leaderboard", response_class=HTMLResponse)
    async def public_leaderboard_page(metric: str = "rating") -> HTMLResponse:
        return HTMLResponse(
            _render_leaderboard_page(
                leaderboard=leaderboard_values(metric, 20),
                stats=stats_summary(),
            )
        )

    @app.get("/api", response_class=HTMLResponse)
    @app.get("/api/docs", response_class=HTMLResponse)
    async def public_api_docs_page() -> HTMLResponse:
        return HTMLResponse(_render_api_docs_page())

    @app.get("/health")
    async def health() -> JSONResponse:
        return JSONResponse({"ok": True, "service": "mafia-discord-bot"})

    @app.get("/api/status")
    async def public_status_api() -> JSONResponse:
        return JSONResponse(status_values())

    @app.get("/api/games")
    async def public_games_api() -> JSONResponse:
        status = status_values()
        games = status.get("games", [])
        return JSONResponse({"games": games if isinstance(games, list) else []})

    @app.get("/api/settings")
    async def public_settings_api() -> JSONResponse:
        status = status_values()
        settings = status.get("settings", {})
        return JSONResponse({"settings": settings if isinstance(settings, dict) else {}})

    @app.get("/api/stats")
    async def public_stats_api() -> JSONResponse:
        return JSONResponse(stats_summary())

    @app.get("/api/leaderboard")
    async def public_default_leaderboard_api(limit: int = 10) -> JSONResponse:
        return JSONResponse(leaderboard_values("rating", limit))

    @app.get("/api/leaderboard/{metric}")
    async def public_metric_leaderboard_api(metric: str, limit: int = 10) -> JSONResponse:
        return JSONResponse(leaderboard_values(metric, limit))

    @app.get(route_path, response_class=HTMLResponse)
    async def show_settings_page(token: str) -> HTMLResponse:
        session = sessions.peek(token)
        if session is None:
            return HTMLResponse(_EXPIRED_PAGE, status_code=410)
        page = _render_page(
            session=session,
            action=f"{base_path}/{token}",
            values=get_config_values(),
            status=status_values(),
        )
        return HTMLResponse(page)

    @app.post(route_path, response_class=HTMLResponse)
    async def submit_settings_page(token: str, request: Request) -> HTMLResponse:
        session = sessions.peek(token)
        if session is None:
            return HTMLResponse(_EXPIRED_PAGE, status_code=410)

        form_data = await read_urlencoded_form(request)
        updates, parse_error = parse_form_updates(form_data)
        if parse_error:
            page = _render_page(
                session=session,
                action=f"{base_path}/{token}",
                values=get_config_values(),
                status=status_values(),
                error=parse_error,
            )
            return HTMLResponse(page, status_code=400)

        apply_error = apply_config_updates(updates)
        if apply_error:
            page = _render_page(
                session=session,
                action=f"{base_path}/{token}",
                values=get_config_values(),
                status=status_values(),
                error=apply_error,
            )
            return HTMLResponse(page, status_code=400)

        sessions.consume(token)
        return HTMLResponse(_SAVED_PAGE)

    return app

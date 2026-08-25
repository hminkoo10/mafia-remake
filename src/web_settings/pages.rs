// web_settings/pages.rs — 웹 페이지 HTML 렌더링

use super::*;

pub(crate) fn render_settings_page(
    session: &WebSettingsSession,
    action: &str,
    config: &BotConfig,
    status: Option<&Value>,
    error: Option<&str>,
) -> String {
    let message_html = error.map_or_else(String::new, |message| {
        format!(
            r#"<p class="message error">⚠️ {}</p>"#,
            html_escape(message)
        )
    });
    let rows = WEB_CONFIG_FIELDS
        .iter()
        .map(|field| render_field(*field, config))
        .collect::<Vec<_>>()
        .join("\n");
    let status_html = status.map(render_status_summary).unwrap_or_default();
    format!(
        r#"<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex, nofollow">
<title>마피아 게임 설정</title>
{WEB_PAGE_STYLE}
</head>
<body>
<div class="site-shell">
{}
<p class="meta">{} 님 전용 1회용 링크입니다. 저장하면 이 링크는 더 이상 사용할 수 없습니다.</p>
{}
{message_html}
<form method="post" action="{}">
  <fieldset>
    <legend>설정 항목</legend>
    {rows}
  </fieldset>
  <button type="submit">저장하기</button>
</form>
<p><a href="{}/api-keys">API 키 관리</a></p>
</main>
</div>
</body>
</html>"#,
        render_page_header("🕵️ 마피아 게임 웹 설정", false),
        html_escape(&session.user_label),
        status_html,
        html_escape(action),
        html_escape(action)
    )
}

pub(crate) fn render_api_key_page(
    session: &WebSettingsSession,
    action: &str,
    records: &[ApiKeyRecord],
    issued_key: Option<&str>,
    error: Option<&str>,
) -> String {
    let message_html = error.map_or_else(String::new, |message| {
        format!(
            r#"<p class="message error">⚠️ {}</p>"#,
            html_escape(message)
        )
    });
    let issued_html = issued_key.map_or_else(String::new, |key| {
        format!(
            r#"<section class="panel"><h2>새 API 키</h2><p class="message error">이 키는 지금 한 번만 표시됩니다. 안전한 곳에 보관하세요.</p><pre>{}</pre></section>"#,
            html_escape(key)
        )
    });
    let rows = records
        .iter()
        .map(|record| {
            let state = if record.revoked { "폐기됨" } else { "활성" };
            let action = if record.revoked {
                String::new()
            } else {
                format!(
                    r#"<form method="post" action="{action}"><input type="hidden" name="action" value="revoke"><input type="hidden" name="key_id" value="{}"><button type="submit">폐기</button></form>"#,
                    html_escape(&record.id)
                )
            };
            format!(
                r#"<tr><td>{}</td><td><code>{}</code></td><td>{}</td><td>{}</td><td>{action}</td></tr>"#,
                html_escape(&record.label),
                html_escape(&record.id),
                html_escape(&record.created_at),
                state,
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let table = if rows.is_empty() {
        "<p class=\"meta\">발급된 API 키가 없습니다.</p>".to_string()
    } else {
        format!(
            r#"<table><thead><tr><th>이름</th><th>키 ID</th><th>발급 시각</th><th>상태</th><th></th></tr></thead><tbody>{rows}</tbody></table>"#
        )
    };
    let settings_path = action.trim_end_matches("/api-keys");
    format!(
        r#"<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex, nofollow">
<title>마피아 API 키 관리</title>
{WEB_PAGE_STYLE}
</head>
<body>
<div class="site-shell">
{}
<p class="meta">{} 서버 전용 키입니다. 발급된 키는 이 서버의 보호 API만 사용할 수 있습니다.</p>
{message_html}
{issued_html}
<section class="panel"><h2>키 발급</h2><form method="post" action="{action}"><input type="hidden" name="action" value="create"><label class="row" for="label"><span>키 이름</span><input type="text" id="label" name="label" maxlength="64" required></label><button type="submit">키 발급</button></form></section>
<section class="panel"><h2>발급된 키</h2>{table}</section>
<p><a href="{settings_path}">설정으로 돌아가기</a></p>
</main>
</div>
</body>
</html>"#,
        render_page_header("마피아 API 키 관리", false),
        html_escape(&session.user_label),
        action = html_escape(action),
        settings_path = html_escape(settings_path),
    )
}

pub(crate) fn safe_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => html_escape(text),
        Some(Value::Number(number)) => html_escape(&number.to_string()),
        Some(Value::Bool(value)) => html_escape(&value.to_string()),
        _ => "-".to_string(),
    }
}

pub(crate) fn render_nav() -> &'static str {
    r#"<nav class="nav"><a href="/">홈</a><a href="/status">상태판</a><a href="/leaderboard">리더보드</a><a href="/rating">레이팅 설명</a><a href="/roles">역할 설명</a><a href="/tiers">티어 능력</a><a href="/api/docs">API 문서</a></nav>"#
}

pub(crate) fn render_status_summary(status: &Value) -> String {
    let bot = status.get("bot").unwrap_or(&Value::Null);
    let settings = status.get("settings").unwrap_or(&Value::Null);
    let games_len = status
        .get("games")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let cards = [
        (
            "봇 상태",
            if bot["ready"].as_bool().unwrap_or(false) {
                "온라인".to_string()
            } else {
                "시작 중".to_string()
            },
        ),
        ("서버 수", safe_text(bot.get("guild_count"))),
        ("진행 중 게임", games_len.to_string()),
        (
            "모집 중 서버",
            safe_text(status.get("recruiting_guild_count")),
        ),
        (
            "게임 시작",
            if settings["game_enabled"].as_bool().unwrap_or(false) {
                "활성화".to_string()
            } else {
                "비활성화".to_string()
            },
        ),
        ("업타임", safe_text(bot.get("uptime"))),
    ];
    format!(
        r#"<section class="grid">{}</section>"#,
        cards
            .into_iter()
            .map(|(label, value)| format!(
                r#"<div class="card"><span>{}</span><strong>{}</strong></div>"#,
                html_escape(label),
                value
            ))
            .collect::<Vec<_>>()
            .join("")
    )
}

pub(crate) fn render_games_table(status: &Value) -> String {
    let Some(games) = status.get("games").and_then(Value::as_array) else {
        return r#"<section class="panel"><h2>진행 중 게임</h2><p class="meta">현재 진행 중인 게임이 없습니다.</p></section>"#.to_string();
    };
    if games.is_empty() {
        return r#"<section class="panel"><h2>진행 중 게임</h2><p class="meta">현재 진행 중인 게임이 없습니다.</p></section>"#.to_string();
    }
    let rows = games
        .iter()
        .map(|item| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}/{}</td><td>{}</td><td>{}</td></tr>",
                safe_text(item.get("guild_name")),
                safe_text(item.get("channel_name")),
                safe_text(item.get("phase")),
                safe_text(item.get("day")),
                safe_text(item.get("alive_count")),
                safe_text(item.get("participant_count")),
                safe_text(item.get("dead_count")),
                safe_text(item.get("elapsed")),
            )
        })
        .collect::<Vec<_>>()
        .join("");
    format!(
        r#"<section class="panel"><h2>진행 중 게임</h2><table><thead><tr><th>서버</th><th>채널</th><th>단계</th><th>일차</th><th>생존/참가</th><th>사망</th><th>진행 시간</th></tr></thead><tbody>{rows}</tbody></table></section>"#
    )
}

pub(crate) fn base_html(title: &str, body: &str, auto_refresh: bool) -> String {
    let refresh = if auto_refresh {
        r#"<meta http-equiv="refresh" content="20">"#
    } else {
        ""
    };
    format!(
        r#"<!DOCTYPE html><html lang="ko"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="robots" content="noindex">{refresh}<title>{}</title>{WEB_PAGE_STYLE}</head><body><div class="site-shell">{}{body}</main></div></body></html>"#,
        html_escape(title),
        render_page_header(title, true),
    )
}

pub(crate) fn render_page_header(title: &str, with_nav: bool) -> String {
    let nav = if with_nav { render_nav() } else { "" };
    format!(
        r#"<header class="site-header"><a class="site-mark" href="/" aria-label="마피아 봇 홈">M</a><div><p class="eyebrow">MAFIA REMAKE</p><h1>{}</h1></div></header>{nav}<main>"#,
        html_escape(title),
    )
}

pub(crate) fn render_home_page(
    status: &Value,
    leaderboard: &Value,
    stats_summary: &Value,
) -> String {
    let body = format!(
        r#"<p class="meta">봇 상태와 전적을 한눈에 보는 홈입니다. 상태 정보는 20초마다 자동 새로고침됩니다.</p>{}{}{}"#,
        render_status_summary(status),
        render_games_table(status),
        render_stats_cards(stats_summary),
    );
    let body = format!(
        "{body}<section class=\"panel\"><h2>레이팅 TOP 3</h2>{}</section>",
        render_leaderboard_podium(leaderboard)
    );
    base_html("마피아 봇 홈", &body, true)
}

pub(crate) fn render_status_page(status: &Value) -> String {
    let settings = status.get("settings").unwrap_or(&Value::Null);
    let rows = [
        (
            "최대 인원",
            safe_text(settings.get("max_player_count_text")),
        ),
        ("기본 구성", safe_text(settings.get("role_summary"))),
        ("특수룰 수", safe_text(settings.get("special_summary"))),
        ("익명 채팅", safe_text(settings.get("anonymous_mode_text"))),
        ("채팅 슬로우모드", safe_text(settings.get("slowmode_text"))),
        ("교주팀", safe_text(settings.get("cult_team_text"))),
    ]
    .into_iter()
    .map(|(label, value)| format!("<tr><th>{}</th><td>{value}</td></tr>", html_escape(label)))
    .collect::<Vec<_>>()
    .join("");
    let body = format!(
        r#"<p class="meta">진행 중 게임, 서버 연결 상태, 주요 게임 설정만 보여줍니다. 20초마다 자동 새로고침됩니다.</p>{}<section class="panel"><h2>현재 주요 설정</h2><table><tbody>{rows}</tbody></table></section>{}"#,
        render_status_summary(status),
        render_games_table(status),
    );
    base_html("마피아 봇 상태판", &body, true)
}

pub(crate) fn render_stats_cards(stats_summary: &Value) -> String {
    let cards = [
        (
            "기록된 유저",
            safe_text(stats_summary.get("recorded_players")),
        ),
        (
            "누적 플레이",
            safe_text(stats_summary.get("total_player_games")),
        ),
        ("누적 시간", safe_text(stats_summary.get("total_playtime"))),
        (
            "평균 레이팅",
            safe_text(stats_summary.get("average_rating")),
        ),
    ];
    format!(
        r#"<section class="grid">{}</section>"#,
        cards
            .into_iter()
            .map(|(label, value)| format!(
                r#"<div class="card"><span>{}</span><strong>{value}</strong></div>"#,
                html_escape(label)
            ))
            .collect::<Vec<_>>()
            .join("")
    )
}

pub(crate) fn render_metric_tabs(leaderboard: &Value) -> String {
    let current = leaderboard
        .get("metric")
        .and_then(Value::as_str)
        .unwrap_or("rating");
    let Some(metrics) = leaderboard.get("metrics").and_then(Value::as_array) else {
        return String::new();
    };
    let links = metrics
        .iter()
        .filter_map(|metric| {
            let key = metric.get("key").and_then(Value::as_str)?;
            let name = metric.get("name").and_then(Value::as_str).unwrap_or(key);
            let class_attr = if key == current {
                r#" class="active""#
            } else {
                ""
            };
            Some(format!(
                r#"<a href="/leaderboard?metric={}"{}>{}</a>"#,
                html_escape(key),
                class_attr,
                html_escape(name)
            ))
        })
        .collect::<Vec<_>>()
        .join("");
    format!(r#"<div class="metric-tabs">{links}</div>"#)
}

pub(crate) fn render_leaderboard_podium(leaderboard: &Value) -> String {
    let entries = leaderboard
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if entries.is_empty() {
        return r#"<p class="meta">아직 기록된 게임 전적이 없습니다.</p>"#.to_string();
    }
    let cards = entries
        .iter()
        .take(3)
        .map(|entry| {
            format!(
                r#"<div class="podium-card"><div class="rank">#{}</div><div class="name">{}</div><div class="rating">{}점 · {}랭크</div><div class="meta">{}승 {}패 · 승률 {} · 연승 {}</div></div>"#,
                safe_text(entry.get("rank")),
                safe_text(entry.get("name")),
                safe_text(entry.get("rating")),
                safe_text(entry.get("rating_rank")),
                safe_text(entry.get("wins")),
                safe_text(entry.get("losses")),
                safe_text(entry.get("winrate_text")),
                safe_text(entry.get("streak_text")),
            )
        })
        .collect::<Vec<_>>()
        .join("");
    format!(r#"<div class="podium">{cards}</div>"#)
}

pub(crate) fn render_leaderboard_page(leaderboard: &Value, stats_summary: &Value) -> String {
    let body = format!(
        r#"<p class="meta">현재 기준: <span class="pill">{}</span></p>{}{}{}{}"#,
        safe_text(leaderboard.get("metric_name")),
        render_metric_tabs(leaderboard),
        render_leaderboard_podium(leaderboard),
        render_leaderboard_table(leaderboard, false),
        render_stats_cards(stats_summary),
    );
    base_html("마피아 리더보드", &body, false)
}

pub(crate) fn render_leaderboard_table(leaderboard: &Value, compact: bool) -> String {
    let entries = leaderboard
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if entries.is_empty() {
        return r#"<p class="meta">아직 기록된 게임 전적이 없습니다.</p>"#.to_string();
    }
    let rows = entries
        .iter()
        .map(|entry| {
            format!(
                r#"<tr><td class="num">{}</td><td>{}</td><td class="num">{}점 · {}</td><td>{}승 {}패</td><td class="num">{}</td><td class="num">{}</td><td class="num">{}</td><td class="num">{}</td><td>{}</td></tr>"#,
                safe_text(entry.get("rank")),
                safe_text(entry.get("name")),
                safe_text(entry.get("rating")),
                safe_text(entry.get("rating_rank")),
                safe_text(entry.get("wins")),
                safe_text(entry.get("losses")),
                safe_text(entry.get("winrate_text")),
                safe_text(entry.get("streak_text")),
                safe_text(entry.get("games")),
                safe_text(entry.get("mafia_team_games")),
                safe_text(entry.get("playtime")),
            )
        })
        .collect::<Vec<_>>()
        .join("");
    let title = if compact {
        ""
    } else {
        "<h2>전체 순위</h2>"
    };
    format!(
        r#"<section class="panel">{title}<table><thead><tr><th class="num">순위</th><th>이름</th><th class="num">레이팅/랭크</th><th>승패</th><th class="num">승률</th><th class="num">연승</th><th class="num">판수</th><th class="num">마피아팀</th><th>게임시간</th></tr></thead><tbody>{rows}</tbody></table></section>"#
    )
}

pub(crate) fn render_rating_page() -> String {
    let rank_rows = [
        ("X", "상위 10%", "현재 풀에서 최상위권입니다."),
        ("SS", "상위 10~25%", "정상 바로 아래 경쟁 구간입니다."),
        ("S", "상위 25~45%", "평균보다 확실히 위입니다."),
        ("A", "상위 45~70%", "중간 구간입니다."),
        ("B", "상위 70~90%", "따라잡는 구간입니다."),
        ("C", "하위 10%", "지금이 바닥, 올라갈 일만 남았습니다."),
        (
            "배치",
            "레이팅 반영 10판 미만",
            "10판을 채우면 랭크가 배정됩니다.",
        ),
    ]
    .into_iter()
    .map(|(rank, range, description)| {
        format!(
            "<tr><td><strong>{}</strong></td><td>{}</td><td>{}</td></tr>",
            html_escape(rank),
            html_escape(range),
            html_escape(description)
        )
    })
    .collect::<Vec<_>>()
    .join("");
    let gain_rows = [
        (
            "승리",
            "기본 +24점",
            "역할 활약·연승·상대 난이도에 따라 +5~+45점",
            "어떤 경우에도 이기면 최소 +5점은 오릅니다.",
        ),
        (
            "패배",
            "기본 -12점",
            "역할 활약으로 0점까지 완화, 최대 손실 -20점",
            "승리의 절반만 잃습니다. 잘한 판은 거의 잃지 않습니다.",
        ),
        (
            "배치 (첫 10판)",
            "승리 1.5배",
            "패배 0.5배",
            "초반에는 빠르게 올라가고 천천히 떨어집니다.",
        ),
    ]
    .into_iter()
    .map(|(rating, win, loss, note)| {
        format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            html_escape(rating),
            html_escape(win),
            html_escape(loss),
            html_escape(note)
        )
    })
    .collect::<Vec<_>>()
    .join("");
    let role_rows = [
        ("의사", "마피아 공격 치료 성공", "+5"),
        ("경찰", "마피아팀 조사 성공", "+4"),
        ("자경단원", "마피아팀 숙청 처형 성공", "+6"),
        ("용병", "의뢰 처형 성공", "+6"),
        ("성직자", "소생 성공", "+6"),
        ("스파이/마녀/청부업자", "마피아팀 접선", "+4"),
        ("교주", "포교 성공", "+5"),
        ("군인", "방탄 발동", "+5"),
        ("테러리스트", "적팀 반격", "+6"),
        ("도둑", "도벽 실행 + 접선", "+3 / +2"),
        ("최면술사", "비시민 직업 확인", "대상당 +3, 최대 +9"),
        (
            "핵심 능력 미사용",
            "2일차 이후까지 생존했는데 능력 미사용",
            "-2",
        ),
    ]
    .into_iter()
    .map(|(role, action, points)| {
        format!(
            "<tr><td>{}</td><td>{}</td><td class=\"num\">{}</td></tr>",
            html_escape(role),
            html_escape(action),
            html_escape(points)
        )
    })
    .collect::<Vec<_>>()
    .join("");
    let body = format!(
        r#"<p class="meta">레이팅은 승패를 기본으로 역할 활약과 상대 난이도를 더해 계산합니다. 마피아 게임 특성상 패배는 절반쯤 강제되기 때문에, 승리(+24)가 패배(-12)보다 두 배 무겁게 설계되어 있습니다. 절반만 이겨도 점수는 꾸준히 오릅니다.</p>
<section class="grid">
  <div class="card"><span>초기 레이팅</span><strong>1000점 (실버)</strong></div>
  <div class="card"><span>승리 기본</span><strong>+24점 (최소 +5 보장)</strong></div>
  <div class="card"><span>패배 기본</span><strong>-12점 (최대 -20)</strong></div>
  <div class="card"><span>역할 보정</span><strong>±14점</strong></div>
  <div class="card"><span>연승 보너스</span><strong>연승당 +3, 최대 +12점</strong></div>
  <div class="card"><span>티어 산정</span><strong>유동 커트라인 (상대 백분위)</strong></div>
</section>
<section class="panel">
  <h2>점수 계산</h2>
  <p class="meta">승리 +24 / 패배 -12에서 시작해 역할 활약 점수(±14), 연승 보너스, 상대 난이도 보정(±25%)이 더해집니다. 승리는 아무리 깎여도 +5점이 보장되고, 패배는 활약으로 0점까지 줄일 수 있지만 오르지는 않으며 한 판에 -20점을 넘게 잃지 않습니다. 첫 10판은 배치 구간이라 승리는 1.5배로 오르고 패배는 절반만 잃습니다. 첫 사망자가 패배하면 손실의 25%를 추가로 완화합니다.</p>
  <table><thead><tr><th>상황</th><th>기본</th><th>보정</th><th>비고</th></tr></thead><tbody>{gain_rows}</tbody></table>
</section>
<section class="panel">
  <h2>랭크표 (유동 커트라인)</h2>
  <p class="meta">랭크는 고정 점수가 아니라 배치(10판)를 마친 플레이어들 사이의 상대 위치로 정해집니다. 커트라인이 실제 분포를 따라 움직이므로, 내 점수가 그대로여도 다른 사람이 치고 올라오면 랭크가 내려갈 수 있습니다. 현재 커트라인은 디스코드 `/랭크컷` 명령어로 확인할 수 있습니다.</p>
  <table><thead><tr><th>랭크</th><th>기준 (백분위)</th><th>설명</th></tr></thead><tbody>{rank_rows}</tbody></table>
</section>
<section class="panel">
  <h2>역할 기여 점수</h2>
  <p class="meta">승패 점수와 별개로 역할을 잘 수행하면 추가 점수를 받습니다. 한 판 역할 보정은 최종적으로 -14점부터 +14점까지만 반영됩니다.</p>
  <table><thead><tr><th>역할</th><th>대표 기여</th><th class="num">점수</th></tr></thead><tbody>{role_rows}</tbody></table>
</section>
<section class="panel">
  <h2>게임 끝나고 보이는 로그 읽는 법</h2>
  <pre>- 닉네임 (의사) 1200 -&gt; 1232 (+32) [팀 +24 / 직업 +5 / 연승 +3]
  사유: 소속 진영 승리, 마피아 공격 치료 성공 +5, 2연승 보너스 +3</pre>
  <p class="meta">팀 점수는 승패(+24/-12)에 상대 난이도·배치 보정을 곱한 값이고, 직업 점수는 해당 판 활약입니다. 합산 뒤 승리 최소 +5 / 패배 0~-20 범위로 자르고, 첫 사망 완화와 티어 강등 보호를 적용한 값이 최종 변화량입니다.</p>
</section>
<section class="panel">
  <h2>자주 묻는 질문</h2>
  <table><tbody>
    <tr><th>졌는데 왜 점수가 안 깎였나요?</th><td>역할 활약 점수가 손실을 상쇄했기 때문입니다. 패배로 점수가 오르지는 않습니다.</td></tr>
    <tr><th>제일 먼저 죽고 졌는데 왜 덜 깎였나요?</th><td>첫 사망자는 게임에 영향을 줄 기회가 가장 적으므로, 패배 시 최종 손실의 25%를 완화합니다.</td></tr>
    <tr><th>마피아 판은 무조건 지는 판도 있는데 손해 아닌가요?</th><td>그래서 승리가 패배의 두 배입니다. 승률 50%면 판당 평균 +6점씩 오르고, 3판 중 1판만 이겨도 본전입니다.</td></tr>
    <tr><th>티어가 떨어질 수도 있나요?</th><td>네. 티어는 상대 위치라서 내가 지거나 다른 사람이 올라오면 내려갈 수 있습니다. 대신 점수 자체는 한 판에 -20을 넘게 잃지 않습니다.</td></tr>
    <tr><th>역할 행동을 실패하면 무조건 감점인가요?</th><td>아닙니다. 능력을 제출했다면 핵심 능력 미사용 감점은 피합니다. 성공 이벤트가 없으면 추가 점수만 없는 구조입니다.</td></tr>
    <tr><th>랭크는 어디서 보나요?</th><td>내정보, 리더보드, 웹 리더보드, API 응답에서 볼 수 있습니다.</td></tr>
  </tbody></table>
</section>"#
    );
    base_html("마피아 레이팅 설명", &body, false)
}

pub(crate) fn render_roles_page() -> String {
    let sections = [
        (
            "시민팀",
            "시민팀은 공개 정보와 밤 행동 결과를 모아 마피아팀을 제거하는 진영입니다.",
        ),
        (
            "마피아팀",
            "마피아팀은 밤 행동과 낮 발언을 맞춰 시민팀의 추론을 흔드는 진영입니다.",
        ),
        (
            "교주팀",
            "교주팀은 포교로 독자 세력을 만들고 숫자 우위를 노리는 진영입니다.",
        ),
        ("중립", "중립 역할은 별도 승리 조건을 중심으로 움직입니다."),
        (
            "상태",
            "상태 항목은 특정 역할이 만드는 임시 상태 설명입니다.",
        ),
    ];
    let mut body = String::from(
        r#"<p class="meta">메인 웹 전용 역할 설명입니다. 디스코드 명령어 설명은 그대로 유지하고, 여기서는 판 운영에 필요한 세부 규칙과 판단 포인트를 길게 보여줍니다.</p>"#,
    );
    for (team, description) in sections {
        let cards = WEB_ROLE_GUIDES
            .iter()
            .filter(|guide| guide.team == team)
            .map(render_role_card)
            .collect::<Vec<_>>()
            .join("");
        if cards.is_empty() {
            continue;
        }
        let count = WEB_ROLE_GUIDES
            .iter()
            .filter(|guide| guide.team == team)
            .count();
        let _ = write!(
            body,
            r#"<section class="panel role-section"><h2>{}<span class="pill">{}개</span></h2><p class="meta">{}</p><div class="role-grid">{}</div></section>"#,
            html_escape(team),
            count,
            html_escape(description),
            cards
        );
    }
    base_html("마피아 역할 설명", &body, false)
}

pub(crate) fn render_tier_ability_cards(abilities: &[TierAbility]) -> String {
    abilities
        .iter()
        .map(|ability| {
            format!(
                r#"<article class="role-card"><div class="role-head"><div class="role-title"><h3>{}</h3></div><div class="role-tags"><span class="pill">{}티어</span></div></div><p class="role-summary">{}</p></article>"#,
                html_escape(ability.value()),
                ability.tier(),
                html_escape(ability.description())
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

pub(crate) fn render_tier_section(
    title: &str,
    description: &str,
    abilities: &[TierAbility],
) -> String {
    if abilities.is_empty() {
        return String::new();
    }
    format!(
        r#"<section class="panel role-section"><h2>{}<span class="pill">{}개</span></h2><p class="meta">{}</p><div class="role-grid">{}</div></section>"#,
        html_escape(title),
        abilities.len(),
        html_escape(description),
        render_tier_ability_cards(abilities)
    )
}

/// 티어 능력 설명 페이지. 풀 구성은 실제 배정 로직(tier4_pool)에서 그대로
/// 가져와 코드와 자동으로 동기화된다.
pub(crate) fn render_tiers_page() -> String {
    let mut body = String::from(
        r#"<section class="panel"><h2>개인 티어 시스템</h2><p class="meta">게임마다 모든 플레이어가 티어를 비공개로 배정받습니다. 내 티어와 능력은 게임 시작 시 역할 DM에서 확인할 수 있고, 게임 결과에서 전원의 티어가 공개됩니다.</p><ul><li>확률: 2티어 40% / 3티어 30% / 4티어 15% / 5티어 10% / 6티어 5%</li><li>2티어는 능력이 없고, 3티어는 3티어 능력 1개를 받습니다.</li><li>4티어부터는 소속·역할에 맞는 풀에서 4티어 이상 능력을 받습니다 — 4티어 1개, 5티어 2개, 6티어 3개 (서로 다른 능력).</li><li>4티어 이상 풀이 개수보다 작으면(예: 시민팀 풀은 유언·확성 2개) 나머지는 3티어 능력(가호·달변)으로 채웁니다.</li><li>같은 능력이 여러 플레이어에게 겹쳐서 배정될 수 있습니다.</li></ul></section>"#,
    );
    body.push_str(&render_tier_section(
        "3티어 능력",
        "소속과 무관하게 3티어가 되면 이 중 하나를 받습니다.",
        TIER3_ABILITIES,
    ));
    body.push_str(&render_tier_section(
        "4티어 이상 · 마피아 본대",
        "역할이 '마피아'인 플레이어의 풀입니다.",
        TIER4_MAFIA_ABILITIES,
    ));
    body.push_str(&render_tier_section(
        "4티어 이상 · 마피아팀 보조 공통",
        "스파이·마담·도둑 등 마피아팀 보조 직업이 공통으로 받을 수 있는 능력입니다. 아래 역할 전용 능력과 합쳐진 풀에서 뽑힙니다.",
        TIER4_MAFIA_SUPPORT_ABILITIES,
    ));
    // 역할 전용 능력: 보조 공통 풀에 덧붙는 부분만 뽑아 보여준다.
    let support_common = TIER4_MAFIA_SUPPORT_ABILITIES;
    let exclusive_roles = [
        Role::Spy,
        Role::Fraudster,
        Role::Madam,
        Role::Thief,
        Role::Witch,
        Role::Scientist,
        Role::Contractor,
        Role::Godfather,
    ];
    let mut exclusive_sections = String::new();
    for role in exclusive_roles {
        let extras = tier4_pool(role)
            .into_iter()
            .filter(|ability| !support_common.contains(ability))
            .collect::<Vec<_>>();
        if extras.is_empty() {
            continue;
        }
        let _ = write!(
            exclusive_sections,
            r#"<h3 class="meta">{}</h3><div class="role-grid">{}</div>"#,
            html_escape(role.value()),
            render_tier_ability_cards(&extras)
        );
    }
    if !exclusive_sections.is_empty() {
        let _ = write!(
            body,
            r#"<section class="panel role-section"><h2>4티어 이상 · 보조 역할 전용</h2><p class="meta">해당 역할일 때만 풀에 추가되는 고유 능력입니다 (보조 공통 능력과 합쳐진 풀에서 뽑힙니다).</p>{exclusive_sections}</section>"#,
        );
    }
    body.push_str(&render_tier_section(
        "4티어 이상 · 교주",
        "교주 전용 풀 전체입니다.",
        &tier4_pool(Role::CultLeader),
    ));
    body.push_str(&render_tier_section(
        "4티어 이상 · 그 외 (시민팀·중립)",
        "마피아팀·교주가 아닌 플레이어의 풀입니다.",
        TIER4_CITIZEN_ABILITIES,
    ));
    base_html("티어 능력 설명", &body, false)
}

pub(crate) fn render_role_card(guide: &WebRoleGuide) -> String {
    let tips = guide
        .tips
        .iter()
        .map(|tip| format!("<li>{}</li>", html_escape(tip)))
        .collect::<Vec<_>>()
        .join("");
    let rating_hint = role_rating_hint(guide.role);
    let hover_text = format!(
        "{}: {} 레이팅 요소: {} 주의: {}",
        guide.role.value(),
        guide.summary,
        rating_hint,
        guide.caution
    );
    format!(
        r#"<article class="role-card"><div class="role-head"><div class="role-title"><h3>{}</h3><span class="role-help" tabindex="0" aria-label="{} 상세 설명" data-tip="{}">?</span></div><div class="role-tags"><span class="pill">{}</span><span class="pill">{}</span></div></div><p class="role-summary">{}</p><p class="role-rating"><strong>레이팅:</strong> {}</p><h4>운영 포인트</h4><ul>{}</ul><p class="role-note"><strong>주의:</strong> {}</p></article>"#,
        html_escape(guide.role.value()),
        html_escape(guide.role.value()),
        html_escape(&hover_text),
        html_escape(guide.team),
        html_escape(guide.kind),
        html_escape(guide.summary),
        html_escape(rating_hint),
        tips,
        html_escape(guide.caution)
    )
}

pub(crate) fn role_rating_hint(role: Role) -> &'static str {
    match role {
        Role::Citizen => "생존 승리, 공개 정보 정리, 투표 기여를 중심으로 평가합니다.",
        Role::Police => "조사 결과 공개와 생존한 정보 유지 기여를 크게 봅니다.",
        Role::Doctor => "치료 성공, 핵심 직업 보호, 보호 동선 판단을 평가합니다.",
        Role::Nurse => "보호 보조와 핵심 직업 생존 지원을 평가합니다.",
        Role::Agent => "수사 결과로 의심 대상을 좁힌 기여를 평가합니다.",
        Role::Vigilante => "정확한 조사와 처형 압박, 오처형 회피를 평가합니다.",
        Role::Inspector => "같은 팀 수사 성공, 직업 정보 공유, 정체 공개 타이밍을 평가합니다.",
        Role::Reporter => "취재 공개 정보가 투표 판단에 준 기여를 평가합니다.",
        Role::Hacker => "행동 정보로 거짓 직업 주장이나 밤 동선을 잡은 기여를 평가합니다.",
        Role::Detective => "추적 결과를 누적해 행동 모순을 밝힌 기여를 평가합니다.",
        Role::Shaman => "사망자 정보와 공개 추론을 연결한 기여를 평가합니다.",
        Role::Priest => "부활 또는 정화 선택으로 판세를 바꾼 기여를 평가합니다.",
        Role::Soldier => "방탄 생존과 공격 유도 정보 제공을 평가합니다.",
        Role::Gangster => "투표 제어와 핵심 타이밍 방해 기여를 평가합니다.",
        Role::Prophet => "장기 생존과 예언 타이밍으로 만든 확정 정보를 평가합니다.",
        Role::Psychologist => "관계 분석으로 팀 구도를 좁힌 기여를 평가합니다.",
        Role::Hypnotist => "최면 누적과 해제 타이밍으로 얻은 판별 정보를 평가합니다.",
        Role::Mercenary => "의뢰인 보호, 의뢰 달성 뒤 처형 판단을 평가합니다.",
        Role::Lover => "연인 생존 연계와 공개 타이밍 조절을 평가합니다.",
        Role::Fraudster => "변장으로 조사를 속이고 교섭 접선을 살린 기여를 평가합니다.",
        Role::CivilServant => "조회 성공으로 직업 정보를 확보한 기여를 평가합니다.",
        Role::Paparazzi => "공유받은 직업 정보를 추리와 투표에 연결한 기여를 평가합니다.",
        Role::Mafia => "밤 처형 성공, 낮 발언 교란, 팀 승리 기여를 평가합니다.",
        Role::Spy => "접선, 정보 전달, 시민팀 추론 방해를 평가합니다.",
        Role::Contractor => "청부 표적 압박과 마피아팀 승리 보조를 평가합니다.",
        Role::Thief => "탈취한 능력을 독립적으로 활용한 기여를 평가합니다.",
        Role::Witch => "저주로 시민팀 행동을 흔든 기여를 평가합니다.",
        Role::Scientist => "마피아팀 승리 기여와 첫 사망·부활 뒤 생존 변수 창출을 평가합니다.",
        Role::Madam => "접대, 접선, 밤 대화 합류 후 정보 공유를 평가합니다.",
        Role::Graverobber => "도굴한 역할의 가치와 이후 행동 기여를 평가합니다.",
        Role::Godfather => "마피아팀 지휘, 은폐, 처형 우선순위 판단을 평가합니다.",
        Role::Villain => "마피아팀 보조와 낮 발언 교란 기여를 평가합니다.",
        Role::CultLeader => "포교 성공, 교주팀 생존, 숫자 우위 운영을 평가합니다.",
        Role::Fanatic => "교주팀 보조와 포교 이후 정보 교란 기여를 평가합니다.",
        Role::Joker => "단독 승리 조건 달성과 처형 유도 성공을 크게 평가합니다.",
        Role::Politician => "찬반투표와 공개 정치 운영으로 만든 판세 기여를 평가합니다.",
        Role::Judge => "처형 판정으로 확정 구도를 만든 기여를 평가합니다.",
        Role::Terrorist => "교환 압박과 희생 타이밍으로 만든 판세 기여를 평가합니다.",
        Role::Frog => "개구리 상태에서 생존하거나 정보 혼선을 관리한 기여를 평가합니다.",
    }
}

pub(crate) fn render_api_docs_page(base_url: &str) -> String {
    let base_url = base_url.trim_end_matches('/');
    let api_url = format!("{base_url}/api");
    let protected_api_url = format!("{api_url}/v1");
    let public_endpoints = [
        ("GET /health", "봇 웹 서버가 살아 있는지 확인합니다."),
        (
            "GET /api/status",
            "봇 연결 상태, 진행 중 게임, 공개 설정 요약을 반환합니다.",
        ),
        ("GET /api/games", "진행 중 게임 목록만 반환합니다."),
        (
            "GET /api/settings",
            "공개 가능한 게임 설정 요약을 반환합니다.",
        ),
        ("GET /api/stats", "전적 요약 정보를 반환합니다."),
        (
            "GET /api/leaderboard",
            "레이팅 기준 리더보드를 반환합니다. 각 항목에 rating_rank가 포함됩니다.",
        ),
        (
            "GET /api/leaderboard/{metric}",
            "wins, streak, winrate, games, mafia, playtime, rating 기준 리더보드를 반환합니다. 각 항목에 rating_rank와 win_streak가 포함됩니다.",
        ),
    ];
    let protected_endpoints = [
        (
            "GET /api/v1/me",
            "API 키 정보와 서버 범위를 반환합니다. API 키 필요.",
        ),
        (
            "GET /api/v1/config",
            "게임 설정 요약을 반환합니다. API 키 필요.",
        ),
        ("GET /api/v1/stats", "전적 요약을 반환합니다. API 키 필요."),
        (
            "GET /api/v1/stats/leaderboard",
            "Laravel-friendly leaderboard JSON. Query: sort, limit. API key required.",
        ),
        (
            "GET /stats/leaderboard",
            "Alias for Laravel spec. Query: sort, limit. API key required.",
        ),
        (
            "GET /api/v1/stats/user/{user_id}",
            "Laravel-friendly user profile stats. API key required.",
        ),
        (
            "GET /api/v1/stats/user/{user_id}/games",
            "Laravel-friendly user game history. Query: page, per_page. API key required.",
        ),
        (
            "GET /stats/user/{user_id}",
            "Alias for Laravel spec. API key required.",
        ),
        (
            "GET /stats/user/{user_id}/games",
            "Alias for Laravel spec. Query: page, per_page. API key required.",
        ),
        (
            "GET /api/v1/leaderboard/{metric}",
            "보호 리더보드를 반환합니다. streak 정렬과 win_streak 필드가 포함됩니다. API 키 필요.",
        ),
        (
            "GET /api/v1/games",
            "키 발급 서버의 진행 중 게임을 반환합니다. API 키 필요.",
        ),
        (
            "GET /api/v1/games/recent",
            "Laravel-friendly recent completed games. Query: page, limit/per_page. API key required.",
        ),
        (
            "GET /games/recent",
            "Alias for Laravel spec. Query: page, limit/per_page. API key required.",
        ),
        (
            "GET /api/v1/game/{game_key}",
            "Laravel-friendly completed game summary by replay game_key. API key required.",
        ),
        (
            "GET /api/v1/game/{game_key}/result",
            "Laravel-friendly completed game result summary. API key required.",
        ),
        (
            "GET /api/v1/game/{game_key}/events",
            "Laravel-friendly replay timeline events. API key required.",
        ),
        (
            "GET /game/{game_key}",
            "Alias for Laravel spec. API key required.",
        ),
        (
            "GET /game/{game_key}/result",
            "Alias for Laravel spec. API key required.",
        ),
        (
            "GET /game/{game_key}/events",
            "Alias for Laravel spec. API key required.",
        ),
        (
            "GET /api/v1/games/{guild_id}",
            "참가자, 직업, 단계, 타이머를 포함한 게임 상세를 반환합니다. API 키 필요.",
        ),
        (
            "GET /api/v1/games/{guild_id}/replay",
            "Replay JSON with participants, votes, role actions, phase results, and rating log. API key required.",
        ),
        (
            "POST /api/v1/games/{guild_id}/actions",
            "JSON action: skip_day, extend_day 또는 stop. API 키 필요.",
        ),
        (
            "GET /api/v1/replays",
            "Recent replay summaries for the API key guild. Includes active game and completed games.",
        ),
        (
            "GET /api/v1/replays/{game_key}",
            "Full replay JSON by game_key. API key required.",
        ),
        (
            "GET /api/v1/recruitments/{guild_id}",
            "모집 인원과 역할 구성을 반환합니다. API 키 필요.",
        ),
        (
            "POST /api/v1/recruitments/{guild_id}/actions",
            "JSON action: start 또는 cancel. API 키 필요.",
        ),
    ];
    let render_rows = |endpoints: &[(&str, &str)]| {
        endpoints
            .iter()
            .map(|(path, desc)| {
                format!(
                    r#"<div class="endpoint"><code>{}</code><span>{}</span></div>"#,
                    html_escape(path),
                    html_escape(desc)
                )
            })
            .collect::<Vec<_>>()
            .join("")
    };
    let public_rows = render_rows(&public_endpoints);
    let protected_rows = render_rows(&protected_endpoints);
    let body = format!(
        r#"<p class="meta">기본 API 주소는 <code>{api_url}</code>입니다. 모든 응답은 JSON이며, <code>limit</code>은 1~50 범위입니다.</p>
<section class="panel"><h2>인증</h2><p>관리자는 <code>/마피아웹설정</code>에서 서버 전용 API 키를 발급합니다. 보호 API는 키 발급 서버의 데이터와 작업만 허용합니다.</p><pre>X-API-Key: mfr_...
Authorization: Bearer mfr_...</pre></section>
<section class="panel"><h2>공개 조회 API</h2>{public_rows}</section>
<section class="panel"><h2>보호 관리 API</h2>{protected_rows}</section>
<section class="panel"><h2>관리 작업 본문</h2><pre>POST {protected_api_url}/games/{{guild_id}}/actions
{{"action":"skip_day"}}   # 낮 토론 즉시 종료
{{"action":"extend_day"}} # 연장 투표 중 1분 연장 승인
{{"action":"stop"}}       # 게임 종료

POST {protected_api_url}/recruitments/{{guild_id}}/actions
{{"action":"start"}}      # 최소 인원 충족 시 즉시 시작
{{"action":"cancel"}}     # 모집 취소</pre></section>
<section class="panel"><h2>응답 코드</h2><pre>200 성공 · 400 잘못된 요청 · 401 키 없음/오류 · 403 다른 서버 키 · 404 대상 없음 · 409 현재 상태에서 작업 불가</pre></section>
<section class="panel"><h2>호출 예시</h2><pre>curl -H "X-API-Key: mfr_..." {protected_api_url}/games/123

curl -X POST -H "Authorization: Bearer mfr_..." -H "Content-Type: application/json" \
  -d '{{"action":"skip_day"}}' {protected_api_url}/games/123/actions</pre></section>"#,
        api_url = html_escape(&api_url),
        protected_api_url = html_escape(&protected_api_url),
    );
    base_html("마피아 봇 API 문서", &body, false)
}

pub(crate) fn render_field(field: WebConfigField, config: &BotConfig) -> String {
    let field_id = format!("field_{}", field.name);
    let label = html_escape(field.label);
    match field.kind {
        WebFieldKind::Bool => {
            let checked = if config_value(config, field.name) == "true" {
                " checked"
            } else {
                ""
            };
            format!(
                r#"<label class="row" for="{field_id}"><span>{label}</span><input type="checkbox" id="{field_id}" name="{}"{checked}></label>"#,
                field.name
            )
        }
        WebFieldKind::Int => {
            let min_attr = field
                .min_value
                .map(|value| format!(r#" min="{value}""#))
                .unwrap_or_default();
            format!(
                r#"<label class="row" for="{field_id}"><span>{label}</span><input type="number" id="{field_id}" name="{}" value="{}"{min_attr} required></label>"#,
                field.name,
                html_escape(&config_value(config, field.name))
            )
        }
        WebFieldKind::Text => format!(
            r#"<label class="row" for="{field_id}"><span>{label}</span><input type="text" id="{field_id}" name="{}" value="{}" required></label>"#,
            field.name,
            html_escape(&config_value(config, field.name))
        ),
        WebFieldKind::IntList => {
            let value = config
                .blacklist_user_ids
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                r#"<label class="row" for="{field_id}"><span>{label}<br><small>한 줄에 하나씩, 또는 쉼표/공백으로 구분</small></span><textarea id="{field_id}" name="{}">{}</textarea></label>"#,
                field.name,
                html_escape(&value)
            )
        }
    }
}

pub(crate) fn render_message_page(title: &str, message: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex, nofollow">
<title>{}</title>
{WEB_PAGE_STYLE}
</head>
<body>
<div class="site-shell">
{}
<p>{}</p>
</main>
</div>
</body>
</html>"#,
        html_escape(title),
        render_page_header(title, false),
        html_escape(message)
    )
}

pub(crate) fn expired_page() -> String {
    render_message_page(
        "🔒 링크가 만료되었습니다",
        "이 링크는 더 이상 유효하지 않습니다. 디스코드에서 /마피아웹설정 명령어를 다시 실행해 새 링크를 발급받으세요.",
    )
}

pub(crate) fn saved_page() -> String {
    render_message_page(
        "✅ 설정을 저장했습니다",
        "마피아 게임 설정이 반영되었습니다. 이 창은 닫으셔도 됩니다.",
    )
}

pub(crate) fn config_value(config: &BotConfig, name: &str) -> String {
    match name {
        "participant_role" => config.participant_role.clone(),
        "manager_role" => config.manager_role.clone(),
        "game_enabled" => config.game_enabled.to_string(),
        "max_player_count" => config.max_player_count.to_string(),
        "recruitment_seconds" => config.recruitment_seconds.to_string(),
        "night_seconds" => config.night_seconds.to_string(),
        "discussion_seconds" => config.discussion_seconds.to_string(),
        "vote_seconds" => config.vote_seconds.to_string(),
        "chat_slowmode_seconds" => config.chat_slowmode_seconds.to_string(),
        "default_mafia_count" => config.default_mafia_count.to_string(),
        "default_doctor_count" => (config.default_doctor_count > 0).to_string(),
        "default_police_count" => (config.default_police_count > 0).to_string(),
        "default_joker_count" => config.default_joker_count.to_string(),
        "citizen_special_count" => config.citizen_special_count.to_string(),
        "mafia_special_count" => config.mafia_special_count.to_string(),
        "neutral_special_count" => config.neutral_special_count.to_string(),
        "reveal_death_roles" => config.reveal_death_roles.to_string(),
        "reveal_public_police_status" => config.reveal_public_police_status.to_string(),
        "reveal_morning_mafia_count" => config.reveal_morning_mafia_count.to_string(),
        "show_confirmation_vote_counts" => config.show_confirmation_vote_counts.to_string(),
        "anonymous_mode" => config.anonymous_mode.to_string(),
        "anonymous_name_mode" => config.anonymous_name_mode.clone(),
        "use_agent" => config.use_agent.to_string(),
        "use_vigilante" => config.use_vigilante.to_string(),
        "enable_detective" => config.enable_detective.to_string(),
        "enable_inspector" => config.enable_inspector.to_string(),
        "enable_graverobber" => config.enable_graverobber.to_string(),
        "enable_spy" => config.enable_spy.to_string(),
        "enable_contractor" => config.enable_contractor.to_string(),
        "enable_fraudster" => config.enable_fraudster.to_string(),
        "enable_witch" => config.enable_witch.to_string(),
        "enable_scientist" => config.enable_scientist.to_string(),
        "enable_madam" => config.enable_madam.to_string(),
        "enable_godfather" => config.enable_godfather.to_string(),
        "enable_joker" => config.enable_joker.to_string(),
        "enable_politician" => config.enable_politician.to_string(),
        "enable_judge" => config.enable_judge.to_string(),
        "enable_reporter" => config.enable_reporter.to_string(),
        "enable_hacker" => config.enable_hacker.to_string(),
        "enable_terrorist" => config.enable_terrorist.to_string(),
        "enable_lover" => config.enable_lover.to_string(),
        "enable_civil_servant" => config.enable_civil_servant.to_string(),
        "enable_paparazzi" => config.enable_paparazzi.to_string(),
        "enable_shaman" => config.enable_shaman.to_string(),
        "enable_priest" => config.enable_priest.to_string(),
        "enable_soldier" => config.enable_soldier.to_string(),
        "enable_nurse" => config.enable_nurse.to_string(),
        "enable_gangster" => config.enable_gangster.to_string(),
        "enable_prophet" => config.enable_prophet.to_string(),
        "enable_psychologist" => config.enable_psychologist.to_string(),
        "enable_hypnotist" => config.enable_hypnotist.to_string(),
        "enable_mercenary" => config.enable_mercenary.to_string(),
        "enable_thief" => config.enable_thief.to_string(),
        "enable_cult_team" => config.enable_cult_team.to_string(),
        "blacklist_user_ids" => config
            .blacklist_user_ids
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

pub(crate) fn parse_form_updates(
    body: &str,
) -> std::result::Result<HashMap<String, String>, String> {
    let raw_form = parse_urlencoded(body);
    let mut updates = HashMap::new();
    for field in WEB_CONFIG_FIELDS {
        if matches!(field.kind, WebFieldKind::Bool) {
            updates.insert(
                field.name.to_string(),
                raw_form.contains_key(field.name).to_string(),
            );
            continue;
        }
        let raw_value = raw_form
            .get(field.name)
            .ok_or_else(|| format!("'{}' 값이 비어 있습니다.", field.label))?;
        let text_value = raw_value.trim();
        if matches!(field.kind, WebFieldKind::IntList) && text_value.is_empty() {
            updates.insert(field.name.to_string(), String::new());
            continue;
        }
        if text_value.is_empty() {
            return Err(format!("'{}' 값이 비어 있습니다.", field.label));
        }
        if matches!(field.kind, WebFieldKind::Int) {
            let parsed = text_value
                .parse::<u64>()
                .map_err(|_| format!("'{}' 값은 숫자여야 합니다.", field.label))?;
            if let Some(min_value) = field.min_value
                && parsed < min_value
            {
                return Err(format!(
                    "'{}' 값은 {min_value} 이상이어야 합니다.",
                    field.label
                ));
            }
        }
        updates.insert(field.name.to_string(), text_value.to_string());
    }
    Ok(updates)
}

pub(crate) fn apply_updates(
    config: &mut BotConfig,
    updates: &HashMap<String, String>,
) -> std::result::Result<(), String> {
    let previous = config.clone();
    for field in WEB_CONFIG_FIELDS {
        let value = updates
            .get(field.name)
            .ok_or_else(|| format!("'{}' 값이 비어 있습니다.", field.label))?;
        match field.kind {
            WebFieldKind::Bool => set_bool(config, field.name, value == "true")?,
            WebFieldKind::Text => set_text(config, field.name, value.clone())?,
            WebFieldKind::Int => set_int(config, field.name, value.parse::<u64>().unwrap_or(0))?,
            WebFieldKind::IntList => set_int_list(config, field.name, value)?,
        }
    }
    if let Err(error) = validate_config(config) {
        *config = previous;
        return Err(error);
    }
    Ok(())
}

pub(crate) fn set_bool(
    config: &mut BotConfig,
    name: &str,
    value: bool,
) -> std::result::Result<(), String> {
    match name {
        "game_enabled" => config.game_enabled = value,
        "reveal_death_roles" => config.reveal_death_roles = value,
        "reveal_public_police_status" => config.reveal_public_police_status = value,
        "reveal_morning_mafia_count" => config.reveal_morning_mafia_count = value,
        "show_confirmation_vote_counts" => config.show_confirmation_vote_counts = value,
        "anonymous_mode" => config.anonymous_mode = value,
        "use_agent" => config.use_agent = value,
        "use_vigilante" => config.use_vigilante = value,
        "enable_detective" => config.enable_detective = value,
        "enable_inspector" => config.enable_inspector = value,
        "default_doctor_count" => config.default_doctor_count = u32::from(value),
        "default_police_count" => config.default_police_count = u32::from(value),
        "enable_graverobber" => config.enable_graverobber = value,
        "enable_spy" => config.enable_spy = value,
        "enable_contractor" => config.enable_contractor = value,
        "enable_fraudster" => config.enable_fraudster = value,
        "enable_witch" => config.enable_witch = value,
        "enable_scientist" => config.enable_scientist = value,
        "enable_madam" => config.enable_madam = value,
        "enable_godfather" => config.enable_godfather = value,
        "enable_joker" => config.enable_joker = value,
        "enable_politician" => config.enable_politician = value,
        "enable_judge" => config.enable_judge = value,
        "enable_reporter" => config.enable_reporter = value,
        "enable_hacker" => config.enable_hacker = value,
        "enable_terrorist" => config.enable_terrorist = value,
        "enable_lover" => config.enable_lover = value,
        "enable_civil_servant" => config.enable_civil_servant = value,
        "enable_paparazzi" => config.enable_paparazzi = value,
        "enable_shaman" => config.enable_shaman = value,
        "enable_priest" => config.enable_priest = value,
        "enable_soldier" => config.enable_soldier = value,
        "enable_nurse" => config.enable_nurse = value,
        "enable_gangster" => config.enable_gangster = value,
        "enable_prophet" => config.enable_prophet = value,
        "enable_psychologist" => config.enable_psychologist = value,
        "enable_hypnotist" => config.enable_hypnotist = value,
        "enable_mercenary" => config.enable_mercenary = value,
        "enable_thief" => config.enable_thief = value,
        "enable_cult_team" => config.enable_cult_team = value,
        _ => return Err("알 수 없는 설정 항목입니다.".to_string()),
    }
    Ok(())
}

pub(crate) fn set_text(
    config: &mut BotConfig,
    name: &str,
    value: String,
) -> std::result::Result<(), String> {
    match name {
        "participant_role" => config.participant_role = value,
        "manager_role" => config.manager_role = value,
        "anonymous_name_mode" => config.anonymous_name_mode = value,
        _ => return Err("알 수 없는 설정 항목입니다.".to_string()),
    }
    Ok(())
}

pub(crate) fn set_int(
    config: &mut BotConfig,
    name: &str,
    value: u64,
) -> std::result::Result<(), String> {
    match name {
        "max_player_count" => config.max_player_count = value as u32,
        "recruitment_seconds" => {
            config.recruitment_seconds = value.clamp(
                config::MIN_RECRUITMENT_SECONDS,
                config::MAX_RECRUITMENT_SECONDS,
            )
        }
        "night_seconds" => config.night_seconds = value,
        "discussion_seconds" => config.discussion_seconds = value,
        "vote_seconds" => config.vote_seconds = value,
        "chat_slowmode_seconds" => config.chat_slowmode_seconds = value,
        "default_mafia_count" => config.default_mafia_count = value as u32,
        "default_joker_count" => config.default_joker_count = value as u32,
        "citizen_special_count" => config.citizen_special_count = value as u32,
        "mafia_special_count" => config.mafia_special_count = value as u32,
        "neutral_special_count" => config.neutral_special_count = value as u32,
        _ => return Err("알 수 없는 설정 항목입니다.".to_string()),
    }
    Ok(())
}

pub(crate) fn set_int_list(
    config: &mut BotConfig,
    name: &str,
    value: &str,
) -> std::result::Result<(), String> {
    match name {
        "blacklist_user_ids" => {
            let normalized = value.replace(',', " ");
            let mut values = Vec::new();
            for chunk in normalized.split_whitespace() {
                values.push(chunk.parse::<u64>().map_err(|_| {
                    "블랙리스트 유저 ID 목록에는 숫자 ID만 입력할 수 있습니다.".to_string()
                })?);
            }
            values.sort_unstable();
            values.dedup();
            config.blacklist_user_ids = values;
        }
        _ => return Err("알 수 없는 설정 항목입니다.".to_string()),
    }
    Ok(())
}

pub(crate) fn validate_config(config: &BotConfig) -> std::result::Result<(), String> {
    if config.default_mafia_count < 1 {
        return Err("마피아는 최소 1명이어야 합니다.".to_string());
    }
    if !can_fill_special_slots(
        config,
        CITIZEN_SPECIAL_ROLES,
        config.citizen_special_count as usize,
    ) {
        return Err(
            "활성화된 시민 특수 역할로 설정한 인원 수를 구성할 수 없습니다. 연인은 2명으로 계산됩니다."
                .to_string(),
        );
    }
    let mafia_enabled = enabled_special_count(config, MAFIA_SPECIAL_ROLES);
    if config.mafia_special_count as usize > mafia_enabled {
        return Err("마피아 특수룰 수가 활성화된 마피아 특수 역할보다 많습니다.".to_string());
    }
    let neutral_enabled = enabled_special_count(config, NEUTRAL_SPECIAL_ROLES);
    if config.neutral_special_count as usize > neutral_enabled {
        return Err("중립 특수룰 수가 활성화된 중립 특수 역할보다 많습니다.".to_string());
    }
    if config.mafia_special_count > config.default_mafia_count {
        return Err(format!(
            "마피아 특수룰 수는 전체 마피아 수보다 많을 수 없습니다. 현재 마피아 {}명, 마피아 특수 {}명입니다.",
            config.default_mafia_count, config.mafia_special_count
        ));
    }
    if config
        .default_mafia_count
        .saturating_sub(config.mafia_special_count)
        < 1
    {
        return Err("접선 전 특수 마피아만으로는 게임을 진행할 수 없습니다. 일반 마피아가 최소 1명 필요합니다.".to_string());
    }
    let minimum_players = minimum_player_count(config);
    let max_players = if config.max_player_count == 0 {
        MAX_GAME_PLAYERS
    } else {
        (config.max_player_count as usize).min(MAX_GAME_PLAYERS)
    };
    if max_players < minimum_players {
        return Err(format!(
            "현재 설정의 최소 시작 인원은 {minimum_players}명이라 최대 인원 {max_players}명으로 시작할 수 없습니다."
        ));
    }
    Ok(())
}

pub(crate) fn enabled_special_count(config: &BotConfig, roles: &[Role]) -> usize {
    roles
        .iter()
        .filter(|role| special_role_enabled(config, **role))
        .count()
}

pub(crate) fn special_role_enabled(config: &BotConfig, role: Role) -> bool {
    match role {
        Role::Inspector => config.enable_inspector,
        Role::Detective => config.enable_detective,
        Role::Graverobber => config.enable_graverobber,
        Role::Spy => config.enable_spy,
        Role::Contractor => config.enable_contractor,
        Role::Fraudster => config.enable_fraudster,
        Role::Witch => config.enable_witch,
        Role::Scientist => config.enable_scientist,
        Role::Madam => config.enable_madam,
        Role::Godfather => config.enable_godfather,
        Role::Joker => config.enable_joker,
        Role::Politician => config.enable_politician,
        Role::Judge => config.enable_judge,
        Role::Reporter => config.enable_reporter,
        Role::Hacker => config.enable_hacker,
        Role::Terrorist => config.enable_terrorist,
        Role::Lover => config.enable_lover,
        Role::CivilServant => config.enable_civil_servant,
        Role::Paparazzi => config.enable_paparazzi,
        Role::Shaman => config.enable_shaman,
        Role::Priest => config.enable_priest,
        Role::Soldier => config.enable_soldier,
        Role::Nurse => config.enable_nurse,
        Role::Gangster => config.enable_gangster,
        Role::Prophet => config.enable_prophet,
        Role::Psychologist => config.enable_psychologist,
        Role::Hypnotist => config.enable_hypnotist,
        Role::Mercenary => config.enable_mercenary,
        Role::Thief => config.enable_thief,
        _ => true,
    }
}

pub(crate) fn special_role_player_count(role: Role) -> usize {
    if role == Role::Lover { 2 } else { 1 }
}

pub(crate) fn can_fill_special_slots(
    config: &BotConfig,
    roles: &[Role],
    target_slots: usize,
) -> bool {
    let mut possible = vec![false; target_slots + 1];
    possible[0] = true;
    for slots in roles
        .iter()
        .filter(|role| special_role_enabled(config, **role))
        .map(|role| special_role_player_count(*role))
    {
        if slots > target_slots {
            continue;
        }
        for total in (slots..=target_slots).rev() {
            possible[total] |= possible[total - slots];
        }
    }
    possible[target_slots]
}

pub(crate) fn selected_special_player_count(
    config: &BotConfig,
    roles: &[Role],
    count: u32,
) -> usize {
    let mut candidates = roles
        .iter()
        .filter(|role| special_role_enabled(config, **role))
        .map(|role| special_role_player_count(*role))
        .collect::<Vec<_>>();
    candidates.sort_unstable_by(|left, right| right.cmp(left));
    candidates.into_iter().take(count as usize).sum()
}

pub(crate) fn minimum_player_count(config: &BotConfig) -> usize {
    let cult_count = if config.enable_cult_team { 2 } else { 0 };
    let selected_count = config
        .default_mafia_count
        .saturating_sub(config.mafia_special_count) as usize
        + config.default_doctor_count as usize
        + config.default_police_count as usize
        + if config.enable_joker {
            config.default_joker_count as usize
        } else {
            0
        }
        + config.citizen_special_count as usize
        + selected_special_player_count(config, MAFIA_SPECIAL_ROLES, config.mafia_special_count)
        + selected_special_player_count(
            config,
            NEUTRAL_SPECIAL_ROLES,
            config.neutral_special_count,
        )
        + cult_count;
    3.max(selected_count)
        .max(config.default_mafia_count as usize * 2 + 1)
}

#[derive(Debug)]
pub(crate) struct HttpRequest {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) headers: HashMap<String, String>,
    pub(crate) body: String,
}

pub(crate) async fn read_http_request<S>(stream: &mut S) -> Result<HttpRequest>
where
    S: AsyncRead + Unpin,
{
    let mut buffer = Vec::with_capacity(8192);
    let mut temp = [0u8; 4096];
    let mut header_end = None;
    let mut content_length = 0usize;
    loop {
        let read = stream.read(&mut temp).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&temp[..read]);
        if header_end.is_none()
            && let Some(index) = find_header_end(&buffer)
        {
            header_end = Some(index);
            let headers = String::from_utf8_lossy(&buffer[..index]);
            content_length = parse_content_length(&headers).unwrap_or(0);
        }
        if let Some(index) = header_end
            && buffer.len() >= index + 4 + content_length
        {
            break;
        }
        if buffer.len() > 128 * 1024 {
            bail!("요청이 너무 큽니다.");
        }
    }
    let Some(index) = header_end else {
        bail!("HTTP 헤더를 찾지 못했습니다.");
    };
    let raw_headers = String::from_utf8_lossy(&buffer[..index]).to_string();
    let mut first_line = raw_headers
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace();
    let method = first_line.next().unwrap_or_default().to_string();
    let path = first_line.next().unwrap_or_default().to_string();
    let body_start = index + 4;
    let body_end = (body_start + content_length).min(buffer.len());
    let body = String::from_utf8_lossy(&buffer[body_start..body_end]).to_string();
    Ok(HttpRequest {
        method,
        path,
        headers: parse_http_headers(&raw_headers),
        body,
    })
}

pub(crate) fn http_response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

pub(crate) fn json_response(value: Value) -> String {
    json_response_with_status("200 OK", value)
}

pub(crate) fn json_error(status: &str, message: &str) -> String {
    json_response_with_status(status, json!({"error": message}))
}

pub(crate) fn json_response_with_status(status: &str, value: Value) -> String {
    let body = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json; charset=utf-8\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: Authorization, Content-Type, X-API-Key\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

pub(crate) fn api_options_response() -> String {
    "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: Authorization, Content-Type, X-API-Key\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Max-Age: 600\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
}

pub(crate) fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

pub(crate) fn parse_content_length(headers: &str) -> Option<usize> {
    headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("content-length") {
            value.trim().parse().ok()
        } else {
            None
        }
    })
}

pub(crate) fn parse_http_headers(headers: &str) -> HashMap<String, String> {
    headers
        .lines()
        .skip(1)
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect()
}

pub(crate) fn parse_urlencoded(body: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    for pair in body.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        values.insert(percent_decode(key), percent_decode(value));
    }
    values
}

pub(crate) fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                if let Ok(hex) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                    output.push(hex);
                    index += 3;
                } else {
                    output.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).to_string()
}

pub(crate) fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

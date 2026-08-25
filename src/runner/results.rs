// runner/results.rs — 게임 결과 이미지·승자 발표

use super::*;

#[derive(Clone, Debug)]
pub struct GameResultImageRow {
    pub(crate) name: String,
    pub(crate) role: String,
    pub(crate) team: String,
    /// "3티어 [가호]" / "2티어" — 게임 결과에 공개되는 개인 티어.
    pub(crate) tier_text: String,
    pub(crate) alive: bool,
    pub(crate) before: Option<i64>,
    pub(crate) after: Option<i64>,
    pub(crate) before_rank: Option<String>,
    pub(crate) after_rank: Option<String>,
    pub(crate) delta: Option<i64>,
    pub(crate) team_delta: Option<i64>,
    pub(crate) role_delta: Option<i64>,
    pub(crate) streak_delta: Option<i64>,
    pub(crate) win_streak: Option<i64>,
    pub(crate) best_win_streak: Option<i64>,
    pub(crate) reasons: Vec<String>,
}

pub fn winner_result_text(winner: Winner) -> &'static str {
    match winner {
        Winner::Mafia => "마피아 승리!",
        Winner::Joker => "조커 승리!",
        Winner::Cult => "교주팀 승리!",
        Winner::Citizen => "시민 승리!",
    }
}

pub fn prophet_victory_message(game: &MafiaGame, winner: Winner) -> Option<String> {
    if winner != Winner::Citizen {
        return None;
    }
    let prophet = game.winning_prophet()?;
    Some(format!(
        "예언자 {}님의 힘으로 시민팀이 승리하였습니다!",
        prophet.name
    ))
}

pub fn game_result_display_name(running: &RunningGame, player: &Player) -> String {
    game_result_label(running, player.user_id, &player.name)
}

/// 게임 결과 표기용 이름. 익명 게임이면 "별명 = 실명"으로 번호의 정체를 함께 공개한다.
pub fn game_result_label(running: &RunningGame, user_id: u64, fallback: &str) -> String {
    if running.anonymous_enabled {
        let alias = running
            .anonymous_aliases
            .get(&user_id)
            .map(String::as_str)
            .unwrap_or("익명");
        let real_name = running
            .anonymous_original_names
            .get(&user_id)
            .map(String::as_str)
            .unwrap_or(fallback);
        format!("{alias} = {real_name}")
    } else {
        fallback.to_string()
    }
}

/// 익명 게임의 `game.players`는 이름이 별명으로 덮여 있다. 통계/레이팅에는 실명이
/// 남아야 하므로 기록 직전에 원래 이름으로 되돌린 스냅샷을 만든다.
pub(crate) fn stats_game_snapshot(running: &RunningGame) -> MafiaGame {
    let mut snapshot = running.game.clone();
    if running.anonymous_enabled {
        for player in &mut snapshot.players {
            if let Some(original) = running.anonymous_original_names.get(&player.user_id) {
                player.name.clone_from(original);
            }
        }
    }
    snapshot
}

/// 레이팅/랭크 변동 안내는 실명으로 기록되지만, 익명 게임 결과에서는 어떤 번호가
/// 누구였는지도 함께 보여준다.
pub(crate) fn rating_log_with_result_labels(
    running: &RunningGame,
    rating_log: &[stats::GameRatingLogItem],
) -> Vec<stats::GameRatingLogItem> {
    rating_log
        .iter()
        .map(|item| {
            let mut item = item.clone();
            item.name = game_result_label(running, item.user_id, &item.name);
            item
        })
        .collect()
}

/// 게임 결과에 공개할 티어 표기.
pub fn game_result_tier_text(game: &MafiaGame, user_id: u64) -> String {
    let tier = game.player_tiers.get(&user_id).copied().unwrap_or(2);
    let abilities = game.player_tier_abilities(user_id);
    if abilities.is_empty() {
        format!("{}티어", tier)
    } else {
        format!(
            "{}티어 [{}]",
            tier,
            abilities
                .iter()
                .map(|ability| ability.value())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

pub fn game_result_rows(
    running: &RunningGame,
    rating_log: &[stats::GameRatingLogItem],
) -> Vec<GameResultImageRow> {
    let rating_by_id = rating_log
        .iter()
        .map(|item| (item.user_id, item))
        .collect::<HashMap<_, _>>();
    let mut players = running.game.players.clone();
    players.sort_by_key(|player| game_result_display_name(running, player).to_lowercase());
    players
        .iter()
        .map(|player| {
            let initial_role = running
                .initial_roles
                .get(&player.user_id)
                .copied()
                .unwrap_or(player.role);
            let role = if initial_role == player.role {
                player.role.value().to_string()
            } else {
                format!("{} -> {}", initial_role.value(), player.role.value())
            };
            let rating = rating_by_id.get(&player.user_id).copied();
            GameResultImageRow {
                name: game_result_display_name(running, player),
                role,
                team: final_team_text(&running.game, player).to_string(),
                tier_text: game_result_tier_text(&running.game, player.user_id),
                alive: player.alive,
                before: rating.map(|item| item.before),
                after: rating.map(|item| item.after),
                before_rank: rating.map(|item| item.before_rank.clone()),
                after_rank: rating.map(|item| item.after_rank.clone()),
                delta: rating.map(|item| item.delta),
                team_delta: rating.map(|item| item.team_delta),
                role_delta: rating.map(|item| item.role_delta),
                streak_delta: rating.map(|item| item.streak_delta),
                win_streak: rating.map(|item| item.win_streak),
                best_win_streak: rating.map(|item| item.best_win_streak),
                reasons: rating.map_or_else(Vec::new, |item| item.reasons.clone()),
            }
        })
        .collect()
}

pub fn render_game_result_image(
    winner: Winner,
    elapsed_seconds: i64,
    rows: Vec<GameResultImageRow>,
) -> Option<Vec<u8>> {
    const WIDTH: u32 = 2240;
    const TOP: i32 = 44;
    const SIDE: i32 = 56;
    const HEADER_HEIGHT: i32 = 172;
    const FOOTER: i32 = 56;
    const COL_PLAYER: i32 = SIDE + 42;
    const COL_ROLE: i32 = SIDE + 420;
    const COL_RATING: i32 = SIDE + 720;
    const COL_DELTA: i32 = SIDE + 1010;
    const COL_STREAK: i32 = SIDE + 1164;
    const COL_REASON: i32 = SIDE + 1400;

    let table_top = TOP + HEADER_HEIGHT + 26;
    let row_heights = rows.iter().map(game_result_row_height).collect::<Vec<_>>();
    let table_height = row_heights.iter().sum::<i32>();
    let height = (table_top + table_height + FOOTER).max(520) as u32;
    let mut image = RgbImage::from_pixel(WIDTH, height, image_color("#edf2f7"));
    let font = FontArc::try_from_slice(include_bytes!("../../MalangmalangR.ttf")).ok()?;
    let text = image_color("#172033");
    let muted = image_color("#64748b");
    let soft = image_color("#f8fafc");
    let white = image_color("#ffffff");
    let line = image_color("#d9e2ef");
    let accent = winner_color(winner);

    fill_rect(&mut image, 0, 0, WIDTH, 18, accent);
    fill_rect(&mut image, SIDE, TOP, WIDTH - SIDE as u32 * 2, 150, white);
    fill_rect(&mut image, SIDE, TOP, 10, 150, accent);
    draw_lb_text(
        &mut image,
        &font,
        48.0,
        SIDE + 30,
        TOP + 24,
        winner_result_text(winner),
        text,
    );
    draw_lb_text(
        &mut image,
        &font,
        25.0,
        SIDE + 34,
        TOP + 88,
        format!(
            "플레이 시간 {} · 참가자 {}명 · 최종 역할 / 랭크 / 레이팅 정리",
            stats::play_duration_text(elapsed_seconds),
            rows.len()
        ),
        muted,
    );
    let badge_x = WIDTH as i32 - SIDE - 282;
    fill_rect(&mut image, badge_x, TOP + 44, 250, 54, accent);
    draw_lb_text(
        &mut image,
        &font,
        28.0,
        badge_x + 32,
        TOP + 58,
        winner.value(),
        image_color("#ffffff"),
    );

    fill_rect(
        &mut image,
        SIDE,
        table_top - 52,
        WIDTH - SIDE as u32 * 2,
        52,
        image_color("#1f2937"),
    );
    for (x, label) in [
        (COL_PLAYER, "플레이어"),
        (COL_ROLE, "최종 역할"),
        (COL_RATING, "레이팅"),
        (COL_DELTA, "변동"),
        (COL_STREAK, "연승"),
        (COL_REASON, "랭크/사유"),
    ] {
        draw_lb_text(
            &mut image,
            &font,
            23.0,
            x,
            table_top - 38,
            label,
            image_color("#f8fafc"),
        );
    }

    let mut y = table_top;
    for (index, row) in rows.iter().enumerate() {
        let row_height = row_heights[index];
        let row_fill = if index % 2 == 0 { white } else { soft };
        fill_rect(
            &mut image,
            SIDE,
            y,
            WIDTH - SIDE as u32 * 2,
            row_height as u32,
            row_fill,
        );
        fill_rect(
            &mut image,
            SIDE,
            y + row_height - 1,
            WIDTH - SIDE as u32 * 2,
            1,
            line,
        );
        fill_rect(
            &mut image,
            SIDE,
            y,
            8,
            row_height as u32,
            team_color(&row.team),
        );
        fill_circle(
            &mut image,
            (SIDE + 32, y + 46),
            16,
            if row.alive {
                image_color("#22c55e")
            } else {
                image_color("#ef4444")
            },
        );
        draw_lb_text(
            &mut image,
            &font,
            28.0,
            SIDE + 68,
            y + 18,
            truncate_for_board(&row.name, 22),
            text,
        );
        draw_lb_text(
            &mut image,
            &font,
            20.0,
            SIDE + 70,
            y + 58,
            if row.alive { "생존" } else { "사망" },
            muted,
        );
        draw_lb_text(
            &mut image,
            &font,
            26.0,
            COL_ROLE,
            y + 20,
            truncate_for_board(&row.role, 18),
            text,
        );
        draw_lb_text(
            &mut image,
            &font,
            20.0,
            COL_ROLE + 2,
            y + 58,
            format!("{} · {}", row.team, row.tier_text),
            team_color(&row.team),
        );
        draw_rating_block(&mut image, &font, row, COL_RATING, y, text, muted);
        draw_delta_badge(&mut image, &font, row, COL_DELTA, y);
        draw_streak_badge(&mut image, &font, row, COL_STREAK, y);
        draw_rank_and_reason(&mut image, &font, row, COL_REASON, y, text, muted);
        y += row_height;
    }

    draw_lb_text(
        &mut image,
        &font,
        19.0,
        SIDE,
        height as i32 - 34,
        "마피아 게임 진행 메시지",
        muted,
    );
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .ok()?;
    Some(bytes.into_inner())
}

pub(crate) fn game_result_reason_text(row: &GameResultImageRow) -> String {
    if row.reasons.is_empty() {
        "사유 없음".to_string()
    } else {
        row.reasons.join(", ")
    }
}

pub(crate) fn wrap_result_reason(reason: &str) -> Vec<String> {
    wrap_text_by_chars(reason, 32)
}

pub(crate) fn game_result_row_height(row: &GameResultImageRow) -> i32 {
    let reason_lines = wrap_result_reason(&game_result_reason_text(row))
        .len()
        .max(1) as i32;
    (86 + reason_lines * 24).max(126)
}

pub(crate) fn wrap_text_by_chars(text: &str, max_chars: usize) -> Vec<String> {
    if text.trim().is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let separator = usize::from(!current.is_empty());
        if current.chars().count() + separator + word.chars().count() > max_chars
            && !current.is_empty()
        {
            lines.push(current);
            current = String::new();
        }
        if word.chars().count() > max_chars {
            if !current.is_empty() {
                lines.push(current);
                current = String::new();
            }
            let mut chunk = String::new();
            for ch in word.chars() {
                if chunk.chars().count() >= max_chars {
                    lines.push(chunk);
                    chunk = String::new();
                }
                chunk.push(ch);
            }
            if !chunk.is_empty() {
                current = chunk;
            }
            continue;
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

pub(crate) fn draw_rating_block(
    image: &mut RgbImage,
    font: &FontArc,
    row: &GameResultImageRow,
    x: i32,
    y: i32,
    text: Rgb<u8>,
    muted: Rgb<u8>,
) {
    if let (Some(before), Some(after)) = (row.before, row.after) {
        draw_lb_text(
            image,
            font,
            25.0,
            x,
            y + 20,
            format!("{before} -> {after}"),
            text,
        );
        draw_lb_text(
            image,
            font,
            20.0,
            x,
            y + 58,
            format!(
                "{} -> {}",
                row.before_rank.as_deref().unwrap_or("?"),
                row.after_rank.as_deref().unwrap_or("?")
            ),
            muted,
        );
    } else {
        draw_lb_text(image, font, 24.0, x, y + 34, "기록 없음", muted);
    }
}

pub(crate) fn draw_delta_badge(
    image: &mut RgbImage,
    font: &FontArc,
    row: &GameResultImageRow,
    x: i32,
    y: i32,
) {
    let Some(delta) = row.delta else {
        draw_lb_text(image, font, 23.0, x, y + 34, "-", image_color("#94a3b8"));
        return;
    };
    let fill = if delta > 0 {
        image_color("#dcfce7")
    } else if delta < 0 {
        image_color("#fee2e2")
    } else {
        image_color("#e2e8f0")
    };
    let color = if delta > 0 {
        image_color("#15803d")
    } else if delta < 0 {
        image_color("#b91c1c")
    } else {
        image_color("#475569")
    };
    fill_rect(image, x, y + 22, 128, 42, fill);
    draw_lb_text(
        image,
        font,
        25.0,
        x + 18,
        y + 30,
        format!("{delta:+}"),
        color,
    );
    let detail = game_result_delta_detail(row);
    draw_lb_text(image, font, 18.0, x, y + 70, detail, image_color("#64748b"));
}

pub(crate) fn game_result_delta_detail(row: &GameResultImageRow) -> String {
    format!(
        "팀 {:+} · 직업 {:+}",
        row.team_delta.unwrap_or(0),
        row.role_delta.unwrap_or(0)
    )
}

pub(crate) fn draw_streak_badge(
    image: &mut RgbImage,
    font: &FontArc,
    row: &GameResultImageRow,
    x: i32,
    y: i32,
) {
    let Some(current) = row.win_streak else {
        draw_lb_text(image, font, 23.0, x, y + 34, "-", image_color("#94a3b8"));
        return;
    };
    let best = row.best_win_streak.unwrap_or(current);
    let (fill, color) = if current > 0 {
        (image_color("#dcfce7"), image_color("#15803d"))
    } else {
        (image_color("#e2e8f0"), image_color("#475569"))
    };
    fill_rect(image, x, y + 22, 208, 42, fill);
    draw_lb_text(
        image,
        font,
        23.0,
        x + 14,
        y + 30,
        format!("현재 {current}연승"),
        color,
    );
    draw_lb_text(
        image,
        font,
        18.0,
        x,
        y + 72,
        format!("최고 {best}연승"),
        image_color("#64748b"),
    );
    draw_lb_text(
        image,
        font,
        18.0,
        x,
        y + 98,
        format!("보너스 {:+}", row.streak_delta.unwrap_or(0)),
        image_color("#64748b"),
    );
}

pub(crate) fn draw_rank_and_reason(
    image: &mut RgbImage,
    font: &FontArc,
    row: &GameResultImageRow,
    x: i32,
    y: i32,
    text: Rgb<u8>,
    muted: Rgb<u8>,
) {
    if let (Some(before), Some(after)) = (row.before, row.after) {
        let before_rank = row.before_rank.as_deref().unwrap_or("?");
        let after_rank = row.after_rank.as_deref().unwrap_or("?");
        let rank_text = if before_rank == after_rank {
            format!("{after_rank} 랭크 유지")
        } else if after > before {
            format!("승급 {before_rank} -> {after_rank}")
        } else {
            format!("강등 {before_rank} -> {after_rank}")
        };
        draw_lb_text(image, font, 24.0, x, y + 18, rank_text, text);
    } else {
        draw_lb_text(image, font, 24.0, x, y + 18, "랭크 기록 없음", muted);
    }
    let reason = game_result_reason_text(row);
    for (index, line) in wrap_result_reason(&reason).iter().enumerate() {
        draw_lb_text(
            image,
            font,
            18.0,
            x,
            y + 56 + index as i32 * 24,
            line,
            muted,
        );
    }
}

pub(crate) fn winner_color(winner: Winner) -> Rgb<u8> {
    match winner {
        Winner::Mafia => image_color("#dc2626"),
        Winner::Joker => image_color("#7c3aed"),
        Winner::Cult => image_color("#0891b2"),
        Winner::Citizen => image_color("#16a34a"),
    }
}

pub(crate) fn team_color(team: &str) -> Rgb<u8> {
    match team {
        "마피아팀" => image_color("#dc2626"),
        "교주팀" => image_color("#0891b2"),
        "중립" => image_color("#7c3aed"),
        _ => image_color("#16a34a"),
    }
}

pub async fn send_game_result_image(
    ctx: &serenity::Context,
    running: &Arc<RwLock<RunningGame>>,
    image: Vec<u8>,
) -> serenity::Result<serenity::Message> {
    const FILENAME: &str = "mafia_game_result.png";
    let (channel_id, anonymous_enabled, targets) = {
        let running_read = running.read().await;
        let targets = if running_read.anonymous_enabled {
            running_read
                .game
                .players
                .iter()
                .filter_map(|player| {
                    running_read
                        .anonymous_input_channel_ids
                        .get(&player.user_id)
                        .copied()
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        (
            running_read.channel_id,
            running_read.anonymous_enabled,
            targets,
        )
    };
    let embed = make_embed(
        "게임 종료 결과를 이미지로 정리했습니다.",
        "게임 종료",
        serenity::Colour::DARK_GREEN,
    )
    .attachment(FILENAME);
    let sent = channel_id
        .send_message(
            &ctx.http,
            serenity::CreateMessage::new()
                .embed(embed.clone())
                .add_file(serenity::CreateAttachment::bytes(image.clone(), FILENAME)),
        )
        .await?;
    if anonymous_enabled {
        for target in targets {
            let _ = target
                .send_message(
                    &ctx.http,
                    serenity::CreateMessage::new()
                        .embed(embed.clone())
                        .add_file(serenity::CreateAttachment::bytes(image.clone(), FILENAME)),
                )
                .await;
        }
    }
    Ok(sent)
}

pub async fn announce_winner(
    ctx: &serenity::Context,
    data: &Data,
    running: &Arc<RwLock<RunningGame>>,
) -> Result<bool> {
    let (winner, prophet_message) = {
        let running_read = running.read().await;
        let Some(winner) = running_read.game.winner() else {
            return Ok(false);
        };
        (winner, prophet_victory_message(&running_read.game, winner))
    };
    let (roles_text, elapsed_seconds, record_payload) = {
        let mut running_write = running.write().await;
        running_write.game.phase = Phase::Ended;
        let elapsed_seconds = running_write.started_at.elapsed().as_secs() as i64;
        if running_write.ended_at_iso.is_none() {
            running_write.ended_at_iso =
                Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
        }
        let record_payload = if running_write.stats_recorded {
            None
        } else {
            running_write.stats_recorded = true;
            running_write.record_replay_event(
                "game_ended",
                None,
                &[],
                serde_json::json!({
                    "winner": winner.value(),
                    "winner_key": format!("{:?}", winner),
                    "elapsed_seconds": elapsed_seconds,
                }),
            );
            Some((
                stats_game_snapshot(&running_write),
                running_write.initial_roles.clone(),
                elapsed_seconds,
            ))
        };
        (
            final_role_reveal_text(&running_write),
            elapsed_seconds,
            record_payload,
        )
    };
    upsert_game_status(ctx, running).await;
    if let Some(message) = prophet_message
        && let Err(error) = send_game_embed(
            ctx,
            running,
            message,
            "예언자 승리",
            serenity::Colour::DARK_GREEN,
            vec![],
            true,
            true,
        )
        .await
    {
        eprintln!("failed to announce prophet victory: {error:?}");
    }
    let mut rating_log = Vec::new();
    let mut rating_log_chunks = Vec::new();
    let mut rank_change_chunks = Vec::new();
    if let Some((game_snapshot, initial_roles, elapsed_seconds)) = record_payload {
        let (recorded_rating_log, stats_snapshot) = {
            let mut stats_file = data.stats.write().await;
            let rating_log = stats::record_game_stats(
                &mut stats_file,
                &game_snapshot,
                &initial_roles,
                elapsed_seconds,
                winner,
            );
            (rating_log, stats_file.clone())
        };
        let labeled_rating_log = {
            let running_read = running.read().await;
            rating_log_with_result_labels(&running_read, &recorded_rating_log)
        };
        rating_log_chunks = stats::game_rating_log_chunks(&labeled_rating_log, 3500);
        rank_change_chunks = stats::game_rank_change_chunks(&labeled_rating_log, 3500);
        rating_log = recorded_rating_log;
        let stats_path = data.stats_path.clone();
        match tokio::task::spawn_blocking(move || stats::save_stats(&*stats_path, &stats_snapshot))
            .await
        {
            Ok(Ok(())) => {}
            Ok(Err(error)) => eprintln!("failed to save stats after game end: {error:?}"),
            Err(error) => eprintln!("failed to join stats save task after game end: {error:?}"),
        }
    }
    let completed_replay = {
        let running_read = running.read().await;
        running_read.replay_snapshot("completed", Some(winner), &rating_log)
    };
    {
        let mut completed_replays = data.completed_replays.write().await;
        let game_key = completed_replay["game_key"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        if let Some(index) = completed_replays
            .iter()
            .position(|replay| replay["game_key"].as_str() == Some(game_key.as_str()))
        {
            completed_replays.remove(index);
        }
        completed_replays.push_front(completed_replay);
        while completed_replays.len() > COMPLETED_REPLAY_LIMIT {
            completed_replays.pop_back();
        }
        let completed_replays_path = data.completed_replays_path.clone();
        let completed_replays_snapshot = completed_replays.clone();
        tokio::spawn(async move {
            match tokio::task::spawn_blocking(move || {
                crate::web_settings::save_completed_replays(
                    &*completed_replays_path,
                    &completed_replays_snapshot,
                )
            })
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(error)) => eprintln!("failed to save replay history: {error:?}"),
                Err(error) => eprintln!("failed to join replay history save task: {error:?}"),
            }
        });
    }
    let rows = {
        let running_read = running.read().await;
        game_result_rows(&running_read, &rating_log)
    };
    match tokio::task::spawn_blocking(move || {
        render_game_result_image(winner, elapsed_seconds, rows)
    })
    .await
    {
        Ok(Some(image)) => match send_game_result_image(ctx, running, image).await {
            Ok(_) => return Ok(true),
            Err(error) => eprintln!("failed to announce game result image: {error:?}"),
        },
        Ok(None) => eprintln!("failed to render game result image"),
        Err(error) => eprintln!("failed to join game result image task: {error:?}"),
    }
    if let Err(error) = send_game_embed(
        ctx,
        running,
        format!(
            "{}\n플레이 시간: **{}**\n\n최종 역할 공개\n{}",
            winner_result_text(winner),
            stats::play_duration_text(elapsed_seconds),
            roles_text
        ),
        "게임 종료",
        serenity::Colour::DARK_GREEN,
        vec![],
        true,
        true,
    )
    .await
    {
        eprintln!("failed to announce game winner: {error:?}");
    }
    for (index, chunk) in rank_change_chunks.into_iter().enumerate() {
        let title = if index == 0 {
            "이번 판 랭크 변동".to_string()
        } else {
            format!("이번 판 랭크 변동 {}", index + 1)
        };
        if let Err(error) = send_game_embed(
            ctx,
            running,
            chunk,
            &title,
            serenity::Colour::GOLD,
            vec![],
            false,
            true,
        )
        .await
        {
            eprintln!("failed to announce rank changes: {error:?}");
        }
    }
    for (index, chunk) in rating_log_chunks.into_iter().enumerate() {
        let title = if index == 0 {
            "이번 판 레이팅 로그".to_string()
        } else {
            format!("이번 판 레이팅 로그 {}", index + 1)
        };
        if let Err(error) = send_game_embed(
            ctx,
            running,
            chunk,
            &title,
            serenity::Colour::BLUE,
            vec![],
            false,
            true,
        )
        .await
        {
            eprintln!("failed to announce rating log: {error:?}");
        }
    }
    Ok(true)
}

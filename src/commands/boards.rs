// commands/boards.rs — 리더보드·결과 이미지 렌더링과 관련 명령어

use super::*;

pub fn image_color(hex: &str) -> Rgb<u8> {
    let value = hex.trim_start_matches('#');
    let red = u8::from_str_radix(&value[0..2], 16).unwrap_or(255);
    let green = u8::from_str_radix(&value[2..4], 16).unwrap_or(255);
    let blue = u8::from_str_radix(&value[4..6], 16).unwrap_or(255);
    Rgb([red, green, blue])
}

pub fn fill_rect(image: &mut RgbImage, x: i32, y: i32, width: u32, height: u32, color: Rgb<u8>) {
    let left = x.max(0) as u32;
    let top = y.max(0) as u32;
    let right = (x as i64 + width as i64)
        .clamp(0, image.width() as i64)
        .max(left as i64) as u32;
    let bottom = (y as i64 + height as i64)
        .clamp(0, image.height() as i64)
        .max(top as i64) as u32;

    for pixel_y in top..bottom {
        for pixel_x in left..right {
            image.put_pixel(pixel_x, pixel_y, color);
        }
    }
}

pub fn fill_horizontal_line(image: &mut RgbImage, x0: i32, x1: i32, y: i32, color: Rgb<u8>) {
    if y < 0 || y >= image.height() as i32 {
        return;
    }
    let left = x0.min(x1).max(0) as u32;
    let right = x0.max(x1).min(image.width() as i32 - 1);
    if right < 0 || left > right as u32 {
        return;
    }
    for pixel_x in left..=right as u32 {
        image.put_pixel(pixel_x, y as u32, color);
    }
}

pub fn fill_circle(image: &mut RgbImage, center: (i32, i32), radius: i32, color: Rgb<u8>) {
    let mut x = 0;
    let mut y = radius;
    let mut p = 1 - radius;
    let (x0, y0) = center;

    while x <= y {
        fill_horizontal_line(image, x0 - x, x0 + x, y0 + y, color);
        fill_horizontal_line(image, x0 - y, x0 + y, y0 + x, color);
        fill_horizontal_line(image, x0 - x, x0 + x, y0 - y, color);
        fill_horizontal_line(image, x0 - y, x0 + y, y0 - x, color);

        x += 1;
        if p < 0 {
            p += 2 * x + 1;
        } else {
            y -= 1;
            p += 2 * (x - y) + 1;
        }
    }
}

pub fn blend_channel(left: u8, right: u8, left_weight: f32, right_weight: f32) -> u8 {
    let value = left as f32 * left_weight + right as f32 * right_weight;
    if value < u8::MAX as f32 {
        if value > u8::MIN as f32 {
            value as u8
        } else {
            u8::MIN
        }
    } else {
        u8::MAX
    }
}

pub fn blend_rgb(left: Rgb<u8>, right: Rgb<u8>, left_weight: f32, right_weight: f32) -> Rgb<u8> {
    Rgb([
        blend_channel(left[0], right[0], left_weight, right_weight),
        blend_channel(left[1], right[1], left_weight, right_weight),
        blend_channel(left[2], right[2], left_weight, right_weight),
    ])
}

pub fn layout_lb_glyphs(
    scale: PxScale,
    font: &impl Font,
    text: &str,
    mut visit: impl FnMut(OutlinedGlyph, GlyphRect),
) {
    let font = font.as_scaled(scale);
    let mut last: Option<GlyphId> = None;
    let mut width = 0.0;

    for character in text.chars() {
        let glyph_id = font.glyph_id(character);
        let glyph = glyph_id.with_scale_and_position(scale, point(width, font.ascent()));
        width += font.h_advance(glyph_id);
        if let Some(outlined) = font.outline_glyph(glyph) {
            if let Some(last) = last {
                width += font.kern(glyph_id, last);
            }
            last = Some(glyph_id);
            let bounds = outlined.px_bounds();
            visit(outlined, bounds);
        }
    }
}

pub fn draw_lb_text(
    image: &mut RgbImage,
    font: &FontArc,
    size: f32,
    x: i32,
    y: i32,
    text: impl AsRef<str>,
    color: Rgb<u8>,
) {
    let image_width = image.width() as i32;
    let image_height = image.height() as i32;

    layout_lb_glyphs(PxScale::from(size), font, text.as_ref(), |glyph, bounds| {
        glyph.draw(|glyph_x, glyph_y, value| {
            let image_x = glyph_x as i32 + x + bounds.min.x.round() as i32;
            let image_y = glyph_y as i32 + y + bounds.min.y.round() as i32;
            let value = value.clamp(0.0, 1.0);

            if (0..image_width).contains(&image_x) && (0..image_height).contains(&image_y) {
                let pixel = *image.get_pixel(image_x as u32, image_y as u32);
                image.put_pixel(
                    image_x as u32,
                    image_y as u32,
                    blend_rgb(pixel, color, 1.0 - value, value),
                );
            }
        });
    });
}

pub fn truncate_for_board(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut text = value
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    text.push_str("...");
    text
}

pub fn leaderboard_metric_column(metric: &str) -> &'static str {
    match metric {
        "winrate" => "winrate",
        "streak" => "streak",
        "games" => "games",
        "mafia" => "mafia",
        "playtime" => "time",
        "rating" => "rating",
        _ => "record",
    }
}

pub fn render_leaderboard_image(stats_file: &stats::StatsFile, metric: &str) -> Option<Vec<u8>> {
    let entries = stats::leaderboard_entries(stats_file, metric, 10);
    if entries.is_empty() {
        return None;
    }

    // 랭크/레이팅(맨 오른쪽 칼럼)이 'SS 1368점'처럼 긴 값에서 이미지 오른쪽
    // 끝에 잘리지 않도록 캔버스를 넉넉히 잡는다.
    const IMAGE_WIDTH: u32 = 1360;
    const TOP_PADDING: i32 = 40;
    const SIDE_PADDING: i32 = 48;
    const HEADER_HEIGHT: i32 = 150;
    const ROW_HEIGHT: i32 = 78;
    const BOTTOM_PADDING: i32 = 44;

    let height =
        (TOP_PADDING + HEADER_HEIGHT + ROW_HEIGHT * entries.len() as i32 + BOTTOM_PADDING) as u32;
    let mut image = RgbImage::from_pixel(IMAGE_WIDTH, height, image_color("#111318"));
    let font = FontArc::try_from_slice(include_bytes!("../../MalangmalangR.ttf")).ok()?;

    let text = image_color("#f5f7fb");
    let muted = image_color("#aeb6c8");
    let accent = image_color("#ffd166");
    let panel = image_color("#1d2028");
    let row_dark = image_color("#242832");
    let row_light = image_color("#292e3a");

    draw_lb_text(
        &mut image,
        &font,
        44.0,
        SIDE_PADDING,
        TOP_PADDING,
        "마피아 리더보드",
        text,
    );
    draw_lb_text(
        &mut image,
        &font,
        24.0,
        SIDE_PADDING,
        TOP_PADDING + 58,
        "게임 종료 후 기록된 전적 기준",
        muted,
    );
    fill_rect(
        &mut image,
        IMAGE_WIDTH as i32 - SIDE_PADDING - 230,
        TOP_PADDING + 10,
        210,
        38,
        image_color("#374151"),
    );
    draw_lb_text(
        &mut image,
        &font,
        24.0,
        IMAGE_WIDTH as i32 - SIDE_PADDING - 214,
        TOP_PADDING + 16,
        format!("기준: {}", stats::leaderboard_metric_name(metric)),
        text,
    );

    let panel_top = TOP_PADDING + 116;
    let panel_bottom = height as i32 - BOTTOM_PADDING + 8;
    fill_rect(
        &mut image,
        SIDE_PADDING,
        panel_top,
        IMAGE_WIDTH - (SIDE_PADDING as u32 * 2),
        (panel_bottom - panel_top) as u32,
        panel,
    );

    let columns = HashMap::from([
        ("rank", SIDE_PADDING + 32),
        ("name", SIDE_PADDING + 110),
        ("record", SIDE_PADDING + 390),
        ("games", SIDE_PADDING + 535),
        ("winrate", SIDE_PADDING + 635),
        ("streak", SIDE_PADDING + 745),
        ("mafia", SIDE_PADDING + 850),
        ("time", SIDE_PADDING + 955),
        ("rating", SIDE_PADDING + 1085),
    ]);
    let selected_column = leaderboard_metric_column(metric);
    let header_y = panel_top + 24;
    for (key, label) in [
        ("rank", "#"),
        ("name", "이름"),
        ("record", "승패"),
        ("games", "판수"),
        ("winrate", "승률"),
        ("streak", "연승"),
        ("mafia", "마피아"),
        ("time", "시간"),
        ("rating", "랭크/레이팅"),
    ] {
        draw_lb_text(
            &mut image,
            &font,
            21.0,
            columns[key],
            header_y,
            label,
            if key == selected_column {
                accent
            } else {
                muted
            },
        );
    }

    let row_start_y = panel_top + 62;
    for (index, (_user_id, entry)) in entries.iter().enumerate() {
        let rank = index + 1;
        let y = row_start_y + index as i32 * ROW_HEIGHT;
        let row_fill = if rank % 2 == 1 { row_dark } else { row_light };
        fill_rect(
            &mut image,
            SIDE_PADDING + 18,
            y,
            IMAGE_WIDTH - ((SIDE_PADDING + 18) as u32 * 2),
            (ROW_HEIGHT - 10) as u32,
            row_fill,
        );
        let medal = match rank {
            1 => image_color("#f6c945"),
            2 => image_color("#c4ccd8"),
            3 => image_color("#c58b5b"),
            _ => image_color("#3b4252"),
        };
        fill_circle(&mut image, (columns["rank"] + 17, y + 36), 20, medal);
        draw_lb_text(
            &mut image,
            &font,
            24.0,
            columns["rank"] + if rank < 10 { 9 } else { 3 },
            y + 22,
            rank.to_string(),
            if rank <= 3 {
                image_color("#111318")
            } else {
                text
            },
        );

        let name = if entry.name.is_empty() {
            "알 수 없음".to_string()
        } else {
            truncate_for_board(&entry.name, 13)
        };
        let values = [
            ("name", name),
            ("record", format!("{}승 {}패", entry.wins, entry.losses)),
            ("games", format!("{}판", entry.games)),
            ("winrate", stats::win_rate_text(entry.wins, entry.games)),
            ("streak", format!("{}연승", entry.win_streak)),
            ("mafia", format!("{}회", entry.mafia_team_games)),
            ("time", stats::play_duration_text(entry.play_seconds)),
            (
                "rating",
                format!(
                    "{} {}점",
                    stats::rating_rank(stats_file, entry.rating, entry.rating_games),
                    entry.rating
                ),
            ),
        ];
        for (key, value) in values {
            draw_lb_text(
                &mut image,
                &font,
                if key == "name" { 27.0 } else { 23.0 },
                columns[key],
                y + if key == "name" { 18 } else { 21 },
                value,
                if key == selected_column { accent } else { text },
            );
        }
    }
    draw_lb_text(
        &mut image,
        &font,
        18.0,
        SIDE_PADDING + 18,
        height as i32 - 30,
        "마피아 게임 진행 메시지",
        muted,
    );

    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .ok()?;
    Some(bytes.into_inner())
}

#[poise::command(
    slash_command,
    rename = "리더보드",
    description_localized("ko", "마피아 게임 전적 순위를 확인합니다.")
)]
pub async fn show_leaderboard(
    ctx: Context<'_>,
    #[description = "정렬 기준"] 기준: Option<LeaderboardMetric>,
) -> Result<(), Error> {
    let metric = 기준.map_or("wins", LeaderboardMetric::value);
    let stats_file = Arc::new(ctx.data().stats.read().await.clone());
    let image_stats = Arc::clone(&stats_file);
    if let Some(image) =
        tokio::task::spawn_blocking(move || render_leaderboard_image(image_stats.as_ref(), metric))
            .await?
    {
        ctx.send(
            poise::CreateReply::default().attachment(serenity::CreateAttachment::bytes(
                image,
                format!("mafia_leaderboard_{metric}.png"),
            )),
        )
        .await?;
        return Ok(());
    }
    let text = stats::leaderboard_text(stats_file.as_ref(), metric);
    reply_embed(ctx, text, "리더보드", serenity::Colour::GOLD, false).await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "리더보드초기화",
    description_localized("ko", "마피아 게임 전적과 리더보드를 초기화합니다.")
)]
pub async fn reset_leaderboard(ctx: Context<'_>) -> Result<(), Error> {
    let deferred = defer_best_effort(ctx, "리더보드초기화").await;
    if !require_manager(ctx).await? {
        return Ok(());
    }
    if !deferred {
        let _ = send_channel_embed(
            ctx.http(),
            ctx.channel_id(),
            "리더보드 초기화를 시작했습니다.",
            "리더보드",
            serenity::Colour::GOLD,
            vec![],
        )
        .await;
    }
    let stats_snapshot = {
        let mut stats_file = ctx.data().stats.write().await;
        *stats_file = stats::StatsFile::default();
        stats_file.clone()
    };
    let stats_path = ctx.data().stats_path.clone();
    tokio::task::spawn_blocking(move || stats::save_stats(&*stats_path, &stats_snapshot)).await??;
    reply_embed_with_channel_fallback(
        ctx,
        "리더보드와 개인 전적을 초기화했습니다.",
        "리더보드",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

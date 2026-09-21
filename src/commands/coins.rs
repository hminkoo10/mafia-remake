// commands/coins.rs — 코인: 출석·배팅·내신 쿠폰 명령어, 배팅 입력창, 스타플레이어 투표 처리

use super::*;
use crate::runner::game_result_display_name;

/// 통계 파일을 저장한다 (실패는 로그만).
pub(crate) async fn save_stats_snapshot(data: &Data, snapshot: stats::StatsFile) {
    let stats_path = data.stats_path.clone();
    match tokio::task::spawn_blocking(move || stats::save_stats(&*stats_path, &snapshot)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("failed to save stats: {error:?}"),
        Err(error) => eprintln!("failed to join stats save task: {error:?}"),
    }
}

/// 배팅액을 설정하고 저장한다. 성공하면 보유 코인을 돌려준다.
pub(crate) async fn apply_bet_setting(
    data: &Data,
    user_id: u64,
    name: &str,
    amount: i64,
) -> std::result::Result<i64, String> {
    let (balance, snapshot) = {
        let mut stats_file = data.stats.write().await;
        let balance = stats::set_bet_amount(&mut stats_file, user_id, name, amount)?;
        (balance, stats_file.clone())
    };
    save_stats_snapshot(data, snapshot).await;
    Ok(balance)
}

/// "1,000" / "1000원" 같은 입력을 정수로 읽는다.
pub(crate) fn parse_bet_input(raw: &str) -> Option<i64> {
    let cleaned = raw.trim().replace([',', ' ', '원'], "");
    if cleaned.is_empty() {
        return None;
    }
    cleaned.parse::<i64>().ok()
}

pub(crate) fn bet_set_message(amount: i64, balance: i64) -> String {
    if amount == 0 {
        format!(
            "배팅하지 않도록 설정했습니다.\n보유 코인: **{}**",
            stats::coin_text(balance)
        )
    } else {
        format!(
            "배팅액을 **{}**으로 설정했습니다. 다음 판부터 적용되고 바꾸기 전까지 유지됩니다.\n보유 코인: **{}**",
            stats::coin_text(amount),
            stats::coin_text(balance)
        )
    }
}

/// `/내정보`용 최근 쿠폰 발급 기록 (최근 3건).
pub(crate) fn recent_coupon_text(entry: &stats::PlayerStats) -> String {
    if entry.coupons.is_empty() {
        return String::new();
    }
    let lines = entry
        .coupons
        .iter()
        .rev()
        .take(3)
        .map(|record| {
            let codes = record
                .codes
                .iter()
                .map(|code| format!("`{code}`"))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "- {} {}포인트: {codes}",
                record.issued_at.chars().take(10).collect::<String>(),
                record.points
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("\n최근 쿠폰\n{lines}")
}

#[poise::command(
    slash_command,
    rename = "출석",
    description_localized("ko", "하루 한 번(한국 시간 기준) 출석해 코인을 받습니다.")
)]
pub async fn claim_attendance(ctx: Context<'_>) -> Result<(), Error> {
    let amount = ctx.data().config.read().await.attendance_coins;
    let user = ctx.author();
    let today = stats::kst_today();
    let (outcome, snapshot) = {
        let mut stats_file = ctx.data().stats.write().await;
        let outcome =
            stats::claim_attendance(&mut stats_file, user.id.get(), &user.name, amount, &today);
        (outcome, stats_file.clone())
    };
    match outcome {
        stats::AttendanceOutcome::Claimed { amount, balance } => {
            save_stats_snapshot(ctx.data(), snapshot).await;
            reply_embed(
                ctx,
                format!(
                    "출석 완료! **{}**을 받았습니다.\n보유 코인: **{}**",
                    stats::coin_text(amount),
                    stats::coin_text(balance)
                ),
                "출석",
                serenity::Colour::DARK_GREEN,
                false,
            )
            .await?;
        }
        stats::AttendanceOutcome::AlreadyClaimed { balance } => {
            reply_embed(
                ctx,
                format!(
                    "오늘은 이미 출석했습니다. 한국 시간 자정이 지나면 다시 할 수 있습니다.\n보유 코인: **{}**",
                    stats::coin_text(balance)
                ),
                "출석",
                serenity::Colour::GOLD,
                true,
            )
            .await?;
        }
    }
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "배팅",
    description_localized("ko", "게임 배팅액을 설정합니다. 바꾸기 전까지 유지됩니다.")
)]
pub async fn set_bet(
    ctx: Context<'_>,
    #[description = "배팅액(원). 0이면 배팅하지 않습니다."] 금액: i64,
) -> Result<(), Error> {
    let user = ctx.author();
    match apply_bet_setting(ctx.data(), user.id.get(), &user.name, 금액).await {
        Ok(balance) => {
            reply_embed(
                ctx,
                bet_set_message(금액, balance),
                "배팅 설정",
                serenity::Colour::DARK_GREEN,
                true,
            )
            .await?;
        }
        Err(message) => {
            reply_embed(ctx, message, "배팅 설정", serenity::Colour::RED, true).await?;
        }
    }
    Ok(())
}

/// 쿠폰 발급 API 호출. 성공하면 발급된 코드 목록.
async fn request_coupons(api_url: &str, api_key: &str, points: i64) -> anyhow::Result<Vec<String>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let response = client
        .post(api_url)
        .bearer_auth(api_key)
        .json(&serde_json::json!({ "coupon_type": "one_time", "points": points }))
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let body: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    let ok = body.get("ok").and_then(serde_json::Value::as_bool) == Some(true);
    if !status.is_success() || !ok {
        let detail = body
            .get("error")
            .or_else(|| body.get("message"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| text.chars().take(200).collect());
        anyhow::bail!("HTTP {status}: {detail}");
    }
    let codes = body
        .pointer("/data/codes")
        .and_then(serde_json::Value::as_array)
        .map(|codes| {
            codes
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if codes.is_empty() {
        anyhow::bail!("응답에 쿠폰 코드가 없습니다.");
    }
    Ok(codes)
}

#[poise::command(
    slash_command,
    rename = "내신쿠폰",
    description_localized(
        "ko",
        "코인을 내신 쿠폰 포인트로 교환합니다 (기본 10,000원 = 1포인트)."
    )
)]
pub async fn exchange_coupon(
    ctx: Context<'_>,
    #[description = "교환할 포인트 (1 이상)"] 포인트: i64,
) -> Result<(), Error> {
    if 포인트 < 1 {
        reply_embed(
            ctx,
            "포인트는 1 이상이어야 합니다.",
            "내신 쿠폰",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    }
    let (api_url, api_key, rate) = {
        let config = ctx.data().config.read().await;
        (
            config.coupon_api_url.clone(),
            config.coupon_api_key.clone(),
            config.coupon_coins_per_point.max(1),
        )
    };
    if api_key.trim().is_empty() || api_url.trim().is_empty() {
        reply_embed(
            ctx,
            "쿠폰 API 키가 설정되지 않았습니다. 관리자가 웹 설정에서 등록해야 합니다.",
            "내신 쿠폰",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    }
    let Some(cost) = 포인트.checked_mul(rate) else {
        reply_embed(
            ctx,
            "포인트가 너무 큽니다.",
            "내신 쿠폰",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let user = ctx.author();
    let user_id = user.id.get();
    // 코인을 먼저 차감해 두고(예약), 발급에 실패하면 돌려준다. 발급 중 다른
    // 명령으로 같은 코인을 두 번 쓰는 일을 막기 위해서다.
    let reserved = {
        let mut stats_file = ctx.data().stats.write().await;
        stats::reserve_coins(&mut stats_file, user_id, &user.name, cost)
            .map(|balance| (balance, stats_file.clone()))
    };
    let (balance, snapshot) = match reserved {
        Ok(reserved) => reserved,
        Err(message) => {
            reply_embed(ctx, message, "내신 쿠폰", serenity::Colour::RED, true).await?;
            return Ok(());
        }
    };
    save_stats_snapshot(ctx.data(), snapshot).await;
    if let Err(error) = ctx.defer_ephemeral().await {
        eprintln!("failed to defer coupon exchange: {error:?}");
    }
    match request_coupons(&api_url, &api_key, 포인트).await {
        Ok(codes) => {
            let issued_at =
                chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
            let snapshot = {
                let mut stats_file = ctx.data().stats.write().await;
                stats::record_coupon(
                    &mut stats_file,
                    user_id,
                    &user.name,
                    포인트,
                    codes.clone(),
                    &issued_at,
                );
                stats_file.clone()
            };
            save_stats_snapshot(ctx.data(), snapshot).await;
            let code_lines = codes
                .iter()
                .map(|code| format!("`{code}`"))
                .collect::<Vec<_>>()
                .join("\n");
            reply_embed(
                ctx,
                format!(
                    "쿠폰 발급 완료! **{포인트}포인트** (코인 {} 차감)\n\n쿠폰 코드\n{code_lines}\n\n보유 코인: **{}**\n코드는 본인에게만 보이니 지금 저장해 두세요. `/내정보`에서 최근 발급 기록을 다시 볼 수 있습니다.",
                    stats::coin_text(cost),
                    stats::coin_text(balance)
                ),
                "내신 쿠폰",
                serenity::Colour::DARK_GREEN,
                true,
            )
            .await?;
        }
        Err(error) => {
            eprintln!("coupon issue failed: user_id={user_id} points={포인트} error={error:?}");
            let snapshot = {
                let mut stats_file = ctx.data().stats.write().await;
                stats::refund_coins(&mut stats_file, user_id, &user.name, cost);
                stats_file.clone()
            };
            save_stats_snapshot(ctx.data(), snapshot).await;
            reply_embed(
                ctx,
                format!("쿠폰 발급에 실패해 코인을 돌려드렸습니다.\n원인: {error}"),
                "내신 쿠폰",
                serenity::Colour::RED,
                true,
            )
            .await?;
        }
    }
    Ok(())
}

pub(crate) fn bet_modal(guild_id: serenity::GuildId, current: i64) -> serenity::CreateModal {
    let input =
        serenity::CreateInputText::new(serenity::InputTextStyle::Short, "배팅액(원)", "bet_amount")
            .placeholder("예: 1000 (0이면 배팅 안 함)")
            .min_length(1)
            .max_length(12)
            .required(true)
            .value(current.to_string());
    serenity::CreateModal::new(format!("bet:{}", guild_id.get()), "배팅액 설정")
        .components(vec![serenity::CreateActionRow::InputText(input)])
}

/// `배팅` 버튼: 현재 설정값이 채워진 입력창을 띄운다.
pub async fn handle_bet_open(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
) -> Result<()> {
    let current = data
        .stats
        .read()
        .await
        .users
        .get(&component.user.id.get().to_string())
        .map_or(0, |entry| entry.bet_amount);
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::Modal(bet_modal(guild_id, current)),
        )
        .await?;
    Ok(())
}

/// 배팅액 입력창 제출.
pub async fn handle_bet_submit(
    ctx: &serenity::Context,
    data: &Data,
    modal: &serenity::ModalInteraction,
    _guild_id: serenity::GuildId,
) -> Result<()> {
    let raw = modal_value(modal, "bet_amount").unwrap_or_default();
    let Some(amount) = parse_bet_input(&raw) else {
        send_modal_private(
            ctx,
            modal,
            "배팅액은 0 이상의 숫자로 입력하세요.",
            serenity::Colour::RED,
        )
        .await?;
        return Ok(());
    };
    match apply_bet_setting(data, modal.user.id.get(), &modal.user.name, amount).await {
        Ok(balance) => {
            send_modal_private(
                ctx,
                modal,
                bet_set_message(amount, balance),
                serenity::Colour::DARK_GREEN,
            )
            .await?;
        }
        Err(message) => {
            send_modal_private(ctx, modal, message, serenity::Colour::RED).await?;
        }
    }
    Ok(())
}

/// 스타플레이어 셀렉트: 참가자만, 본인 제외. 마감 전에는 다시 골라 바꿀 수 있다.
pub async fn handle_star_vote(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    guild_id: serenity::GuildId,
) -> Result<()> {
    let Some(running) = data.games.get(&guild_id).map(|entry| entry.clone()) else {
        send_component_private(ctx, component, "진행 중인 게임이 없습니다.").await?;
        return Ok(());
    };
    let Some(target_id) = selected_values(component)
        .first()
        .and_then(|value| value.parse::<u64>().ok())
    else {
        ack_component(ctx, component).await;
        return Ok(());
    };
    let voter_id = component.user.id.get();
    let outcome = {
        let mut running_write = running.write().await;
        let is_participant = running_write
            .game
            .players
            .iter()
            .any(|player| player.user_id == voter_id);
        let target = running_write
            .game
            .players
            .iter()
            .find(|player| player.user_id == target_id)
            .cloned();
        if !running_write.star_vote_open {
            Err("스타플레이어 투표 시간이 아닙니다.".to_string())
        } else if !is_participant {
            Err("이 판 참가자만 투표할 수 있습니다.".to_string())
        } else if voter_id == target_id {
            Err("자기 자신에게는 투표할 수 없습니다.".to_string())
        } else if let Some(target) = target {
            running_write.star_votes.insert(voter_id, target_id);
            let all_voted = running_write
                .game
                .players
                .iter()
                .all(|player| running_write.star_votes.contains_key(&player.user_id));
            Ok((
                game_result_display_name(&running_write, &target),
                all_voted,
                running_write.star_vote_notify.clone(),
            ))
        } else {
            Err("후보가 아닙니다.".to_string())
        }
    };
    match outcome {
        Ok((name, all_voted, notify)) => {
            if all_voted {
                notify.notify_waiters();
            }
            send_component_private(
                ctx,
                component,
                format!(
                    "스타플레이어 투표 완료: **{name}**. 마감 전에는 다시 골라 바꿀 수 있습니다."
                ),
            )
            .await?;
        }
        Err(message) => {
            send_component_private(ctx, component, message).await?;
        }
    }
    Ok(())
}

// commands/coins.rs — 코인: 출석·배팅·선물·내신 쿠폰 명령어, 배팅 입력창, 스타플레이어 투표 처리

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
    let (amount, rules) = {
        let config = ctx.data().config.read().await;
        (config.attendance_coins, config.reward_rules())
    };
    let user = ctx.author();
    let today = stats::kst_today();
    let yesterday = stats::kst_yesterday();
    let (outcome, streak, snapshot) = {
        let mut stats_file = ctx.data().stats.write().await;
        let previous = stats_file
            .users
            .get(&user.id.get().to_string())
            .map(|entry| entry.last_attendance_date.clone())
            .unwrap_or_default();
        let outcome =
            stats::claim_attendance(&mut stats_file, user.id.get(), &user.name, amount, &today);
        let streak = matches!(outcome, stats::AttendanceOutcome::Claimed { .. }).then(|| {
            stats::apply_attendance_streak(
                &mut stats_file,
                user.id.get(),
                &user.name,
                &today,
                &yesterday,
                &previous,
                &rules,
            )
        });
        (outcome, streak, stats_file.clone())
    };
    match outcome {
        stats::AttendanceOutcome::Claimed { amount, balance } => {
            save_stats_snapshot(ctx.data(), snapshot).await;
            let (streak_days, bonus, balance) = streak.map_or((1, 0, balance), |streak| {
                (streak.streak, streak.bonus, streak.balance)
            });
            let bonus_log = if bonus > 0 {
                format!(", 연속 출석 보너스 {}", stats::coin_text(bonus))
            } else {
                String::new()
            };
            ctx.data().audit.push(
                crate::audit_log::COINS,
                format!(
                    "📅 {} 출석 {} (연속 {streak_days}일{bonus_log}, 보유 코인 {})",
                    user.name,
                    stats::coin_text(amount),
                    stats::coin_text(balance)
                ),
            );
            let streak_line = if bonus > 0 {
                format!(
                    "🔥 연속 출석 {streak_days}일째! 보너스 **+{}**",
                    stats::coin_text(bonus)
                )
            } else if rules.streak_week > 0 {
                format!(
                    "🔥 연속 출석 {streak_days}일째 (7일마다 +{}, 다음 보너스까지 {}일)",
                    stats::coin_text(rules.streak_week),
                    7 - streak_days % 7
                )
            } else {
                format!("🔥 연속 출석 {streak_days}일째")
            };
            reply_embed(
                ctx,
                format!(
                    "출석 완료! **{}**을 받았습니다.\n{streak_line}\n보유 코인: **{}**",
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
///
/// 리다이렉트는 따라가지 않는다 (http → https 301을 따라가면 POST가 GET으로 바뀌어
/// API가 깨진다). Cloudflare 보안 확인 페이지(cf-mitigated: challenge)는 봇 쪽에서
/// 통과할 수 없으므로 HTML을 그대로 보여주는 대신 원인을 알려준다.
async fn request_coupons(api_url: &str, api_key: &str, points: i64) -> anyhow::Result<Vec<String>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("mafia-remake-bot/1.0 (+https://github.com/hminkoo10/mafia-remake)")
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let response = client
        .post(api_url)
        .bearer_auth(api_key)
        .header(reqwest::header::ACCEPT, "application/json")
        .json(&serde_json::json!({ "coupon_type": "one_time", "points": points }))
        .send()
        .await?;
    let status = response.status();
    let challenged = response
        .headers()
        .get("cf-mitigated")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("challenge"));
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let text = response.text().await.unwrap_or_default();
    if status.is_redirection() {
        anyhow::bail!(
            "쿠폰 API 주소가 {}(으)로 리다이렉트됩니다. 웹 설정의 쿠폰 API 주소를 https 주소로 바꾸세요.",
            location.unwrap_or_else(|| "다른 주소".to_string())
        );
    }
    if challenged || (!status.is_success() && text.contains("Just a moment")) {
        anyhow::bail!(
            "쿠폰 서버의 Cloudflare 보안 확인이 봇 요청을 차단했습니다 (HTTP {status}). dimigo.store 관리자가 Cloudflare에서 `/api/v1/coupons` 경로를 보안 확인(Managed Challenge)에서 제외해야 합니다."
        );
    }
    let body: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
    let ok = body.get("ok").and_then(serde_json::Value::as_bool) == Some(true);
    if !status.is_success() || !ok {
        let detail = body
            .get("error")
            .or_else(|| body.get("message"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                if text.trim_start().starts_with('<') {
                    "JSON이 아닌 HTML 응답".to_string()
                } else {
                    text.chars().take(200).collect()
                }
            });
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
        // 진행 중인 판에 걸린 배팅은 남긴다 (쿠폰으로 빼 두면 진 배팅이 0원 아래로 사라진다).
        let locked = crate::locked_bet(&ctx.data().bet_locks, user_id);
        stats::reserve_coins(&mut stats_file, user_id, &user.name, cost, locked)
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
            let log_channel_id = ctx.data().config.read().await.log_channel_id;
            send_admin_log(
                ctx.http(),
                log_channel_id,
                "내신 쿠폰 발급",
                format!(
                    "{} 님(`{user_id}`)이 {포인트}포인트 쿠폰 {}장을 발급했습니다 (코인 {} 차감, 남은 코인 {}).",
                    user.name,
                    codes.len(),
                    stats::coin_text(cost),
                    stats::coin_text(balance)
                ),
            )
            .await;
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
            let log_channel_id = ctx.data().config.read().await.log_channel_id;
            send_admin_log(
                ctx.http(),
                log_channel_id,
                "내신 쿠폰 발급 실패",
                format!(
                    "{} 님(`{user_id}`) {포인트}포인트 쿠폰 발급 실패, 코인 {} 환불. 원인: {error}",
                    user.name,
                    stats::coin_text(cost)
                ),
            )
            .await;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum CoinAdminAction {
    #[name = "지급"]
    Give,
    #[name = "차감"]
    Take,
    #[name = "설정"]
    Set,
    #[name = "조회"]
    View,
}

/// 내 코인을 다른 멤버에게 선물한다. 진행 중인 게임에 걸린 배팅액만큼은 남겨 둔다.
#[poise::command(
    slash_command,
    rename = "코인선물",
    description_localized("ko", "내 코인을 다른 멤버에게 선물합니다.")
)]
pub async fn gift_coins(
    ctx: Context<'_>,
    #[description = "코인을 받을 멤버"] 대상: serenity::User,
    #[description = "선물할 금액(원)"]
    #[min = 1]
    금액: i64,
) -> Result<(), Error> {
    const TITLE: &str = "코인 선물";
    if ctx.guild_id().is_none() {
        reply_embed(
            ctx,
            "서버 안에서만 사용할 수 있습니다.",
            TITLE,
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    }
    if 대상.bot {
        reply_embed(
            ctx,
            "봇에게는 코인을 선물할 수 없습니다.",
            TITLE,
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    }
    let sender = ctx.author();
    let sender_id = sender.id.get();
    let receiver_id = 대상.id.get();
    let rules = ctx.data().config.read().await.economy_rules();
    let now_ms = chrono::Utc::now().timestamp_millis();
    let applied = {
        let mut stats_file = ctx.data().stats.write().await;
        // 판 시작(배팅 확정)과 정산도 이 통계 쓰기 잠금 안에서 배팅 잠금을 바꾸므로 끼어들 틈이 없다.
        // 코인은 서버와 상관없이 하나라서 모든 서버의 판을 본다.
        let locked = crate::locked_bet(&ctx.data().bet_locks, sender_id);
        stats::gift_coins(
            &mut stats_file,
            sender_id,
            &sender.name,
            receiver_id,
            &대상.name,
            금액,
            locked,
            &rules,
            now_ms,
        )
        .map(|gift| (gift, stats_file.clone()))
    };
    let (gift, snapshot) = match applied {
        Ok(applied) => applied,
        Err(message) => {
            reply_embed(ctx, message, TITLE, serenity::Colour::RED, true).await?;
            return Ok(());
        }
    };
    save_stats_snapshot(ctx.data(), snapshot).await;
    eprintln!(
        "coin gift: from={sender_id} to={receiver_id} amount={} fee={} sender_after={} receiver_after={}",
        gift.amount, gift.fee, gift.sender_balance, gift.receiver_balance
    );
    let fee_text = if gift.fee > 0 {
        format!(
            " (수수료 {}은 복지 금고로, 받은 금액 {})",
            stats::coin_text(gift.fee),
            stats::coin_text(gift.received)
        )
    } else {
        String::new()
    };
    let log_channel_id = ctx.data().config.read().await.log_channel_id;
    send_admin_log(
        ctx.http(),
        log_channel_id,
        TITLE,
        format!(
            "{} 님(`{sender_id}`)이 {} 님(`{receiver_id}`)에게 {}을 선물했습니다{fee_text}. 보낸 사람 남은 코인 {} / 받은 사람 코인 {}",
            sender.name,
            대상.name,
            stats::coin_text(gift.amount),
            stats::coin_text(gift.sender_balance),
            stats::coin_text(gift.receiver_balance)
        ),
    )
    .await;
    reply_embed(
        ctx,
        format!(
            "<@{sender_id}> 님이 <@{receiver_id}> 님에게 **{}**을 선물했습니다{fee_text}.\n보낸 분 남은 코인: {}",
            stats::coin_text(gift.amount),
            stats::coin_text(gift.sender_balance)
        ),
        TITLE,
        serenity::Colour::GOLD,
        false,
    )
    .await?;
    Ok(())
}

/// 관리자: 유저 코인 지급·차감·설정·조회. 차감은 보유액까지만 되고 0원 아래로
/// 내려가지 않는다. 조정 내역은 서버 로그에 남긴다.
#[poise::command(
    slash_command,
    rename = "코인관리",
    description_localized("ko", "관리자: 유저의 코인을 지급·차감·설정·조회합니다.")
)]
pub async fn manage_coins(
    ctx: Context<'_>,
    #[description = "동작"] 동작: CoinAdminAction,
    #[description = "대상 유저"] 유저: serenity::User,
    #[description = "금액(원). 조회는 생략"] 금액: Option<i64>,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let user_id = 유저.id.get();
    let name = 유저.name.clone();
    let admin_id = ctx.author().id.get();
    let outcome: std::result::Result<String, String> = match 동작 {
        CoinAdminAction::View => {
            let stats_read = ctx.data().stats.read().await;
            let entry = stats_read.users.get(&user_id.to_string());
            let last_attendance = entry
                .map(|entry| entry.last_attendance_date.as_str())
                .filter(|date| !date.is_empty())
                .unwrap_or("없음");
            Ok(format!(
                "{name} 님 코인: **{}**\n배팅 설정: {} / 스타플레이어: {}회 / 마지막 출석: {last_attendance} / 쿠폰 교환: {}포인트",
                stats::coin_text(entry.map_or(0, |entry| entry.coins)),
                stats::coin_text(entry.map_or(0, |entry| entry.bet_amount)),
                entry.map_or(0, |entry| entry.star_player_count),
                entry.map_or(0, |entry| entry.coupon_points_exchanged),
            ))
        }
        CoinAdminAction::Give | CoinAdminAction::Take | CoinAdminAction::Set => match 금액 {
            None => Err("금액을 입력하세요.".to_string()),
            Some(amount) if amount < 0 => Err("금액은 0 이상으로 입력하세요.".to_string()),
            Some(amount) => {
                let applied = {
                    let mut stats_file = ctx.data().stats.write().await;
                    let change = match 동작 {
                        CoinAdminAction::Give => {
                            Ok(stats::adjust_coins(&mut stats_file, user_id, &name, amount))
                        }
                        CoinAdminAction::Take => Ok(stats::adjust_coins(
                            &mut stats_file,
                            user_id,
                            &name,
                            -amount,
                        )),
                        _ => stats::set_coins(&mut stats_file, user_id, &name, amount),
                    };
                    change.map(|change| (change, stats_file.clone()))
                };
                match applied {
                    Ok((change, snapshot)) => {
                        save_stats_snapshot(ctx.data(), snapshot).await;
                        eprintln!(
                            "coin admin: admin={admin_id} target={user_id} action={동작:?} amount={amount} before={} after={}",
                            change.before, change.after
                        );
                        let action_name = match 동작 {
                            CoinAdminAction::Give => "지급",
                            CoinAdminAction::Take => "차감",
                            _ => "설정",
                        };
                        let log_channel_id = ctx.data().config.read().await.log_channel_id;
                        send_admin_log(
                            ctx.http(),
                            log_channel_id,
                            "코인 관리",
                            format!(
                                "{} 님이 {name} 님(`{user_id}`) 코인 {action_name} {}: {} → {}",
                                ctx.author().name,
                                stats::coin_text(amount),
                                stats::coin_text(change.before),
                                stats::coin_text(change.after)
                            ),
                        )
                        .await;
                        let verb = match 동작 {
                            CoinAdminAction::Give => "지급",
                            CoinAdminAction::Take => "차감",
                            _ => "설정",
                        };
                        let clipped = if 동작 == CoinAdminAction::Take && amount > change.before {
                            " (보유액까지만 차감)"
                        } else {
                            ""
                        };
                        Ok(format!(
                            "{name} 님 코인 {verb}: {} → **{}** ({}){clipped}",
                            stats::coin_text(change.before),
                            stats::coin_text(change.after),
                            stats::signed_coin_text(change.after - change.before)
                        ))
                    }
                    Err(message) => Err(message),
                }
            }
        },
    };
    match outcome {
        Ok(message) => {
            reply_embed(
                ctx,
                message,
                "코인 관리",
                serenity::Colour::DARK_GREEN,
                false,
            )
            .await?;
        }
        Err(message) => {
            reply_embed(ctx, message, "코인 관리", serenity::Colour::RED, true).await?;
        }
    }
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "쿠폰발급",
    description_localized(
        "ko",
        "관리자: 코인 쿠폰을 발급합니다. 코드는 1회용이며 만료일은 선택입니다 (기본 무기한)."
    )
)]
pub async fn issue_coupons(
    ctx: Context<'_>,
    #[description = "쿠폰 하나로 받을 코인(원)"] 코인: i64,
    #[description = "발급 개수 (1~100)"] 개수: i64,
    #[description = "쿠폰 코드 텍스트 (비우면 무작위 코드, 2개 이상이면 -1, -2 접미)"]
    쿠폰텍스트: Option<String>,
    #[description = "만료일 YYYY-MM-DD (비우면 무기한, 그날까지 사용 가능)"] 만료일: Option<String>,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let today = stats::kst_today();
    let expires_on = match 만료일
        .as_deref()
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
    {
        Some(raw) => match stats::parse_coupon_expiry(raw, &today) {
            Ok(value) => Some(value),
            Err(message) => {
                reply_embed(ctx, message, "쿠폰 발급", serenity::Colour::RED, true).await?;
                return Ok(());
            }
        },
        None => None,
    };
    let issued_at = chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
    let admin_id = ctx.author().id.get();
    let issued = {
        let mut stats_file = ctx.data().stats.write().await;
        let mut rng = mafia_remake::system_random::rng();
        stats::issue_coupons(
            &mut stats_file,
            코인,
            개수,
            쿠폰텍스트.as_deref(),
            expires_on.clone(),
            admin_id,
            &issued_at,
            &mut rng,
        )
        .map(|codes| (codes, stats_file.clone()))
    };
    let (codes, snapshot) = match issued {
        Ok(issued) => issued,
        Err(message) => {
            reply_embed(ctx, message, "쿠폰 발급", serenity::Colour::RED, true).await?;
            return Ok(());
        }
    };
    save_stats_snapshot(ctx.data(), snapshot).await;
    let expiry_text = expires_on
        .as_deref()
        .map_or("무기한".to_string(), |date| format!("{date}까지"));
    let code_block = format!("```\n{}\n```", codes.join("\n"));
    let log_channel_id = ctx.data().config.read().await.log_channel_id;
    send_admin_log(
        ctx.http(),
        log_channel_id,
        "쿠폰 발급",
        format!(
            "{} 님이 {} 쿠폰 {}장을 발급했습니다 ({expiry_text}).\n{code_block}",
            ctx.author().name,
            stats::coin_text(코인),
            codes.len()
        ),
    )
    .await;
    reply_embed(
        ctx,
        format!(
            "**{}** 쿠폰 {}장을 발급했습니다 ({expiry_text}). 코드는 각각 한 사람이 한 번만 쓸 수 있습니다.\n`/쿠폰사용 코드`로 사용합니다.\n{code_block}",
            stats::coin_text(코인),
            codes.len()
        ),
        "쿠폰 발급",
        serenity::Colour::DARK_GREEN,
        true,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "쿠폰사용",
    description_localized("ko", "코인 쿠폰 코드를 사용해 코인을 받습니다.")
)]
pub async fn redeem_coupon(
    ctx: Context<'_>,
    #[description = "쿠폰 코드"] 코드: String,
) -> Result<(), Error> {
    let user = ctx.author();
    let today = stats::kst_today();
    let redeemed_at = chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
    let result = {
        let mut stats_file = ctx.data().stats.write().await;
        stats::redeem_coupon(
            &mut stats_file,
            user.id.get(),
            &user.name,
            &코드,
            &today,
            &redeemed_at,
        )
        .map(|redemption| (redemption, stats_file.clone()))
    };
    match result {
        Ok((redemption, snapshot)) => {
            save_stats_snapshot(ctx.data(), snapshot).await;
            let log_channel_id = ctx.data().config.read().await.log_channel_id;
            send_admin_log(
                ctx.http(),
                log_channel_id,
                "쿠폰 사용",
                format!(
                    "{} 님(`{}`)이 쿠폰 `{}`을 사용해 {}을 받았습니다 (보유 {}).",
                    user.name,
                    user.id.get(),
                    redemption.code,
                    stats::coin_text(redemption.coins),
                    stats::coin_text(redemption.balance)
                ),
            )
            .await;
            reply_embed(
                ctx,
                format!(
                    "쿠폰 `{}` 사용 완료! **{}**을 받았습니다.\n보유 코인: **{}**",
                    redemption.code,
                    stats::coin_text(redemption.coins),
                    stats::coin_text(redemption.balance)
                ),
                "쿠폰 사용",
                serenity::Colour::DARK_GREEN,
                true,
            )
            .await?;
        }
        Err(message) => {
            reply_embed(ctx, message, "쿠폰 사용", serenity::Colour::RED, true).await?;
        }
    }
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "쿠폰목록",
    description_localized("ko", "관리자: 아직 사용되지 않은 코인 쿠폰을 확인합니다.")
)]
pub async fn list_coupons(ctx: Context<'_>) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let today = stats::kst_today();
    let (lines, total) = {
        let stats_read = ctx.data().stats.read().await;
        let coupons = stats::active_coupons(&stats_read, &today);
        let lines = coupons
            .iter()
            .take(50)
            .map(|coupon| {
                format!(
                    "`{}` {} ({})",
                    coupon.code,
                    stats::coin_text(coupon.coins),
                    coupon
                        .expires_on
                        .as_deref()
                        .map_or("무기한".to_string(), |date| format!("{date}까지"))
                )
            })
            .collect::<Vec<_>>();
        (lines, coupons.len())
    };
    let message = if lines.is_empty() {
        "사용 가능한 쿠폰이 없습니다.".to_string()
    } else {
        format!(
            "사용 가능한 쿠폰 {total}장{}\n{}",
            if total > 50 {
                " (50장까지 표시)"
            } else {
                ""
            },
            lines.join("\n")
        )
    };
    reply_embed(ctx, message, "쿠폰 목록", serenity::Colour::GOLD, true).await?;
    Ok(())
}

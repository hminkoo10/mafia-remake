// commands/casino_cmds.rs — 카지노: 테이블 생성·닫기·목록·입장 명령, Discord 채널 중계(상태 임베드·
// 웹훅 채팅·결과 안내), 채널 채팅 → 테이블 채팅.

use super::*;
use crate::casino_hub::{CasinoHub, TableBinding, personal_link, table_channel_name};
use mafia_remake::casino::{
    CasinoEvent, ChatMessage, GameKind, HandResult, Phase, TableView, blackjack_value, signed_chips,
};
use std::sync::atomic::Ordering;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum CasinoGameChoice {
    #[name = "홀덤"]
    Holdem,
    #[name = "블랙잭"]
    Blackjack,
}

impl CasinoGameChoice {
    fn kind(self) -> GameKind {
        match self {
            Self::Holdem => GameKind::Holdem,
            Self::Blackjack => GameKind::Blackjack,
        }
    }
}

fn card_text(card: &str) -> String {
    if card == "??" {
        return "🂠".to_string();
    }
    let mut chars = card.chars();
    let rank = match chars.next() {
        Some('T') => "10".to_string(),
        Some(rank) => rank.to_string(),
        None => return card.to_string(),
    };
    let suit = match chars.next() {
        Some('s') => "♠",
        Some('h') => "♥",
        Some('d') => "♦",
        Some('c') => "♣",
        _ => "",
    };
    format!("{rank}{suit}")
}

fn cards_text(cards: &[String]) -> String {
    if cards.is_empty() {
        "-".to_string()
    } else {
        cards
            .iter()
            .map(|card| card_text(card))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// 테이블 채널에 올리는 상태 임베드 본문.
pub fn render_table_status(view: &TableView, base_url: &str) -> String {
    let mut lines = Vec::new();
    lines.push(format!("**{}** · {}", view.name, view.kind.value()));
    match view.round.as_ref() {
        Some(round) => {
            let mut status = format!("단계: **{}**", round.phase_text);
            if view.kind == GameKind::Holdem {
                status.push_str(&format!(" · 팟 **{}**", format_number(round.pot)));
                status.push_str(&format!("\n보드: {}", cards_text(&round.board)));
            } else {
                status.push_str(&format!("\n딜러: {}", cards_text(&round.dealer)));
                if let Some(total) = round.dealer_total {
                    status.push_str(&format!(" ({total})"));
                }
            }
            if round.phase != Phase::Complete && round.deadline > 0 {
                let turn_name = view
                    .seats
                    .iter()
                    .flatten()
                    .find(|seat| seat.seat as i32 == round.turn)
                    .map(|seat| seat.name.clone());
                let deadline_seconds = round.deadline / 1000;
                match turn_name {
                    Some(name) => status.push_str(&format!(
                        "\n차례: **{name}** (마감 <t:{deadline_seconds}:R>)"
                    )),
                    None => status.push_str(&format!("\n마감 <t:{deadline_seconds}:R>")),
                }
            }
            lines.push(status);
        }
        None => lines.push("아직 라운드가 없습니다. 자리에 앉아 시작하세요.".to_string()),
    }
    let seats = view
        .seats
        .iter()
        .flatten()
        .map(|seat| {
            let mut flags = Vec::new();
            if seat.folded {
                flags.push("폴드");
            }
            if seat.sit_out {
                flags.push("자리 비움");
            }
            if seat.leaving {
                flags.push("퇴장 대기");
            }
            let mut line = format!(
                "{}번 **{}** {}칩",
                seat.seat + 1,
                seat.name,
                format_number(seat.stack)
            );
            if seat.bet > 0 {
                line.push_str(&format!(" (베팅 {})", format_number(seat.bet)));
            }
            if view.kind == GameKind::Blackjack && !seat.hands.is_empty() {
                let hands = seat
                    .hands
                    .iter()
                    .map(|hand| {
                        let mut text = format!("{} = {}", cards_text(&hand.cards), hand.total);
                        if let Some(result) = &hand.result {
                            text.push_str(&format!(" {result}"));
                        }
                        text
                    })
                    .collect::<Vec<_>>()
                    .join(" / ");
                line.push_str(&format!("\n   {hands}"));
            } else if view.kind == GameKind::Holdem && !seat.cards.is_empty() {
                line.push_str(&format!(" {}", cards_text(&seat.cards)));
            }
            if !flags.is_empty() {
                line.push_str(&format!(" · {}", flags.join(", ")));
            }
            line
        })
        .collect::<Vec<_>>();
    if seats.is_empty() {
        lines.push("좌석: 비어 있음".to_string());
    } else {
        lines.push(format!(
            "좌석 ({}/{})\n{}",
            seats.len(),
            view.seats.len(),
            seats.join("\n")
        ));
    }
    lines.push(format!("🎙️ 소피아: {}", view.narration));
    lines.push(format!(
        "참여하려면 Discord에서 `/카지노입장 테이블:{}` 을 입력해 개인 링크를 받으세요.\n{}",
        view.name,
        base_url.trim_end_matches('/')
    ));
    lines.join("\n\n")
}

/// 핸드/라운드 결과 본문: 참가자별 손익(순증감)과 보드/딜러 카드.
fn render_hand_result(result: &HandResult, kind: GameKind) -> String {
    let mut lines = Vec::new();
    if kind == GameKind::Blackjack && !result.board.is_empty() {
        let (total, _) = blackjack_value(&result.board);
        let total_text = if total > 21 {
            "버스트".to_string()
        } else {
            total.to_string()
        };
        lines.push(format!(
            "딜러: {} ({total_text})",
            cards_text(&result.board)
        ));
    }
    for entry in &result.results {
        lines.push(format!(
            "**{}** {} · {}",
            entry.name,
            signed_chips(entry.net),
            entry.label
        ));
    }
    if result.results.is_empty() {
        lines.push(result.summary.clone());
    }
    if kind == GameKind::Holdem && !result.board.is_empty() {
        lines.push(format!("보드: {}", cards_text(&result.board)));
    }
    lines.join("\n")
}

fn format_number(value: i64) -> String {
    mafia_remake::casino::format_chips(value)
}

fn casino_base_url(data: &Data) -> String {
    data.casino_base_url.as_str().to_string()
}

// ------------------------------------------------------------ 명령어

#[poise::command(
    slash_command,
    rename = "카지노테이블생성",
    description_localized("ko", "관리자: 카지노 테이블을 만들고 전용 채널을 엽니다.")
)]
pub async fn create_casino_table(
    ctx: Context<'_>,
    #[description = "게임 종류"] 종류: CasinoGameChoice,
    #[description = "테이블 이름 (2~24자)"] 이름: String,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let Some(guild_id) = ctx.guild_id() else {
        reply_embed(
            ctx,
            "서버에서만 사용할 수 있습니다.",
            "카지노",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let hub = ctx.data().casino.clone();
    if hub.find_table(&이름).await.is_some() {
        reply_embed(
            ctx,
            "같은 이름의 테이블이 이미 있습니다.",
            "카지노",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    }
    // 채널 생성은 3초를 넘길 수 있으니 여기서부터 지연 응답으로 바꾼다.
    let deferred = defer_best_effort(ctx, "카지노테이블생성").await;
    let serenity_ctx = ctx.serenity_context();
    let category = source_category(serenity_ctx, ctx.channel_id()).await;
    let channel_name = table_channel_name(&이름);
    let Some(channel) = create_text_channel_safe(
        serenity_ctx,
        guild_id,
        &channel_name,
        vec![],
        category,
        "카지노 테이블 채널 생성",
        0,
        Some(format!(
            "{} 테이블 · 웹에서 플레이, 채팅은 이 채널과 연결됩니다.",
            종류.kind().value()
        )),
    )
    .await
    else {
        reply_embed(
            ctx,
            "테이블 채널을 만들지 못했습니다. 봇의 채널 관리 권한을 확인하세요.",
            "카지노",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let binding = TableBinding {
        guild_id: guild_id.get(),
        channel_id: channel.id.get(),
        ..Default::default()
    };
    let table_id = match hub.create_table(종류.kind(), &이름, ctx.author().id.get(), binding) {
        Ok(id) => id,
        Err(message) => {
            let _ = channel.delete(&serenity_ctx.http).await;
            reply_embed(ctx, message, "카지노", serenity::Colour::RED, true).await?;
            return Ok(());
        }
    };
    hub.save().await;
    refresh_table_status(serenity_ctx, ctx.data(), &table_id).await;
    let log_channel_id = ctx.data().config.read().await.log_channel_id;
    send_admin_log(
        ctx.http(),
        log_channel_id,
        "카지노 테이블",
        format!(
            "{} 님이 {} 테이블 **{}**(<#{}>)을 만들었습니다.",
            ctx.author().name,
            종류.kind().value(),
            이름,
            channel.id.get()
        ),
    )
    .await;
    let message = format!(
        "{} 테이블 **{}**을 만들었습니다. 채널: <#{}>\n참가자는 `/카지노입장 테이블:{}` 으로 개인 링크를 받습니다.",
        종류.kind().value(),
        이름,
        channel.id.get(),
        이름
    );
    if deferred {
        reply_embed(
            ctx,
            message,
            "카지노 테이블",
            serenity::Colour::DARK_GREEN,
            false,
        )
        .await?;
    } else {
        reply_embed_with_channel_fallback(
            ctx,
            message,
            "카지노 테이블",
            serenity::Colour::DARK_GREEN,
            false,
        )
        .await?;
    }
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "카지노테이블닫기",
    description_localized(
        "ko",
        "관리자: 테이블을 닫고 모든 칩을 코인으로 돌려준 뒤 채널을 지웁니다."
    )
)]
pub async fn close_casino_table(
    ctx: Context<'_>,
    #[description = "테이블 이름"] 테이블: String,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let hub = ctx.data().casino.clone();
    let Some((table_id, table)) = hub.find_table(&테이블).await else {
        reply_embed(
            ctx,
            "그 이름의 테이블이 없습니다.",
            "카지노",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
    let name = table.read().await.name.clone();
    let binding = hub.binding(&table_id);
    let events = hub.close_table(&table_id).await;
    let refunds = events
        .iter()
        .filter_map(|event| match event {
            CasinoEvent::CashOut { name, amount, .. } => {
                Some(format!("{name} {}원", format_number(*amount)))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if let Some(binding) = binding {
        let channel_id = serenity::ChannelId::new(binding.channel_id);
        let _ = send_channel_embed(
            ctx.http(),
            channel_id,
            format!(
                "테이블이 닫혔습니다. 남은 칩은 코인으로 돌아갔습니다.{}",
                if refunds.is_empty() {
                    String::new()
                } else {
                    format!("\n{}", refunds.join("\n"))
                }
            ),
            "카지노 테이블 종료",
            serenity::Colour::DARK_GREY,
            vec![],
        )
        .await;
        let _ = channel_id.delete(ctx.http()).await;
    }
    let log_channel_id = ctx.data().config.read().await.log_channel_id;
    send_admin_log(
        ctx.http(),
        log_channel_id,
        "카지노 테이블",
        format!(
            "{} 님이 테이블 **{name}**을 닫았습니다. 반환: {}",
            ctx.author().name,
            if refunds.is_empty() {
                "없음".to_string()
            } else {
                refunds.join(", ")
            }
        ),
    )
    .await;
    reply_embed(
        ctx,
        format!(
            "테이블 **{name}**을 닫았습니다.{}",
            if refunds.is_empty() {
                String::new()
            } else {
                format!("\n칩 반환: {}", refunds.join(", "))
            }
        ),
        "카지노 테이블",
        serenity::Colour::DARK_GREEN,
        false,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "카지노테이블목록",
    description_localized("ko", "열려 있는 카지노 테이블을 봅니다.")
)]
pub async fn list_casino_tables(ctx: Context<'_>) -> Result<(), Error> {
    let summaries = ctx.data().casino.summaries().await;
    let message = if summaries.is_empty() {
        "열려 있는 테이블이 없습니다. 관리자가 `/카지노테이블생성`으로 만들 수 있습니다."
            .to_string()
    } else {
        summaries
            .iter()
            .map(|summary| {
                format!(
                    "**{}** ({}) · {}/{}명 · {}{}",
                    summary.name,
                    summary.kind_text,
                    summary.seated,
                    summary.seat_count,
                    summary
                        .phase_text
                        .clone()
                        .unwrap_or_else(|| "대기 중".to_string()),
                    summary
                        .channel_id
                        .map_or(String::new(), |id| format!(" · <#{id}>"))
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    reply_embed(ctx, message, "카지노 테이블", serenity::Colour::GOLD, false).await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "카지노입장",
    description_localized("ko", "테이블에 들어갈 개인 링크를 받습니다 (본인 전용, 12시간 유효).")
)]
pub async fn enter_casino(
    ctx: Context<'_>,
    #[description = "테이블 이름 (비우면 첫 테이블)"] 테이블: Option<String>,
) -> Result<(), Error> {
    if ctx.guild_id().is_none() {
        reply_embed(
            ctx,
            "서버에서만 사용할 수 있습니다.",
            "카지노",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    }
    let hub = ctx.data().casino.clone();
    let summaries = hub.summaries().await;
    if summaries.is_empty() {
        reply_embed(
            ctx,
            "열려 있는 테이블이 없습니다. 관리자가 `/카지노테이블생성`으로 만들 수 있습니다.",
            "카지노",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    }
    let table_id = match 테이블
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(query) => match hub.find_table(query).await {
            Some((id, _)) => id,
            None => {
                reply_embed(
                    ctx,
                    "그 이름의 테이블이 없습니다. `/카지노테이블목록`으로 확인하세요.",
                    "카지노",
                    serenity::Colour::RED,
                    true,
                )
                .await?;
                return Ok(());
            }
        },
        None => summaries[0].id.clone(),
    };
    let user = ctx.author();
    let display_name = ctx
        .author_member()
        .await
        .map(|member| member.display_name().to_string())
        .unwrap_or_else(|| user.name.clone());
    let token = hub.issue_session(user.id.get(), display_name.clone());
    let link = personal_link(&casino_base_url(ctx.data()), &token, &table_id);
    let coins = hub.coins_of(user.id.get()).await;
    reply_embed(
        ctx,
        format!(
            "아래 링크로 테이블에 들어가세요.\n{link}\n\n⚠️ 이 링크는 **{display_name}** 님 전용이고 12시간 동안 유효합니다. 다른 사람과 공유하지 마세요.\n보유 코인: **{}** (바이인은 5,000~20,000원, 퇴장하면 남은 칩이 코인으로 돌아옵니다)",
            stats::coin_text(coins)
        ),
        "카지노 입장",
        serenity::Colour::DARK_GREEN,
        true,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "카지노상태",
    description_localized("ko", "내 코인과 테이블 상황을 봅니다.")
)]
pub async fn casino_status(ctx: Context<'_>) -> Result<(), Error> {
    let hub = ctx.data().casino.clone();
    let user = ctx.author();
    let coins = hub.coins_of(user.id.get()).await;
    let seated = hub.seated_table_of(user.id.get()).await;
    let seated_text = match seated {
        Some(id) => {
            let mut stack = None;
            if let Some(table) = hub.table(&id) {
                let table = table.read().await;
                if let Some(seat) = table
                    .seat_index(user.id.get())
                    .and_then(|index| table.seats[index].as_ref())
                {
                    stack = Some((table.name.clone(), seat.stack));
                }
            }
            match stack {
                Some((name, stack)) => format!(
                    "앉은 테이블: **{name}** (테이블 칩 {})",
                    format_number(stack)
                ),
                None => "앉은 테이블: 있음".to_string(),
            }
        }
        None => "앉은 테이블: 없음".to_string(),
    };
    let summaries = hub.summaries().await;
    let tables = if summaries.is_empty() {
        "열려 있는 테이블 없음".to_string()
    } else {
        summaries
            .iter()
            .map(|summary| {
                format!(
                    "- {} ({}) {}/{}명",
                    summary.name, summary.kind_text, summary.seated, summary.seat_count
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    reply_embed(
        ctx,
        format!(
            "보유 코인: **{}**\n{seated_text}\n하우스 누적: {}\n\n테이블\n{tables}",
            stats::coin_text(coins),
            stats::coin_text(hub.house.load(Ordering::Relaxed))
        ),
        "카지노",
        serenity::Colour::GOLD,
        true,
    )
    .await?;
    Ok(())
}

// ------------------------------------------------------------ Discord 중계

/// 테이블 채널의 웹훅 (없으면 만든다).
async fn casino_webhook(
    ctx: &serenity::Context,
    hub: &CasinoHub,
    channel_id: serenity::ChannelId,
) -> Option<serenity::Webhook> {
    if let Some(webhook) = hub.webhooks.get(&channel_id.get()) {
        return Some(webhook.clone());
    }
    let existing = channel_id
        .webhooks(&ctx.http)
        .await
        .ok()
        .and_then(|webhooks| {
            webhooks
                .into_iter()
                .find(|webhook| webhook.name.as_deref() == Some("Mafia Casino"))
        });
    let webhook = match existing {
        Some(webhook) => webhook,
        None => match crate::http_pool::with_fallback(ctx, |http| async move {
            channel_id
                .create_webhook(
                    &http,
                    serenity::CreateWebhook::new("Mafia Casino")
                        .audit_log_reason("카지노 테이블 채팅 웹훅 생성"),
                )
                .await
        })
        .await
        {
            Ok(webhook) => webhook,
            Err(error) => {
                eprintln!(
                    "failed to create casino webhook for channel {}: {error:?}",
                    channel_id.get()
                );
                return None;
            }
        },
    };
    hub.webhooks.insert(channel_id.get(), webhook.clone());
    Some(webhook)
}

/// 아직 채널로 보내지 않은 테이블 채팅을 웹훅으로 보낸다.
async fn relay_chat_to_channel(
    ctx: &serenity::Context,
    hub: &CasinoHub,
    table_id: &str,
    channel_id: serenity::ChannelId,
) {
    let messages: Vec<ChatMessage> = hub.unrelayed_messages(table_id).await;
    if messages.is_empty() {
        return;
    }
    let Some(webhook) = casino_webhook(ctx, hub, channel_id).await else {
        return;
    };
    let mut last_seq = 0;
    for message in messages {
        // Discord 채널에서 온 메시지는 이미 채널에 있으니 다시 보내지 않는다.
        if message.from_discord {
            last_seq = message.seq;
            continue;
        }
        let username = if message.dealer {
            "소피아 (딜러)".to_string()
        } else {
            message.name.clone()
        };
        let execute = serenity::ExecuteWebhook::new()
            .content(message.text.chars().take(1900).collect::<String>())
            .username(username)
            .allowed_mentions(
                serenity::CreateAllowedMentions::new()
                    .all_users(false)
                    .all_roles(false)
                    .everyone(false),
            );
        if let Err(error) = webhook.execute(&ctx.http, false, execute).await {
            eprintln!(
                "failed to relay casino chat to channel {}: {error:?}",
                channel_id.get()
            );
            break;
        }
        last_seq = message.seq;
    }
    if last_seq > 0 {
        hub.update_binding(table_id, |binding| {
            binding.relayed_message_seq = binding.relayed_message_seq.max(last_seq);
        });
    }
}

/// 상태 임베드를 갱신하고, 새 핸드 결과가 있으면 알린다.
pub async fn refresh_table_status(ctx: &serenity::Context, data: &Data, table_id: &str) {
    let hub = data.casino.clone();
    let Some(binding) = hub.binding(table_id) else {
        return;
    };
    let Some(view) = hub.view_for(table_id, None).await else {
        return;
    };
    let channel_id = serenity::ChannelId::new(binding.channel_id);
    relay_chat_to_channel(ctx, &hub, table_id, channel_id).await;
    if let Some(result) = view.history.first() {
        if binding.announced_result_id.as_deref() != Some(result.id.as_str()) {
            let _ = send_channel_embed(
                &ctx.http,
                channel_id,
                render_hand_result(result, view.kind),
                if view.kind == GameKind::Holdem {
                    "핸드 결과"
                } else {
                    "라운드 결과"
                },
                serenity::Colour::GOLD,
                vec![],
            )
            .await;
            hub.update_binding(table_id, |binding| {
                binding.announced_result_id = Some(result.id.clone())
            });
        }
    }
    let body = render_table_status(&view, &casino_base_url(data));
    let embed = make_embed(body, "카지노 테이블 현황", serenity::Colour::DARK_GREEN);
    let existing = hub
        .binding(table_id)
        .and_then(|binding| binding.status_message_id);
    if let Some(message_id) = existing {
        let edited = channel_id
            .edit_message(
                &ctx.http,
                serenity::MessageId::new(message_id),
                serenity::EditMessage::new().embed(embed.clone()),
            )
            .await;
        if edited.is_ok() {
            return;
        }
    }
    match channel_id
        .send_message(&ctx.http, serenity::CreateMessage::new().embed(embed))
        .await
    {
        Ok(message) => {
            let _ = message.pin(&ctx.http).await;
            hub.update_binding(table_id, |binding| {
                binding.status_message_id = Some(message.id.get())
            });
            hub.save().await;
        }
        Err(error) => eprintln!("failed to post casino status for {table_id}: {error:?}"),
    }
}

/// 테이블 변경 알림을 받아 채널을 갱신하는 작업. 임베드 갱신은 테이블당 1.5초로 묶는다.
pub async fn run_casino_relay(ctx: serenity::Context, data: Data) {
    let hub = data.casino.clone();
    let mut updates = hub.updates.subscribe();
    let mut last_refresh: HashMap<String, Instant> = HashMap::new();
    let mut pending: HashSet<String> = HashSet::new();
    let mut flush = tokio::time::interval(Duration::from_millis(1_500));
    loop {
        tokio::select! {
            update = updates.recv() => {
                match update {
                    Ok(update) => { pending.insert(update.table_id); }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        for entry in hub.tables.iter() { pending.insert(entry.key().clone()); }
                    }
                    Err(_) => break,
                }
            }
            _ = flush.tick() => {
                if pending.is_empty() { continue; }
                let due = pending.iter().filter(|id| last_refresh.get(*id).is_none_or(|at| at.elapsed() >= Duration::from_millis(1_400))).cloned().collect::<Vec<_>>();
                for table_id in due {
                    pending.remove(&table_id);
                    if hub.binding(&table_id).is_none() { continue; }
                    refresh_table_status(&ctx, &data, &table_id).await;
                    last_refresh.insert(table_id, Instant::now());
                }
            }
        }
    }
}

/// 시간 초과·유휴 정리 루프.
pub async fn run_casino_ticker(data: Data) {
    let hub = data.casino.clone();
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        interval.tick().await;
        hub.tick_all().await;
    }
}

/// 테이블 채널에 사람이 쓴 메시지를 테이블 채팅에 넣는다. 처리했으면 true.
pub async fn handle_casino_channel_message(data: &Data, message: &serenity::Message) -> bool {
    if message.author.bot || message.webhook_id.is_some() {
        return false;
    }
    let hub = data.casino.clone();
    let Some(table_id) = hub.table_id_for_channel(message.channel_id.get()) else {
        return false;
    };
    let text = message.content.trim();
    if text.is_empty() {
        return true;
    }
    let name = message
        .member
        .as_ref()
        .and_then(|member| member.nick.clone())
        .unwrap_or_else(|| message.author.name.clone());
    let text = text.chars().take(240).collect::<String>();
    hub.relay_chat_from_discord(&table_id, message.author.id.get(), &name, &text)
        .await;
    true
}

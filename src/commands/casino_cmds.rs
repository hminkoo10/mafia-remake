// commands/casino_cmds.rs — 카지노: 테이블 생성·닫기·목록·입장 명령, Discord 채널 중계(상태 임베드·
// 웹훅 채팅·결과 안내), 채널 채팅 → 테이블 채팅.

use super::*;
use crate::casino_hub::{
    CasinoHub, MAX_TABLES, PanelBinding, TableBinding, UpdateKind, personal_link,
    table_channel_name, table_channel_overwrites,
};
use mafia_remake::casino::{
    CasinoEvent, ChatMessage, GameKind, HandResult, Phase, SettingsRequest, TableSettings,
    TableView, blackjack_value, signed_chips,
};
use poise::serenity_prelude::CacheHttp;
use std::collections::{BTreeSet, HashMap, HashSet};
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
    let rules = &view.rules;
    let stakes = match view.kind {
        GameKind::Holdem => format!(
            "블라인드 {}/{}",
            format_number(rules.small_blind),
            format_number(rules.big_blind)
        ),
        GameKind::Blackjack => format!(
            "베팅 {}~{}",
            format_number(rules.min_bet),
            format_number(rules.max_bet)
        ),
    };
    lines.push(format!(
        "**{}** · {} · {stakes} · 바이인 {}~{}",
        view.name,
        view.kind.value(),
        format_number(rules.min_buy_in),
        format_number(rules.max_buy_in)
    ));
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
    lines.push(format!("🎙️ {}: {}", view.dealer.name, view.narration));
    lines.push(format!(
        "아래 **테이블 입장** 버튼을 누르거나 `/카지노입장 테이블:{}` 을 입력하면 개인 링크를 받습니다.\n{}",
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
        let headline = if entry.won > 0 {
            // 이긴 베팅이 돌려준 총액 (블랙잭 5,000 → 12,500). 예전 기록은 이익만 있다.
            if entry.paid > 0 {
                format!("이김 {}", format_number(entry.paid))
            } else {
                format!("이김 {}", signed_chips(entry.won))
            }
        } else if entry.net < 0 {
            format!("패배 {}", signed_chips(entry.net))
        } else {
            "푸시".to_string()
        };
        let notes = if entry.notes.is_empty() {
            String::new()
        } else {
            format!(" · {}", entry.notes.join(" · "))
        };
        lines.push(format!(
            "**{}** {headline}\n순손익 {} · {}{notes}",
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
#[allow(clippy::too_many_arguments)]
pub async fn create_casino_table(
    ctx: Context<'_>,
    #[description = "게임 종류"] 종류: CasinoGameChoice,
    #[description = "테이블 이름 (2~24자)"] 이름: String,
    #[description = "블랙잭 최소 베팅 (기본 100, 100 단위)"] 최소베팅: Option<i64>,
    #[description = "블랙잭 최대 베팅 (기본 최소 베팅×50, 최소 5,000)"] 최대베팅: Option<i64>,
    #[description = "블랙잭 사이드베팅 최대 (기본 최대 베팅의 절반, 0이면 사이드베팅 없음)"]
    사이드최대: Option<i64>,
    #[description = "홀덤 빅 블라인드 (기본 100, 짝수, 스몰 블라인드는 절반)"] 빅블라인드: Option<
        i64,
    >,
    #[description = "최소 바이인 (기본: 홀덤 빅 블라인드×50, 블랙잭 최소 베팅×50)"]
    최소바이인: Option<i64>,
    #[description = "최대 바이인 (기본: 홀덤 빅 블라인드×200, 블랙잭 최대 베팅×4)"]
    최대바이인: Option<i64>,
    #[description = "액션 제한 시간 (초, 10~120, 기본 30)"] 제한시간: Option<i64>,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let settings = match TableSettings::build(
        종류.kind(),
        SettingsRequest {
            min_bet: 최소베팅,
            max_bet: 최대베팅,
            big_blind: 빅블라인드,
            min_buy_in: 최소바이인,
            max_buy_in: 최대바이인,
            side_bet_max: 사이드최대,
            turn_secs: 제한시간,
        },
    ) {
        Ok(settings) => settings,
        Err(message) => {
            reply_embed(ctx, message, "카지노", serenity::Colour::RED, true).await?;
            return Ok(());
        }
    };
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
    if ctx.data().casino.find_table(&이름).await.is_some() {
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
    let message = match create_table_body(
        ctx.serenity_context(),
        ctx.data(),
        guild_id,
        ctx.channel_id(),
        종류.kind(),
        &이름,
        ctx.author().id.get(),
        &ctx.author().name,
        settings,
        false,
    )
    .await
    {
        Ok(message) => message,
        Err(message) => {
            reply_embed(ctx, message, "카지노", serenity::Colour::RED, true).await?;
            return Ok(());
        }
    };
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
    let Some((table_id, _)) = ctx.data().casino.find_table(&테이블).await else {
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
    // 채널 알림·삭제·로그·패널 갱신이 3초를 넘길 수 있으니 여기서부터 지연 응답으로 바꾼다.
    defer_best_effort(ctx, "카지노테이블닫기").await;
    let Some((name, refunds)) = close_table_body(
        ctx.serenity_context(),
        ctx.data(),
        &table_id,
        &ctx.author().name,
    )
    .await?
    else {
        reply_embed(
            ctx,
            "이미 닫힌 테이블입니다.",
            "카지노",
            serenity::Colour::RED,
            true,
        )
        .await?;
        return Ok(());
    };
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
                    "**{}** ({} · {}) · {}/{}명 · {}{}",
                    summary.name,
                    summary.kind_text,
                    summary.stakes,
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
    let table_name = match hub.table(&table_id) {
        Some(table) => table.read().await.name.clone(),
        None => table_id.clone(),
    };
    let message = personal_entry_text(
        ctx.data(),
        &table_id,
        &table_name,
        user.id.get(),
        &display_name,
    )
    .await;
    reply_embed(
        ctx,
        message,
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
    let message = casino_status_text(ctx.data(), ctx.author().id).await;
    reply_embed(ctx, message, "카지노", serenity::Colour::GOLD, true).await?;
    Ok(())
}

pub async fn casino_status_text(data: &Data, user_id: serenity::UserId) -> String {
    let hub = data.casino.clone();
    let now = crate::casino_hub::now_ms();
    let coins = hub.coins_of(user_id.get()).await;
    let seated = hub.seated_table_of(user_id.get()).await;
    let seated_text = match seated {
        Some(id) => {
            let mut stack = None;
            if let Some(table) = hub.table(&id) {
                let table = table.read().await;
                if let Some(seat) = table
                    .seat_index(user_id.get())
                    .and_then(|index| table.seats[index].as_ref())
                {
                    // 웹 화면처럼, 카드를 다 열기 전에는 이번 라운드 당첨금을 빼고 보여 준다.
                    stack = Some((table.name.clone(), seat.visible_stack(now)));
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
    format!(
        "보유 코인: **{}**\n{seated_text}\n하우스 누적: {}\n\n테이블\n{tables}",
        stats::coin_text(coins),
        // 아직 카드를 여는 중인 라운드의 하우스 손익은 결과가 공개된 뒤에 더해 보여 준다.
        stats::coin_text(hub.visible_house().await)
    )
}

/// 개인 링크 안내문 (슬래시 명령과 상태 임베드 버튼이 같이 쓴다).
async fn personal_entry_text(
    data: &Data,
    table_id: &str,
    table_name: &str,
    user_id: u64,
    display_name: &str,
) -> String {
    let hub = data.casino.clone();
    let token = hub.issue_session(user_id, display_name.to_string());
    let link = personal_link(&casino_base_url(data), &token, table_id);
    let coins = hub.coins_of(user_id).await;
    let buy_in = match hub.table(table_id) {
        Some(table) => {
            let table = table.read().await;
            format!(
                "{}~{}원",
                format_number(table.settings.min_buy_in),
                format_number(table.settings.max_buy_in)
            )
        }
        None => "5,000~20,000원".to_string(),
    };
    format!(
        "**{table_name}** 테이블 링크입니다.\n{link}\n\n⚠️ 이 링크는 **{display_name}** 님 전용이고 12시간 동안 유효합니다. 다른 사람과 공유하지 마세요.\n보유 코인: **{}** (바이인은 {buy_in}, 퇴장하면 남은 칩이 코인으로 돌아옵니다)",
        stats::coin_text(coins),
    )
}

/// 카지노 패널 본문. 진행 단계 대신 진행 중/대기 중만 보여 줘서 자주 바뀌지 않게 한다.
pub fn render_casino_panel(summaries: &[crate::casino_hub::TableSummary]) -> String {
    if summaries.is_empty() {
        return "버튼으로 테이블에 들어가거나 내 코인을 확인할 수 있습니다. 관리자는 테이블을 만들고, 설정을 바꾸고, 닫을 수 있습니다.\n\n열려 있는 테이블이 없습니다. 관리자는 아래 버튼으로 테이블을 만들 수 있습니다.".to_string();
    }
    let mut lines = vec![
        "버튼으로 테이블에 들어가거나 내 코인을 확인할 수 있습니다. 관리자는 테이블을 만들고, 설정을 바꾸고, 닫을 수 있습니다.".to_string(),
        format!("열린 테이블 ({}/{})", summaries.len(), MAX_TABLES),
    ];
    lines.extend(summaries.iter().map(|summary| {
        format!(
            "🎴 **{}** · {} · {} · {}/{}명 · {}{}",
            summary.name,
            summary.kind_text,
            summary.stakes,
            summary.seated,
            summary.seat_count,
            if summary.playing {
                "진행 중"
            } else {
                "대기 중"
            },
            summary
                .channel_id
                .map_or(String::new(), |id| format!(" · <#{}>", id))
        )
    }));
    lines.join("\n\n")
}

/// 패널 버튼. 공개 메시지라 custom_id에 유저 ID를 넣지 않는다. 관리 버튼은 누를 때마다 권한을 본다.
pub fn panel_components() -> Vec<serenity::CreateActionRow> {
    vec![
        serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new("casino_panel:enter")
                .label("테이블 입장")
                .emoji('🎰')
                .style(serenity::ButtonStyle::Success),
            serenity::CreateButton::new("casino_panel:me")
                .label("내 정보")
                .emoji('💰')
                .style(serenity::ButtonStyle::Secondary),
        ]),
        serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new("casino_panel:create_holdem")
                .label("홀덤 테이블 만들기")
                .style(serenity::ButtonStyle::Primary),
            serenity::CreateButton::new("casino_panel:create_blackjack")
                .label("블랙잭 테이블 만들기")
                .style(serenity::ButtonStyle::Primary),
            serenity::CreateButton::new("casino_panel:settings")
                .label("테이블 설정")
                .emoji('🔧')
                .style(serenity::ButtonStyle::Secondary),
            serenity::CreateButton::new("casino_panel:close")
                .label("테이블 닫기")
                .style(serenity::ButtonStyle::Danger),
        ]),
    ]
}

#[poise::command(
    slash_command,
    rename = "카지노패널",
    description_localized(
        "ko",
        "관리자: 이 채널에 카지노 메인 패널(버튼으로 입장·테이블 관리)을 올립니다."
    )
)]
pub async fn casino_panel(ctx: Context<'_>) -> Result<(), Error> {
    // 권한 확인·메시지 게시·고정·저장이 3초를 넘길 수 있으니 먼저 (본인에게만 보이게) 미룬다.
    if let Err(error) = ctx.defer_ephemeral().await {
        eprintln!("failed to defer 카지노패널: {error:?}");
    }
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let channel_id = ctx.channel_id();
    let hub = ctx.data().casino.clone();
    let summaries = hub.summaries().await;
    let text = render_casino_panel(&summaries);
    let message = channel_id
        .send_message(
            ctx.http(),
            serenity::CreateMessage::new()
                .embed(make_embed(text.clone(), "카지노", serenity::Colour::GOLD))
                .components(panel_components()),
        )
        .await?;
    let _ = message.pin(ctx.http()).await;
    let replaced = hub.set_panel_with_text(
        Some(PanelBinding {
            guild_id: guild_id.get(),
            channel_id: channel_id.get(),
            message_id: message.id.get(),
        }),
        Some(text),
    );
    // 바꾸기 전 연결의 패널을 지운다. 동시에 두 번 올려도 고정된 패널이 하나만 남는다.
    if let Some(old) = replaced
        && old.message_id != message.id.get()
    {
        let _ = serenity::ChannelId::new(old.channel_id)
            .delete_message(ctx.http(), serenity::MessageId::new(old.message_id))
            .await;
    }
    hub.save().await;
    // 올리는 동안 테이블이 바뀌었으면 바로 맞춘다 (같으면 아무 요청도 보내지 않는다).
    refresh_casino_panel(ctx.serenity_context(), ctx.data()).await;
    ctx.send(
        poise::CreateReply::default()
            .content("카지노 패널을 올렸습니다. 테이블이 바뀌면 자동으로 갱신됩니다.")
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

/// 패널 본문이 바뀌었으면 패널 메시지를 고친다.
pub async fn refresh_casino_panel(ctx: &serenity::Context, data: &Data) {
    let hub = data.casino.clone();
    let Some(binding) = hub.panel() else { return };
    let text = render_casino_panel(&hub.summaries().await);
    if hub.panel_text().as_deref() == Some(text.as_str()) {
        return;
    }
    let channel_id = serenity::ChannelId::new(binding.channel_id);
    let result = channel_id
        .edit_message(
            &ctx.http,
            serenity::MessageId::new(binding.message_id),
            serenity::EditMessage::new()
                .embed(make_embed(text.clone(), "카지노", serenity::Colour::GOLD))
                .components(panel_components()),
        )
        .await;
    match result {
        Ok(_) => hub.record_panel_text(binding.message_id, text),
        // 패널 메시지나 채널이 지워졌으면 연결을 끊는다 (다시 올리려면 /카지노패널).
        // 그 사이 새 패널이 올라왔으면 새 연결은 건드리지 않는다.
        Err(serenity::Error::Http(http_error))
            if http_error.status_code().map(|code| code.as_u16()) == Some(404) =>
        {
            if hub.clear_panel_if(binding.message_id) {
                hub.save().await;
            }
        }
        Err(error) => eprintln!("failed to refresh casino panel: {error:?}"),
    }
}

/// 모달의 금액 칸. 비우면 None(기본값), 쉼표·공백·'원'은 무시한다.
pub fn parse_amount(text: &str) -> Result<Option<i64>, String> {
    let original = text.trim();
    if original.is_empty() {
        return Ok(None);
    }
    let value = original
        .strip_suffix('원')
        .unwrap_or(original)
        .chars()
        .filter(|ch| !matches!(ch, ',' | '_' | ' '))
        .collect::<String>();
    if value.is_empty() || !value.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(format!("숫자로 입력하세요: {text}"));
    }
    match value.parse::<i64>() {
        Ok(amount) if amount <= 10_000_000_000 => Ok(Some(amount)),
        _ => Err(format!("숫자로 입력하세요: {text}")),
    }
}

/// "5000~20000" 꼴의 범위 칸. 숫자 하나만 쓰면 최소값만 정한다.
pub fn parse_range(text: &str) -> Result<(Option<i64>, Option<i64>), String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok((None, None));
    }
    let tilde = trimmed.find('~');
    let hyphen = trimmed.find('-').filter(|position| *position > 0);
    let separator = match (tilde, hyphen) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(position), None) | (None, Some(position)) => Some(position),
        (None, None) => None,
    };
    let Some(separator) = separator else {
        return Ok((parse_amount(trimmed)?, None));
    };
    if trimmed[separator + 1..].contains('~') || trimmed[separator + 1..].contains('-') {
        return Err("범위는 5000~20000처럼 입력하세요.".to_string());
    }
    let min = parse_amount(&trimmed[..separator])?;
    let max = parse_amount(&trimmed[separator + 1..])?;
    Ok((min, max))
}

fn modal_input(
    label: &str,
    custom_id: &str,
    placeholder: &str,
    max_length: u16,
    required: bool,
    value: Option<String>,
) -> serenity::CreateInputText {
    let mut input =
        serenity::CreateInputText::new(serenity::InputTextStyle::Short, label, custom_id)
            .placeholder(placeholder)
            .max_length(max_length)
            .required(required);
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        input = input.value(value);
    }
    input
}

pub fn create_table_modal(kind: GameKind) -> serenity::CreateModal {
    let mut inputs = vec![serenity::CreateActionRow::InputText(
        modal_input("이름", "name", "테이블 이름 (2~24자)", 24, true, None).min_length(2),
    )];
    match kind {
        GameKind::Holdem => {
            inputs.push(serenity::CreateActionRow::InputText(modal_input(
                "빅 블라인드",
                "big_blind",
                "비우면 100 · 10~50,000 짝수, 스몰은 절반",
                20,
                false,
                None,
            )));
            inputs.push(serenity::CreateActionRow::InputText(modal_input(
                "바이인 범위",
                "buy_in_range",
                "예: 5000~20000 · 비우면 빅 블라인드 ×50~×200",
                40,
                false,
                None,
            )));
        }
        GameKind::Blackjack => {
            inputs.push(serenity::CreateActionRow::InputText(modal_input(
                "베팅 범위",
                "bet_range",
                "예: 100~5000 · 비우면 100~5,000",
                40,
                false,
                None,
            )));
            inputs.push(serenity::CreateActionRow::InputText(modal_input(
                "사이드베팅 최대",
                "side_max",
                "0이면 사이드베팅 없음 · 비우면 최대 베팅의 절반",
                20,
                false,
                None,
            )));
            inputs.push(serenity::CreateActionRow::InputText(modal_input(
                "바이인 범위",
                "buy_in_range",
                "예: 5000~20000 · 비우면 최소 베팅×50~최대 베팅×4",
                40,
                false,
                None,
            )));
        }
    }
    inputs.push(serenity::CreateActionRow::InputText(modal_input(
        "제한 시간(초)",
        "turn_secs",
        "10~120 · 비우면 30",
        5,
        false,
        None,
    )));
    serenity::CreateModal::new(
        format!("casino_create:{}", kind.key()),
        match kind {
            GameKind::Holdem => "홀덤 테이블 만들기",
            GameKind::Blackjack => "블랙잭 테이블 만들기",
        },
    )
    .components(inputs)
}

pub fn table_settings_modal(
    table_id: &str,
    kind: GameKind,
    name: &str,
    settings: TableSettings,
) -> serenity::CreateModal {
    let title = format!("{name} 설정").chars().take(45).collect::<String>();
    let mut inputs = vec![serenity::CreateActionRow::InputText(
        modal_input(
            "이름",
            "name",
            "테이블 이름 (2~24자)",
            24,
            true,
            Some(name.to_string()),
        )
        .min_length(2),
    )];
    let buy_in = format!("{}~{}", settings.min_buy_in, settings.max_buy_in);
    match kind {
        GameKind::Holdem => {
            inputs.push(serenity::CreateActionRow::InputText(modal_input(
                "빅 블라인드",
                "big_blind",
                "비우면 기본값 · 짝수",
                20,
                false,
                Some(settings.big_blind.to_string()),
            )));
            inputs.push(serenity::CreateActionRow::InputText(modal_input(
                "바이인 범위",
                "buy_in_range",
                "예: 5000~20000",
                40,
                false,
                Some(buy_in),
            )));
        }
        GameKind::Blackjack => {
            inputs.push(serenity::CreateActionRow::InputText(modal_input(
                "베팅 범위",
                "bet_range",
                "예: 100~5000",
                40,
                false,
                Some(format!("{}~{}", settings.min_bet, settings.max_bet)),
            )));
            inputs.push(serenity::CreateActionRow::InputText(modal_input(
                "사이드베팅 최대",
                "side_max",
                "0이면 사이드베팅 없음",
                20,
                false,
                Some(settings.side_bet_max.to_string()),
            )));
            inputs.push(serenity::CreateActionRow::InputText(modal_input(
                "바이인 범위",
                "buy_in_range",
                "예: 5000~20000",
                40,
                false,
                Some(buy_in),
            )));
        }
    }
    inputs.push(serenity::CreateActionRow::InputText(modal_input(
        "제한 시간(초)",
        "turn_secs",
        "10~120 · 비우면 기본값",
        5,
        false,
        Some((settings.turn_ms / 1000).to_string()),
    )));
    serenity::CreateModal::new(format!("casino_settings:{table_id}"), title).components(inputs)
}

fn modal_text(modal: &serenity::ModalInteraction, id: &str) -> String {
    modal_value(modal, id).unwrap_or_default()
}

fn settings_from_modal(
    modal: &serenity::ModalInteraction,
    kind: GameKind,
) -> Result<TableSettings, String> {
    let (min_bet, max_bet, big_blind) = match kind {
        GameKind::Holdem => (None, None, parse_amount(&modal_text(modal, "big_blind"))?),
        GameKind::Blackjack => {
            let (min, max) = parse_range(&modal_text(modal, "bet_range"))?;
            (min, max, None)
        }
    };
    let (min_buy_in, max_buy_in) = parse_range(&modal_text(modal, "buy_in_range"))?;
    let side_bet_max = match kind {
        GameKind::Blackjack => parse_amount(&modal_text(modal, "side_max"))?,
        GameKind::Holdem => None,
    };
    let turn_secs = parse_amount(&modal_text(modal, "turn_secs"))?;
    TableSettings::build(
        kind,
        SettingsRequest {
            min_bet,
            max_bet,
            big_blind,
            min_buy_in,
            max_buy_in,
            side_bet_max,
            turn_secs,
        },
    )
}

/// 권한 확인(HTTP)이 실패했을 때: 인터랙션에 답하지 않으면 "상호작용 실패"로 보이므로
/// 거절 안내로 바꿔 답한다.
fn permission_check_failed(error: Error) -> Option<String> {
    eprintln!("casino panel permission check failed: {error:?}");
    Some("권한을 확인하지 못했습니다. 잠시 후 다시 시도하세요.".to_string())
}

/// 카지노 패널에서 누른 사람에게만 보이는 안내 (제목 "카지노").
async fn casino_component_private(
    ctx: &serenity::Context,
    component: &serenity::ComponentInteraction,
    message: impl Into<String>,
) -> serenity::Result<()> {
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::Message(
                serenity::CreateInteractionResponseMessage::new()
                    .ephemeral(true)
                    .embed(make_embed(message, "카지노", serenity::Colour::RED)),
            ),
        )
        .await
}

/// 카지노 모달 제출에 대한 본인 전용 안내 (제목 "카지노").
async fn casino_modal_private(
    ctx: &serenity::Context,
    modal: &serenity::ModalInteraction,
    message: impl Into<String>,
) -> serenity::Result<()> {
    modal
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::Message(
                serenity::CreateInteractionResponseMessage::new()
                    .ephemeral(true)
                    .embed(make_embed(message, "카지노", serenity::Colour::RED)),
            ),
        )
        .await
}

fn panel_select(
    action: &str,
    summaries: &[crate::casino_hub::TableSummary],
    seated: Option<&str>,
) -> serenity::CreateActionRow {
    let options = summaries
        .iter()
        .map(|summary| {
            let description = format!(
                "{} · {} · {}/{}명{}",
                summary.kind_text,
                summary.stakes,
                summary.seated,
                summary.seat_count,
                if seated == Some(summary.id.as_str()) {
                    " · 앉아 있음"
                } else {
                    ""
                }
            );
            serenity::CreateSelectMenuOption::new(
                summary.name.chars().take(100).collect::<String>(),
                summary.id.clone(),
            )
            .description(description.chars().take(100).collect::<String>())
        })
        .collect::<Vec<_>>();
    serenity::CreateActionRow::SelectMenu(
        serenity::CreateSelectMenu::new(
            format!("casino_pick:{action}"),
            serenity::CreateSelectMenuKind::String { options },
        )
        .placeholder("테이블을 선택하세요")
        .min_values(1)
        .max_values(1),
    )
}

pub async fn handle_casino_panel(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    action: &str,
) -> Result<()> {
    if matches!(
        action,
        "create_holdem" | "create_blackjack" | "settings" | "close"
    ) && let Some(message) = manager_denial(ctx, data, component.guild_id, component.user.id)
        .await
        .unwrap_or_else(permission_check_failed)
    {
        casino_component_private(ctx, component, message).await?;
        return Ok(());
    }
    match action {
        "enter" => {
            let summaries = data.casino.summaries().await;
            match summaries.as_slice() {
                [] => {
                    casino_component_private(ctx, component, "열려 있는 테이블이 없습니다.").await?
                }
                [summary] => {
                    let name = summary.name.clone();
                    let display_name = component
                        .member
                        .as_ref()
                        .map(|member| member.display_name().to_string())
                        .unwrap_or_else(|| component.user.name.clone());
                    component
                        .create_response(
                            ctx,
                            serenity::CreateInteractionResponse::Message(
                                serenity::CreateInteractionResponseMessage::new()
                                    .ephemeral(true)
                                    .embed(make_embed(
                                        personal_entry_text(
                                            data,
                                            &summary.id,
                                            &name,
                                            component.user.id.get(),
                                            &display_name,
                                        )
                                        .await,
                                        "카지노 입장",
                                        serenity::Colour::DARK_GREEN,
                                    )),
                            ),
                        )
                        .await?;
                }
                _ => {
                    let seated = data.casino.seated_table_of(component.user.id.get()).await;
                    // 앉아 있는 테이블을 맨 위에 둔다.
                    let mut summaries = summaries.clone();
                    summaries.sort_by_key(|summary| seated.as_deref() != Some(summary.id.as_str()));
                    component
                        .create_response(
                            ctx,
                            serenity::CreateInteractionResponse::Message(
                                serenity::CreateInteractionResponseMessage::new()
                                    .ephemeral(true)
                                    .embed(make_embed(
                                        "입장할 테이블을 선택하세요.",
                                        "카지노 입장",
                                        serenity::Colour::GOLD,
                                    ))
                                    .components(vec![panel_select(
                                        "enter",
                                        &summaries,
                                        seated.as_deref(),
                                    )]),
                            ),
                        )
                        .await?;
                }
            }
        }
        "me" => {
            component
                .create_response(
                    ctx,
                    serenity::CreateInteractionResponse::Message(
                        serenity::CreateInteractionResponseMessage::new()
                            .ephemeral(true)
                            .embed(make_embed(
                                casino_status_text(data, component.user.id).await,
                                "카지노",
                                serenity::Colour::GOLD,
                            )),
                    ),
                )
                .await?;
        }
        "create_holdem" | "create_blackjack" => {
            let kind = if action == "create_holdem" {
                GameKind::Holdem
            } else {
                GameKind::Blackjack
            };
            component
                .create_response(
                    ctx,
                    serenity::CreateInteractionResponse::Modal(create_table_modal(kind)),
                )
                .await?;
        }
        "settings" | "close" => {
            let summaries = data.casino.summaries().await;
            if summaries.is_empty() {
                casino_component_private(ctx, component, "열려 있는 테이블이 없습니다.").await?;
            } else {
                component
                    .create_response(
                        ctx,
                        serenity::CreateInteractionResponse::Message(
                            serenity::CreateInteractionResponseMessage::new()
                                .ephemeral(true)
                                .embed(make_embed(
                                    "테이블을 선택하세요.",
                                    "카지노",
                                    serenity::Colour::GOLD,
                                ))
                                .components(vec![panel_select(action, &summaries, None)]),
                        ),
                    )
                    .await?;
            }
        }
        _ => ack_component(ctx, component).await,
    }
    Ok(())
}

pub async fn handle_casino_pick(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    action: &str,
) -> Result<()> {
    let Some(table_id) = selected_values(component).into_iter().next() else {
        casino_component_private(ctx, component, "테이블을 선택하세요.").await?;
        return Ok(());
    };
    if matches!(action, "settings" | "close")
        && let Some(message) = manager_denial(ctx, data, component.guild_id, component.user.id)
            .await
            .unwrap_or_else(permission_check_failed)
    {
        casino_component_private(ctx, component, message).await?;
        return Ok(());
    }
    match action {
        "enter" => {
            let Some(table) = data.casino.table(&table_id) else {
                component
                    .create_response(
                        ctx,
                        serenity::CreateInteractionResponse::UpdateMessage(
                            serenity::CreateInteractionResponseMessage::new()
                                .embed(make_embed(
                                    "이미 닫힌 테이블입니다.",
                                    "카지노",
                                    serenity::Colour::RED,
                                ))
                                .components(Vec::new()),
                        ),
                    )
                    .await?;
                return Ok(());
            };
            let name = table.read().await.name.clone();
            let display_name = component
                .member
                .as_ref()
                .map(|member| member.display_name().to_string())
                .unwrap_or_else(|| component.user.name.clone());
            component
                .create_response(
                    ctx,
                    serenity::CreateInteractionResponse::UpdateMessage(
                        serenity::CreateInteractionResponseMessage::new()
                            .embed(make_embed(
                                personal_entry_text(
                                    data,
                                    &table_id,
                                    &name,
                                    component.user.id.get(),
                                    &display_name,
                                )
                                .await,
                                "카지노 입장",
                                serenity::Colour::DARK_GREEN,
                            ))
                            .components(Vec::new()),
                    ),
                )
                .await?;
        }
        "settings" => {
            let Some(table) = data.casino.table(&table_id) else {
                casino_component_private(ctx, component, "테이블을 찾을 수 없습니다.").await?;
                return Ok(());
            };
            // 모달 응답(HTTP)을 기다리는 동안 테이블 잠금을 쥐지 않는다.
            let modal = {
                let table = table.read().await;
                // 라운드가 끝나기를 기다리는 설정이 있으면 그 값에서 시작한다.
                let settings = table.pending_settings.unwrap_or(table.settings);
                table_settings_modal(&table_id, table.kind, &table.name, settings)
            };
            component
                .create_response(ctx, serenity::CreateInteractionResponse::Modal(modal))
                .await?;
        }
        "close" => {
            let Some(table) = data.casino.table(&table_id) else {
                casino_component_private(ctx, component, "이미 닫힌 테이블입니다.").await?;
                return Ok(());
            };
            let name = table.read().await.name.clone();
            component
                .create_response(
                    ctx,
                    serenity::CreateInteractionResponse::UpdateMessage(
                        serenity::CreateInteractionResponseMessage::new()
                            .embed(make_embed(
                                format!(
                                    "**{name}** 테이블을 닫을까요? 앉아 있는 사람의 칩은 코인으로 돌아가고 채널이 삭제됩니다."
                                ),
                                "카지노 테이블 닫기",
                                serenity::Colour::RED,
                            ))
                            .components(vec![serenity::CreateActionRow::Buttons(vec![
                                serenity::CreateButton::new(format!(
                                    "casino_close_confirm:{table_id}"
                                ))
                                .label("닫기")
                                .style(serenity::ButtonStyle::Danger),
                                serenity::CreateButton::new("casino_close_cancel")
                                    .label("취소")
                                    .style(serenity::ButtonStyle::Secondary),
                            ])]),
                    ),
                )
                .await?;
        }
        _ => ack_component(ctx, component).await,
    }
    Ok(())
}

async fn create_table_body(
    ctx: &serenity::Context,
    data: &Data,
    guild_id: serenity::GuildId,
    source_channel_id: serenity::ChannelId,
    kind: GameKind,
    name: &str,
    created_by: u64,
    created_by_name: &str,
    settings: TableSettings,
    panel_entry: bool,
) -> std::result::Result<String, String> {
    let hub = data.casino.clone();
    // 채널을 만드는 동안 이름을 맡아 둔다 (동시에 만든 이름·바꾼 이름과 겹치지 않게).
    // 개수 한도와 이름 길이도 여기서 먼저 확인해, 만들었다 지우는 채널이 생기지 않게 한다.
    let reservation = hub.reserve_table_name(name).await?;
    let category = source_category(ctx, source_channel_id).await;
    let channel_name = table_channel_name(name);
    let Some(channel) = create_text_channel_safe(
        ctx,
        guild_id,
        &channel_name,
        table_channel_overwrites(guild_id.get(), data.bot_user_id.get(), &BTreeSet::new()),
        category,
        "카지노 테이블 채널 생성",
        0,
        Some(format!(
            "{} 테이블 · 웹에서 플레이, 채팅은 이 채널과 연결됩니다.",
            kind.value()
        )),
    )
    .await
    else {
        return Err(
            "테이블 채널을 만들지 못했습니다. 봇의 채널 관리 권한을 확인하세요.".to_string(),
        );
    };
    let binding = TableBinding {
        guild_id: guild_id.get(),
        channel_id: channel.id.get(),
        channel_name: Some(channel.name.clone()),
        ..Default::default()
    };
    let table_id = match hub.create_table(kind, name, created_by, binding, settings) {
        Ok(id) => id,
        Err(message) => {
            drop(reservation);
            let _ = channel.delete(ctx).await;
            return Err(message);
        }
    };
    drop(reservation);
    hub.save().await;
    refresh_table_status(ctx, data, &table_id).await;
    refresh_casino_panel(ctx, data).await;
    let log_channel_id = data.config.read().await.log_channel_id;
    send_admin_log(
        ctx.http(),
        log_channel_id,
        "카지노 테이블",
        format!(
            "{created_by_name} 님이 {} 테이블 **{name}**(<#{}>)을 만들었습니다. {}",
            kind.value(),
            channel.id.get(),
            settings.summary(kind)
        ),
    )
    .await;
    Ok(format!(
        "{} 테이블 **{name}**을 만들었습니다. 채널: <#{}>\n방 설정: {}\n{}",
        kind.value(),
        channel.id.get(),
        settings.summary(kind),
        if panel_entry {
            "참가자는 카지노 패널의 **테이블 입장** 버튼으로 개인 링크를 받습니다.".to_string()
        } else {
            format!("참가자는 `/카지노입장 테이블:{name}` 으로 개인 링크를 받습니다.")
        }
    ))
}

async fn close_table_body(
    ctx: &serenity::Context,
    data: &Data,
    table_id: &str,
    actor_name: &str,
) -> Result<Option<(String, Vec<String>)>> {
    let hub = data.casino.clone();
    let Some(table) = hub.table(table_id) else {
        return Ok(None);
    };
    let name = table.read().await.name.clone();
    let binding = hub.binding(table_id);
    let events = hub.close_table(table_id).await;
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
    let log_channel_id = data.config.read().await.log_channel_id;
    send_admin_log(
        ctx.http(),
        log_channel_id,
        "카지노 테이블",
        format!(
            "{} 님이 테이블 **{name}**을 닫았습니다. 반환: {}",
            actor_name,
            if refunds.is_empty() {
                "없음".to_string()
            } else {
                refunds.join(", ")
            }
        ),
    )
    .await;
    refresh_casino_panel(ctx, data).await;
    Ok(Some((name, refunds)))
}

pub async fn handle_casino_close_confirm(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    table_id: &str,
) -> Result<()> {
    if let Some(message) = manager_denial(ctx, data, component.guild_id, component.user.id)
        .await
        .unwrap_or_else(permission_check_failed)
    {
        casino_component_private(ctx, component, message).await?;
        return Ok(());
    }
    component.defer(ctx).await?;
    let result = close_table_body(ctx, data, table_id, &component.user.name).await?;
    let message = match result {
        Some((name, refunds)) => format!(
            "테이블 **{name}**을 닫았습니다.{}",
            if refunds.is_empty() {
                String::new()
            } else {
                format!("\n칩 반환: {}", refunds.join(", "))
            }
        ),
        None => "이미 닫힌 테이블입니다.".to_string(),
    };
    component
        .edit_response(
            ctx,
            serenity::EditInteractionResponse::new()
                .embed(make_embed(
                    message,
                    "카지노 테이블",
                    serenity::Colour::DARK_GREEN,
                ))
                .components(Vec::new()),
        )
        .await?;
    Ok(())
}

pub async fn handle_casino_close_cancel(
    ctx: &serenity::Context,
    component: &serenity::ComponentInteraction,
) -> Result<()> {
    component
        .create_response(
            ctx,
            serenity::CreateInteractionResponse::UpdateMessage(
                serenity::CreateInteractionResponseMessage::new()
                    .embed(make_embed(
                        "취소했습니다.",
                        "카지노",
                        serenity::Colour::DARK_GREY,
                    ))
                    .components(Vec::new()),
            ),
        )
        .await?;
    Ok(())
}

pub async fn handle_casino_create_submit(
    ctx: &serenity::Context,
    data: &Data,
    modal: &serenity::ModalInteraction,
    kind_key: &str,
) -> Result<()> {
    if let Some(message) = manager_denial(ctx, data, modal.guild_id, modal.user.id)
        .await
        .unwrap_or_else(permission_check_failed)
    {
        casino_modal_private(ctx, modal, message).await?;
        return Ok(());
    }
    let kind = match kind_key {
        "holdem" => GameKind::Holdem,
        "blackjack" => GameKind::Blackjack,
        _ => {
            casino_modal_private(ctx, modal, "잘못된 테이블 종류입니다.").await?;
            return Ok(());
        }
    };
    let name = modal_text(modal, "name");
    let settings = match settings_from_modal(modal, kind) {
        Ok(settings) => settings,
        Err(message) => {
            casino_modal_private(ctx, modal, message).await?;
            return Ok(());
        }
    };
    let Some(guild_id) = modal.guild_id else {
        casino_modal_private(ctx, modal, "서버 안에서만 사용할 수 있습니다.").await?;
        return Ok(());
    };
    modal.defer_ephemeral(ctx).await?;
    let response = create_table_body(
        ctx,
        data,
        guild_id,
        modal.channel_id,
        kind,
        &name,
        modal.user.id.get(),
        &modal.user.name,
        settings,
        true,
    )
    .await;
    let (message, colour) = match response {
        Ok(message) => (message, serenity::Colour::DARK_GREEN),
        Err(message) => (message, serenity::Colour::RED),
    };
    modal
        .edit_response(
            ctx,
            serenity::EditInteractionResponse::new().embed(make_embed(message, "카지노", colour)),
        )
        .await?;
    Ok(())
}

pub async fn handle_casino_settings_submit(
    ctx: &serenity::Context,
    data: &Data,
    modal: &serenity::ModalInteraction,
    table_id: &str,
) -> Result<()> {
    if let Some(message) = manager_denial(ctx, data, modal.guild_id, modal.user.id)
        .await
        .unwrap_or_else(permission_check_failed)
    {
        casino_modal_private(ctx, modal, message).await?;
        return Ok(());
    }
    let Some(table) = data.casino.table(table_id) else {
        casino_modal_private(ctx, modal, "테이블을 찾을 수 없습니다.").await?;
        return Ok(());
    };
    let (kind, old_name, effective) = {
        let table = table.read().await;
        // 라운드 끝을 기다리는 변경이 있으면 그것이 지금 정해진 설정이다.
        (
            table.kind,
            table.name.clone(),
            table.pending_settings.unwrap_or(table.settings),
        )
    };
    let name = modal_text(modal, "name");
    let settings = match settings_from_modal(modal, kind) {
        Ok(settings) => settings,
        Err(message) => {
            casino_modal_private(ctx, modal, message).await?;
            return Ok(());
        }
    };
    modal.defer_ephemeral(ctx).await?;
    let name = name.trim().to_string();
    let renamed = if name == old_name {
        Ok(false)
    } else {
        data.casino.rename_table(table_id, &name).await
    };
    let renamed = match renamed {
        Ok(changed) => changed,
        Err(message) => {
            modal
                .edit_response(
                    ctx,
                    serenity::EditInteractionResponse::new().embed(make_embed(
                        message,
                        "카지노",
                        serenity::Colour::RED,
                    )),
                )
                .await?;
            return Ok(());
        }
    };
    let mut notes = Vec::new();
    if renamed {
        notes.push(format!("이름을 **{name}**(으)로 바꿨습니다."));
    }
    // 채널 이름을 테이블 이름에 맞춘다. 봇이 마지막으로 정한 채널 이름(예전 저장본은 옛 테이블
    // 이름에서 만든 이름)과 다를 때만 보내므로, 전에 제한에 걸려 못 바꾼 이름도 이번에 맞추고,
    // 대소문자·공백만 달라 채널 이름이 그대로면 보내지 않는다.
    let desired_channel = table_channel_name(&name);
    let binding = data.casino.binding(table_id).filter(|binding| {
        binding
            .channel_name
            .clone()
            .unwrap_or_else(|| table_channel_name(&old_name))
            != desired_channel
    });
    if let Some(binding) = binding {
        // Discord는 채널 이름을 10분에 두 번까지만 바꾸게 한다. 넘으면 요청이 몇 분씩 멈추고
        // 그동안 이 채널의 다른 요청(삭제 등)도 막히므로, 넘을 것 같으면 보내지 않는다.
        if data.casino.take_channel_rename(binding.channel_id) {
            match serenity::ChannelId::new(binding.channel_id)
                .edit(ctx, serenity::EditChannel::new().name(&desired_channel))
                .await
            {
                Ok(channel) => {
                    data.casino.update_binding(table_id, |binding| {
                        binding.channel_name = Some(channel.name.clone());
                    });
                    if !renamed {
                        notes.push("채널 이름을 테이블 이름에 맞췄습니다.".to_string());
                    }
                }
                Err(error) => {
                    eprintln!("failed to rename casino table channel: {error:?}");
                    notes.push(
                        "채널 이름은 바꾸지 못했습니다. 나중에 설정 창을 다시 제출하면 다시 시도합니다."
                            .to_string(),
                    );
                }
            }
        } else {
            notes.push(
                "채널 이름은 Discord 제한(10분에 두 번)으로 이번에는 바꾸지 않았습니다. 10분 뒤 설정 창을 다시 제출하면 맞춥니다."
                    .to_string(),
            );
        }
    }
    let settings_changed = settings != effective;
    let applied = if settings_changed {
        match data.casino.update_settings(table_id, settings).await {
            Ok(applied) => Some(applied),
            Err(message) => {
                modal
                    .edit_response(
                        ctx,
                        serenity::EditInteractionResponse::new().embed(make_embed(
                            message,
                            "카지노",
                            serenity::Colour::RED,
                        )),
                    )
                    .await?;
                return Ok(());
            }
        }
    } else {
        None
    };
    if notes.is_empty() && !settings_changed {
        modal
            .edit_response(
                ctx,
                serenity::EditInteractionResponse::new().embed(make_embed(
                    "바뀐 내용이 없습니다.",
                    "카지노",
                    serenity::Colour::DARK_GREY,
                )),
            )
            .await?;
        return Ok(());
    }
    data.casino.save().await;
    refresh_casino_panel(ctx, data).await;
    let log_channel_id = data.config.read().await.log_channel_id;
    // 채널 이름만 맞춘 경우는 기록하지 않는다.
    if renamed || settings_changed {
        let rename_log = if renamed {
            format!(" 이름: {old_name} → {name}")
        } else {
            String::new()
        };
        let settings_log = if settings_changed {
            format!(" 설정: {}", settings.summary(kind))
        } else {
            String::new()
        };
        send_admin_log(
            ctx.http(),
            log_channel_id,
            "카지노 테이블",
            format!(
                "{} 님이 테이블 **{old_name}**을 바꿨습니다.{rename_log}{settings_log}",
                modal.user.name,
            ),
        )
        .await;
    }
    match applied {
        Some(true) => notes.push(format!(
            "방 설정을 적용했습니다: {}",
            settings.summary(kind)
        )),
        Some(false) => notes.push(format!(
            "라운드가 진행 중이라 이번 라운드가 끝나면 적용됩니다: {}",
            settings.summary(kind)
        )),
        None => {}
    }
    modal
        .edit_response(
            ctx,
            serenity::EditInteractionResponse::new().embed(make_embed(
                notes.join("\n"),
                "카지노",
                serenity::Colour::DARK_GREEN,
            )),
        )
        .await?;
    Ok(())
}

/// 상태 임베드의 버튼 custom_id.
fn casino_enter_custom_id(table_id: &str) -> String {
    format!("casino_enter:{table_id}")
}

/// 상태 임베드 아래 버튼: 테이블 입장 (개인 링크 발급).
fn status_components(table_id: &str) -> Vec<serenity::CreateActionRow> {
    vec![serenity::CreateActionRow::Buttons(vec![
        serenity::CreateButton::new(casino_enter_custom_id(table_id))
            .label("테이블 입장")
            .emoji('🎰')
            .style(serenity::ButtonStyle::Success),
    ])]
}

/// "테이블 입장" 버튼: 누른 사람에게만 개인 링크를 보여준다.
pub async fn handle_casino_enter(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    table_id: &str,
) -> Result<()> {
    let user = &component.user;
    let display_name = component
        .member
        .as_ref()
        .map(|member| member.display_name().to_string())
        .unwrap_or_else(|| user.name.clone());
    let table_name = match data.casino.table(table_id) {
        Some(table) => Some(table.read().await.name.clone()),
        None => None,
    };
    let (title, message, colour) = match table_name {
        Some(name) => (
            "카지노 입장",
            personal_entry_text(data, table_id, &name, user.id.get(), &display_name).await,
            serenity::Colour::DARK_GREEN,
        ),
        None => (
            "카지노",
            "이 테이블은 닫혔습니다. `/카지노테이블목록`으로 열려 있는 테이블을 확인하세요."
                .to_string(),
            serenity::Colour::RED,
        ),
    };
    component
        .create_response(
            &ctx.http,
            serenity::CreateInteractionResponse::Message(
                serenity::CreateInteractionResponseMessage::new()
                    .ephemeral(true)
                    .embed(make_embed(message, title, colour)),
            ),
        )
        .await?;
    Ok(())
}

// ------------------------------------------------------------ Discord 중계

/// 이 웹훅으로는 다시 보내도 실패하는 오류인지 (토큰 없음, 지워진 웹훅, 권한 없음).
fn webhook_unusable(error: &serenity::Error) -> bool {
    match error {
        serenity::Error::Model(serenity::ModelError::NoTokenSet) => true,
        serenity::Error::Http(serenity::HttpError::UnsuccessfulRequest(response)) => {
            matches!(response.status_code.as_u16(), 401 | 403 | 404)
        }
        _ => false,
    }
}

/// 테이블 채널의 웹훅 (없으면 만든다).
async fn casino_webhook(
    ctx: &serenity::Context,
    hub: &CasinoHub,
    channel_id: serenity::ChannelId,
) -> Option<serenity::Webhook> {
    if let Some(webhook) = hub.webhooks.get(&channel_id.get()) {
        return Some(webhook.clone());
    }
    // 토큰이 있는 웹훅만 쓸 수 있다. Discord는 다른 앱(예전에 보조 봇 토큰으로 만든 웹훅)의 토큰을 목록에서
    // 빼고 주므로, 그런 웹훅으로 보내면 NoTokenSet으로 매번 실패한다. 그런 옛 웹훅은 지우고 새로 만든다.
    let mut existing = None;
    if let Ok(webhooks) = channel_id.webhooks(&ctx.http).await {
        for webhook in webhooks {
            if webhook.name.as_deref() != Some("Mafia Casino") {
                continue;
            }
            if webhook.token.is_some() && existing.is_none() {
                existing = Some(webhook);
            } else if webhook.token.is_none() {
                let _ = ctx
                    .http
                    .delete_webhook(webhook.id, Some("토큰을 읽을 수 없는 카지노 웹훅 교체"))
                    .await;
            }
        }
    }
    // 웹훅의 기본 아바타는 딜러(소피아) 얼굴. 참가자 메시지는 보낼 때 avatar_url로 바꾼다.
    let avatar = crate::casino_web::dealer_avatar_png()
        .map(|png| serenity::CreateAttachment::bytes(png.to_vec(), "sophia.png"));
    let mut webhook = match existing {
        Some(webhook) => webhook,
        None => {
            // 메인 봇으로만 만든다: 보조 봇 소유 웹훅은 재시작 뒤 목록에서 토큰이 빠진다.
            let mut builder = serenity::CreateWebhook::new("Mafia Casino")
                .audit_log_reason("카지노 테이블 채팅 웹훅 생성");
            if let Some(avatar) = avatar.as_ref() {
                builder = builder.avatar(avatar);
            }
            match channel_id.create_webhook(&ctx.http, builder).await {
                Ok(webhook) => webhook,
                Err(error) => {
                    eprintln!(
                        "failed to create casino webhook for channel {}: {error:?}",
                        channel_id.get()
                    );
                    return None;
                }
            }
        }
    };
    // 예전에 아바타 없이 만들어진 웹훅이면 딜러 얼굴을 붙인다.
    if webhook.avatar.is_none() {
        if let Some(avatar) = avatar.as_ref() {
            if let Err(error) = webhook
                .edit(&ctx.http, serenity::EditWebhook::new().avatar(avatar))
                .await
            {
                eprintln!(
                    "failed to set casino webhook avatar for channel {}: {error:?}",
                    channel_id.get()
                );
            }
        }
    }
    hub.webhooks.insert(channel_id.get(), webhook.clone());
    Some(webhook)
}

/// 참가자의 Discord 아바타 URL (서버 프로필 아바타 우선). 캐시한다.
async fn player_avatar_url(
    ctx: &serenity::Context,
    hub: &CasinoHub,
    guild_id: u64,
    user_id: u64,
) -> Option<String> {
    if let Some(url) = hub.avatars.get(&user_id) {
        return Some(url.clone());
    }
    let user = serenity::UserId::new(user_id);
    let url = match serenity::GuildId::new(guild_id)
        .member(&ctx.http, user)
        .await
    {
        Ok(member) => Some(member.face()),
        Err(_) => user.to_user(&ctx.http).await.ok().map(|user| user.face()),
    }?;
    hub.avatars.insert(user_id, url.clone());
    Some(url)
}

/// 아직 채널로 보내지 않은 테이블 채팅을 웹훅으로 보낸다.
async fn relay_chat_to_channel(
    ctx: &serenity::Context,
    hub: &CasinoHub,
    table_id: &str,
    guild_id: u64,
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
            format!("{} (딜러)", message.name)
        } else {
            message.name.clone()
        };
        let mut execute = serenity::ExecuteWebhook::new()
            .content(message.text.chars().take(1900).collect::<String>())
            .username(username);
        if !message.dealer {
            if let Some(url) = match message.user_id {
                Some(user_id) => player_avatar_url(ctx, hub, guild_id, user_id).await,
                None => None,
            } {
                execute = execute.avatar_url(url);
            }
        }
        let execute = execute.allowed_mentions(
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
            // 웹훅이 지워졌거나 토큰이 없으면 다음 중계 때 다시 찾거나 만든다 (같은 오류를 계속 내지 않게).
            if webhook_unusable(&error) {
                hub.webhooks.remove(&channel_id.get());
            }
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

/// 상태 임베드가 웹 화면보다 먼저 결과를 보여 주지 않게, 아직 착지하거나 뒤집히지 않은
/// 카드(딜러 홀 카드·드로, 보드, 쇼다운 카드, 블랙잭 히트)를 가린다. 화면 상태는 정산되면
/// 뒤집을 시각과 함께 카드를 미리 보내므로, 그 시각 전에는 여기서 "??"로 되돌린다.
/// 연출이 끝나면 결과 안내가 공개되며 임베드를 다시 그린다.
///
/// 가린 카드 중 가장 먼저 드러나는 시각을 돌려준다 (가린 카드가 없으면 None). 중계는 그 시각이
/// 지난 뒤 공개 시각 알림(`RevealTick`)이 오면 임베드를 다시 그려 "??"를 걷어 낸다.
fn hide_unrevealed_cards(view: &mut TableView, now: i64) -> Option<i64> {
    const HIDDEN: &str = "??";
    let mut next_reveal: Option<i64> = None;
    // 화면 상태가 이미 가려 보낸 카드("??")는 그 시각이 지나도 그대로라 세지 않는다.
    let mut hide = |card: &mut String, shown_at: i64| {
        if shown_at > now && card != HIDDEN {
            *card = HIDDEN.to_string();
            next_reveal = Some(next_reveal.map_or(shown_at, |at| at.min(shown_at)));
        }
    };
    if let Some(round) = view.round.as_mut() {
        for (index, card) in round.dealer.iter_mut().enumerate() {
            let landed_at = round.dealer_reveal_at.get(index).copied().unwrap_or(0);
            let flipped_at = if index == 1 { round.dealer_flip_at } else { 0 };
            hide(card, landed_at.max(flipped_at));
        }
        for (index, card) in round.board.iter_mut().enumerate() {
            let landed_at = round.board_reveal_at.get(index).copied().unwrap_or(0);
            let flipped_at = if index < 3 { round.board_flip_at } else { 0 };
            hide(card, landed_at.max(flipped_at));
        }
    }
    for seat in view.seats.iter_mut().flatten() {
        for card in &mut seat.cards {
            hide(card, seat.showdown_at);
        }
        for hand in &mut seat.hands {
            for (card, at) in hand.cards.iter_mut().zip(&hand.reveal_at) {
                hide(card, *at);
            }
        }
    }
    next_reveal
}

/// 상태 임베드를 갱신하고, 새 핸드 결과가 있으면 알린다. 임베드에서 가린 카드 중 가장 먼저
/// 드러나는 시각을 돌려준다 (없으면 None).
pub async fn refresh_table_status(
    ctx: &serenity::Context,
    data: &Data,
    table_id: &str,
) -> Option<i64> {
    let hub = data.casino.clone();
    let binding = hub.binding(table_id)?;
    let (mut view, now) = hub.view_now(table_id, None).await?;
    let next_reveal = hide_unrevealed_cards(&mut view, now);
    let channel_id = serenity::ChannelId::new(binding.channel_id);
    relay_chat_to_channel(ctx, &hub, table_id, binding.guild_id, channel_id).await;
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
    let components = status_components(table_id);
    let existing = hub
        .binding(table_id)
        .and_then(|binding| binding.status_message_id);
    if let Some(message_id) = existing {
        let edited = channel_id
            .edit_message(
                &ctx.http,
                serenity::MessageId::new(message_id),
                serenity::EditMessage::new()
                    .embed(embed.clone())
                    .components(components.clone()),
            )
            .await;
        if edited.is_ok() {
            return next_reveal;
        }
    }
    match channel_id
        .send_message(
            &ctx.http,
            serenity::CreateMessage::new()
                .embed(embed)
                .components(components),
        )
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
    next_reveal
}

/// 테이블 변경 알림을 받아 채널을 갱신하는 작업. 임베드 갱신은 테이블당 1.5초로 묶는다.
async fn sync_table_channel_permissions(
    ctx: &serenity::Context,
    data: &Data,
    table_id: &str,
    last_seated: &mut HashMap<u64, BTreeSet<u64>>,
) {
    let Some((guild_id, channel_id, seated)) = data.casino.table_channel_targets(table_id).await
    else {
        return;
    };
    if last_seated.get(&channel_id) == Some(&seated) {
        return;
    }
    let desired = table_channel_overwrites(guild_id, data.bot_user_id.get(), &seated);
    let Ok(channel) = serenity::ChannelId::new(channel_id)
        .to_channel(&ctx.http)
        .await
    else {
        eprintln!("failed to fetch casino channel permissions: channel_id={channel_id}");
        return;
    };
    let Some(channel) = channel.guild() else {
        eprintln!("casino table channel is not a guild channel: channel_id={channel_id}");
        return;
    };
    let mut success = true;
    for overwrite in &desired {
        if channel
            .permission_overwrites
            .iter()
            .find(|current| current.kind == overwrite.kind)
            != Some(overwrite)
        {
            if let Err(error) = serenity::ChannelId::new(channel_id)
                .create_permission(&ctx.http, overwrite.clone())
                .await
            {
                eprintln!(
                    "failed to apply casino channel permission: channel_id={channel_id} kind={:?} error={error:?}",
                    overwrite.kind
                );
                success = false;
            }
        }
    }
    for current in &channel.permission_overwrites {
        if let serenity::PermissionOverwriteType::Member(user_id) = current.kind
            && !desired.iter().any(|item| item.kind == current.kind)
        {
            if let Err(error) = serenity::ChannelId::new(channel_id)
                .delete_permission(&ctx.http, current.kind)
                .await
            {
                eprintln!(
                    "failed to remove casino channel permission: channel_id={channel_id} user_id={} error={error:?}",
                    user_id.get()
                );
                success = false;
            }
        }
    }
    if success {
        last_seated.insert(channel_id, seated);
    }
}

pub async fn run_casino_relay(ctx: serenity::Context, data: Data) {
    let hub = data.casino.clone();
    let mut updates = hub.updates.subscribe();
    let mut last_refresh: HashMap<String, Instant> = HashMap::new();
    // 패널은 3초에 한 번까지만 고친다. 그 사이에 바뀐 것은 다음 틱에 반영한다.
    let mut last_panel_refresh: Option<Instant> = None;
    let mut panel_dirty = false;
    let mut pending: HashSet<String> = HashSet::new();
    // 테이블별로 마지막 임베드가 가린 카드 중 가장 먼저 드러나는 시각.
    let mut masked_until: HashMap<String, i64> = HashMap::new();
    let mut last_channel_seated: HashMap<u64, BTreeSet<u64>> = HashMap::new();
    // DashMap 가드를 await 너머로 쥐지 않도록 테이블 id만 먼저 모은다.
    let table_ids: Vec<String> = hub
        .bindings
        .iter()
        .map(|entry| entry.key().clone())
        .collect();
    for table_id in &table_ids {
        sync_table_channel_permissions(&ctx, &data, table_id, &mut last_channel_seated).await;
    }
    let mut flush = tokio::time::interval(Duration::from_millis(1_500));
    loop {
        tokio::select! {
            update = updates.recv() => {
                match update {
                    // 공개 시각 알림은 화면용이다. 새로 공개된 채팅·결과가 있거나, 지난 임베드에서
                    // "??"로 가린 카드(히트·딜러 드로·보드 등)가 그 사이 드러났을 때만 채널을 고친다
                    // (같은 내용으로 상태 임베드를 다시 고치지 않게).
                    Ok(update) => {
                        sync_table_channel_permissions(
                            &ctx,
                            &data,
                            &update.table_id,
                            &mut last_channel_seated,
                        )
                        .await;
                        let unmasked = masked_until
                            .get(&update.table_id)
                            .is_some_and(|at| *at <= crate::casino_hub::now_ms());
                        if update.kind == UpdateKind::Changed || unmasked || hub.has_relay_work(&update.table_id).await {
                            pending.insert(update.table_id);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        for entry in hub.tables.iter() { pending.insert(entry.key().clone()); }
                    }
                    Err(_) => break,
                }
            }
            _ = flush.tick() => {
                let due = pending.iter().filter(|id| last_refresh.get(*id).is_none_or(|at| at.elapsed() >= Duration::from_millis(1_400))).cloned().collect::<Vec<_>>();
                for table_id in due {
                    pending.remove(&table_id);
                    panel_dirty = true;
                    if hub.binding(&table_id).is_none() { continue; }
                    match refresh_table_status(&ctx, &data, &table_id).await {
                        Some(at) => masked_until.insert(table_id.clone(), at),
                        None => masked_until.remove(&table_id),
                    };
                    last_refresh.insert(table_id, Instant::now());
                }
                if panel_dirty && last_panel_refresh.is_none_or(|at| at.elapsed() >= Duration::from_secs(3)) {
                    panel_dirty = false;
                    refresh_casino_panel(&ctx, &data).await;
                    last_panel_refresh = Some(Instant::now());
                }
            }
        }
    }
}

/// 시간 초과·유휴 정리 루프.
pub async fn run_casino_ticker(data: Data) {
    let hub = data.casino.clone();
    // 카드 연출(수백 ms 단위)을 매끄럽게 밀어 주기 위해 250ms마다 돈다.
    let mut interval = tokio::time::interval(Duration::from_millis(250));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::casino_hub::TableSummary;

    #[test]
    fn a_webhook_without_token_is_replaced_not_retried() {
        // 보조 봇이 만든 웹훅은 목록에서 토큰이 빠져 NoTokenSet이 난다: 캐시에서 빼고 새로 만들어야 한다.
        assert!(webhook_unusable(&serenity::Error::Model(
            serenity::ModelError::NoTokenSet
        )));
        // 일시적인 오류(예: 요청 형식)는 같은 웹훅을 계속 쓴다.
        assert!(!webhook_unusable(&serenity::Error::Other("timeout")));
    }

    #[test]
    fn status_embed_hides_cards_until_they_are_revealed() {
        use mafia_remake::casino::{CasinoCommand, CasinoTable, table_view};
        let mut table = CasinoTable::new("t", GameKind::Holdem, "상태", 1, 0);
        for (user, seat) in [(1, 0), (2, 1)] {
            table
                .apply_command(
                    user,
                    "손님",
                    &CasinoCommand::Join {
                        seat,
                        amount: 10_000,
                        name: format!("손님{user}"),
                    },
                    None,
                    0,
                )
                .unwrap();
        }
        let mut deck = [
            "Kh", "Qd", "Kc", "Qs", "2c", "7h", "8d", "9s", "3c", "Jd", "4c", "5h",
        ]
        .iter()
        .map(|card| card.to_string())
        .collect::<Vec<_>>();
        deck.reverse();
        table.start_with_deck(1, deck, 1_000).unwrap();
        // 헤즈업: 버튼(스몰)이 콜, 빅이 체크하면 플롭이 나간다.
        for (user, command, at) in [
            (1, CasinoCommand::Call, 100_000),
            (2, CasinoCommand::Check, 100_100),
        ] {
            table
                .apply_command(user, "손님", &command, None, at)
                .unwrap();
        }
        let flip_at = table.round.as_ref().unwrap().board_flip_at;
        assert!(flip_at > 100_100);
        let mut early = table_view(&table, None, 100_100);
        hide_unrevealed_cards(&mut early, 100_100);
        assert_eq!(early.round.as_ref().unwrap().board, vec!["??"; 3]);
        let mut late = table_view(&table, None, flip_at);
        hide_unrevealed_cards(&mut late, flip_at);
        assert_eq!(late.round.as_ref().unwrap().board, vec!["7h", "8d", "9s"]);
        // 끝까지 체크해 쇼다운으로 간다. 쇼다운 카드도 뒤집는 시각 전에는 가린다.
        for street in 1..=3 {
            let at = 100_000 + street * 100_000;
            for (user, offset) in [(2, 0), (1, 100)] {
                table
                    .apply_command(user, "손님", &CasinoCommand::Check, None, at + offset)
                    .unwrap();
            }
        }
        let settled_at = 400_100;
        let mut settled = table_view(&table, None, settled_at);
        let flips = settled
            .seats
            .iter()
            .flatten()
            .map(|seat| seat.showdown_at)
            .collect::<Vec<_>>();
        assert!(flips.iter().all(|at| *at > settled_at), "{flips:?}");
        hide_unrevealed_cards(&mut settled, settled_at);
        assert!(
            settled
                .seats
                .iter()
                .flatten()
                .all(|seat| seat.cards.iter().all(|card| card == "??"))
        );
        let last_flip = flips.iter().copied().max().unwrap();
        let mut shown = table_view(&table, None, last_flip);
        hide_unrevealed_cards(&mut shown, last_flip);
        assert_eq!(shown.seats[1].as_ref().unwrap().cards, vec!["Kh", "Kc"]);
    }

    fn blackjack_with_deck(cards: &[&str], bet: i64) -> mafia_remake::casino::CasinoTable {
        use mafia_remake::casino::{CasinoCommand, CasinoTable};
        let mut table = CasinoTable::new("b", GameKind::Blackjack, "블랙잭", 1, 0);
        table
            .apply_command(
                1,
                "손님",
                &CasinoCommand::Join {
                    seat: 0,
                    amount: 10_000,
                    name: "손님1".to_string(),
                },
                None,
                0,
            )
            .unwrap();
        let mut deck = cards
            .iter()
            .map(|card| card.to_string())
            .collect::<Vec<_>>();
        deck.reverse();
        table.start_with_deck(1, deck, 1_000).unwrap();
        table
            .apply_command(
                1,
                "손님",
                &CasinoCommand::Bet {
                    amount: bet,
                    pairs: 0,
                    plus3: 0,
                },
                None,
                1_100,
            )
            .unwrap();
        table
    }

    /// 봇 바이너리의 테스트는 엔진을 실제 연출 시간으로 돌린다. 히트한 카드가 착지하기 전에 그린
    /// 임베드는 그 카드를 가리고, 착지 시각을 알려 줘서 중계가 그 뒤 공개 알림에 다시 그리게 한다.
    #[test]
    fn status_embed_reports_when_a_masked_card_lands() {
        use mafia_remake::casino::{CasinoCommand, table_view};
        // 딜: 나 2h, 딜러 9c, 나 3h, 딜러 7d. 히트 4h.
        let mut table = blackjack_with_deck(&["2h", "9c", "3h", "7d", "4h", "Td"], 100);
        let dealt = table.round.as_ref().unwrap().reveal_until;
        table
            .apply_command(1, "손님", &CasinoCommand::Hit, None, dealt)
            .unwrap();
        let landing = table.seat(0).unwrap().hands[0].reveal_at[2];
        assert!(landing > dealt);
        let mut early = table_view(&table, None, dealt);
        assert_eq!(hide_unrevealed_cards(&mut early, dealt), Some(landing));
        assert_eq!(early.seats[0].as_ref().unwrap().hands[0].cards[2], "??");
        // 착지한 뒤에 다시 그리면 가릴 것이 없다 (딜러 홀 카드는 화면 상태가 이미 가려 보낸다).
        let mut late = table_view(&table, None, landing);
        assert_eq!(hide_unrevealed_cards(&mut late, landing), None);
        assert_eq!(late.seats[0].as_ref().unwrap().hands[0].cards[2], "4h");
    }

    #[test]
    fn status_line_keeps_the_play_phase_until_the_dealer_flips() {
        use mafia_remake::casino::table_view;
        // 딜러 내추럴 (업카드 T): 딜하자마자 정산되지만 카드는 아직 날아가는 중이다.
        let table = blackjack_with_deck(&["9h", "Td", "7c", "As"], 500);
        let round = table.round.as_ref().unwrap();
        assert_eq!(round.phase, Phase::Complete);
        let flip_at = round.dealer_flip_at;
        assert!(flip_at > 1_100 && flip_at < round.reveal_until);
        let status = render_table_status(&table_view(&table, None, 1_100), "https://casino");
        assert!(status.contains("단계: **플레이 중**"), "{status}");
        let status = render_table_status(&table_view(&table, None, flip_at), "https://casino");
        assert!(status.contains("단계: **라운드 종료**"), "{status}");
    }

    fn input_count(modal: serenity::CreateModal) -> (usize, String) {
        let json = serde_json::to_value(modal).unwrap();
        let count = json["components"].as_array().unwrap().len();
        (count, json.to_string())
    }

    #[test]
    fn parse_ranges_accept_common_formats() {
        assert_eq!(
            parse_range("5000~20000").unwrap(),
            (Some(5000), Some(20000))
        );
        assert_eq!(
            parse_range("5,000 ~ 20,000").unwrap(),
            (Some(5000), Some(20000))
        );
        assert_eq!(
            parse_range("5000-20000").unwrap(),
            (Some(5000), Some(20000))
        );
        assert_eq!(parse_range("5000").unwrap(), (Some(5000), None));
        assert_eq!(parse_range("").unwrap(), (None, None));
    }

    #[test]
    fn parse_amount_rejects_invalid_or_huge_values() {
        assert_eq!(parse_amount("5000원").unwrap(), Some(5000));
        assert!(parse_amount("abc").is_err());
        assert!(parse_range("1~2~3").is_err());
        assert!(parse_range("-5").is_err());
        assert!(parse_amount("10000000001").is_err());
    }

    #[test]
    fn holdem_create_modal_has_four_nonempty_inputs() {
        let (count, json) = input_count(create_table_modal(GameKind::Holdem));
        assert_eq!(count, 4);
        for id in ["name", "big_blind", "buy_in_range", "turn_secs"] {
            assert!(json.contains(&format!("\"custom_id\":\"{id}\"")));
        }
        assert!(!json.contains("\"value\":\"\""));
        assert!(json.contains("\"max_length\":24"));
    }

    #[test]
    fn blackjack_create_modal_has_five_nonempty_inputs() {
        let (count, json) = input_count(create_table_modal(GameKind::Blackjack));
        assert_eq!(count, 5);
        for id in ["name", "bet_range", "side_max", "buy_in_range", "turn_secs"] {
            assert!(json.contains(&format!("\"custom_id\":\"{id}\"")));
        }
        assert!(!json.contains("\"value\":\"\""));
    }

    #[test]
    fn settings_modals_prefill_without_empty_values() {
        let settings = TableSettings::default();
        let (holdem_count, holdem_json) = input_count(table_settings_modal(
            "holdem-1",
            GameKind::Holdem,
            "홀덤",
            settings,
        ));
        let (blackjack_count, blackjack_json) = input_count(table_settings_modal(
            "blackjack-1",
            GameKind::Blackjack,
            "블랙잭",
            settings,
        ));
        assert_eq!(holdem_count, 4);
        assert_eq!(blackjack_count, 5);
        assert!(!holdem_json.contains("\"value\":\"\""));
        assert!(!blackjack_json.contains("\"value\":\"\""));
        assert!(holdem_json.contains("casino_settings:holdem-1"));
        assert!(blackjack_json.contains("casino_settings:blackjack-1"));
    }

    #[test]
    fn panel_render_handles_empty_and_two_tables() {
        assert!(render_casino_panel(&[]).contains("열려 있는 테이블이 없습니다."));
        let summaries = vec![
            TableSummary {
                id: "h1".to_string(),
                kind: GameKind::Holdem,
                kind_text: "홀덤".to_string(),
                name: "하이 롤러".to_string(),
                seated: 2,
                seat_count: 6,
                playing: true,
                phase_text: Some("베팅".to_string()),
                channel_id: Some(100),
                stakes: "블라인드 50/100".to_string(),
            },
            TableSummary {
                id: "b1".to_string(),
                kind: GameKind::Blackjack,
                kind_text: "블랙잭".to_string(),
                name: "블랙잭".to_string(),
                seated: 1,
                seat_count: 6,
                playing: false,
                phase_text: None,
                channel_id: None,
                stakes: "베팅 100~5,000".to_string(),
            },
        ];
        let rendered = render_casino_panel(&summaries);
        assert!(rendered.contains("열린 테이블 (2/12)"));
        assert!(rendered.contains("진행 중") && rendered.contains("대기 중"));
        assert!(rendered.contains("<#100>") && !rendered.contains("<#0>"));
    }

    #[test]
    fn panel_components_use_only_static_custom_ids() {
        let json = serde_json::to_string(&panel_components()).unwrap();
        for id in [
            "casino_panel:enter",
            "casino_panel:me",
            "casino_panel:create_holdem",
            "casino_panel:create_blackjack",
            "casino_panel:settings",
            "casino_panel:close",
        ] {
            assert!(json.contains(id));
        }
        assert!(!json.contains("123456789"));
    }
}

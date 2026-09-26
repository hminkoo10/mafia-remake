// commands/stock_cmds.rs — 주식 시장: /주식(시세·매매·잔고·주문·차트·뉴스·순위·청약·증권 링크),
// /회사(설립·정보·상장·배당·유상증자·신주인수·자사주·위험도·소개·해산), 관리 명령, 시세판·뉴스 채널 중계, 차트 이미지

use super::*;
use crate::stock_hub::{Need, now_ms};
use mafia_remake::stocks::{
    self as market_engine, Candle, CompanyDetail, CompanyStatus, NewsItem, NewsKind, Sector, Side,
    StockMarket, format_amount,
};

const TITLE: &str = "주식";

// ------------------------------------------------------------ 표기

fn won(amount: i64) -> String {
    format!("{}원", format_amount(amount))
}

/// "▲1.25%" / "▼0.40%" / "0.00%".
fn change_text(change_bp: i64) -> String {
    let percent = change_bp.abs() as f64 / 100.0;
    match change_bp.signum() {
        1 => format!("▲{percent:.2}%"),
        -1 => format!("▼{percent:.2}%"),
        _ => "0.00%".to_string(),
    }
}

fn index_change(market: &StockMarket) -> String {
    let prev = market.index.prev_close.max(1.0);
    let bp = ((market.index.value - prev) / prev * 10_000.0).round() as i64;
    format!("{:.2} {}", market.index.value, change_text(bp))
}

/// 사용자 표시 이름 (서버 별명 우선).
async fn display_name(ctx: Context<'_>) -> String {
    ctx.author_member()
        .await
        .map(|member| member.display_name().to_string())
        .unwrap_or_else(|| ctx.author().name.clone())
}

fn kst_clock(unix_ms: i64) -> String {
    let kst = chrono::FixedOffset::east_opt(9 * 3600).expect("KST offset");
    chrono::DateTime::from_timestamp_millis(unix_ms)
        .map(|time| time.with_timezone(&kst).format("%-m/%-d %H:%M").to_string())
        .unwrap_or_default()
}

/// 종목 자동완성: "하늘반도체 (100010) 72,000원 ▲1.2%" 꼴, 값은 종목코드.
pub async fn autocomplete_company(
    ctx: Context<'_>,
    partial: &str,
) -> Vec<serenity::AutocompleteChoice> {
    let market = ctx.data().stocks.market.read().await;
    let now = market.clock(now_ms());
    let partial = partial.trim();
    let mut rows = market
        .companies
        .values()
        .filter(|company| company.status.is_active())
        .filter(|company| {
            partial.is_empty()
                || company.name.contains(partial)
                || company.code.starts_with(partial)
        })
        .map(|company| {
            let summary = market.summary_of(company, now);
            (
                summary.market_cap,
                serenity::AutocompleteChoice::new(
                    format!(
                        "{} ({}) {} {} · {}",
                        company.name,
                        company.code,
                        won(company.price),
                        change_text(summary.change_bp),
                        company.status.label()
                    ),
                    company.code.clone(),
                ),
            )
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|(cap, _)| std::cmp::Reverse(*cap));
    rows.into_iter()
        .take(25)
        .map(|(_, choice)| choice)
        .collect()
}

/// 종목 찾기 (코드·이름).
async fn resolve(ctx: Context<'_>, query: &str) -> std::result::Result<String, String> {
    ctx.data()
        .stocks
        .find_code(query)
        .await
        .ok_or_else(|| format!("'{query}' 종목을 찾지 못했습니다."))
}

async fn fail(ctx: Context<'_>, message: impl Into<String>) -> Result<(), Error> {
    reply_embed(ctx, message, TITLE, serenity::Colour::RED, true).await?;
    Ok(())
}

// ------------------------------------------------------------ /주식

#[poise::command(
    slash_command,
    rename = "주식",
    description_localized("ko", "주식 시장: 시세·매매·잔고·차트·뉴스·청약"),
    subcommands(
        "stock_quote",
        "stock_buy",
        "stock_sell",
        "stock_balance",
        "stock_orders",
        "stock_cancel",
        "stock_chart",
        "stock_news",
        "stock_ranking",
        "stock_subscribe",
        "stock_unsubscribe",
        "stock_web"
    ),
    subcommand_required
)]
pub async fn stock(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// 시장 요약 본문 (시세판과 /주식 시세가 같이 쓴다).
pub fn market_board_text(market: &StockMarket, now: i64) -> String {
    let mut lines = vec![format!("마피아 종합지수 **{}**", index_change(market))];
    if market.time_shift_ms > 0 {
        lines.push(format!(
            "⏩ 시장 시각 {} (관리자가 게임일을 넘겨 실제보다 {} 앞섬)",
            kst_clock(now),
            market_engine::duration_text(market.time_shift_ms)
        ));
    }
    if now < market.halted_until {
        lines.push(format!(
            "⛔ 서킷브레이커: {}까지 모든 거래 정지",
            kst_clock(market.halted_until)
        ));
    }
    let mut system = Vec::new();
    let mut player = Vec::new();
    let mut ipos = Vec::new();
    for company in market
        .companies
        .values()
        .filter(|company| company.status.is_active())
    {
        let summary = market.summary_of(company, now);
        let flags = [
            summary.managed.then_some("관리"),
            summary.halted.then_some("정지"),
            matches!(company.status, CompanyStatus::Liquidating { .. }).then_some("정리매매"),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("·");
        let flag = if flags.is_empty() {
            String::new()
        } else {
            format!(" [{flags}]")
        };
        match &company.status {
            CompanyStatus::Subscription(offering) => ipos.push(format!(
                "🆕 {} ({}) 공모가 {} · {}까지 청약",
                company.name,
                company.code,
                won(offering.price),
                kst_clock(offering.closes_at)
            )),
            CompanyStatus::Private => {}
            _ => {
                let line = format!(
                    "`{}` {} **{}** {}{flag}",
                    company.code,
                    company.name,
                    format_amount(company.price),
                    change_text(summary.change_bp)
                );
                if company.is_player() {
                    player.push((summary.market_cap, line));
                } else {
                    system.push((summary.market_cap, line));
                }
            }
        }
    }
    system.sort_by_key(|(cap, _)| std::cmp::Reverse(*cap));
    player.sort_by_key(|(cap, _)| std::cmp::Reverse(*cap));
    lines.push(String::new());
    lines.extend(system.into_iter().map(|(_, line)| line));
    if !player.is_empty() {
        lines.push(String::new());
        lines.push("**플레이어 회사**".to_string());
        lines.extend(player.into_iter().take(15).map(|(_, line)| line));
    }
    if !ipos.is_empty() {
        lines.push(String::new());
        lines.push("**공모주 청약**".to_string());
        lines.extend(ipos);
    }
    lines.join("\n")
}

fn company_text(detail: &CompanyDetail) -> String {
    let summary = &detail.summary;
    let mut lines = vec![format!(
        "{} ({}) · {} · {}",
        summary.name, summary.code, summary.sector, summary.status
    )];
    lines.push(format!(
        "현재가 **{}** {} (전일 {})",
        won(summary.price),
        change_text(summary.change_bp),
        format_amount(summary.prev_close)
    ));
    lines.push(format!(
        "시가 {} · 고가 {} · 저가 {} · 거래량 {}주",
        format_amount(detail.open),
        format_amount(detail.high),
        format_amount(detail.low),
        format_amount(summary.volume)
    ));
    lines.push(format!(
        "가격제한 {} ~ {} · 시가총액 {}",
        format_amount(detail.lower_limit),
        format_amount(detail.upper_limit),
        won(summary.market_cap)
    ));
    let pbr = if detail.bvps > 0 {
        format!("{:.2}", summary.price as f64 / detail.bvps as f64)
    } else {
        "-".to_string()
    };
    lines.push(format!(
        "주당순자산 {} (PBR {pbr}) · 자본총계 {} · 발행 {}주",
        format_amount(detail.bvps),
        won(detail.equity),
        format_amount(detail.shares)
    ));
    if detail.dividend_yield_bp > 0 {
        lines.push(format!(
            "배당수익률 연 {:.2}% (분기마다 이익이 나면 지급)",
            detail.dividend_yield_bp as f64 / 100.0
        ));
    }
    if let Some(founder) = &summary.founder_name {
        lines.push(format!("대표 {founder} · 사업 위험도 {}단계", detail.risk));
    }
    if summary.managed {
        lines.push("⚠️ 관리종목 (자본잠식 50% 이상)".to_string());
    }
    if let Some((until, reason)) = &detail.liquidation {
        lines.push(format!("⚠️ {reason}: {} 상장폐지", kst_clock(*until)));
    }
    if let Some((at, reason)) = &detail.delisted {
        lines.push(format!("상장폐지됨 ({reason}, {})", kst_clock(*at)));
    }
    if let Some(offering) = &detail.ipo {
        let institutions = if detail.ipo_institutions > 0 {
            format!(" (기관 {}주)", format_amount(detail.ipo_institutions))
        } else {
            String::new()
        };
        lines.push(format!(
            "🆕 공모 청약 중: 공모가 {} · {}주{institutions} · 청약 {}주 · {}까지 (`/주식 청약`)",
            won(offering.price),
            format_amount(offering.shares),
            format_amount(detail.ipo_requested),
            kst_clock(offering.closes_at)
        ));
    }
    if let Some(rights) = &detail.rights {
        lines.push(format!(
            "유상증자: 신주 {}주, 발행가 {} · {}까지 (`/회사 신주인수`)",
            format_amount(rights.shares),
            won(rights.price),
            kst_clock(rights.until)
        ));
    }
    if let Some(dividend) = &detail.pending_dividend {
        lines.push(format!(
            "배당 예정: 주당 {} · {} 지급",
            won(dividend.per_share),
            kst_clock(dividend.pay_at)
        ));
    }
    if let Some(buyback) = &detail.buyback {
        let total = buyback.total.max(buyback.budget);
        lines.push(format!(
            "자사주 매입 중: 예산 {} 중 {} 사용 · {}주 매입 · {}까지 (회사가 현재가에 매수 호가를 냅니다)",
            won(total),
            won(total - buyback.budget),
            buyback.bought,
            kst_clock(buyback.until)
        ));
    }
    if matches!(summary.status, "상장" | "정리매매") {
        let asks = detail
            .book
            .asks
            .iter()
            .take(5)
            .rev()
            .map(|level| {
                format!(
                    "매도 {} · {}주",
                    format_amount(level.price),
                    format_amount(level.qty)
                )
            })
            .collect::<Vec<_>>();
        let bids = detail
            .book
            .bids
            .iter()
            .take(5)
            .map(|level| {
                format!(
                    "매수 {} · {}주",
                    format_amount(level.price),
                    format_amount(level.qty)
                )
            })
            .collect::<Vec<_>>();
        if !asks.is_empty() || !bids.is_empty() {
            lines.push(String::new());
            lines.push("**호가**".to_string());
            lines.extend(asks);
            lines.extend(bids);
        }
    }
    let upcoming = match detail.consensus {
        Some(consensus) => format!(" · 예상 순이익 {}", won(consensus)),
        None => String::new(),
    };
    lines.push(String::new());
    lines.push(format!(
        "다음 실적 발표 {}{upcoming}",
        kst_clock(detail.next_earnings_at)
    ));
    for quarter in detail.quarters.iter().rev().take(3) {
        let dividend = if quarter.dividend > 0 {
            format!(", 주당 배당 {}", format_amount(quarter.dividend))
        } else {
            String::new()
        };
        lines.push(format!(
            "- {}분기: 순이익 {} (예상 {}){dividend}",
            quarter.quarter,
            won(quarter.profit),
            won(quarter.consensus)
        ));
    }
    if !detail.holders.is_empty() && summary.player {
        lines.push(String::new());
        lines.push("**주요 주주**".to_string());
        for holder in detail.holders.iter().take(5) {
            lines.push(format!(
                "- {} {}주 ({:.1}%)",
                holder.name,
                format_amount(holder.qty),
                holder.share_bp as f64 / 100.0
            ));
        }
    }
    if !detail.description.is_empty() {
        lines.push(String::new());
        lines.push(detail.description.clone());
    }
    lines.join("\n")
}

#[poise::command(
    slash_command,
    rename = "시세",
    description_localized("ko", "시장 전체 또는 한 종목의 시세를 봅니다.")
)]
pub async fn stock_quote(
    ctx: Context<'_>,
    #[description = "종목 (비우면 시장 전체)"]
    #[autocomplete = "autocomplete_company"]
    종목: Option<String>,
) -> Result<(), Error> {
    let now = ctx.data().stocks.now().await;
    let (rules, _) = ctx.data().stocks.rules().await;
    let text = match 종목 {
        None => market_board_text(&*ctx.data().stocks.market.read().await, now),
        Some(query) => {
            let code = match resolve(ctx, &query).await {
                Ok(code) => code,
                Err(message) => return fail(ctx, message).await,
            };
            let market = ctx.data().stocks.market.read().await;
            match market.company_detail(&code, now, &rules) {
                Some(detail) => company_text(&detail),
                None => return fail(ctx, "종목을 찾지 못했습니다.").await,
            }
        }
    };
    reply_embed(ctx, text, "시세", serenity::Colour::DARK_GREEN, true).await?;
    Ok(())
}

async fn place(
    ctx: Context<'_>,
    side: Side,
    query: String,
    qty: i64,
    price: Option<i64>,
) -> Result<(), Error> {
    let code = match resolve(ctx, &query).await {
        Ok(code) => code,
        Err(message) => return fail(ctx, message).await,
    };
    let name = display_name(ctx).await;
    let user = ctx.author().id.get();
    let result = ctx
        .data()
        .stocks
        .order(user, &name, &code, side, qty, price)
        .await;
    let result = match result {
        Ok(result) => result,
        Err(message) => return fail(ctx, message).await,
    };
    let company = ctx
        .data()
        .stocks
        .market
        .read()
        .await
        .companies
        .get(&code)
        .map(|company| company.name.clone())
        .unwrap_or_default();
    let mut lines = Vec::new();
    if result.filled > 0 {
        let cost = match side {
            Side::Buy => format!("수수료 {}", won(result.fee)),
            Side::Sell => format!("수수료 {} · 거래세 {}", won(result.fee), won(result.tax)),
        };
        lines.push(format!(
            "{company} {}주 {} 체결 · 평균 {} · 금액 {} ({cost})",
            format_amount(result.filled),
            side.label(),
            won(result.avg_price),
            won(result.notional)
        ));
    } else {
        lines.push(format!("{company} {} 주문을 받았습니다.", side.label()));
    }
    if let Some(id) = result.resting {
        lines.push(format!(
            "지정가 {}에 {}주가 호가에 남았습니다 (주문번호 {id}, 24게임일 동안 유효).",
            won(result.limit.unwrap_or(0)),
            format_amount(result.resting_qty)
        ));
    } else if result.filled < qty {
        lines.push(format!(
            "{}주는 체결되지 않아 취소했습니다 (시장가는 남기지 않습니다).",
            format_amount(qty - result.filled)
        ));
    }
    if result.refund > 0 && side == Side::Buy {
        lines.push(format!(
            "쓰지 않은 증거금 {}은 돌려받았습니다.",
            won(result.refund)
        ));
    }
    reply_embed(
        ctx,
        lines.join("\n"),
        side.label(),
        serenity::Colour::DARK_GREEN,
        true,
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "매수",
    description_localized("ko", "주식을 삽니다 (가격을 비우면 시장가).")
)]
pub async fn stock_buy(
    ctx: Context<'_>,
    #[description = "종목"]
    #[autocomplete = "autocomplete_company"]
    종목: String,
    #[description = "수량(주)"]
    #[min = 1]
    수량: i64,
    #[description = "지정가 (비우면 시장가)"]
    #[min = 1]
    가격: Option<i64>,
) -> Result<(), Error> {
    place(ctx, Side::Buy, 종목, 수량, 가격).await
}

#[poise::command(
    slash_command,
    rename = "매도",
    description_localized("ko", "주식을 팝니다 (가격을 비우면 시장가).")
)]
pub async fn stock_sell(
    ctx: Context<'_>,
    #[description = "종목"]
    #[autocomplete = "autocomplete_company"]
    종목: String,
    #[description = "수량(주)"]
    #[min = 1]
    수량: i64,
    #[description = "지정가 (비우면 시장가)"]
    #[min = 1]
    가격: Option<i64>,
) -> Result<(), Error> {
    place(ctx, Side::Sell, 종목, 수량, 가격).await
}

#[poise::command(
    slash_command,
    rename = "잔고",
    description_localized("ko", "내 주식 잔고와 손익을 봅니다.")
)]
pub async fn stock_balance(ctx: Context<'_>) -> Result<(), Error> {
    let user = ctx.author().id.get();
    let now = ctx.data().stocks.now().await;
    let view = ctx
        .data()
        .stocks
        .market
        .read()
        .await
        .account_view(user, now);
    let coins = ctx
        .data()
        .stats
        .read()
        .await
        .users
        .get(&user.to_string())
        .map_or(0, |entry| entry.coins);
    let mut lines = vec![format!(
        "평가액 **{}** · 보유 코인 {} · 주문·청약에 묶인 코인 {}",
        won(view.stock_value),
        won(coins),
        won(view.pending)
    )];
    lines.push(format!(
        "평가 손익 {} · 실현 손익 {} · 낸 수수료·세금 {}",
        stats::signed_coin_text(view.unrealized),
        stats::signed_coin_text(view.realized),
        won(view.fees)
    ));
    if view.positions.is_empty() {
        lines.push("보유 주식이 없습니다. `/주식 매수`로 사 보세요.".to_string());
    }
    for position in &view.positions {
        let lockup = if position.lockup_qty > 0 {
            format!(
                " · 보호예수 {}주({}까지)",
                position.lockup_qty,
                kst_clock(position.lockup_until)
            )
        } else {
            String::new()
        };
        lines.push(format!(
            "- {} {}주 · 평균 {} → {} · {} ({}){lockup}",
            position.name,
            format_amount(position.qty),
            format_amount(position.avg_price),
            format_amount(position.price),
            stats::signed_coin_text(position.pnl),
            change_text(position.pnl_bp)
        ));
    }
    if !view.subscriptions.is_empty() {
        lines.push(String::new());
        lines.push("**청약**".to_string());
        for subscription in &view.subscriptions {
            lines.push(format!(
                "- {} {}주 (증거금 {}, {} 마감)",
                subscription.name,
                format_amount(subscription.qty),
                won(subscription.deposit),
                kst_clock(subscription.closes_at)
            ));
        }
    }
    if !view.rights.is_empty() {
        lines.push(String::new());
        lines.push("**신주인수권**".to_string());
        for claim in &view.rights {
            lines.push(format!(
                "- {} {}주 중 {}주 행사 · 발행가 {} · {}까지",
                claim.name,
                claim.granted,
                claim.exercised,
                won(claim.price),
                kst_clock(claim.until)
            ));
        }
    }
    reply_embed(ctx, lines.join("\n"), "잔고", serenity::Colour::GOLD, true).await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "주문",
    description_localized("ko", "호가에 남은 내 지정가 주문을 보고 취소합니다.")
)]
pub async fn stock_orders(ctx: Context<'_>) -> Result<(), Error> {
    let user = ctx.author().id.get();
    let view = {
        let market = ctx.data().stocks.market.read().await;
        market.account_view(user, market.clock(now_ms()))
    };
    if view.orders.is_empty() {
        reply_embed(
            ctx,
            "호가에 남은 주문이 없습니다.",
            "주문",
            serenity::Colour::GOLD,
            true,
        )
        .await?;
        return Ok(());
    }
    let lines = view
        .orders
        .iter()
        .map(|order| {
            format!(
                "#{} {} {} {}주 @ {} (남은 {}주, {}까지)",
                order.id,
                order.name,
                order.side.label(),
                format_amount(order.original),
                won(order.limit),
                format_amount(order.remaining),
                kst_clock(order.expires_at)
            )
        })
        .collect::<Vec<_>>();
    let buttons = view
        .orders
        .iter()
        .take(5)
        .map(|order| {
            serenity::CreateButton::new(format!("stock_cancel:{}", order.id))
                .label(format!("#{} 취소", order.id))
                .style(serenity::ButtonStyle::Secondary)
        })
        .collect::<Vec<_>>();
    ctx.send(
        poise::CreateReply::default()
            .embed(make_embed(lines.join("\n"), "주문", serenity::Colour::GOLD))
            .components(vec![serenity::CreateActionRow::Buttons(buttons)])
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "취소",
    description_localized("ko", "지정가 주문을 취소합니다.")
)]
pub async fn stock_cancel(
    ctx: Context<'_>,
    #[description = "주문번호"]
    #[min = 1]
    주문번호: u64,
) -> Result<(), Error> {
    match ctx
        .data()
        .stocks
        .cancel(ctx.author().id.get(), 주문번호)
        .await
    {
        Ok(()) => {
            reply_embed(
                ctx,
                format!("주문 #{주문번호}을 취소했습니다."),
                "주문 취소",
                serenity::Colour::DARK_GREEN,
                true,
            )
            .await?;
            Ok(())
        }
        Err(message) => fail(ctx, message).await,
    }
}

/// "주문 취소" 버튼.
pub async fn handle_stock_cancel(
    ctx: &serenity::Context,
    data: &Data,
    component: &serenity::ComponentInteraction,
    order_id: &str,
) -> anyhow::Result<()> {
    let message = match order_id.parse::<u64>() {
        Ok(id) => match data.stocks.cancel(component.user.id.get(), id).await {
            Ok(()) => format!("주문 #{id}을 취소했습니다."),
            Err(message) => message,
        },
        Err(_) => "주문번호가 올바르지 않습니다.".to_string(),
    };
    component
        .create_response(
            &ctx.http,
            serenity::CreateInteractionResponse::Message(
                serenity::CreateInteractionResponseMessage::new()
                    .ephemeral(true)
                    .embed(make_embed(message, "주문 취소", serenity::Colour::GOLD)),
            ),
        )
        .await?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum ChartRange {
    #[name = "분봉 (최근 2시간)"]
    Minute,
    #[name = "일봉 (게임일, 최근 3일)"]
    Day,
}

#[poise::command(
    slash_command,
    rename = "차트",
    description_localized("ko", "종목 차트 이미지를 봅니다.")
)]
pub async fn stock_chart(
    ctx: Context<'_>,
    #[description = "종목"]
    #[autocomplete = "autocomplete_company"]
    종목: String,
    #[description = "기간"] 기간: Option<ChartRange>,
) -> Result<(), Error> {
    let code = match resolve(ctx, &종목).await {
        Ok(code) => code,
        Err(message) => return fail(ctx, message).await,
    };
    let range = 기간.unwrap_or(ChartRange::Minute);
    let (title, candles) = {
        let market = ctx.data().stocks.market.read().await;
        let Some(company) = market.companies.get(&code) else {
            return fail(ctx, "종목을 찾지 못했습니다.").await;
        };
        let series = market
            .candles
            .series
            .get(&code)
            .cloned()
            .unwrap_or_default();
        let candles = match range {
            ChartRange::Minute => series
                .minute
                .iter()
                .rev()
                .take(120)
                .rev()
                .copied()
                .collect::<Vec<_>>(),
            ChartRange::Day => series
                .day
                .iter()
                .rev()
                .take(72)
                .rev()
                .copied()
                .collect::<Vec<_>>(),
        };
        let summary = market.summary_of(company, market.clock(now_ms()));
        (
            format!(
                "{} ({}) {} {}",
                company.name,
                company.code,
                won(company.price),
                change_text(summary.change_bp)
            ),
            candles,
        )
    };
    let minute = range == ChartRange::Minute;
    let image =
        tokio::task::spawn_blocking(move || render_candle_chart(&title, &candles, minute)).await?;
    match image {
        Some(bytes) => {
            ctx.send(
                poise::CreateReply::default()
                    .attachment(serenity::CreateAttachment::bytes(
                        bytes,
                        format!("chart_{code}.png"),
                    ))
                    .ephemeral(true),
            )
            .await?;
            Ok(())
        }
        None => fail(ctx, "아직 차트를 그릴 거래 기록이 없습니다.").await,
    }
}

#[poise::command(
    slash_command,
    rename = "뉴스",
    description_localized("ko", "최근 증권 뉴스·공시를 봅니다.")
)]
pub async fn stock_news(
    ctx: Context<'_>,
    #[description = "종목 (비우면 전체)"]
    #[autocomplete = "autocomplete_company"]
    종목: Option<String>,
) -> Result<(), Error> {
    let code = match 종목 {
        Some(query) => match resolve(ctx, &query).await {
            Ok(code) => Some(code),
            Err(message) => return fail(ctx, message).await,
        },
        None => None,
    };
    let market = ctx.data().stocks.market.read().await;
    let lines = market
        .news
        .iter()
        .rev()
        .filter(|news| code.is_none() || news.code == code)
        .take(12)
        .map(news_line)
        .collect::<Vec<_>>();
    drop(market);
    let text = if lines.is_empty() {
        "아직 뉴스가 없습니다.".to_string()
    } else {
        format!("최근 소식\n{}", lines.join("\n"))
    };
    reply_embed(ctx, text, "증권 뉴스", serenity::Colour::BLUE, true).await?;
    Ok(())
}

fn news_line(news: &NewsItem) -> String {
    let tone = match news.tone {
        1 => "🔺",
        -1 => "🔻",
        _ => "▪️",
    };
    format!(
        "{tone} `{}` [{}] {}",
        kst_clock(news.at),
        news.kind.label(),
        news.headline
    )
}

#[poise::command(
    slash_command,
    rename = "순위",
    description_localized("ko", "주식 재산 순위를 봅니다.")
)]
pub async fn stock_ranking(ctx: Context<'_>) -> Result<(), Error> {
    let now = ctx.data().stocks.now().await;
    let ranking = ctx.data().stocks.market.read().await.ranking(now);
    let text = if ranking.is_empty() {
        "아직 주식을 가진 사람이 없습니다.".to_string()
    } else {
        let lines = ranking
            .iter()
            .take(10)
            .enumerate()
            .map(|(index, entry)| {
                format!(
                    "{}. {} · 평가액 {} · 손익 {}",
                    index + 1,
                    entry.name,
                    won(entry.value),
                    stats::signed_coin_text(entry.profit)
                )
            })
            .collect::<Vec<_>>();
        format!("주식 재산 순위\n{}", lines.join("\n"))
    };
    reply_embed(ctx, text, "주식 순위", serenity::Colour::GOLD, false).await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "청약",
    description_localized("ko", "공모주를 청약합니다 (증거금 = 공모가 × 수량).")
)]
pub async fn stock_subscribe(
    ctx: Context<'_>,
    #[description = "공모 중인 회사"]
    #[autocomplete = "autocomplete_company"]
    종목: String,
    #[description = "청약 수량(주)"]
    #[min = 1]
    수량: i64,
) -> Result<(), Error> {
    let code = match resolve(ctx, &종목).await {
        Ok(code) => code,
        Err(message) => return fail(ctx, message).await,
    };
    let name = display_name(ctx).await;
    let user = ctx.author().id.get();
    let result = ctx
        .data()
        .stocks
        .transact(
            user,
            |market, _| market.subscription_cost(&code, 수량).map(Need::Exactly),
            |market, _, _, now| market.subscribe(user, &name, &code, 수량, now),
        )
        .await;
    match result {
        Ok(deposit) => {
            reply_embed(
                ctx,
                format!(
                    "{}주 청약을 넣었습니다 (증거금 {}). 마감 때 배정되고, 배정되지 않은 증거금은 돌려받습니다.",
                    format_amount(수량),
                    won(deposit)
                ),
                "청약",
                serenity::Colour::DARK_GREEN,
                true,
            )
            .await?;
            Ok(())
        }
        Err(message) => fail(ctx, message).await,
    }
}

#[poise::command(
    slash_command,
    rename = "청약취소",
    description_localized("ko", "마감 전에 청약을 취소합니다.")
)]
pub async fn stock_unsubscribe(
    ctx: Context<'_>,
    #[description = "공모 중인 회사"]
    #[autocomplete = "autocomplete_company"]
    종목: String,
) -> Result<(), Error> {
    let code = match resolve(ctx, &종목).await {
        Ok(code) => code,
        Err(message) => return fail(ctx, message).await,
    };
    let user = ctx.author().id.get();
    let result = ctx
        .data()
        .stocks
        .transact(
            user,
            |_, _| Ok(Need::Nothing),
            |market, _, _, _| market.cancel_subscription(user, &code),
        )
        .await;
    match result {
        Ok(refund) => {
            reply_embed(
                ctx,
                format!("청약을 취소하고 {}을 돌려받았습니다.", won(refund)),
                "청약 취소",
                serenity::Colour::DARK_GREEN,
                true,
            )
            .await?;
            Ok(())
        }
        Err(message) => fail(ctx, message).await,
    }
}

#[poise::command(
    slash_command,
    rename = "증권",
    description_localized("ko", "증권 사이트(마피아증권: 차트·호가·주문) 개인 링크를 받습니다.")
)]
pub async fn stock_web(ctx: Context<'_>) -> Result<(), Error> {
    let name = display_name(ctx).await;
    let token = ctx
        .data()
        .casino
        .issue_session(ctx.author().id.get(), name.clone());
    let link = crate::stock_web::stocks_link(&ctx.data().casino_base_url, &token);
    let admin = matches!(
        manager_denial(
            ctx.serenity_context(),
            ctx.data(),
            ctx.guild_id(),
            ctx.author().id
        )
        .await,
        Ok(None)
    );
    if admin {
        ctx.data()
            .stocks
            .grant_web_admin(
                ctx.author().id.get(),
                now_ms() + crate::casino_hub::CASINO_SESSION_TTL_SECONDS as i64 * 1000,
            )
            .await;
    }
    let admin_note = if admin {
        "\n\n🛠️ 관리자이므로 사이트의 '관리' 탭에서 거래정지·재개, 시장 집계, 게임일 넘기기, 시세판·뉴스 채널 연결도 할 수 있습니다."
    } else {
        ""
    };
    reply_embed(
        ctx,
        format!(
            "마피아증권 링크입니다.\n{link}\n\n⚠️ 이 링크는 **{name}** 님 전용이고 12시간 동안 유효합니다. 다른 사람과 공유하지 마세요.{admin_note}"
        ),
        "마피아증권",
        serenity::Colour::DARK_GREEN,
        true,
    )
    .await?;
    Ok(())
}

// ------------------------------------------------------------ /회사

#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum SectorChoice {
    #[name = "반도체"]
    Semiconductor,
    #[name = "2차전지"]
    Battery,
    #[name = "자동차"]
    Auto,
    #[name = "인터넷"]
    Internet,
    #[name = "게임"]
    Game,
    #[name = "바이오"]
    Bio,
    #[name = "은행"]
    Bank,
    #[name = "레저"]
    Leisure,
    #[name = "유통"]
    Retail,
    #[name = "통신"]
    Telecom,
    #[name = "조선"]
    Shipbuilding,
    #[name = "엔터"]
    Entertainment,
}

impl SectorChoice {
    fn sector(self) -> Sector {
        match self {
            Self::Semiconductor => Sector::Semiconductor,
            Self::Battery => Sector::Battery,
            Self::Auto => Sector::Auto,
            Self::Internet => Sector::Internet,
            Self::Game => Sector::Game,
            Self::Bio => Sector::Bio,
            Self::Bank => Sector::Bank,
            Self::Leisure => Sector::Leisure,
            Self::Retail => Sector::Retail,
            Self::Telecom => Sector::Telecom,
            Self::Shipbuilding => Sector::Shipbuilding,
            Self::Entertainment => Sector::Entertainment,
        }
    }
}

#[poise::command(
    slash_command,
    rename = "회사",
    description_localized("ko", "회사 설립·상장·경영(배당·증자·자사주·위험도)·해산"),
    subcommands(
        "company_found",
        "company_info",
        "company_ipo",
        "company_dividend",
        "company_rights",
        "company_exercise",
        "company_buyback",
        "company_risk",
        "company_describe",
        "company_dissolve"
    ),
    subcommand_required
)]
pub async fn company(_ctx: Context<'_>) -> Result<(), Error> {
    Ok(())
}

/// 내가 대표인 회사 코드.
async fn my_company(ctx: Context<'_>) -> std::result::Result<String, String> {
    let user = ctx.author().id.get();
    let market = ctx.data().stocks.market.read().await;
    market
        .companies
        .values()
        .find(|company| company.founder() == Some(user) && company.status.is_active())
        .map(|company| company.code.clone())
        .ok_or_else(|| "대표로 있는 회사가 없습니다. `/회사 설립`으로 세워 보세요.".to_string())
}

async fn company_done(
    ctx: Context<'_>,
    result: std::result::Result<String, String>,
) -> Result<(), Error> {
    match result {
        Ok(message) => {
            reply_embed(ctx, message, "회사", serenity::Colour::DARK_GREEN, true).await?;
            Ok(())
        }
        Err(message) => fail(ctx, message).await,
    }
}

#[poise::command(
    slash_command,
    rename = "설립",
    description_localized("ko", "코인으로 회사를 세웁니다 (자본금은 회사 현금이 됩니다).")
)]
pub async fn company_found(
    ctx: Context<'_>,
    #[description = "회사 이름 (2~12자)"] 이름: String,
    #[description = "업종"] 업종: SectorChoice,
    #[description = "자본금(원)"]
    #[min = 1]
    자본금: i64,
) -> Result<(), Error> {
    let name = display_name(ctx).await;
    let user = ctx.author().id.get();
    let result = ctx
        .data()
        .stocks
        .transact(
            user,
            |_, rules| Ok(Need::Exactly(StockMarket::founding_cost(자본금, rules))),
            |market, _, rules, now| {
                market.found_company(user, &name, &이름, 업종.sector(), 자본금, now, rules)
            },
        )
        .await;
    let (rules, _) = ctx.data().stocks.rules().await;
    company_done(
        ctx,
        result.map(|code| {
            format!(
                "회사를 세웠습니다 (종목코드 {code}). 자본금 {}이 회사 현금이 되었고, 설립 수수료 {}는 복지 금고로 갔습니다.\n1게임일이 지나면 `/회사 상장`으로 공모를 열 수 있습니다. 회사 실적은 실제 1주마다 발표되고, 사업 위험도(`/회사 위험도`)에 따라 오르내립니다.",
                won(자본금),
                won(market_engine::bp_part(자본금, rules.found_fee_bp))
            )
        }),
    )
    .await
}

#[poise::command(
    slash_command,
    rename = "정보",
    description_localized("ko", "회사 정보(재무·주주·일정)를 봅니다.")
)]
pub async fn company_info(
    ctx: Context<'_>,
    #[description = "회사 (비우면 내 회사)"]
    #[autocomplete = "autocomplete_company"]
    회사: Option<String>,
) -> Result<(), Error> {
    let code = match 회사 {
        Some(query) => resolve(ctx, &query).await,
        None => my_company(ctx).await,
    };
    let code = match code {
        Ok(code) => code,
        Err(message) => return fail(ctx, message).await,
    };
    let (rules, _) = ctx.data().stocks.rules().await;
    let text = {
        let market = ctx.data().stocks.market.read().await;
        market
            .company_detail(&code, market.clock(now_ms()), &rules)
            .map(|detail| company_text(&detail))
    };
    match text {
        Some(text) => {
            reply_embed(ctx, text, "회사 정보", serenity::Colour::DARK_GREEN, true).await?;
            Ok(())
        }
        None => fail(ctx, "회사를 찾지 못했습니다.").await,
    }
}

#[poise::command(
    slash_command,
    rename = "상장",
    description_localized("ko", "내 회사의 공모 청약을 엽니다 (1게임일 뒤 상장).")
)]
pub async fn company_ipo(
    ctx: Context<'_>,
    #[description = "공모가 (주당 순자산의 0.8~3배)"]
    #[min = 1]
    공모가: i64,
    #[description = "공모 주식 수 (지금 주식의 10~100%)"]
    #[min = 1]
    공모주식수: i64,
) -> Result<(), Error> {
    let code = match my_company(ctx).await {
        Ok(code) => code,
        Err(message) => return fail(ctx, message).await,
    };
    let user = ctx.author().id.get();
    let result = ctx
        .data()
        .stocks
        .transact(
            user,
            |_, _| Ok(Need::Nothing),
            |market, _, rules, now| {
                let offering = market.start_ipo(user, &code, 공모가, 공모주식수, now, rules)?;
                let bvps = market
                    .companies
                    .get(&code)
                    .map_or(0.0, |company| company.bvps());
                let institutions = mafia_remake::stocks::institution_shares(
                    offering.shares,
                    offering.price,
                    bvps,
                    rules,
                );
                Ok((offering, institutions))
            },
        )
        .await;
    company_done(
        ctx,
        result.map(|(offering, institutions)| {
            let institutions = if institutions > 0 {
                format!(
                    " 그중 {}주는 기관이 받아 가 상장 뒤 시장에서 거래되고, 플레이어는 나머지를 나눠 받습니다.",
                    format_amount(institutions)
                )
            } else {
                " 공모가가 주당 순자산의 2배 이상이라 기관은 청약하지 않습니다.".to_string()
            };
            format!(
                "공모 청약을 열었습니다: 공모가 {}, {}주, {} 마감.{institutions} 청약이 공모 주식의 절반에 못 미치면 무산됩니다. 상장 뒤 대표 지분은 보호예수로 한동안 팔 수 없습니다.",
                won(offering.price),
                format_amount(offering.shares),
                kst_clock(offering.closes_at)
            )
        }),
    )
    .await
}

#[poise::command(
    slash_command,
    rename = "배당",
    description_localized("ko", "내 회사의 현금배당을 결정합니다 (다음 게임일에 지급).")
)]
pub async fn company_dividend(
    ctx: Context<'_>,
    #[description = "주당 배당금(원)"]
    #[min = 1]
    주당배당금: i64,
) -> Result<(), Error> {
    let code = match my_company(ctx).await {
        Ok(code) => code,
        Err(message) => return fail(ctx, message).await,
    };
    let user = ctx.author().id.get();
    let result = ctx
        .data()
        .stocks
        .transact(
            user,
            |_, _| Ok(Need::Nothing),
            |market, _, rules, now| market.declare_dividend(user, &code, 주당배당금, now, rules),
        )
        .await;
    company_done(
        ctx,
        result.map(|total| {
            format!(
                "주당 {} 현금배당을 결정했습니다 (총 {}). 다음 게임일이 시작될 때 그때의 주주에게 회사 현금에서 지급되고, 주가는 배당금만큼 내려갑니다.",
                won(주당배당금),
                won(total)
            )
        }),
    )
    .await
}

#[poise::command(
    slash_command,
    rename = "유상증자",
    description_localized("ko", "주주배정 유상증자로 회사 자금을 모읍니다.")
)]
pub async fn company_rights(
    ctx: Context<'_>,
    #[description = "신주 수"]
    #[min = 1]
    신주: i64,
    #[description = "발행가 (현재가의 70~100%)"]
    #[min = 1]
    발행가: i64,
) -> Result<(), Error> {
    let code = match my_company(ctx).await {
        Ok(code) => code,
        Err(message) => return fail(ctx, message).await,
    };
    let user = ctx.author().id.get();
    let result = ctx
        .data()
        .stocks
        .transact(
            user,
            |_, _| Ok(Need::Nothing),
            |market, _, rules, now| market.start_rights(user, &code, 신주, 발행가, now, rules),
        )
        .await;
    company_done(
        ctx,
        result.map(|()| {
            "유상증자를 결정했습니다. 지금 주주가 보유 비율대로 신주인수권을 받고, 1게임일 동안 `/회사 신주인수`로 행사할 수 있습니다. 행사한 만큼만 발행됩니다.".to_string()
        }),
    )
    .await
}

#[poise::command(
    slash_command,
    rename = "신주인수",
    description_localized("ko", "유상증자 신주인수권을 행사합니다.")
)]
pub async fn company_exercise(
    ctx: Context<'_>,
    #[description = "유상증자 중인 회사"]
    #[autocomplete = "autocomplete_company"]
    회사: String,
    #[description = "행사 수량(주)"]
    #[min = 1]
    수량: i64,
) -> Result<(), Error> {
    let code = match resolve(ctx, &회사).await {
        Ok(code) => code,
        Err(message) => return fail(ctx, message).await,
    };
    let name = display_name(ctx).await;
    let user = ctx.author().id.get();
    let result = ctx
        .data()
        .stocks
        .transact(
            user,
            |market, _| market.rights_cost(user, &code, 수량).map(Need::Exactly),
            |market, _, _, now| market.exercise_rights(user, &name, &code, 수량, now),
        )
        .await;
    company_done(
        ctx,
        result.map(|cost| {
            format!(
                "신주 {}주를 인수했습니다 (대금 {}). 청약 기간이 끝나면 주식이 들어옵니다.",
                format_amount(수량),
                won(cost)
            )
        }),
    )
    .await
}

#[poise::command(
    slash_command,
    rename = "자사주",
    description_localized("ko", "회사 현금으로 자사주를 사서 소각합니다 (1게임일).")
)]
pub async fn company_buyback(
    ctx: Context<'_>,
    #[description = "매입 예산(원, 회사 현금의 절반까지)"]
    #[min = 1]
    예산: i64,
) -> Result<(), Error> {
    let code = match my_company(ctx).await {
        Ok(code) => code,
        Err(message) => return fail(ctx, message).await,
    };
    let user = ctx.author().id.get();
    let result = ctx
        .data()
        .stocks
        .transact(
            user,
            |_, _| Ok(Need::Nothing),
            |market, _, rules, now| market.start_buyback(user, &code, 예산, now, rules),
        )
        .await;
    company_done(
        ctx,
        result.map(|()| {
            format!("{} 규모 자사주 매입을 시작했습니다. 1게임일 동안 시장에서 조금씩 사서 소각하고, 그동안 대표는 주식을 팔 수 없습니다.", won(예산))
        }),
    )
    .await
}

#[poise::command(
    slash_command,
    rename = "위험도",
    description_localized(
        "ko",
        "사업 위험도(1~5)를 정합니다. 높을수록 실적이 크게 오르내립니다."
    )
)]
pub async fn company_risk(
    ctx: Context<'_>,
    #[description = "위험도 1(안정)~5(공격)"]
    #[min = 1]
    #[max = 5]
    단계: u8,
) -> Result<(), Error> {
    let code = match my_company(ctx).await {
        Ok(code) => code,
        Err(message) => return fail(ctx, message).await,
    };
    let user = ctx.author().id.get();
    let result = ctx
        .data()
        .stocks
        .transact(
            user,
            |_, _| Ok(Need::Nothing),
            |market, _, _, now| market.set_risk(user, &code, 단계, now),
        )
        .await;
    company_done(
        ctx,
        result.map(|()| {
            format!("사업 위험도를 {단계}단계로 정했습니다. 분기 실적의 변동 폭이 바뀌고, 1주 동안 다시 바꿀 수 없습니다. 적자가 쌓여 자본이 절반 아래로 줄면 관리종목, 0이 되면 파산입니다.")
        }),
    )
    .await
}

#[poise::command(
    slash_command,
    rename = "소개",
    description_localized("ko", "회사 소개 문구를 씁니다 (120자).")
)]
pub async fn company_describe(
    ctx: Context<'_>,
    #[description = "소개 (120자까지)"] 내용: String,
) -> Result<(), Error> {
    let code = match my_company(ctx).await {
        Ok(code) => code,
        Err(message) => return fail(ctx, message).await,
    };
    let user = ctx.author().id.get();
    let result = ctx
        .data()
        .stocks
        .transact(
            user,
            |_, _| Ok(Need::Nothing),
            |market, _, _, _| market.set_description(user, &code, &내용),
        )
        .await;
    company_done(ctx, result.map(|()| "회사 소개를 바꿨습니다.".to_string())).await
}

#[poise::command(
    slash_command,
    rename = "해산",
    description_localized(
        "ko",
        "회사를 해산합니다 (상장사는 대표 지분 50% 이상일 때 청산·상장폐지)."
    )
)]
pub async fn company_dissolve(
    ctx: Context<'_>,
    #[description = "정말 해산하려면 회사 이름을 그대로 입력하세요"] 확인: String,
) -> Result<(), Error> {
    let code = match my_company(ctx).await {
        Ok(code) => code,
        Err(message) => return fail(ctx, message).await,
    };
    let name = ctx
        .data()
        .stocks
        .market
        .read()
        .await
        .companies
        .get(&code)
        .map(|company| company.name.clone())
        .unwrap_or_default();
    if 확인.trim() != name {
        return fail(
            ctx,
            format!("해산하려면 회사 이름 '{name}'을 그대로 입력하세요."),
        )
        .await;
    }
    let user = ctx.author().id.get();
    let result = ctx
        .data()
        .stocks
        .transact(
            user,
            |_, _| Ok(Need::Nothing),
            |market, _, rules, now| market.dissolve(user, &code, now, rules),
        )
        .await;
    company_done(
        ctx,
        result.map(|()| {
            "해산을 결정했습니다. 비상장 회사는 회사 현금을 바로 돌려받고, 상장사는 1게임일 거래정지 뒤 남은 현금을 주주 모두에게 지분만큼 나누고 상장폐지됩니다.".to_string()
        }),
    )
    .await
}

// ------------------------------------------------------------ 관리

#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
pub enum StockAdminAction {
    #[name = "거래정지"]
    Halt,
    #[name = "거래재개"]
    Resume,
    #[name = "시장 집계 보기"]
    Stats,
    #[name = "게임일 넘기기"]
    SkipDays,
}

/// 주식 시장이 코인을 얼마나 만들고 없앴는지 (관리자용 누적).
pub fn market_stats_text(market: &StockMarket) -> String {
    let stats = &market.stats;
    let net = stats.lp_sold - stats.lp_bought + stats.dividends - stats.ipo_burned;
    [
        format!(
            "시장조성자에게서 산 금액 (코인 사라짐): {}",
            won(stats.lp_bought)
        ),
        format!("시장조성자에게 판 금액 (코인 생김): {}", won(stats.lp_sold)),
        format!("플레이어끼리 체결: {}", won(stats.p2p)),
        format!(
            "금고로 간 수수료 / 거래세: {} / {}",
            won(stats.fees),
            won(stats.taxes)
        ),
        format!("시스템 회사 배당 (코인 생김): {}", won(stats.dividends)),
        format!(
            "시스템 회사 공모 대금 (코인 사라짐): {}",
            won(stats.ipo_burned)
        ),
        format!(
            "플레이어 회사 분기 실적 (회사 현금): {}",
            signed_won(stats.company_earnings)
        ),
        String::new(),
        format!(
            "**시장이 만든 코인: {}** (판 금액 − 산 금액 + 배당 − 공모 대금)",
            signed_won(net)
        ),
    ]
    .join("\n")
}

fn signed_won(amount: i64) -> String {
    if amount > 0 {
        format!("+{}", won(amount))
    } else {
        won(amount)
    }
}

#[poise::command(
    slash_command,
    rename = "주식관리",
    description_localized("ko", "관리자: 종목 거래정지·재개, 시장 코인 집계, 게임일 넘기기")
)]
pub async fn manage_stocks(
    ctx: Context<'_>,
    #[description = "동작"] 동작: StockAdminAction,
    #[description = "종목 (거래정지·재개 때)"]
    #[autocomplete = "autocomplete_company"]
    종목: Option<String>,
    #[description = "넘길 게임일 수 (게임일 넘기기 때, 기본 1)"]
    #[min = 1]
    #[max = 24]
    일수: Option<i64>,
) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    if 동작 == StockAdminAction::SkipDays {
        return skip_game_days(ctx, 일수.unwrap_or(1)).await;
    }
    if 동작 == StockAdminAction::Stats {
        let text = market_stats_text(&*ctx.data().stocks.market.read().await);
        reply_embed(
            ctx,
            text,
            "주식 시장 집계",
            serenity::Colour::DARK_GREEN,
            true,
        )
        .await?;
        return Ok(());
    }
    let Some(종목) = 종목 else {
        return fail(ctx, "거래정지·재개할 종목을 골라 주세요.").await;
    };
    let code = match resolve(ctx, &종목).await {
        Ok(code) => code,
        Err(message) => return fail(ctx, message).await,
    };
    let halted = 동작 == StockAdminAction::Halt;
    let result = ctx
        .data()
        .stocks
        .transact(
            0,
            |_, _| Ok(Need::Nothing),
            |market, _, _, now| market.set_admin_halt(&code, halted, now),
        )
        .await;
    match result {
        Ok(name) => {
            let text = format!(
                "{name}: {}",
                if halted {
                    "거래정지"
                } else {
                    "거래재개"
                }
            );
            let log_channel_id = ctx.data().config.read().await.log_channel_id;
            send_admin_log(
                ctx.http(),
                log_channel_id,
                "주식 관리",
                format!("{} 님이 {text}", ctx.author().name),
            )
            .await;
            reply_embed(ctx, text, "주식 관리", serenity::Colour::DARK_GREEN, false).await?;
            Ok(())
        }
        Err(message) => fail(ctx, message).await,
    }
}

/// 관리자: 게임일을 넘긴다 (청약·배당·유상증자·보호예수 등 게임일 단위 일정이 그만큼 앞당겨진다).
async fn skip_game_days(ctx: Context<'_>, days: i64) -> Result<(), Error> {
    if let Err(error) = ctx.defer_ephemeral().await {
        eprintln!("failed to defer 게임일 넘기기: {error:?}");
    }
    match ctx.data().stocks.skip_game_days(days).await {
        Ok((_, skipped)) => {
            let skipped = market_engine::duration_text(skipped);
            let text = format!(
                "게임일을 {days}일 넘겼습니다. 시장 시계가 {skipped} 앞당겨졌습니다. 그 사이의 시세·주문·청약·배당은 모두 처리했습니다."
            );
            let log_channel_id = ctx.data().config.read().await.log_channel_id;
            send_admin_log(
                ctx.http(),
                log_channel_id,
                "주식 관리",
                format!(
                    "{} 님이 게임일을 {days}일 넘김 ({skipped})",
                    ctx.author().name
                ),
            )
            .await;
            reply_embed(ctx, text, "주식 관리", serenity::Colour::DARK_GREEN, true).await?;
            Ok(())
        }
        Err(message) => fail(ctx, message).await,
    }
}

#[poise::command(
    slash_command,
    rename = "주식패널",
    description_localized("ko", "관리자: 이 채널에 1분마다 갱신되는 시세판을 올립니다.")
)]
pub async fn stock_panel(ctx: Context<'_>) -> Result<(), Error> {
    if let Err(error) = ctx.defer_ephemeral().await {
        eprintln!("failed to defer 주식패널: {error:?}");
    }
    if !require_manager(ctx).await? {
        return Ok(());
    }
    let text = {
        let market = ctx.data().stocks.market.read().await;
        market_board_text(&market, market.clock(now_ms()))
    };
    let message = ctx
        .channel_id()
        .send_message(
            ctx.http(),
            serenity::CreateMessage::new().embed(make_embed(
                text.clone(),
                "증권 시세판",
                serenity::Colour::DARK_GREEN,
            )),
        )
        .await?;
    let _ = message.pin(ctx.http()).await;
    let old = {
        let mut bindings = ctx
            .data()
            .stocks
            .bindings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let old = (bindings.panel_channel, bindings.panel_message);
        bindings.panel_channel = ctx.channel_id().get();
        bindings.panel_message = message.id.get();
        bindings.panel_text = text;
        if bindings.news_channel == 0 {
            bindings.news_channel = ctx.channel_id().get();
        }
        old
    };
    if old.1 != 0 && old.1 != message.id.get() {
        let _ = serenity::ChannelId::new(old.0)
            .delete_message(ctx.http(), serenity::MessageId::new(old.1))
            .await;
    }
    ctx.data().stocks.save_bindings().await;
    ctx.send(
        poise::CreateReply::default()
            .content("시세판을 올렸습니다. 1분마다 바뀐 시세로 고칩니다. 뉴스 채널이 없으면 이 채널에 뉴스도 올립니다 (`/주식뉴스채널`).")
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

#[poise::command(
    slash_command,
    rename = "주식뉴스채널",
    description_localized("ko", "관리자: 증권 뉴스·공시를 이 채널에 올립니다.")
)]
pub async fn stock_news_channel(ctx: Context<'_>) -> Result<(), Error> {
    if !require_manager(ctx).await? {
        return Ok(());
    }
    ctx.data()
        .stocks
        .bindings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .news_channel = ctx.channel_id().get();
    ctx.data().stocks.save_bindings().await;
    reply_embed(
        ctx,
        "이제 이 채널에 증권 뉴스·공시가 올라옵니다.",
        "주식 뉴스",
        serenity::Colour::DARK_GREEN,
        true,
    )
    .await?;
    Ok(())
}

// ------------------------------------------------------------ 틱·중계

/// 5초마다 시장을 돌리고, 새 뉴스를 뉴스 채널에 올리고, 1분마다 시세판을 고친다.
pub async fn run_stock_ticker(ctx: serenity::Context, data: Data) {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    let mut last_panel = Instant::now() - Duration::from_secs(120);
    loop {
        interval.tick().await;
        data.stocks.tick().await;
        let news = data.stocks.take_news().await;
        if !news.is_empty() {
            post_news(&ctx, &data, news).await;
        }
        if last_panel.elapsed() >= Duration::from_secs(60) {
            last_panel = Instant::now();
            refresh_stock_panel(&ctx, &data).await;
        }
    }
}

async fn post_news(ctx: &serenity::Context, data: &Data, news: Vec<NewsItem>) {
    let channel = data
        .stocks
        .bindings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .news_channel;
    if channel == 0 {
        return;
    }
    for (text, colour) in news_messages(&news) {
        let embed = serenity::CreateEmbed::new()
            .title("증권 뉴스")
            .description(text)
            .color(colour);
        if let Err(error) = serenity::ChannelId::new(channel)
            .send_message(&ctx.http, serenity::CreateMessage::new().embed(embed))
            .await
        {
            eprintln!("failed to post stock news: {error:?}");
        }
    }
}

/// 뉴스 채널 메시지: 한 번에 모인 소식을 한 메시지로 묶고, 넘치면 나눈다 (잘라 버리지 않는다).
/// 종목별 변동성 완화장치(2분 거래정지)는 잦아서 빼고, 서킷브레이커 같은 시장 전체 정지는 올린다.
fn news_messages(news: &[NewsItem]) -> Vec<(String, serenity::Colour)> {
    const LIMIT: usize = 3_900;
    let mut chunks: Vec<Vec<&NewsItem>> = Vec::new();
    let mut length = 0;
    for item in news
        .iter()
        .filter(|item| !matches!(item.kind, NewsKind::Halt) || item.code.is_none())
    {
        let line = news_line(item).chars().count() + 1;
        match chunks.last_mut() {
            Some(chunk) if length + line <= LIMIT => {
                chunk.push(item);
                length += line;
            }
            _ => {
                chunks.push(vec![item]);
                length = line;
            }
        }
    }
    chunks
        .into_iter()
        .map(|chunk| {
            let text = chunk
                .iter()
                .map(|item| news_line(item))
                .collect::<Vec<_>>()
                .join("\n")
                .chars()
                .take(LIMIT)
                .collect::<String>();
            let colour = if chunk.iter().any(|item| item.tone < 0)
                && !chunk.iter().any(|item| item.tone > 0)
            {
                serenity::Colour::RED
            } else {
                serenity::Colour::BLUE
            };
            (text, colour)
        })
        .collect()
}

async fn refresh_stock_panel(ctx: &serenity::Context, data: &Data) {
    let (channel, message, previous, retired) = {
        let bindings = data
            .stocks
            .bindings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (
            bindings.panel_channel,
            bindings.panel_message,
            bindings.panel_text.clone(),
            bindings.retired_panel,
        )
    };
    // 웹에서 채널을 바꿨으면 옛 시세판을 지운다.
    if retired.1 != 0 && (channel == 0 || message != 0) {
        let _ = serenity::ChannelId::new(retired.0)
            .delete_message(&ctx.http, serenity::MessageId::new(retired.1))
            .await;
        data.stocks
            .bindings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retired_panel = (0, 0);
        data.stocks.save_bindings().await;
    }
    if channel == 0 {
        return;
    }
    let text = {
        let market = data.stocks.market.read().await;
        market_board_text(&market, market.clock(now_ms()))
    };
    if message == 0 {
        // 웹에서 시세판 채널만 정했다: 새로 올려 고정한다.
        let posted = serenity::ChannelId::new(channel)
            .send_message(
                &ctx.http,
                serenity::CreateMessage::new().embed(make_embed(
                    text.clone(),
                    "증권 시세판",
                    serenity::Colour::DARK_GREEN,
                )),
            )
            .await;
        match posted {
            Ok(posted) => {
                let current = {
                    let mut bindings = data
                        .stocks
                        .bindings
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let current = bindings.panel_channel == channel && bindings.panel_message == 0;
                    if current {
                        bindings.panel_message = posted.id.get();
                        bindings.panel_text = text;
                    }
                    current
                };
                if current {
                    let _ = posted.pin(&ctx.http).await;
                    data.stocks.save_bindings().await;
                } else {
                    // 올리는 사이에 채널이 또 바뀌었다: 방금 올린 것은 지우고 다음 갱신 때 새 채널에 올린다.
                    let _ = posted.delete(&ctx.http).await;
                }
            }
            Err(error) => eprintln!("failed to post stock panel: {error:?}"),
        }
        return;
    }
    if text == previous {
        return;
    }
    let result = serenity::ChannelId::new(channel)
        .edit_message(
            &ctx.http,
            serenity::MessageId::new(message),
            serenity::EditMessage::new().embed(make_embed(
                text.clone(),
                "증권 시세판",
                serenity::Colour::DARK_GREEN,
            )),
        )
        .await;
    match result {
        Ok(_) => {
            data.stocks
                .bindings
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .panel_text = text;
        }
        Err(error) => eprintln!("failed to refresh stock panel: {error:?}"),
    }
}

// ------------------------------------------------------------ 차트 이미지

/// 캔들 차트 PNG (위: 가격, 아래: 거래량). 봉이 두 개 미만이면 None.
pub fn render_candle_chart(title: &str, candles: &[Candle], minute: bool) -> Option<Vec<u8>> {
    if candles.len() < 2 {
        return None;
    }
    const WIDTH: u32 = 1200;
    const HEIGHT: u32 = 680;
    const LEFT: i32 = 30;
    const RIGHT: i32 = 130;
    const TOP: i32 = 90;
    const PRICE_BOTTOM: i32 = 500;
    const VOLUME_TOP: i32 = 520;
    const VOLUME_BOTTOM: i32 = 640;
    let mut image = RgbImage::from_pixel(WIDTH, HEIGHT, image_color("#0f1419"));
    let font = FontArc::try_from_slice(include_bytes!("../../MalangmalangR.ttf")).ok()?;
    let text = image_color("#e8ecf2");
    let muted = image_color("#8b95a5");
    let grid = image_color("#1f2630");
    let up = image_color("#f0444f");
    let down = image_color("#3d7cf0");
    draw_lb_text(&mut image, &font, 34.0, LEFT, 24, title, text);
    let high = candles.iter().map(|candle| candle.h).max()?;
    let low = candles.iter().map(|candle| candle.l).min()?;
    let span = (high - low).max(1) as f64;
    let top_price = high as f64 + span * 0.05;
    let bottom_price = (low as f64 - span * 0.05).max(0.0);
    let price_range = (top_price - bottom_price).max(1.0);
    let y_of = |price: i64| -> i32 {
        PRICE_BOTTOM
            - ((price as f64 - bottom_price) / price_range * (PRICE_BOTTOM - TOP) as f64) as i32
    };
    for step in 0..=4 {
        let price = bottom_price + price_range * step as f64 / 4.0;
        let y = y_of(price as i64);
        fill_horizontal_line(&mut image, LEFT, WIDTH as i32 - RIGHT, y, grid);
        draw_lb_text(
            &mut image,
            &font,
            20.0,
            WIDTH as i32 - RIGHT + 10,
            y - 12,
            format_amount(price as i64),
            muted,
        );
    }
    let plot_width = (WIDTH as i32 - RIGHT - LEFT) as f64;
    let slot = plot_width / candles.len() as f64;
    let body = ((slot * 0.7) as u32).max(1);
    let max_volume = candles
        .iter()
        .map(|candle| candle.v)
        .max()
        .unwrap_or(1)
        .max(1) as f64;
    for (index, candle) in candles.iter().enumerate() {
        let x = LEFT + (slot * index as f64 + slot * 0.15) as i32;
        let center = x + body as i32 / 2;
        let colour = if candle.c >= candle.o { up } else { down };
        let (wick_top, wick_bottom) = (y_of(candle.h), y_of(candle.l));
        fill_rect(
            &mut image,
            center,
            wick_top,
            1,
            (wick_bottom - wick_top).max(1) as u32,
            colour,
        );
        let (body_top, body_bottom) = (y_of(candle.o.max(candle.c)), y_of(candle.o.min(candle.c)));
        fill_rect(
            &mut image,
            x,
            body_top,
            body,
            (body_bottom - body_top).max(1) as u32,
            colour,
        );
        let volume_height =
            (candle.v as f64 / max_volume * (VOLUME_BOTTOM - VOLUME_TOP) as f64) as i32;
        fill_rect(
            &mut image,
            x,
            VOLUME_BOTTOM - volume_height,
            body,
            volume_height.max(1) as u32,
            blend_rgb(colour, image_color("#0f1419"), 0.5, 0.5),
        );
    }
    // 시간 표시: 처음·가운데·끝.
    for index in [0, candles.len() / 2, candles.len() - 1] {
        let x = LEFT + (slot * index as f64) as i32;
        let label = if minute {
            kst_clock(candles[index].t)
                .split(' ')
                .nth(1)
                .unwrap_or_default()
                .to_string()
        } else {
            kst_clock(candles[index].t)
        };
        draw_lb_text(&mut image, &font, 20.0, x, VOLUME_BOTTOM + 8, label, muted);
    }
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .ok()?;
    Some(bytes.into_inner())
}

#[cfg(test)]
mod tests {
    use super::{market_stats_text, news_messages};
    use mafia_remake::stocks::{NewsItem, NewsKind, StockMarket, StockRules};

    fn news(id: u64, kind: NewsKind, code: Option<&str>, headline: String) -> NewsItem {
        NewsItem {
            id,
            at: 0,
            kind,
            code: code.map(str::to_string),
            headline,
            body: String::new(),
            tone: 0,
        }
    }

    #[test]
    fn news_channel_skips_vi_halts_and_splits_long_batches() {
        let batch = vec![
            news(
                1,
                NewsKind::Halt,
                Some("100010"),
                "변동성 완화장치".to_string(),
            ),
            news(2, NewsKind::Halt, None, "서킷브레이커".to_string()),
            news(
                3,
                NewsKind::Disclosure,
                Some("700010"),
                "회사 설립".to_string(),
            ),
        ];
        let messages = news_messages(&batch);
        assert_eq!(messages.len(), 1);
        let text = &messages[0].0;
        assert!(!text.contains("변동성 완화장치"), "종목별 VI는 뺀다");
        assert!(text.contains("서킷브레이커") && text.contains("회사 설립"));

        let long = (0..100)
            .map(|id| news(id, NewsKind::News, None, "가".repeat(80)))
            .collect::<Vec<_>>();
        let messages = news_messages(&long);
        assert!(messages.len() > 1, "넘치면 나눈다");
        assert!(
            messages
                .iter()
                .all(|(text, _)| text.chars().count() <= 3_900)
        );
        assert_eq!(
            messages
                .iter()
                .map(|(text, _)| text.lines().count())
                .sum::<usize>(),
            100,
            "잘라 버리지 않는다"
        );
    }

    #[test]
    fn rumors_are_tagged_once() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let draft = mafia_remake::stocks::company_news(
            "한빛바이오",
            mafia_remake::stocks::Sector::Bio,
            1.0,
            &mut rng,
        );
        assert!(draft.rumor.is_some());
        let line = super::news_line(&news(1, NewsKind::Rumor, Some("100050"), draft.headline));
        assert_eq!(line.matches("[루머]").count(), 1, "{line}");
    }

    #[test]
    fn market_stats_show_the_coins_the_market_made() {
        let mut market = StockMarket::new(0, &StockRules::default());
        market.stats.lp_bought = 1_000;
        market.stats.lp_sold = 1_500;
        market.stats.dividends = 200;
        market.stats.ipo_burned = 100;
        market.stats.company_earnings = -300;
        let text = market_stats_text(&market);
        assert!(text.contains("시장이 만든 코인: +600원"), "{text}");
        assert!(text.contains("(회사 현금): -300원"), "{text}");
    }
}

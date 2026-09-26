// stock_web.rs — 증권 사이트(마피아증권): 개인 링크 페이지(/stocks/<토큰>)와 API(/stocks/api/...).
// 카지노와 따로 떨어진 사이트다 (프론트엔드는 stocks-web/). 개인 링크 세션만 카지노와 같은 표를 쓴다.
// 코인이 오가는 작업은 모두 StockHub(transact)를 거치므로 Discord 명령과 같은 규칙·장부를 쓴다.

use crate::casino_hub::{CasinoSession, SharedHub};
use crate::casino_web::{
    asset_response, bearer_token, content_type_for, error_response, read_static_file,
};
use crate::stock_hub::{Need, SharedStocks, StockHub, now_ms};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path as AxumPath, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use mafia_remake::stocks::{
    AccountView, Candle, CompanyDetail, CompanyStatus, MarketStats, MarketSummary, NewsItem,
    Sector, Side, StockMarket, duration_text, format_amount,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tower_http::compression::CompressionLayer;

include!(concat!(env!("OUT_DIR"), "/stocks_static.rs"));

#[derive(Clone)]
pub struct StocksWebState {
    /// 개인 링크 세션 (카지노와 같은 표. 토큰은 누구인지 확인할 뿐이다).
    pub sessions: SharedHub,
    pub stocks: SharedStocks,
    /// 로그 채널 기록 (웹 관리 작업). 개발 서버에는 없다.
    pub audit: Option<std::sync::Arc<crate::audit_log::AuditLog>>,
    /// 내장된 페이지 대신 디스크의 stocks-web/dist를 쓸 때 (STOCKS_STATIC_DIR).
    pub static_dir: Option<String>,
}

pub fn stocks_router(state: StocksWebState) -> Router {
    let api = Router::new()
        .route("/stocks/api/state", get(state_handler))
        .route("/stocks/api/candles", get(candles_handler))
        .route("/stocks/api/order", post(order_handler))
        .route("/stocks/api/cancel", post(cancel_handler))
        .route("/stocks/api/subscribe", post(subscribe_handler))
        .route("/stocks/api/unsubscribe", post(unsubscribe_handler))
        .route("/stocks/api/company", post(company_handler))
        .route("/stocks/api/ranking", get(ranking_handler))
        .route("/stocks/api/admin", post(admin_handler))
        .layer(CompressionLayer::new());
    Router::new()
        .merge(api)
        .route("/stocks", get(stocks_index))
        .route("/stocks/", get(stocks_index))
        .route("/stocks/{*path}", get(stocks_asset))
        .with_state(state)
}

/// Discord에서 받은 개인 링크.
pub fn stocks_link(base_url: &str, token: &str) -> String {
    format!("{}/stocks/{token}", base_url.trim_end_matches('/'))
}

fn session_of(state: &StocksWebState, headers: &HeaderMap) -> Option<CasinoSession> {
    bearer_token(headers).and_then(|token| state.sessions.session(&token))
}

fn expired() -> Response {
    error_response(
        StatusCode::UNAUTHORIZED,
        "UNAUTHORIZED",
        "링크가 만료됐습니다. Discord에서 `/주식 증권`으로 새 링크를 받으세요.",
    )
}

// ------------------------------------------------------------ 페이지

async fn stocks_index(State(state): State<StocksWebState>) -> Response {
    serve_asset(&state, "/index.html")
}

async fn stocks_asset(
    State(state): State<StocksWebState>,
    AxumPath(path): AxumPath<String>,
) -> Response {
    if path.starts_with("assets/") || path.contains('.') {
        return try_serve_asset(&state, &format!("/{path}"))
            .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response());
    }
    // /stocks/<토큰> 같은 페이지 경로는 index.html을 준다.
    serve_asset(&state, "/index.html")
}

fn serve_asset(state: &StocksWebState, asset_path: &str) -> Response {
    try_serve_asset(state, asset_path).unwrap_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            "stocks-web/dist가 없습니다. `cd stocks-web && npm ci && npm run build` 후 다시 빌드하세요.",
        )
            .into_response()
    })
}

fn try_serve_asset(state: &StocksWebState, asset_path: &str) -> Option<Response> {
    if let Some(dir) = state.static_dir.as_deref()
        && let Some(body) = read_static_file(Path::new(dir), asset_path.trim_start_matches('/'))
    {
        return Some(asset_response(
            asset_path,
            content_type_for(asset_path),
            Bytes::from(body),
            None,
        ));
    }
    STOCKS_ASSETS
        .iter()
        .find(|asset| asset.path == asset_path)
        .map(|asset| {
            asset_response(
                asset.path,
                asset.content_type,
                Bytes::from_static(asset.body),
                None,
            )
        })
}

// ------------------------------------------------------------ 상태

#[derive(Debug, Serialize)]
pub struct SectorView {
    pub key: Sector,
    pub name: &'static str,
}

#[derive(Debug, Serialize)]
pub struct StockRulesView {
    pub enabled: bool,
    pub day_ms: i64,
    pub fee_ppm: i64,
    pub tax_ppm: i64,
    pub limit_bp: i64,
    pub found_min_capital: i64,
    pub found_fee_bp: i64,
    pub ipo_fee_bp: i64,
    pub listing_min_equity: i64,
    pub lockup_days: i64,
    pub max_companies: i64,
    pub sectors: Vec<SectorView>,
}

#[derive(Debug, Serialize)]
pub struct StockMe {
    /// JS 숫자로는 정밀도가 모자라 문자열로 보낸다.
    pub user_id: String,
    pub name: String,
    /// 주식에 쓸 수 있는 코인 (진행 중인 마피아 판에 걸린 배팅은 뺀다).
    pub coins: i64,
    /// 사이트 관리자 (관리자가 `/주식 증권`으로 받은 링크).
    pub admin: bool,
}

/// 진행 중인 유상증자 (주주배정).
#[derive(Debug, Serialize)]
pub struct RightsOfferingView {
    pub code: String,
    pub name: String,
    pub price: i64,
    pub shares: i64,
    pub until: i64,
    /// 지금까지 인수된 주식 수.
    pub exercised: i64,
}

/// 관리자 화면.
#[derive(Debug, Serialize)]
pub struct AdminView {
    pub stats: MarketStats,
    /// 시세판·뉴스 채널 (JS 정밀도 때문에 문자열).
    pub panel_channel: String,
    pub news_channel: String,
    /// 관리자가 거래정지한 종목 코드.
    pub halted: Vec<String>,
}

/// 청약 중인 공모.
#[derive(Debug, Serialize)]
pub struct OfferingView {
    pub code: String,
    pub name: String,
    pub sector: &'static str,
    pub player: bool,
    pub founder_name: Option<String>,
    pub price: i64,
    pub shares: i64,
    pub opens_at: i64,
    pub closes_at: i64,
    pub min_fill_bp: i64,
    /// 지금까지 들어온 청약 수량.
    pub requested: i64,
}

#[derive(Debug, Serialize)]
pub struct StockState {
    /// 시장 시각 (관리자가 게임일을 넘기면 실제 시각보다 앞선다).
    pub server_time: i64,
    /// 시장 시계가 실제 시각보다 앞선 시간 (ms).
    pub time_shift_ms: i64,
    pub me: StockMe,
    pub market: MarketSummary,
    pub selected: Option<CompanyDetail>,
    /// 고른 종목의 최근 뉴스·공시.
    pub selected_news: Vec<NewsItem>,
    pub account: AccountView,
    /// 시장 전체의 최근 뉴스 (새것부터).
    pub news: Vec<NewsItem>,
    pub offerings: Vec<OfferingView>,
    /// 내가 대표인 회사 (상세).
    pub my_companies: Vec<CompanyDetail>,
    pub rights_offerings: Vec<RightsOfferingView>,
    /// 관리자에게만.
    pub admin: Option<AdminView>,
    pub rules: StockRulesView,
}

#[derive(Debug, Deserialize)]
pub struct StateQuery {
    #[serde(default)]
    pub code: Option<String>,
}

fn invalid(message: String) -> Response {
    error_response(StatusCode::BAD_REQUEST, "INVALID", message)
}

async fn build_state(stocks: &StockHub, user: u64, name: &str, code: Option<&str>) -> StockState {
    let (rules, _) = stocks.rules().await;
    let coins = stocks.coins_of(user).await;
    let market = stocks.market.read().await;
    let now = market.clock(now_ms());
    let summary = market.market_summary(now, &rules);
    let selected_code = code
        .and_then(|query| market.find_company(query))
        .map(|company| company.code.clone())
        .or_else(|| {
            summary
                .companies
                .first()
                .map(|company| company.code.clone())
        });
    let selected_news = selected_code
        .as_deref()
        .map(|code| {
            market
                .news
                .iter()
                .rev()
                .filter(|item| item.code.as_deref() == Some(code))
                .take(20)
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let offerings = market
        .companies
        .values()
        .filter_map(|company| match &company.status {
            CompanyStatus::Subscription(offering) => Some(OfferingView {
                code: company.code.clone(),
                name: company.name.clone(),
                sector: company.sector.name(),
                player: company.is_player(),
                founder_name: market.summary_of(company, now).founder_name,
                price: offering.price,
                shares: offering.shares,
                opens_at: offering.opens_at,
                closes_at: offering.closes_at,
                min_fill_bp: offering.min_fill_bp,
                requested: market
                    .subscriptions
                    .iter()
                    .filter(|subscription| subscription.code == company.code)
                    .map(|subscription| subscription.qty)
                    .sum(),
            }),
            _ => None,
        })
        .collect();
    let my_companies = market
        .companies
        .values()
        .filter(|company| company.founder() == Some(user) && company.status.is_active())
        .filter_map(|company| market.company_detail(&company.code, now, &rules))
        .collect();
    let rights_offerings = market
        .companies
        .values()
        .filter_map(|company| {
            let rights = company.rights.as_ref()?;
            Some(RightsOfferingView {
                code: company.code.clone(),
                name: company.name.clone(),
                price: rights.price,
                shares: rights.shares,
                until: rights.until,
                exercised: rights.exercised.values().sum(),
            })
        })
        .collect();
    let is_admin = stocks.is_web_admin(user);
    let admin = is_admin.then(|| {
        let bindings = stocks
            .bindings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        AdminView {
            stats: market.stats.clone(),
            panel_channel: bindings.panel_channel.to_string(),
            news_channel: bindings.news_channel.to_string(),
            halted: market
                .companies
                .values()
                .filter(|company| company.admin_halt && company.status.is_active())
                .map(|company| company.code.clone())
                .collect(),
        }
    });
    StockState {
        server_time: now,
        time_shift_ms: market.time_shift_ms,
        me: StockMe {
            user_id: user.to_string(),
            name: name.to_string(),
            coins,
            admin: is_admin,
        },
        selected: selected_code.and_then(|code| market.company_detail(&code, now, &rules)),
        selected_news,
        account: market.account_view(user, now),
        news: market.news.iter().rev().take(40).cloned().collect(),
        offerings,
        my_companies,
        rights_offerings,
        admin,
        market: summary,
        rules: StockRulesView {
            enabled: rules.enabled,
            day_ms: rules.day_ms(),
            fee_ppm: rules.fee_ppm,
            tax_ppm: rules.tax_ppm,
            limit_bp: rules.limit_bp,
            found_min_capital: rules.found_min_capital,
            found_fee_bp: rules.found_fee_bp,
            ipo_fee_bp: rules.ipo_fee_bp,
            listing_min_equity: rules.listing_min_equity,
            lockup_days: rules.lockup_days,
            max_companies: rules.max_companies,
            sectors: Sector::ALL
                .iter()
                .map(|sector| SectorView {
                    key: *sector,
                    name: sector.name(),
                })
                .collect(),
        },
    }
}

async fn state_handler(
    State(state): State<StocksWebState>,
    headers: HeaderMap,
    Query(query): Query<StateQuery>,
) -> Response {
    let Some(session) = session_of(&state, &headers) else {
        return expired();
    };
    let stocks = &*state.stocks;
    Json(
        build_state(
            stocks,
            session.user_id,
            &session.name,
            query.code.as_deref(),
        )
        .await,
    )
    .into_response()
}

// ------------------------------------------------------------ 봉

#[derive(Debug, Deserialize)]
pub struct CandleQuery {
    /// 종목 코드, 또는 종합지수는 `INDEX` (100배 정수).
    pub code: String,
    /// `minute`(실제 1분) 또는 `day`(게임 하루).
    #[serde(default)]
    pub range: Option<String>,
}

#[derive(Debug, Serialize)]
struct CandleResponse {
    code: String,
    range: &'static str,
    candles: Vec<Candle>,
}

async fn candles_handler(
    State(state): State<StocksWebState>,
    headers: HeaderMap,
    Query(query): Query<CandleQuery>,
) -> Response {
    if session_of(&state, &headers).is_none() {
        return expired();
    }
    let stocks = &*state.stocks;
    let market = stocks.market.read().await;
    let series = if query.code == "INDEX" {
        Some(&market.candles.index)
    } else {
        market.candles.series.get(&query.code)
    };
    let day = query.range.as_deref() == Some("day");
    let candles = series
        .map(|series| {
            if day {
                series.day.iter().copied().collect()
            } else {
                series.minute.iter().copied().collect()
            }
        })
        .unwrap_or_default();
    Json(CandleResponse {
        code: query.code,
        range: if day { "day" } else { "minute" },
        candles,
    })
    .into_response()
}

// ------------------------------------------------------------ 주문·청약

/// 작업 결과와 새 화면 상태 (한 번 더 불러오지 않아도 되게).
#[derive(Debug, Serialize)]
struct ActionResponse<T: Serialize> {
    result: T,
    message: String,
    state: StockState,
}

async fn respond<T: Serialize>(
    stocks: &StockHub,
    user: u64,
    name: &str,
    code: Option<&str>,
    result: T,
    message: String,
) -> Response {
    let state = build_state(stocks, user, name, code).await;
    Json(ActionResponse {
        result,
        message,
        state,
    })
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct OrderBody {
    pub code: String,
    pub side: Side,
    pub qty: i64,
    /// 지정가 (없으면 시장가).
    #[serde(default)]
    pub price: Option<i64>,
}

async fn order_handler(
    State(state): State<StocksWebState>,
    headers: HeaderMap,
    Json(body): Json<OrderBody>,
) -> Response {
    let Some(session) = session_of(&state, &headers) else {
        return expired();
    };
    let stocks = &*state.stocks;
    let result = stocks
        .order(
            session.user_id,
            &session.name,
            &body.code,
            body.side,
            body.qty,
            body.price,
        )
        .await;
    match result {
        Ok(result) => {
            let side = body.side.label();
            let resting = result.resting.map(|_| result.resting_qty);
            let limit = format_amount(result.limit.unwrap_or_default());
            let message = match (result.filled, resting) {
                (0, Some(qty)) => format!("{side} {qty}주를 {limit}에 호가에 걸었습니다"),
                (0, None) => format!("{side} 주문이 체결되지 않았습니다"),
                (filled, None) => format!(
                    "{filled}주 {side} 체결 (평균 {})",
                    format_amount(result.avg_price)
                ),
                (filled, Some(qty)) => format!(
                    "{filled}주 {side} 체결 (평균 {}), 남은 {qty}주는 호가에 걸었습니다",
                    format_amount(result.avg_price)
                ),
            };
            respond(
                stocks,
                session.user_id,
                &session.name,
                Some(&body.code),
                result,
                message,
            )
            .await
        }
        Err(message) => invalid(message),
    }
}

#[derive(Debug, Deserialize)]
pub struct CancelBody {
    pub order_id: u64,
    /// 화면에서 보고 있던 종목 (새 상태에 그대로 싣는다).
    #[serde(default)]
    pub code: Option<String>,
}

async fn cancel_handler(
    State(state): State<StocksWebState>,
    headers: HeaderMap,
    Json(body): Json<CancelBody>,
) -> Response {
    let Some(session) = session_of(&state, &headers) else {
        return expired();
    };
    let stocks = &*state.stocks;
    match stocks.cancel(session.user_id, body.order_id).await {
        Ok(()) => {
            respond(
                stocks,
                session.user_id,
                &session.name,
                body.code.as_deref(),
                (),
                "주문을 취소했습니다".to_string(),
            )
            .await
        }
        Err(message) => invalid(message),
    }
}

#[derive(Debug, Deserialize)]
pub struct SubscribeBody {
    pub code: String,
    #[serde(default)]
    pub qty: i64,
}

async fn subscribe_handler(
    State(state): State<StocksWebState>,
    headers: HeaderMap,
    Json(body): Json<SubscribeBody>,
) -> Response {
    let Some(session) = session_of(&state, &headers) else {
        return expired();
    };
    let stocks = &*state.stocks;
    let (user, name) = (session.user_id, session.name.as_str());
    let (code, qty) = (body.code.as_str(), body.qty);
    let result = stocks
        .transact(
            user,
            |market, _| market.subscription_cost(code, qty).map(Need::Exactly),
            |market, _, _, now| market.subscribe(user, name, code, qty, now),
        )
        .await;
    match result {
        Ok(deposit) => {
            let message = format!("{qty}주 청약 (증거금 {})", format_amount(deposit));
            respond(stocks, user, name, Some(code), deposit, message).await
        }
        Err(message) => invalid(message),
    }
}

async fn unsubscribe_handler(
    State(state): State<StocksWebState>,
    headers: HeaderMap,
    Json(body): Json<SubscribeBody>,
) -> Response {
    let Some(session) = session_of(&state, &headers) else {
        return expired();
    };
    let stocks = &*state.stocks;
    let (user, name, code) = (session.user_id, session.name.as_str(), body.code.as_str());
    let result = stocks
        .transact(
            user,
            |_, _| Ok(Need::Nothing),
            |market, _, _, _| market.cancel_subscription(user, code),
        )
        .await;
    match result {
        Ok(refund) => {
            let message = format!("청약을 취소했습니다 (환불 {})", format_amount(refund));
            respond(stocks, user, name, Some(code), refund, message).await
        }
        Err(message) => invalid(message),
    }
}

// ------------------------------------------------------------ 순위

#[derive(Debug, Clone, Serialize)]
struct RankingRow {
    rank: usize,
    name: String,
    /// 주식 평가액 + 묶인 코인.
    value: i64,
    /// 평가 손익 + 실현 손익.
    profit: i64,
    me: bool,
}

#[derive(Debug, Serialize)]
struct RankingResponse {
    rows: Vec<RankingRow>,
    /// 50위 밖이어도 내 순위.
    mine: Option<RankingRow>,
    total: usize,
}

async fn ranking_handler(State(state): State<StocksWebState>, headers: HeaderMap) -> Response {
    let Some(session) = session_of(&state, &headers) else {
        return expired();
    };
    let market = state.stocks.market.read().await;
    let now = market.clock(now_ms());
    let rows = market
        .ranking(now)
        .into_iter()
        .enumerate()
        .map(|(index, entry)| RankingRow {
            rank: index + 1,
            name: entry.name,
            value: entry.value,
            profit: entry.profit,
            me: entry.user == session.user_id,
        })
        .collect::<Vec<_>>();
    let mine = rows.iter().find(|row| row.me).cloned();
    let total = rows.len();
    Json(RankingResponse {
        rows: rows.into_iter().take(50).collect(),
        mine,
        total,
    })
    .into_response()
}

// ------------------------------------------------------------ 관리자

/// 관리자 작업 (거래정지·재개, 게임일 넘기기, 시세판·뉴스 채널).
#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AdminAction {
    Halt {
        code: String,
    },
    Resume {
        code: String,
    },
    Skip {
        days: i64,
    },
    Channels {
        /// 채널 ID (문자열, 빈 값이면 끊음, 없으면 그대로).
        #[serde(default)]
        panel_channel: Option<String>,
        #[serde(default)]
        news_channel: Option<String>,
    },
}

/// 채널 ID 칸: 없으면 그대로(None), 비었으면 끊기(0).
fn channel_id(text: Option<String>) -> Result<Option<u64>, String> {
    match text {
        None => Ok(None),
        Some(text) if text.trim().is_empty() => Ok(Some(0)),
        Some(text) => text
            .trim()
            .parse::<u64>()
            .map(Some)
            .map_err(|_| "채널 ID는 숫자여야 합니다.".to_string()),
    }
}

async fn admin_handler(
    State(state): State<StocksWebState>,
    headers: HeaderMap,
    Json(action): Json<AdminAction>,
) -> Response {
    let Some(session) = session_of(&state, &headers) else {
        return expired();
    };
    let stocks = &*state.stocks;
    if !stocks.is_web_admin(session.user_id) {
        return error_response(
            StatusCode::FORBIDDEN,
            "FORBIDDEN",
            "관리자만 쓸 수 있습니다. 관리자 역할로 Discord에서 `/주식 증권` 링크를 다시 받으세요.",
        );
    }
    let outcome: Result<(String, Option<String>), String> = match action {
        AdminAction::Halt { code } => stocks
            .transact(
                0,
                |_, _| Ok(Need::Nothing),
                |market, _, _, now| market.set_admin_halt(&code, true, now),
            )
            .await
            .map(|name| (format!("{name} 거래정지"), Some(code))),
        AdminAction::Resume { code } => stocks
            .transact(
                0,
                |_, _| Ok(Need::Nothing),
                |market, _, _, now| market.set_admin_halt(&code, false, now),
            )
            .await
            .map(|name| (format!("{name} 거래재개"), Some(code))),
        AdminAction::Skip { days } => stocks.skip_game_days(days).await.map(|(_, skipped)| {
            (
                format!(
                    "게임일을 {days}일 넘겼습니다 (시장 시계 {} 앞당김)",
                    duration_text(skipped)
                ),
                None,
            )
        }),
        AdminAction::Channels {
            panel_channel,
            news_channel,
        } => match (channel_id(panel_channel), channel_id(news_channel)) {
            (Ok(panel), Ok(news)) => {
                let moved = stocks.set_channels(panel, news).await;
                let note = match (moved, panel) {
                    (false, _) => "",
                    (true, Some(0)) => " 시세판 연결을 끊었습니다.",
                    (true, _) => " 시세판은 1분 안에 새 채널에 올라갑니다.",
                };
                Ok((format!("채널 연결을 저장했습니다.{note}"), None))
            }
            (Err(message), _) | (_, Err(message)) => Err(message),
        },
    };
    match outcome {
        Ok((message, code)) => {
            if let Some(audit) = &state.audit {
                audit.push(
                    crate::audit_log::STOCKS,
                    format!("🛠️ {} (웹 관리) {message}", session.name),
                );
            }
            respond(
                stocks,
                session.user_id,
                &session.name,
                code.as_deref(),
                (),
                message,
            )
            .await
        }
        Err(message) => invalid(message),
    }
}

// ------------------------------------------------------------ 회사

/// 회사 작업. 설립·신주인수 말고는 내가 대표인 회사(`code`, 없으면 첫 회사)에 한다.
#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum CompanyAction {
    Found {
        name: String,
        sector: Sector,
        capital: i64,
    },
    Ipo {
        price: i64,
        shares: i64,
    },
    Dividend {
        per_share: i64,
    },
    Rights {
        shares: i64,
        price: i64,
    },
    Exercise {
        code: String,
        qty: i64,
    },
    Buyback {
        budget: i64,
    },
    Risk {
        risk: u8,
    },
    Describe {
        text: String,
    },
    /// 확인용으로 회사 이름을 그대로 적는다.
    Dissolve {
        confirm: String,
    },
}

#[derive(Debug, Deserialize)]
struct CompanyTarget {
    #[serde(default)]
    code: Option<String>,
}

async fn company_handler(
    State(state): State<StocksWebState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let Some(session) = session_of(&state, &headers) else {
        return expired();
    };
    let stocks = &*state.stocks;
    let target = serde_json::from_value::<CompanyTarget>(body.clone())
        .ok()
        .and_then(|target| target.code);
    let action = match serde_json::from_value::<CompanyAction>(body) {
        Ok(action) => action,
        Err(_) => return invalid("알 수 없는 회사 작업입니다.".to_string()),
    };
    let (user, name) = (session.user_id, session.name.as_str());
    let outcome = run_company_action(stocks, user, name, target, action).await;
    match outcome {
        Ok((message, code)) => respond(stocks, user, name, code.as_deref(), (), message).await,
        Err(message) => invalid(message),
    }
}

/// 회사 작업을 하고 (안내 문구, 새 상태에서 보여 줄 종목)을 돌려준다.
async fn run_company_action(
    stocks: &StockHub,
    user: u64,
    name: &str,
    target: Option<String>,
    action: CompanyAction,
) -> Result<(String, Option<String>), String> {
    match action {
        CompanyAction::Found {
            name: company_name,
            sector,
            capital,
        } => {
            let code = stocks
                .transact(
                    user,
                    |_, rules| Ok(Need::Exactly(StockMarket::founding_cost(capital, rules))),
                    |market, _, rules, now| {
                        market.found_company(user, name, &company_name, sector, capital, now, rules)
                    },
                )
                .await?;
            Ok((format!("{company_name} 설립 완료"), Some(code)))
        }
        CompanyAction::Exercise { code, qty } => {
            let paid = stocks
                .transact(
                    user,
                    |market, _| market.rights_cost(user, &code, qty).map(Need::Exactly),
                    |market, _, _, now| market.exercise_rights(user, name, &code, qty, now),
                )
                .await?;
            Ok((
                format!("신주 {qty}주를 인수했습니다 (납입 {})", format_amount(paid)),
                Some(code),
            ))
        }
        action => {
            let (code, company_name) = {
                let market = stocks.market.read().await;
                let mut owned = market.companies.values().filter(|company| {
                    company.founder() == Some(user) && company.status.is_active()
                });
                let found = match target.as_deref() {
                    Some(code) => owned.find(|company| company.code == code),
                    None => owned.next(),
                };
                let company = found.ok_or_else(|| "대표로 있는 회사가 없습니다.".to_string())?;
                (company.code.clone(), company.name.clone())
            };
            let code = code.as_str();
            let message = match action {
                CompanyAction::Ipo { price, shares } => stocks
                    .transact(
                        user,
                        |_, _| Ok(Need::Nothing),
                        |market, _, rules, now| {
                            market.start_ipo(user, code, price, shares, now, rules)
                        },
                    )
                    .await
                    .map(|offering| {
                        format!(
                            "공모 청약을 열었습니다: {}주, 공모가 {}",
                            offering.shares,
                            format_amount(offering.price)
                        )
                    })?,
                CompanyAction::Dividend { per_share } => stocks
                    .transact(
                        user,
                        |_, _| Ok(Need::Nothing),
                        |market, _, rules, now| {
                            market.declare_dividend(user, code, per_share, now, rules)
                        },
                    )
                    .await
                    .map(|total| format!("배당을 결정했습니다 (총 {})", format_amount(total)))?,
                CompanyAction::Rights { shares, price } => stocks
                    .transact(
                        user,
                        |_, _| Ok(Need::Nothing),
                        |market, _, rules, now| {
                            market.start_rights(user, code, shares, price, now, rules)
                        },
                    )
                    .await
                    .map(|()| "유상증자를 결정했습니다".to_string())?,
                CompanyAction::Buyback { budget } => stocks
                    .transact(
                        user,
                        |_, _| Ok(Need::Nothing),
                        |market, _, rules, now| {
                            market.start_buyback(user, code, budget, now, rules)
                        },
                    )
                    .await
                    .map(|()| "자사주 매입을 시작했습니다".to_string())?,
                CompanyAction::Risk { risk } => stocks
                    .transact(
                        user,
                        |_, _| Ok(Need::Nothing),
                        |market, _, _, now| market.set_risk(user, code, risk, now),
                    )
                    .await
                    .map(|()| format!("사업 위험도를 {risk}(으)로 바꿨습니다"))?,
                CompanyAction::Describe { text } => stocks
                    .transact(
                        user,
                        |_, _| Ok(Need::Nothing),
                        |market, _, _, _| market.set_description(user, code, &text),
                    )
                    .await
                    .map(|()| "회사 소개를 바꿨습니다".to_string())?,
                CompanyAction::Dissolve { confirm } => {
                    if confirm.trim() != company_name {
                        return Err(format!(
                            "해산하려면 회사 이름 '{company_name}'을(를) 그대로 적으세요."
                        ));
                    }
                    stocks
                        .transact(
                            user,
                            |_, _| Ok(Need::Nothing),
                            |market, _, rules, now| market.dissolve(user, code, now, rules),
                        )
                        .await
                        .map(|()| "해산을 결정했습니다".to_string())?
                }
                CompanyAction::Found { .. } | CompanyAction::Exercise { .. } => {
                    return Err("알 수 없는 회사 작업입니다.".to_string());
                }
            };
            Ok((message, Some(code.to_string())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{StocksWebState, stocks_link, stocks_router};
    use crate::casino_hub::CasinoHub;
    use crate::stock_hub::StockHub;
    use std::sync::Arc;

    #[tokio::test]
    async fn web_orders_move_coins_through_the_stock_hub() {
        let dir = std::env::temp_dir().join(format!(
            "mafia-stock-web-{}-{}",
            std::process::id(),
            mafia_remake::atomic_file::next_seq()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut stats = mafia_remake::stats::StatsFile::default();
        crate::stats::refund_coins(&mut stats, 7, "투자자", 5_000_000);
        let stats = Arc::new(tokio::sync::RwLock::new(stats));
        let stats_path = Arc::new(dir.join("stats.json"));
        let hub = Arc::new(
            CasinoHub::load(dir.join("casino.json"), stats.clone(), stats_path.clone()).unwrap(),
        );
        let stocks =
            Arc::new(StockHub::load(dir.join("stocks.json"), stats.clone(), stats_path).unwrap());
        let token = hub.issue_session(7, "투자자".to_string());
        let router = stocks_router(StocksWebState {
            sessions: hub,
            stocks: stocks.clone(),
            audit: None,
            static_dir: None,
        });
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        let base = format!("http://{address}/stocks/api");
        let client = reqwest::Client::new();

        // 개인 링크는 증권 사이트 페이지(index.html)를 연다.
        let page = client
            .get(stocks_link(&format!("http://{address}/"), &token))
            .send()
            .await
            .unwrap();
        assert_eq!(page.status(), 200);
        assert!(
            page.headers()["content-type"]
                .to_str()
                .unwrap()
                .starts_with("text/html")
        );

        let state: serde_json::Value = client
            .get(format!("{base}/state"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(state["me"]["user_id"], "7");
        assert_eq!(state["me"]["coins"], 5_000_000);
        let code = state["selected"]["summary"]["code"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(
            state["selected"]["book"]["asks"].as_array().unwrap().len(),
            10
        );

        let bought = client
            .post(format!("{base}/order"))
            .bearer_auth(&token)
            .json(&serde_json::json!({ "code": code, "side": "buy", "qty": 3 }))
            .send()
            .await
            .unwrap();
        assert_eq!(bought.status(), 200);
        let bought: serde_json::Value = bought.json().await.unwrap();
        assert_eq!(bought["result"]["filled"], 3);
        let spent = bought["result"]["notional"].as_i64().unwrap()
            + bought["result"]["fee"].as_i64().unwrap();
        assert_eq!(bought["state"]["me"]["coins"], 5_000_000 - spent);
        assert_eq!(bought["state"]["account"]["positions"][0]["qty"], 3);
        assert_eq!(
            stats.read().await.users.get("7").unwrap().coins,
            5_000_000 - spent
        );

        // 잘못된 요청은 400과 한국어 안내, 세션이 없으면 401.
        let rejected = client
            .post(format!("{base}/order"))
            .bearer_auth(&token)
            .json(&serde_json::json!({ "code": code, "side": "sell", "qty": 99 }))
            .send()
            .await
            .unwrap();
        assert_eq!(rejected.status(), 400);
        let rejected: serde_json::Value = rejected.json().await.unwrap();
        assert!(!rejected["error"]["message"].as_str().unwrap().is_empty());
        let anonymous = client.get(format!("{base}/state")).send().await.unwrap();
        assert_eq!(anonymous.status(), 401);
        let anonymous: serde_json::Value = anonymous.json().await.unwrap();
        assert!(
            anonymous["error"]["message"]
                .as_str()
                .unwrap()
                .contains("/주식 증권")
        );

        let founded = client
            .post(format!("{base}/company"))
            .bearer_auth(&token)
            .json(&serde_json::json!({
                "action": "found", "name": "웹테스트상사", "sector": "game", "capital": 1_000_000
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(founded.status(), 200);
        let founded: serde_json::Value = founded.json().await.unwrap();
        assert_eq!(
            founded["state"]["my_companies"][0]["summary"]["name"],
            "웹테스트상사"
        );
        let wrong_name = client
            .post(format!("{base}/company"))
            .bearer_auth(&token)
            .json(&serde_json::json!({ "action": "dissolve", "confirm": "다른이름" }))
            .send()
            .await
            .unwrap();
        assert_eq!(wrong_name.status(), 400);

        let candles: serde_json::Value = client
            .get(format!("{base}/candles?code=INDEX&range=minute"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(candles["range"], "minute");
        assert!(candles["candles"].is_array());

        // 순위: 주식을 가진 사람은 나뿐이다.
        let ranking: serde_json::Value = client
            .get(format!("{base}/ranking"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(ranking["mine"]["rank"], 1);
        assert_eq!(ranking["rows"][0]["me"], true);

        // 관리자 기능: 권한이 없으면 403, 권한을 받으면 거래정지와 채널 연결을 한다.
        let halt = serde_json::json!({ "action": "halt", "code": code });
        let denied = client
            .post(format!("{base}/admin"))
            .bearer_auth(&token)
            .json(&halt)
            .send()
            .await
            .unwrap();
        assert_eq!(denied.status(), 403);
        stocks
            .grant_web_admin(7, crate::stock_hub::now_ms() + 3_600_000)
            .await;
        let halted: serde_json::Value = client
            .post(format!("{base}/admin"))
            .bearer_auth(&token)
            .json(&halt)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(halted["state"]["me"]["admin"], true);
        assert_eq!(halted["state"]["admin"]["halted"][0], code.as_str());
        let channels: serde_json::Value = client
            .post(format!("{base}/admin"))
            .bearer_auth(&token)
            .json(
                &serde_json::json!({ "action": "channels", "news_channel": "1234567890123456789" }),
            )
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            channels["state"]["admin"]["news_channel"],
            "1234567890123456789"
        );
        assert_eq!(channels["message"], "채널 연결을 저장했습니다.");
        let panel: serde_json::Value = client
            .post(format!("{base}/admin"))
            .bearer_auth(&token)
            .json(&serde_json::json!({ "action": "channels", "panel_channel": "42" }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(panel["state"]["admin"]["panel_channel"], "42");
        assert!(
            panel["message"]
                .as_str()
                .unwrap()
                .contains("새 채널에 올라갑니다")
        );
        let bad = client
            .post(format!("{base}/admin"))
            .bearer_auth(&token)
            .json(&serde_json::json!({ "action": "channels", "panel_channel": "abc" }))
            .send()
            .await
            .unwrap();
        assert_eq!(bad.status(), 400);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

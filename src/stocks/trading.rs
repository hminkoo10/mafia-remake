// stocks/trading.rs — 호가(시장조성자 + 플레이어 지정가), 주문·체결, 취소, 가격 영향, 수수료·세금
//
// 가격 영향: 시장조성자와 체결된 수량만큼 투자 심리(영구)가 선형으로 움직이고, 호가를 먹은 만큼
// 현재가가 움직였다가(일시) 몇 분에 걸쳐 되돌아간다. 영구 영향이 선형이면 사고팔기를 반복해
// 가격을 조종해도 이익이 나지 않는다 (Huberman–Stanzl). 스프레드·수수료 때문에 반복할수록 손해다.

use super::corporate::format_amount;
use super::model::*;
use super::price::*;
use serde::Serialize;

/// 호가 한 단계 (화면용).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BookLevel {
    pub price: i64,
    pub qty: i64,
    /// 그중 플레이어 주문 수량.
    pub players: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Book {
    /// 매도 호가 (낮은 값부터).
    pub asks: Vec<BookLevel>,
    /// 매수 호가 (높은 값부터).
    pub bids: Vec<BookLevel>,
}

/// 주문 요청.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderRequest {
    pub user: u64,
    pub name: String,
    pub code: String,
    pub side: Side,
    pub qty: i64,
    /// 지정가 (없으면 시장가: 체결되지 않은 수량은 취소).
    pub limit: Option<i64>,
}

/// 주문 결과.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct OrderResult {
    pub filled: i64,
    pub notional: i64,
    pub avg_price: i64,
    pub fee: i64,
    pub tax: i64,
    /// 호가에 남은 주문 번호.
    pub resting: Option<u64>,
    pub resting_qty: i64,
    /// 지정가를 호가 단위에 맞춘 값.
    pub limit: Option<i64>,
    /// 매수: 쓰지 않아 돌려준 증거금.
    pub refund: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Lp,
    Order(u64),
    /// 자사주 매입 중인 회사의 매수 호가.
    Buyback,
}

#[derive(Debug, Clone, Copy)]
struct Liquidity {
    price: i64,
    qty: i64,
    source: Source,
}

/// 체결 한 건 (내부).
#[derive(Debug, Clone, Copy)]
struct Fill {
    price: i64,
    qty: i64,
    source: Source,
}

/// 호가를 먹은 결과.
#[derive(Debug, Clone, Default)]
struct TakeOutcome {
    filled: i64,
    notional: i64,
}

impl StockMarket {
    /// 시장조성자 스프레드: 매도 1호가는 현재가에서 `up` 호가 위, 매수 1호가는 `down` 호가 아래.
    fn lp_spread(company: &Company) -> (usize, usize) {
        let mid = company.quote_mid();
        let tick = tick_size(mid) as f64;
        let ticks = ((mid as f64 * company.spread_bp as f64 / BP as f64) / tick)
            .ceil()
            .max(1.0) as usize;
        let up = ticks.div_ceil(2).max(1);
        (up, ticks - up)
    }

    /// 플레이어 회사: 시장조성자가 더 살 수 있는 수량.
    fn lp_buy_room(company: &Company, rules: &StockRules) -> Option<i64> {
        company.lp_inventory.map(|inventory| {
            let cap = company.shares.saturating_mul(rules.lp_inventory_bp.max(0)) / BP;
            (cap - inventory).max(0)
        })
    }

    /// 시장조성자 매도 호가 (가격, 수량), 낮은 값부터.
    fn lp_asks(&self, company: &Company, rules: &StockRules) -> Vec<(i64, i64)> {
        let (_, upper) = self.limits(company, rules);
        let cap = if upper >= i64::MAX / 8 {
            company.price.saturating_mul(3)
        } else {
            upper
        };
        let mut left = company.lp_inventory.unwrap_or(i64::MAX);
        let (up, _) = Self::lp_spread(company);
        let mut price = company.quote_mid();
        for _ in 0..up {
            price = tick_up(price);
        }
        let mut levels = Vec::new();
        for level in 0..MAX_WALK_LEVELS {
            if price > cap || left <= 0 {
                break;
            }
            let qty = ((company.depth as f64) * (1.0 + 0.35 * level as f64)) as i64;
            let qty = qty.max(1).min(left);
            left -= qty;
            levels.push((price, qty));
            price = tick_up(price);
        }
        levels
    }

    /// 시장조성자 매수 호가 (가격, 수량), 높은 값부터.
    fn lp_bids(&self, company: &Company, rules: &StockRules) -> Vec<(i64, i64)> {
        let (lower, upper) = self.limits(company, rules);
        let mut left = Self::lp_buy_room(company, rules).unwrap_or(i64::MAX);
        let (_, down) = Self::lp_spread(company);
        let mut price = company.quote_mid().min(upper);
        for _ in 0..down {
            price = tick_down(price);
        }
        // 하한가에서는 시장조성자도 사지 않는다 (하한가 매도 잔량이 쌓인다).
        if company.quote_mid() <= lower && lower > 1 {
            return Vec::new();
        }
        let mut levels = Vec::new();
        for level in 0..MAX_WALK_LEVELS {
            if price < lower.max(1) || left <= 0 {
                break;
            }
            let qty = ((company.depth as f64) * (1.0 + 0.35 * level as f64)) as i64;
            let qty = qty.max(1).min(left);
            left -= qty;
            levels.push((price, qty));
            if price <= 1 {
                break;
            }
            price = tick_down(price);
        }
        levels
    }

    /// 자사주 매입 중인 회사의 매수 호가: 현재가에, 남은 예산으로 (수수료까지) 살 수 있는 만큼.
    /// 실제 장내 매입처럼 주주는 여기에 바로 팔 수 있다 (대표는 매입 중 팔 수 없다).
    fn buyback_bid(company: &Company, rules: &StockRules) -> Option<(i64, i64)> {
        let buyback = company.buyback.as_ref()?;
        let price = floor_tick(company.quote_mid()).max(1);
        let qty = affordable_qty(buyback.budget, price, rules);
        (qty > 0).then_some((price, qty))
    }

    /// 플레이어끼리 체결할 수 있는 가격인지 (현재가 ±밴드).
    fn in_p2p_band(company: &Company, price: i64, rules: &StockRules) -> bool {
        let reference = company.quote_mid();
        (price - reference).abs().saturating_mul(BP) <= rules.p2p_band_bp.saturating_mul(reference)
    }

    /// 화면용 호가창 (시장조성자 + 플레이어 주문을 가격별로 합친다).
    pub fn book(&self, code: &str, rules: &StockRules) -> Option<Book> {
        let company = self.companies.get(code)?;
        if !company.status.is_tradable() {
            return Some(Book::default());
        }
        let mut asks: Vec<BookLevel> = self
            .lp_asks(company, rules)
            .into_iter()
            .map(|(price, qty)| BookLevel {
                price,
                qty,
                players: 0,
            })
            .collect();
        let mut bids: Vec<BookLevel> = self
            .lp_bids(company, rules)
            .into_iter()
            .map(|(price, qty)| BookLevel {
                price,
                qty,
                players: 0,
            })
            .collect();
        if let Some((price, qty)) = Self::buyback_bid(company, rules) {
            match bids.iter_mut().find(|level| level.price == price) {
                Some(level) => level.qty += qty,
                None => bids.push(BookLevel {
                    price,
                    qty,
                    players: 0,
                }),
            }
        }
        for order in self.orders.iter().filter(|order| order.code == code) {
            let list = match order.side {
                Side::Sell => &mut asks,
                Side::Buy => &mut bids,
            };
            match list.iter_mut().find(|level| level.price == order.limit) {
                Some(level) => {
                    level.qty += order.remaining;
                    level.players += order.remaining;
                }
                None => list.push(BookLevel {
                    price: order.limit,
                    qty: order.remaining,
                    players: order.remaining,
                }),
            }
        }
        asks.sort_by_key(|level| level.price);
        bids.sort_by_key(|level| std::cmp::Reverse(level.price));
        asks.truncate(BOOK_LEVELS);
        bids.truncate(BOOK_LEVELS);
        Some(Book { asks, bids })
    }

    /// 매수에 필요한 최대 금액 (지정가 또는 상한가 기준, 수수료 포함). 봇은 이만큼(또는 가진 만큼)을 증거금으로 잡는다.
    pub fn max_buy_cost(
        &self,
        code: &str,
        qty: i64,
        limit: Option<i64>,
        rules: &StockRules,
    ) -> Result<i64, String> {
        let company = self
            .companies
            .get(code)
            .ok_or_else(|| "그런 종목이 없습니다.".to_string())?;
        let (_, upper) = self.limits(company, rules);
        let cap = if upper >= i64::MAX / 8 {
            company.price.saturating_mul(3)
        } else {
            upper
        };
        let price = limit.map_or(cap, floor_tick).min(cap).max(1);
        let notional = price
            .checked_mul(qty.max(0))
            .ok_or_else(|| "주문 금액이 너무 큽니다.".to_string())?;
        Ok(notional.saturating_add(rules.fee(notional)))
    }

    /// 새 주문. 매수는 `budget`(봇이 사용자 코인에서 확인한 증거금)만큼을 장부에서 먼저 빼고, 쓰지 않은
    /// 만큼을 돌려준다. 지정가의 남은 수량은 호가에 남는다.
    pub fn place_order(
        &mut self,
        request: &OrderRequest,
        budget: i64,
        now: i64,
        rules: &StockRules,
    ) -> Result<OrderResult, String> {
        if request.qty <= 0 {
            return Err("수량은 1주 이상이어야 합니다.".to_string());
        }
        let company = self
            .companies
            .get(&request.code)
            .ok_or_else(|| "그런 종목이 없습니다.".to_string())?;
        self.trading_open(company, rules, now)?;
        // 시스템 회사는 평균 거래량 기준, 플레이어 회사(주식 수가 적다)는 발행 주식의 20%까지.
        let order_cap = if company.is_player() {
            (company.shares / 5).max(10)
        } else {
            (company.adv.saturating_mul(rules.order_limit_pct.max(1)) / 100).max(10)
        };
        if request.qty > order_cap {
            return Err(format!(
                "한 번에 주문할 수 있는 수량은 {order_cap}주까지입니다."
            ));
        }
        let (lower, upper) = self.limits(company, rules);
        let limit = match request.limit {
            None => None,
            Some(raw) => {
                let aligned = match request.side {
                    Side::Buy => floor_tick(raw),
                    Side::Sell => ceil_tick(raw),
                };
                if aligned < lower || aligned > upper {
                    return Err("지정가가 오늘의 가격제한폭을 벗어났습니다.".to_string());
                }
                let reference = company.quote_mid();
                if (aligned - reference).abs().saturating_mul(BP)
                    > rules.order_band_bp.saturating_mul(reference)
                {
                    return Err(format!(
                        "지정가는 현재가의 ±{}% 안에서만 낼 수 있습니다.",
                        rules.order_band_bp / 100
                    ));
                }
                Some(aligned)
            }
        };
        let account = self.accounts.get(&request.user);
        let position = account
            .and_then(|account| account.positions.get(&request.code))
            .cloned()
            .unwrap_or_default();
        match request.side {
            Side::Buy => {
                if !company.is_player() {
                    let limit_qty = company.shares.saturating_mul(rules.holding_limit_bp) / BP;
                    let pending = self
                        .orders
                        .iter()
                        .filter(|order| {
                            order.user == request.user
                                && order.code == request.code
                                && order.side == Side::Buy
                        })
                        .map(|order| order.remaining)
                        .sum::<i64>();
                    if position.qty + pending + request.qty > limit_qty {
                        return Err(format!(
                            "한 사람이 가질 수 있는 한도(발행 주식의 {}%, {}주)를 넘습니다.",
                            rules.holding_limit_bp as f64 / 100.0,
                            limit_qty
                        ));
                    }
                }
                if budget <= 0 {
                    return Err("매수할 코인이 없습니다.".to_string());
                }
            }
            Side::Sell => {
                if position.available(now) < request.qty {
                    let lockup = if now < position.lockup_until {
                        position.lockup_qty
                    } else {
                        0
                    };
                    return Err(if lockup > 0 {
                        format!(
                            "팔 수 있는 주식은 {}주입니다 (보호예수 {}주는 아직 팔 수 없습니다).",
                            position.available(now),
                            lockup
                        )
                    } else {
                        format!("팔 수 있는 주식은 {}주입니다.", position.available(now))
                    });
                }
                if company.buyback.is_some() && company.founder() == Some(request.user) {
                    return Err("자사주 매입 기간에는 설립자가 주식을 팔 수 없습니다.".to_string());
                }
            }
        }
        let code = request.code.clone();
        let mut result = OrderResult {
            limit,
            ..OrderResult::default()
        };
        match request.side {
            Side::Buy => {
                self.transfer(
                    request.user,
                    &request.name,
                    -budget,
                    format!("{code} 매수 증거금"),
                );
                let outcome = self.take(
                    &code,
                    Side::Buy,
                    request.qty,
                    limit,
                    Some(budget),
                    Some(request.user),
                    now,
                    rules,
                );
                let fee = rules.fee(outcome.notional);
                let spent = outcome.notional + fee;
                self.credit_buyer(
                    request.user,
                    &request.name,
                    &code,
                    outcome.filled,
                    outcome.notional,
                    fee,
                    now,
                );
                result.filled = outcome.filled;
                result.notional = outcome.notional;
                result.fee = fee;
                let mut left = budget - spent;
                let remaining = request.qty - outcome.filled;
                if let (Some(limit), true) = (limit, remaining > 0) {
                    let need = limit.saturating_mul(remaining);
                    let reserve = need.saturating_add(rules.fee(need)).min(left);
                    // 남은 증거금으로 살 수 있는 만큼만 호가에 남긴다.
                    let affordable = affordable_qty(reserve, limit, rules).min(remaining);
                    if affordable > 0 {
                        let reserved = limit
                            .saturating_mul(affordable)
                            .saturating_add(rules.fee(limit.saturating_mul(affordable)))
                            .min(left);
                        left -= reserved;
                        let id = self.rest_order(
                            request,
                            Side::Buy,
                            limit,
                            affordable,
                            reserved,
                            now,
                            rules,
                        );
                        result.resting = Some(id);
                        result.resting_qty = affordable;
                        self.log(format!(
                            "📝 {} · {} 지정가 매수 {affordable}주 @{} 주문 #{id} (묶인 코인 {})",
                            request.name,
                            self.label(&code),
                            format_amount(limit),
                            format_amount(reserved)
                        ));
                    }
                }
                if left > 0 {
                    self.transfer(
                        request.user,
                        &request.name,
                        left,
                        format!("{code} 매수 증거금 반환"),
                    );
                }
                result.refund = left.max(0);
            }
            Side::Sell => {
                let outcome = self.take(
                    &code,
                    Side::Sell,
                    request.qty,
                    limit,
                    None,
                    Some(request.user),
                    now,
                    rules,
                );
                let fee = rules.fee(outcome.notional);
                let tax = rules.tax(outcome.notional);
                self.debit_seller(
                    request.user,
                    &request.name,
                    &code,
                    outcome.filled,
                    outcome.notional,
                    fee,
                    tax,
                    now,
                );
                result.filled = outcome.filled;
                result.notional = outcome.notional;
                result.fee = fee;
                result.tax = tax;
                let remaining = request.qty - outcome.filled;
                if let (Some(limit), true) = (limit, remaining > 0) {
                    let account = self.account_mut(request.user, &request.name);
                    if let Some(position) = account.positions.get_mut(&code) {
                        position.locked += remaining;
                    }
                    let id = self.rest_order(request, Side::Sell, limit, remaining, 0, now, rules);
                    result.resting = Some(id);
                    result.resting_qty = remaining;
                    self.log(format!(
                        "📝 {} · {} 지정가 매도 {remaining}주 @{} 주문 #{id}",
                        request.name,
                        self.label(&code),
                        format_amount(limit)
                    ));
                }
            }
        }
        if result.filled > 0 {
            result.avg_price = result.notional / result.filled;
        }
        self.version += 1;
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    fn rest_order(
        &mut self,
        request: &OrderRequest,
        side: Side,
        limit: i64,
        qty: i64,
        reserved: i64,
        now: i64,
        rules: &StockRules,
    ) -> u64 {
        let id = self.next_order_id.max(1);
        self.next_order_id = id + 1;
        self.orders.push(Order {
            id,
            user: request.user,
            name: request.name.clone(),
            code: request.code.clone(),
            side,
            limit,
            remaining: qty,
            original: qty,
            reserved,
            created_at: now,
            expires_at: now + rules.order_days.max(1) * rules.day_ms(),
        });
        id
    }

    /// 사용자 주문 취소.
    pub fn cancel_order(&mut self, user: u64, id: u64) -> Result<(), String> {
        let order = self
            .orders
            .iter()
            .find(|order| order.id == id)
            .ok_or_else(|| "그런 주문이 없습니다.".to_string())?;
        if order.user != user {
            return Err("본인 주문만 취소할 수 있습니다.".to_string());
        }
        self.close_order(id, "주문 취소");
        self.version += 1;
        Ok(())
    }

    /// 주문을 닫는다: 매수는 남은 증거금을 돌려주고, 매도는 묶인 주식을 푼다.
    pub(super) fn close_order(&mut self, id: u64, reason: &str) {
        let Some(index) = self.orders.iter().position(|order| order.id == id) else {
            return;
        };
        let order = self.orders.remove(index);
        if reason != "주문 체결 완료" {
            let refund = if order.side == Side::Buy && order.reserved > 0 {
                format!(", 반환 {}", format_amount(order.reserved))
            } else {
                String::new()
            };
            self.log(format!(
                "✖️ {} · {} 주문 #{} {reason} ({} {}주 @{}{refund})",
                order.name,
                self.label(&order.code),
                order.id,
                order.side.label(),
                order.remaining,
                format_amount(order.limit)
            ));
        }
        match order.side {
            Side::Buy => {
                if order.reserved > 0 {
                    self.transfer(
                        order.user,
                        &order.name,
                        order.reserved,
                        format!("{} {reason}", order.code),
                    );
                }
            }
            Side::Sell => {
                if let Some(position) = self
                    .accounts
                    .get_mut(&order.user)
                    .and_then(|account| account.positions.get_mut(&order.code))
                {
                    position.locked = (position.locked - order.remaining).max(0);
                }
            }
        }
    }

    /// 사용자의 모든 주문을 닫는다 (종목 상장폐지 등).
    pub(super) fn close_orders_for(&mut self, code: &str, reason: &str) {
        let ids = self
            .orders
            .iter()
            .filter(|order| order.code == code)
            .map(|order| order.id)
            .collect::<Vec<_>>();
        for id in ids {
            self.close_order(id, reason);
        }
    }

    /// 매수 체결을 계좌에 넣는다.
    #[allow(clippy::too_many_arguments)]
    fn credit_buyer(
        &mut self,
        user: u64,
        name: &str,
        code: &str,
        qty: i64,
        notional: i64,
        fee: i64,
        now: i64,
    ) {
        if qty <= 0 {
            return;
        }
        let label = self.label(code);
        self.to_treasury(fee, format!("{code} 매수 수수료"));
        self.stats.fees = self.stats.fees.saturating_add(fee);
        let account = self.account_mut(user, name);
        let position = account.positions.entry(code.to_string()).or_default();
        position.qty += qty;
        position.cost = position.cost.saturating_add(notional + fee);
        account.fees = account.fees.saturating_add(fee);
        account.trades += 1;
        account.fills.push_back(FillRecord {
            at: now,
            code: code.to_string(),
            side: Side::Buy,
            qty,
            price: notional / qty,
            cost: fee,
        });
        while account.fills.len() > FILL_HISTORY_LIMIT {
            account.fills.pop_front();
        }
        self.log(format!(
            "🔴 {name} · {label} {qty}주 매수 체결 @{} · 낸 코인 {} (수수료 {})",
            format_amount(notional / qty),
            format_amount(notional + fee),
            format_amount(fee)
        ));
    }

    /// 매도 체결을 계좌에서 빼고 대금을 준다.
    #[allow(clippy::too_many_arguments)]
    fn debit_seller(
        &mut self,
        user: u64,
        name: &str,
        code: &str,
        qty: i64,
        notional: i64,
        fee: i64,
        tax: i64,
        now: i64,
    ) {
        if qty <= 0 {
            return;
        }
        let proceeds = notional - fee - tax;
        let label = self.label(code);
        self.to_treasury(fee + tax, format!("{code} 매도 수수료·거래세"));
        self.stats.fees = self.stats.fees.saturating_add(fee);
        self.stats.taxes = self.stats.taxes.saturating_add(tax);
        self.transfer(user, name, proceeds, format!("{code} 매도 대금"));
        let account = self.account_mut(user, name);
        let position = account.positions.entry(code.to_string()).or_default();
        let before = position.qty.max(1);
        let cost_out = (i128::from(position.cost) * i128::from(qty) / i128::from(before)) as i64;
        position.qty -= qty;
        position.cost -= cost_out;
        if position.qty <= 0 {
            position.qty = 0;
            position.cost = 0;
        }
        account.realized = account.realized.saturating_add(proceeds - cost_out);
        account.fees = account.fees.saturating_add(fee + tax);
        account.trades += 1;
        account.fills.push_back(FillRecord {
            at: now,
            code: code.to_string(),
            side: Side::Sell,
            qty,
            price: notional / qty,
            cost: fee + tax,
        });
        while account.fills.len() > FILL_HISTORY_LIMIT {
            account.fills.pop_front();
        }
        let empty = account.positions.get(code).is_some_and(|position| {
            position.qty == 0 && position.locked == 0 && position.lockup_qty == 0
        });
        if empty {
            account.positions.remove(code);
        }
        self.log(format!(
            "🔵 {name} · {label} {qty}주 매도 체결 @{} · 받은 코인 {} (수수료·세금 {})",
            format_amount(notional / qty),
            format_amount(proceeds),
            format_amount(fee + tax)
        ));
    }

    /// 호가를 먹는다: 시장조성자와 다른 플레이어의 지정가를 가격 순으로 체결한다. 상대 주문과
    /// 시장조성자 쪽 장부·가격은 여기서 처리하고, 주문을 낸 쪽(taker)의 계좌는 부르는 쪽이 처리한다.
    #[allow(clippy::too_many_arguments)]
    fn take(
        &mut self,
        code: &str,
        side: Side,
        qty: i64,
        limit: Option<i64>,
        budget: Option<i64>,
        taker: Option<u64>,
        now: i64,
        rules: &StockRules,
    ) -> TakeOutcome {
        let Some(company) = self.companies.get(code) else {
            return TakeOutcome::default();
        };
        let mut liquidity: Vec<Liquidity> = match side {
            Side::Buy => self.lp_asks(company, rules),
            Side::Sell => self.lp_bids(company, rules),
        }
        .into_iter()
        .map(|(price, qty)| Liquidity {
            price,
            qty,
            source: Source::Lp,
        })
        .collect();
        for order in &self.orders {
            let opposite = order.side != side;
            if order.code != code || !opposite || Some(order.user) == taker {
                continue;
            }
            if !Self::in_p2p_band(company, order.limit, rules) {
                continue;
            }
            liquidity.push(Liquidity {
                price: order.limit,
                qty: order.remaining,
                source: Source::Order(order.id),
            });
        }
        if side == Side::Sell
            && taker != company.founder()
            && let Some((price, qty)) = Self::buyback_bid(company, rules)
        {
            liquidity.push(Liquidity {
                price,
                qty,
                source: Source::Buyback,
            });
        }
        // 가격 순, 같은 가격이면 플레이어 주문(먼저 낸 것) 먼저.
        let created = |source: Source| match source {
            Source::Lp | Source::Buyback => i64::MAX,
            Source::Order(id) => self
                .orders
                .iter()
                .find(|order| order.id == id)
                .map_or(i64::MAX, |order| order.created_at),
        };
        let mut keyed = liquidity
            .into_iter()
            .map(|entry| (entry, created(entry.source)))
            .collect::<Vec<_>>();
        match side {
            Side::Buy => keyed.sort_by_key(|(entry, at)| (entry.price, *at)),
            Side::Sell => keyed.sort_by_key(|(entry, at)| (std::cmp::Reverse(entry.price), *at)),
        }
        let mut remaining = qty;
        let mut notional = 0_i64;
        let mut fills: Vec<Fill> = Vec::new();
        for (entry, _) in keyed {
            if remaining <= 0 {
                break;
            }
            if let Some(limit) = limit {
                let beyond = match side {
                    Side::Buy => entry.price > limit,
                    Side::Sell => entry.price < limit,
                };
                if beyond {
                    break;
                }
            }
            let mut take = remaining.min(entry.qty);
            if let Some(budget) = budget {
                take = take.min(affordable_more(budget, notional, entry.price, rules));
                if take <= 0 {
                    break;
                }
            }
            remaining -= take;
            notional = notional.saturating_add(take.saturating_mul(entry.price));
            fills.push(Fill {
                price: entry.price,
                qty: take,
                source: entry.source,
            });
        }
        if fills.is_empty() {
            return TakeOutcome::default();
        }
        let filled = qty - remaining;
        let mut lp_qty = 0_i64;
        for fill in &fills {
            match fill.source {
                Source::Lp => {
                    lp_qty += fill.qty;
                    let value = fill.qty.saturating_mul(fill.price);
                    match side {
                        Side::Buy => {
                            self.stats.lp_bought = self.stats.lp_bought.saturating_add(value)
                        }
                        Side::Sell => self.stats.lp_sold = self.stats.lp_sold.saturating_add(value),
                    }
                }
                Source::Order(id) => {
                    self.stats.p2p = self
                        .stats
                        .p2p
                        .saturating_add(fill.qty.saturating_mul(fill.price));
                    self.fill_maker(id, fill.qty, fill.price, now, rules);
                }
                Source::Buyback => self.fill_buyback(code, fill.qty, fill.price, rules),
            }
        }
        let last = fills.last().map_or(0, |fill| fill.price);
        self.after_trade(code, side, filled, notional, lp_qty, last, now, rules);
        TakeOutcome { filled, notional }
    }

    /// 호가에 있던 상대 주문이 체결됐다.
    fn fill_maker(&mut self, id: u64, qty: i64, price: i64, now: i64, rules: &StockRules) {
        let Some(order) = self.orders.iter_mut().find(|order| order.id == id) else {
            return;
        };
        let notional = qty.saturating_mul(price);
        let fee = rules.fee(notional);
        order.remaining -= qty;
        let (user, name, code, side) = (
            order.user,
            order.name.clone(),
            order.code.clone(),
            order.side,
        );
        match side {
            Side::Buy => {
                order.reserved -= notional + fee;
                self.credit_buyer(user, &name, &code, qty, notional, fee, now);
            }
            Side::Sell => {
                if let Some(position) = self
                    .accounts
                    .get_mut(&user)
                    .and_then(|account| account.positions.get_mut(&code))
                {
                    position.locked = (position.locked - qty).max(0);
                }
                let tax = rules.tax(notional);
                self.debit_seller(user, &name, &code, qty, notional, fee, tax, now);
            }
        }
        let done = self
            .orders
            .iter()
            .find(|order| order.id == id)
            .is_some_and(|order| order.remaining <= 0);
        if done {
            self.close_order(id, "주문 체결 완료");
        }
    }

    /// 주주가 자사주 매입 호가에 팔았다: 회사 현금으로 사서 바로 소각한다 (회사가 내는 수수료는 금고로).
    fn fill_buyback(&mut self, code: &str, qty: i64, price: i64, rules: &StockRules) {
        let notional = qty.saturating_mul(price);
        let fee = rules.fee(notional);
        self.to_treasury(fee, format!("{code} 자사주 매입 수수료"));
        self.stats.fees = self.stats.fees.saturating_add(fee);
        let before = self.listed_cap_sum();
        if let Some(company) = self.companies.get_mut(code) {
            company.shares = (company.shares - qty).max(1);
            company.equity = (company.equity - notional - fee).max(0);
            if let Some(buyback) = company.buyback.as_mut() {
                buyback.budget = (buyback.budget - notional - fee).max(0);
                buyback.bought += qty;
            }
        }
        self.rebase_index(before);
    }

    /// 체결 뒤: 현재가·거래량·봉, 시장조성자 재고, 가격 영향, 변동성 완화장치.
    #[allow(clippy::too_many_arguments)]
    fn after_trade(
        &mut self,
        code: &str,
        side: Side,
        filled: i64,
        notional: i64,
        lp_qty: i64,
        last: i64,
        now: i64,
        rules: &StockRules,
    ) {
        let Some(company) = self.companies.get(code) else {
            return;
        };
        let (lower, upper) = self.limits(company, rules);
        let signed = match side {
            Side::Buy => lp_qty,
            Side::Sell => -lp_qty,
        };
        let company = self.companies.get_mut(code).expect("company exists");
        if let Some(inventory) = company.lp_inventory.as_mut() {
            *inventory = (*inventory - signed).max(0);
        }
        // 영구 영향: 평균 거래량만큼 사면 하루 변동성만큼 오른다 (선형).
        let adv = company.adv.max(1) as f64;
        company.sentiment =
            (company.sentiment + company.daily_vol * signed as f64 / adv).clamp(-5.0, 5.0);
        let mid_before = company.quote_mid();
        let price = last.clamp(lower, upper);
        company.price = price;
        company.high = company.high.max(price);
        company.low = company.low.min(price);
        company.volume = company.volume.saturating_add(filled);
        company.turnover = company.turnover.saturating_add(notional);
        let noise = company.noise;
        let vi_ref = company.vi_ref;
        let fair = {
            let company = &self.companies[code];
            self.fair_log(company, now)
        };
        let company = self.companies.get_mut(code).expect("company exists");
        // 호가 중심은 먹은 폭의 절반만 따라간다: 방금 산 사람이 곧바로 되팔면 꼭대기가 아니라 그 아래에서
        // 팔린다 (반대편 호가는 그대로 남아 있는 실제 호가창처럼). 그 차이는 몇 분에 걸쳐 사라진다.
        let mid_log = ((mid_before as f64).ln() + (price as f64).ln()) / 2.0;
        company.impact = (mid_log - fair - noise).clamp(-3.0, 3.0);
        company.mid = round_tick(mid_log.exp()).clamp(lower, upper);
        let vi = rules.vi_bp > 0
            && vi_ref > 0
            && (price - vi_ref).abs().saturating_mul(BP) >= rules.vi_bp.saturating_mul(vi_ref);
        if vi {
            company.halted_until = now + super::market::VI_HALT_MS;
            company.vi_ref = price;
        }
        let name = company.name.clone();
        self.candles
            .series
            .entry(code.to_string())
            .or_default()
            .record(now, rules.day_ms(), price, filled);
        if vi {
            self.push_news(
                now,
                NewsKind::Halt,
                Some(code),
                format!("{name} 급변으로 변동성 완화장치 발동, 2분간 거래정지"),
                0,
            );
        }
    }

    /// 호가에 남은 주문을 시간 순으로 다시 맞춰 본다 (시세가 지정가에 닿으면 체결).
    pub(super) fn match_resting(
        &mut self,
        at: i64,
        rules: &StockRules,
        report: &mut super::market::TickReport,
    ) {
        let mut ids = self
            .orders
            .iter()
            .map(|order| (order.created_at, order.id))
            .collect::<Vec<_>>();
        ids.sort_unstable();
        for (_, id) in ids {
            let Some(order) = self.orders.iter().find(|order| order.id == id).cloned() else {
                continue;
            };
            let Some(company) = self.companies.get(&order.code) else {
                continue;
            };
            if self.trading_open(company, rules, at).is_err() {
                continue;
            }
            // 시세가 지정가에 닿았는지 먼저 본다 (대부분 닿지 않으므로 호가를 만들지 않고 넘긴다).
            let reachable = match order.side {
                Side::Buy => {
                    self.lp_asks(company, rules)
                        .first()
                        .is_some_and(|(price, _)| *price <= order.limit)
                        || self.orders.iter().any(|other| {
                            other.code == order.code
                                && other.side == Side::Sell
                                && other.user != order.user
                                && other.limit <= order.limit
                        })
                }
                Side::Sell => {
                    self.lp_bids(company, rules)
                        .first()
                        .is_some_and(|(price, _)| *price >= order.limit)
                        || (Some(order.user) != company.founder()
                            && Self::buyback_bid(company, rules)
                                .is_some_and(|(price, _)| price >= order.limit))
                        || self.orders.iter().any(|other| {
                            other.code == order.code
                                && other.side == Side::Buy
                                && other.user != order.user
                                && other.limit >= order.limit
                        })
                }
            };
            if !reachable {
                continue;
            }
            let budget = (order.side == Side::Buy).then_some(order.reserved);
            let outcome = self.take(
                &order.code,
                order.side,
                order.remaining,
                Some(order.limit),
                budget,
                Some(order.user),
                at,
                rules,
            );
            if outcome.filled <= 0 {
                continue;
            }
            let fee = rules.fee(outcome.notional);
            match order.side {
                Side::Buy => {
                    if let Some(own) = self.orders.iter_mut().find(|own| own.id == id) {
                        own.reserved -= outcome.notional + fee;
                        own.remaining -= outcome.filled;
                    }
                    self.credit_buyer(
                        order.user,
                        &order.name,
                        &order.code,
                        outcome.filled,
                        outcome.notional,
                        fee,
                        at,
                    );
                }
                Side::Sell => {
                    if let Some(own) = self.orders.iter_mut().find(|own| own.id == id) {
                        own.remaining -= outcome.filled;
                    }
                    if let Some(position) = self
                        .accounts
                        .get_mut(&order.user)
                        .and_then(|account| account.positions.get_mut(&order.code))
                    {
                        position.locked = (position.locked - outcome.filled).max(0);
                    }
                    let tax = rules.tax(outcome.notional);
                    self.debit_seller(
                        order.user,
                        &order.name,
                        &order.code,
                        outcome.filled,
                        outcome.notional,
                        fee,
                        tax,
                        at,
                    );
                }
            }
            report.fills.push((
                order.user,
                FillRecord {
                    at,
                    code: order.code.clone(),
                    side: order.side,
                    qty: outcome.filled,
                    price: outcome.notional / outcome.filled,
                    cost: fee,
                },
            ));
            let done = self
                .orders
                .iter()
                .find(|own| own.id == id)
                .is_some_and(|own| own.remaining <= 0);
            if done {
                self.close_order(id, "주문 체결 완료");
            }
        }
    }

    /// 자사주 매입 체결 (회사 현금으로 시장조성자·플레이어 매도 호가를 산다). 산 주식은 소각한다.
    pub(super) fn buy_back(
        &mut self,
        code: &str,
        budget: i64,
        now: i64,
        rules: &StockRules,
    ) -> (i64, i64) {
        let Some(company) = self.companies.get(code) else {
            return (0, 0);
        };
        if self.trading_open(company, rules, now).is_err() || budget <= 0 {
            return (0, 0);
        }
        let limit = company.price.saturating_mul(BP + 200) / BP;
        let founder = company.founder();
        let outcome = self.take(
            code,
            Side::Buy,
            i64::MAX / 4,
            Some(limit),
            Some(budget),
            founder,
            now,
            rules,
        );
        if outcome.filled <= 0 {
            return (0, 0);
        }
        let fee = rules.fee(outcome.notional);
        self.to_treasury(fee, format!("{code} 자사주 매입 수수료"));
        self.stats.fees = self.stats.fees.saturating_add(fee);
        let before = self.listed_cap_sum();
        if let Some(company) = self.companies.get_mut(code) {
            company.shares = (company.shares - outcome.filled).max(1);
            company.equity = (company.equity - outcome.notional - fee).max(0);
        }
        self.rebase_index(before);
        (outcome.filled, outcome.notional + fee)
    }
}

/// `budget` 안에서 수수료까지 내고 살 수 있는 수량.
pub fn affordable_qty(budget: i64, price: i64, rules: &StockRules) -> i64 {
    affordable_more(budget, 0, price, rules)
}

/// 이미 `notional`만큼 샀을 때, `price`에 더 살 수 있는 수량 (수수료 포함 총액이 `budget` 이하).
fn affordable_more(budget: i64, notional: i64, price: i64, rules: &StockRules) -> i64 {
    if price <= 0 {
        return 0;
    }
    let total = |extra: i64| {
        let value = notional.saturating_add(extra.saturating_mul(price));
        value.saturating_add(rules.fee(value))
    };
    // 수수료를 뺀 최대 거래대금으로 어림한 뒤, 올림·최소 수수료 때문에 넘치면 한두 주 줄인다.
    let fee_ppm = i128::from(rules.fee_ppm.max(0));
    let max_value = i128::from(budget.max(0)) * i128::from(PPM) / (i128::from(PPM) + fee_ppm);
    let rough = ((max_value - i128::from(notional)).max(0) / i128::from(price)) as i64;
    let mut qty = rough.max(0);
    for _ in 0..4 {
        if qty > 0 && total(qty) > budget {
            qty -= 1;
        }
    }
    while qty > 0 && total(qty) > budget {
        qty /= 2;
    }
    qty
}

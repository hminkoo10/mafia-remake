// stocks 엔진 테스트

use super::*;
use rand::SeedableRng;
use rand::rngs::StdRng;

const T0: i64 = 1_800_000_000_000;

/// 가격이 스스로 움직이지 않는 규칙 (체결·기업 활동만 본다). 공모는 플레이어끼리만 (기관 배정 없음).
fn quiet() -> StockRules {
    StockRules {
        vol_pct: 0,
        news_pct: 0,
        vi_bp: 0,
        ipo_institution_bp: 0,
        ..StockRules::default()
    }
}

fn market() -> StockMarket {
    StockMarket::new(T0, &quiet())
}

fn rng() -> StdRng {
    StdRng::seed_from_u64(7)
}

/// 장부에 쌓인 코인 이동을 사용자별로 더한다 (금고는 0번).
fn balances(market: &StockMarket) -> std::collections::BTreeMap<u64, i64> {
    let mut out = std::collections::BTreeMap::new();
    for transfer in &market.journal {
        *out.entry(transfer.user).or_insert(0) += transfer.amount;
    }
    out
}

fn buy(
    market: &mut StockMarket,
    user: u64,
    code: &str,
    qty: i64,
    limit: Option<i64>,
    budget: i64,
    now: i64,
) -> Result<OrderResult, String> {
    market.place_order(
        &OrderRequest {
            user,
            name: format!("U{user}"),
            code: code.to_string(),
            side: Side::Buy,
            qty,
            limit,
        },
        budget,
        now,
        &quiet(),
    )
}

fn sell(
    market: &mut StockMarket,
    user: u64,
    code: &str,
    qty: i64,
    limit: Option<i64>,
    now: i64,
) -> Result<OrderResult, String> {
    market.place_order(
        &OrderRequest {
            user,
            name: format!("U{user}"),
            code: code.to_string(),
            side: Side::Sell,
            qty,
            limit,
        },
        0,
        now,
        &quiet(),
    )
}

// ------------------------------------------------------------ 가격 규칙

#[test]
fn tick_sizes_and_limits_follow_the_krx_table() {
    assert_eq!(tick_size(1_999), 1);
    assert_eq!(tick_size(2_000), 5);
    assert_eq!(tick_size(19_990), 10);
    assert_eq!(tick_size(72_000), 100);
    assert_eq!(tick_size(185_000), 100);
    assert_eq!(tick_size(500_000), 1_000);
    assert_eq!(ceil_tick(2_003), 2_005);
    assert_eq!(ceil_tick(19_995), 20_000);
    assert_eq!(floor_tick(49_990), 49_950);
    assert_eq!(tick_up(1_999), 2_000);
    assert_eq!(tick_down(20_000), 19_990);
    assert_eq!(round_tick(72_049.0), 72_000);
    assert_eq!(round_tick(72_051.0), 72_100);
    assert_eq!(price_limits(10_000, 3_000), (7_000, 13_000));
    // 경계를 넘는 제한폭은 각 구간의 호가 단위에 맞춘다.
    let (lower, upper) = price_limits(72_000, 3_000);
    assert_eq!((lower, upper), (50_400, 93_600));
    assert_eq!(lower % tick_size(lower), 0);
    assert_eq!(upper % tick_size(upper), 0);
}

#[test]
fn a_new_market_lists_twelve_companies_at_their_fair_value() {
    let market = market();
    assert_eq!(market.companies.len(), 12);
    assert!((market.index.value - 1_000.0).abs() < 1e-9);
    for company in market.companies.values() {
        let fair = market.fair_log(company, T0).exp();
        let gap = (fair - company.price as f64).abs() / company.price as f64;
        assert!(
            gap < 0.001,
            "{} fair {fair} vs price {}",
            company.name,
            company.price
        );
    }
}

#[test]
fn prices_move_on_ticks_within_limits_and_candles_record_them() {
    let rules = StockRules {
        vi_bp: 0,
        ..StockRules::default()
    };
    let mut market = StockMarket::new(T0, &rules);
    let mut rng = rng();
    // 게임 하루(1시간)를 돌린다.
    let report = market.tick(T0 + HOUR_MS - 1, &rules, &mut rng);
    assert!(report.changed);
    let mut moved = 0;
    for company in market.companies.values() {
        let (lower, upper) = price_limits(company.prev_close, rules.limit_bp);
        assert!(
            company.price >= lower && company.price <= upper,
            "{}",
            company.name
        );
        assert_eq!(
            company.price % tick_size(company.price),
            0,
            "{}",
            company.name
        );
        if company.price != company.open {
            moved += 1;
        }
        let series = &market.candles.series[&company.code];
        assert!(series.minute.len() >= 59, "분봉이 쌓인다");
        assert_eq!(series.day.len(), 1);
    }
    assert!(moved >= 8, "대부분의 종목이 움직인다 ({moved})");
    assert!(market.index.value > 500.0 && market.index.value < 2_000.0);
    // 다음 게임일이 되면 전일 종가가 넘어간다.
    let before = market.companies["100010"].price;
    market.tick(T0 + HOUR_MS + TICK_MS, &rules, &mut rng);
    let company = &market.companies["100010"];
    assert_eq!(company.prev_close, before);
    // 같은 시드면 같은 시세.
    let mut again = StockMarket::new(T0, &rules);
    again.tick(T0 + HOUR_MS - 1, &rules, &mut StdRng::seed_from_u64(7));
    let mut first = StockMarket::new(T0, &rules);
    first.tick(T0 + HOUR_MS - 1, &rules, &mut StdRng::seed_from_u64(7));
    assert_eq!(first.companies, again.companies);
}

#[test]
fn a_long_simulation_stays_realistic() {
    // 실제 하루(게임 24일) 동안 지수와 종목이 터무니없이 움직이지 않는다.
    let rules = StockRules::default();
    let mut market = StockMarket::new(T0, &rules);
    let mut rng = StdRng::seed_from_u64(11);
    let mut now = T0;
    let mut daily = Vec::new();
    for _ in 0..24 {
        now += HOUR_MS;
        let before = market.index.value;
        market.tick(now, &rules, &mut rng);
        daily.push((market.index.value / before).ln());
    }
    let mean = daily.iter().sum::<f64>() / daily.len() as f64;
    let sd = (daily
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / daily.len() as f64)
        .sqrt();
    assert!(sd > 0.001 && sd < 0.05, "지수 게임일 변동성 {sd}");
    assert!(
        market.index.value > 600.0 && market.index.value < 1_600.0,
        "지수 {}",
        market.index.value
    );
    assert!(market.news.len() > 3, "뉴스가 나온다");
    for company in market
        .companies
        .values()
        .filter(|company| company.status.is_tradable())
    {
        let catalog = CATALOG.iter().find(|entry| entry.code == company.code);
        if let Some(entry) = catalog {
            let ratio = company.price as f64 / entry.price as f64;
            assert!(ratio > 0.3 && ratio < 3.0, "{} {ratio}", company.name);
        }
    }
}

// ------------------------------------------------------------ 주문·체결

#[test]
fn a_market_buy_walks_the_book_and_refunds_the_rest() {
    let mut market = market();
    let code = "100010";
    let price = market.companies[code].price;
    let result = buy(&mut market, 1, code, 50, None, 10_000_000, T0).unwrap();
    assert_eq!(result.filled, 50);
    assert!(result.avg_price > price, "매도 호가는 현재가보다 위");
    assert_eq!(result.fee, quiet().fee(result.notional));
    let balances = balances(&market);
    assert_eq!(
        balances[&1],
        -(result.notional + result.fee),
        "쓴 만큼만 빠진다"
    );
    assert_eq!(balances[&TREASURY_USER], result.fee, "수수료는 금고로");
    let position = &market.accounts[&1].positions[code];
    assert_eq!(position.qty, 50);
    assert_eq!(position.cost, result.notional + result.fee);
    assert!(market.companies[code].price > price, "사면 현재가가 오른다");
}

#[test]
fn buying_then_selling_right_away_always_loses() {
    for code in ["100010", "100060", "100120"] {
        let mut market = market();
        let bought = buy(&mut market, 1, code, 200, None, 1_000_000_000, T0).unwrap();
        let sold = sell(&mut market, 1, code, 200, None, T0).unwrap();
        let net = balances(&market)[&1];
        assert!(net < 0, "{code}: 왕복하면 손해 ({net})");
        // 수수료를 빼고도 판 금액이 산 금액보다 작다 (반대편 호가는 체결 전 자리에 남아 있다).
        assert!(
            sold.notional < bought.notional,
            "{code}: {} vs {}",
            sold.notional,
            bought.notional
        );
    }
}

#[test]
fn two_accounts_pumping_together_still_lose() {
    let mut market = market();
    let code = "100120";
    // 2번이 먼저 주식을 가지고 있다.
    buy(&mut market, 2, code, 200, None, 1_000_000_000, T0).unwrap();
    let start = balances(&market);
    let value_before = market.companies[code].quote_mid() * 200;
    // 1번이 크게 사서 올리고, 그 사이 2번이 팔고, 1번도 판다.
    buy(&mut market, 1, code, 240, None, 1_000_000_000, T0).unwrap();
    sell(&mut market, 2, code, 200, None, T0).unwrap();
    sell(&mut market, 1, code, 240, None, T0).unwrap();
    let end = balances(&market);
    let gained = (end[&1] - start.get(&1).copied().unwrap_or(0)) + (end[&2] - start[&2]);
    // 2번이 처음 가진 주식의 가치까지 쳐서, 둘을 합치면 손해다.
    assert!(
        gained < value_before,
        "합쳐서 이득이 없다: {gained} vs {value_before}"
    );
}

#[test]
fn limit_orders_rest_with_reserved_coins_and_cancel_refunds() {
    let mut market = market();
    let code = "100070";
    let price = market.companies[code].price;
    let limit = tick_down(tick_down(price));
    let result = buy(&mut market, 1, code, 100, Some(limit), 5_000_000, T0).unwrap();
    assert_eq!(result.filled, 0);
    let id = result.resting.unwrap();
    let order = market
        .orders
        .iter()
        .find(|order| order.id == id)
        .unwrap()
        .clone();
    assert_eq!(order.reserved, limit * 100 + quiet().fee(limit * 100));
    assert_eq!(balances(&market)[&1], -order.reserved);
    assert!(
        market.cancel_order(2, id).is_err(),
        "남의 주문은 취소 못 한다"
    );
    market.cancel_order(1, id).unwrap();
    assert_eq!(balances(&market)[&1], 0, "취소하면 다 돌려받는다");
    assert!(market.orders.is_empty());
}

#[test]
fn resting_limit_orders_fill_when_the_price_reaches_them() {
    let rules = quiet();
    let mut market = market();
    let code = "100070";
    let price = market.companies[code].price;
    let limit = tick_down(price);
    let result = buy(&mut market, 1, code, 10, Some(limit), 1_000_000, T0).unwrap();
    assert!(result.resting.is_some());
    // 시세가 지정가까지 내려오면 체결된다.
    market.companies.get_mut(code).unwrap().sentiment -= 0.01;
    let report = market.tick(T0 + 3 * TICK_MS, &rules, &mut rng());
    assert_eq!(report.fills.len(), 1);
    assert_eq!(market.accounts[&1].positions[code].qty, 10);
    assert!(market.orders.is_empty());
    // 체결되고 남은 증거금은 돌려받는다.
    let spent = market.accounts[&1].positions[code].cost;
    assert_eq!(balances(&market)[&1], -spent);
}

#[test]
fn players_trade_with_each_other_inside_the_band() {
    let mut market = market();
    let code = "100070";
    buy(&mut market, 2, code, 100, None, 1_000_000_000, T0).unwrap();
    let price = market.companies[code].quote_mid();
    // 2번이 시장조성자 매도 1호가와 같은 값에 팔자를 낸다 (같은 값이면 플레이어 주문이 먼저).
    let ask = market.book(code, &quiet()).unwrap().asks[0].price;
    sell(&mut market, 2, code, 50, Some(ask), T0).unwrap();
    let before = balances(&market);
    let result = buy(&mut market, 1, code, 50, None, 1_000_000_000, T0).unwrap();
    assert_eq!(result.filled, 50);
    assert_eq!(
        market.accounts[&2].positions[code].qty, 50,
        "2번의 주식이 넘어간다"
    );
    let after = balances(&market);
    let notional = ask * 50;
    let rules = quiet();
    assert_eq!(
        after[&2] - before[&2],
        notional - rules.fee(notional) - rules.tax(notional)
    );
    // 시세와 동떨어진 지정가는 받지 않는다.
    assert!(sell(&mut market, 2, code, 10, Some(price * 2), T0).is_err());
}

#[test]
fn order_rules_are_enforced() {
    let mut market = market();
    let code = "100120";
    assert!(
        sell(&mut market, 1, code, 1, None, T0).is_err(),
        "없는 주식은 못 판다"
    );
    assert!(buy(&mut market, 1, code, 0, None, 1_000_000, T0).is_err());
    let rules = quiet();
    let company = &market.companies[code];
    let cap = company.adv * rules.order_limit_pct / 100;
    assert!(
        buy(&mut market, 1, code, cap + 1, None, i64::MAX / 4, T0).is_err(),
        "한 번에 너무 많이"
    );
    // 1인 보유 한도: 발행 주식의 5%.
    let limit = market.companies[code].shares * rules.holding_limit_bp / BP;
    market.account_mut(1, "U1").positions.insert(
        code.to_string(),
        Position {
            qty: limit - 10,
            cost: 1,
            ..Position::default()
        },
    );
    let error = buy(&mut market, 1, code, 11, None, i64::MAX / 4, T0).unwrap_err();
    assert!(error.contains("한도"), "{error}");
    assert!(buy(&mut market, 1, code, 10, None, i64::MAX / 4, T0).is_ok());
    // 가진 코인만큼만 산다.
    let result = buy(&mut market, 3, "100070", 100, None, 10_000, T0).unwrap();
    assert!(result.filled > 0 && result.filled < 2);
    assert!(result.notional + result.fee <= 10_000);
}

#[test]
fn trading_stops_at_the_price_limit_and_during_halts() {
    let mut market = market();
    let code = "100070";
    let (_, upper) = market.limits(&market.companies[code], &quiet());
    {
        let company = market.companies.get_mut(code).unwrap();
        company.price = upper;
        company.mid = upper;
    }
    let result = buy(&mut market, 1, code, 10, None, 1_000_000_000, T0).unwrap();
    assert_eq!(result.filled, 0, "상한가에는 파는 사람이 없다");
    market.companies.get_mut(code).unwrap().admin_halt = true;
    assert!(buy(&mut market, 1, code, 10, None, 1_000_000, T0).is_err());
}

// ------------------------------------------------------------ 플레이어 회사

fn found(market: &mut StockMarket, user: u64, capital: i64) -> String {
    market
        .found_company(
            user,
            &format!("U{user}"),
            "테스트상사",
            Sector::Game,
            capital,
            T0,
            &quiet(),
        )
        .unwrap()
}

#[test]
fn founding_a_company_takes_capital_and_fee() {
    let mut market = market();
    let rules = quiet();
    let code = found(&mut market, 1, 2_000_000);
    let company = &market.companies[&code];
    assert_eq!(company.status, CompanyStatus::Private);
    assert_eq!(company.shares, 400);
    assert_eq!(company.equity, 2_000_000);
    assert_eq!(market.accounts[&1].positions[&code].qty, 400);
    let fee = bp_part(2_000_000, rules.found_fee_bp);
    assert_eq!(balances(&market)[&1], -(2_000_000 + fee));
    assert_eq!(balances(&market)[&TREASURY_USER], fee);
    // 한 사람 한 회사, 이름 중복 금지, 최소 자본.
    assert!(
        market
            .found_company(1, "U1", "다른회사", Sector::Bio, 2_000_000, T0, &rules)
            .is_err()
    );
    assert!(
        market
            .found_company(2, "U2", "테스트 상사", Sector::Bio, 2_000_000, T0, &rules)
            .is_err()
    );
    assert!(
        market
            .found_company(2, "U2", "새회사", Sector::Bio, 10_000, T0, &rules)
            .is_err()
    );
}

#[test]
fn an_ipo_allocates_lists_and_locks_the_founder() {
    let rules = quiet();
    let mut market = market();
    let code = found(&mut market, 1, 2_000_000);
    let day = rules.day_ms();
    assert!(
        market
            .start_ipo(1, &code, 5_000, 200, T0 + 10, &rules)
            .is_err(),
        "설립 1게임일 뒤부터"
    );
    let now = T0 + day;
    assert!(
        market.start_ipo(2, &code, 5_000, 200, now, &rules).is_err(),
        "대표만"
    );
    assert!(
        market
            .start_ipo(1, &code, 50_000, 200, now, &rules)
            .is_err(),
        "공모가 범위"
    );
    market.start_ipo(1, &code, 6_000, 200, now, &rules).unwrap();
    assert!(
        market.subscribe(1, "U1", &code, 10, now).is_err(),
        "대표는 청약 못 한다"
    );
    // 청약 300주 (200주 공모, 1.5:1).
    market.subscribe(2, "U2", &code, 200, now).unwrap();
    market.subscribe(3, "U3", &code, 100, now).unwrap();
    let before = balances(&market);
    assert_eq!(before[&2], -1_200_000);
    let report = market.tick(now + day + TICK_MS, &rules, &mut rng());
    let company = &market.companies[&code];
    assert_eq!(company.status, CompanyStatus::Listed);
    assert_eq!(company.shares, 600);
    let alloc2 = market.accounts[&2].positions[&code].qty;
    let alloc3 = market.accounts[&3].positions[&code].qty;
    assert_eq!(alloc2 + alloc3, 200);
    assert!(
        alloc3 >= 50,
        "균등 배정으로 작은 청약자도 받는다 ({alloc3})"
    );
    // 배정 안 된 증거금은 돌려받고, 공모 대금(수수료 뺀)은 회사 현금이 된다.
    let after = balances(&market);
    assert_eq!(after[&2], -(alloc2 * 6_000));
    let proceeds = 200 * 6_000;
    let fee = bp_part(proceeds, rules.ipo_fee_bp);
    assert_eq!(company.equity, 2_000_000 + proceeds - fee);
    // 첫날 가격은 공모가의 60~400% 안.
    let (lower, upper) = company.first_day_band.unwrap();
    assert!(company.price >= lower && company.price <= upper);
    assert!(
        report
            .news
            .iter()
            .any(|news| news.kind == NewsKind::Listing)
    );
    // 대표 지분은 보호예수.
    let founder = market.accounts[&1].positions[&code].clone();
    assert_eq!(founder.available(now + day + TICK_MS), 0);
    assert!(sell(&mut market, 1, &code, 10, None, now + day + 2 * TICK_MS).is_err());
    assert_eq!(founder.available(now + day * (rules.lockup_days + 2)), 400);
}

#[test]
fn an_undersubscribed_ipo_is_called_off() {
    let rules = quiet();
    let mut market = market();
    let code = found(&mut market, 1, 2_000_000);
    let now = T0 + rules.day_ms();
    market.start_ipo(1, &code, 5_000, 400, now, &rules).unwrap();
    market.subscribe(2, "U2", &code, 100, now).unwrap();
    market.tick(now + rules.day_ms() + TICK_MS, &rules, &mut rng());
    assert_eq!(market.companies[&code].status, CompanyStatus::Private);
    assert_eq!(
        balances(&market)[&2],
        0,
        "무산되면 증거금을 모두 돌려받는다"
    );
}

fn listed_company(market: &mut StockMarket) -> (String, i64) {
    let rules = quiet();
    let code = found(market, 1, 2_000_000);
    let now = T0 + rules.day_ms();
    market.start_ipo(1, &code, 5_000, 200, now, &rules).unwrap();
    market.subscribe(2, "U2", &code, 200, now).unwrap();
    let now = now + rules.day_ms() + TICK_MS;
    market.tick(now, &rules, &mut rng());
    assert_eq!(market.companies[&code].status, CompanyStatus::Listed);
    (code, now)
}

#[test]
fn small_player_companies_still_show_trading_volume() {
    // 600주짜리 플레이어 회사: 틱마다 1주 미만인 거래량이 버려지지 않고 쌓여 하루 거래량과 분봉에 보인다.
    let rules = quiet();
    let mut market = market();
    let (code, now) = listed_company(&mut market);
    assert_eq!(market.companies[&code].shares, 600);
    // 다음 게임일 하루를 통째로 돌린다 (거래량은 게임일마다 새로 센다).
    let end = (rules.day_of(now) + 2) * rules.day_ms() - 1;
    market.tick(end, &rules, &mut rng());
    let company = &market.companies[&code];
    assert!(
        company.volume >= company.shares / 5 && company.volume <= company.shares * 2,
        "하루 거래량 {}주",
        company.volume
    );
    let series = &market.candles.series[&code];
    let last_day = series.minute.iter().rev().take(60).collect::<Vec<_>>();
    let busy = last_day.iter().filter(|candle| candle.v > 0).count();
    assert!(busy >= 40, "분봉 60개 중 {busy}개에만 거래량이 있다");
}

#[test]
fn a_declared_dividend_is_paid_next_day_and_keeps_wealth_neutral() {
    let rules = quiet();
    let mut market = market();
    let (code, now) = listed_company(&mut market);
    let shares = market.companies[&code].shares;
    let equity = market.companies[&code].equity;
    assert!(
        market.declare_dividend(2, &code, 100, now, &rules).is_err(),
        "대표만"
    );
    assert!(
        market
            .declare_dividend(1, &code, equity, now, &rules)
            .is_err(),
        "현금보다 많이는 못 준다"
    );
    market.declare_dividend(1, &code, 500, now, &rules).unwrap();
    let before = balances(&market);
    let price_before = market.companies[&code].price;
    market.tick(now + rules.day_ms() + TICK_MS, &rules, &mut rng());
    let after = balances(&market);
    let holding2 = market.accounts[&2].positions[&code].qty;
    assert_eq!(after[&2] - before[&2], holding2 * 500);
    assert_eq!(market.companies[&code].equity, equity - shares * 500);
    // 배당락: 주가가 배당금만큼 내려간다.
    let price_after = market.companies[&code].price;
    assert!(
        (price_before - 500 - price_after).abs() <= tick_size(price_after) * 2,
        "{price_before} → {price_after}"
    );
}

#[test]
fn a_bankrupt_company_goes_through_liquidation_trading_and_is_delisted() {
    let rules = quiet();
    let mut market = market();
    let (code, now) = listed_company(&mut market);
    // 대표가 매수 주문을 걸어 둔다.
    let price = market.companies[&code].price;
    buy(
        &mut market,
        3,
        &code,
        5,
        Some(tick_down(price)),
        1_000_000,
        now,
    )
    .unwrap();
    // 큰 손실을 낸 실적 발표.
    {
        let company = market.companies.get_mut(&code).unwrap();
        company.equity = -1;
        company.next_earnings_at = now + TICK_MS;
        company.risk = 1;
    }
    let report = market.tick(now + 2 * TICK_MS, &rules, &mut rng());
    assert!(
        report
            .news
            .iter()
            .any(|news| news.headline.contains("파산"))
    );
    assert!(matches!(
        market.companies[&code].status,
        CompanyStatus::Liquidating { trading: true, .. }
    ));
    assert!(
        market.orders.is_empty(),
        "주문은 취소되고 증거금을 돌려받는다"
    );
    assert_eq!(balances(&market).get(&3).copied().unwrap_or(0), 0);
    // 정리매매가 끝나면 상장폐지되고 주식은 사라진다.
    market.tick(
        now + (LIQUIDATION_DAYS + 1) * rules.day_ms(),
        &rules,
        &mut rng(),
    );
    assert!(matches!(
        market.companies[&code].status,
        CompanyStatus::Delisted { .. }
    ));
    assert!(
        market
            .accounts
            .values()
            .all(|account| !account.positions.contains_key(&code))
    );
}

#[test]
fn capital_impairment_marks_then_delists_and_distributes_what_is_left() {
    let rules = quiet();
    let mut market = market();
    let (code, now) = listed_company(&mut market);
    let paid_in = market.companies[&code].paid_in;
    {
        let company = market.companies.get_mut(&code).unwrap();
        company.equity = paid_in / 3;
        company.next_earnings_at = now + TICK_MS;
        company.risk = 1;
    }
    market.tick(now + 2 * TICK_MS, &rules, &mut rng());
    assert!(
        market.companies[&code].managed_since.is_some(),
        "관리종목 지정"
    );
    // 다음 분기에도 그대로면 상장폐지 결정 (한 주를 기다리지 않고 발표 시각을 당긴다).
    let next = now + 4 * TICK_MS;
    {
        let company = market.companies.get_mut(&code).unwrap();
        company.equity = paid_in / 3;
        company.next_earnings_at = next;
    }
    market.tick(next + TICK_MS, &rules, &mut rng());
    assert!(matches!(
        market.companies[&code].status,
        CompanyStatus::Liquidating { .. }
    ));
    let equity = market.companies[&code].equity;
    let shares = market.companies[&code].shares;
    let holding2 = market.accounts[&2].positions[&code].qty;
    let before = balances(&market);
    market.tick(
        next + (LIQUIDATION_DAYS + 1) * rules.day_ms(),
        &rules,
        &mut rng(),
    );
    let after = balances(&market);
    assert!(matches!(
        market.companies[&code].status,
        CompanyStatus::Delisted { .. }
    ));
    assert_eq!(
        after[&2] - before[&2],
        holding2 * (equity / shares),
        "남은 자본을 나눠 받는다"
    );
}

#[test]
fn a_founder_with_a_majority_can_dissolve_and_everyone_gets_book_value() {
    let rules = quiet();
    let mut market = market();
    let (code, now) = listed_company(&mut market);
    assert!(market.dissolve(2, &code, now, &rules).is_err(), "대표만");
    let equity = market.companies[&code].equity;
    let shares = market.companies[&code].shares;
    market.dissolve(1, &code, now, &rules).unwrap();
    assert!(
        buy(&mut market, 3, &code, 1, None, 1_000_000, now).is_err(),
        "청산 대기 중에는 거래정지"
    );
    let before = balances(&market);
    market.tick(now + rules.day_ms() + TICK_MS, &rules, &mut rng());
    let after = balances(&market);
    let per_share = equity / shares;
    assert_eq!(
        after[&1] - before.get(&1).copied().unwrap_or(0),
        400 * per_share
    );
    assert_eq!(after[&2] - before[&2], market_qty_before(200) * per_share);
    assert!(matches!(
        market.companies[&code].status,
        CompanyStatus::Delisted { .. }
    ));
}

fn market_qty_before(qty: i64) -> i64 {
    qty
}

#[test]
fn a_private_company_can_be_dissolved_for_its_cash() {
    let rules = quiet();
    let mut market = market();
    let code = found(&mut market, 1, 2_000_000);
    let fee = bp_part(2_000_000, rules.found_fee_bp);
    market.dissolve(1, &code, T0 + 10, &rules).unwrap();
    assert_eq!(balances(&market)[&1], -fee, "설립 수수료만 잃는다");
    // 회사가 없어졌으니 다시 세울 수 있다.
    assert!(
        market
            .found_company(
                1,
                "U1",
                "두번째회사",
                Sector::Bank,
                1_000_000,
                T0 + 20,
                &rules
            )
            .is_ok()
    );
}

#[test]
fn a_rights_offering_raises_cash_from_shareholders_who_exercise() {
    let rules = quiet();
    let mut market = market();
    let (code, now) = listed_company(&mut market);
    let price = market.companies[&code].price;
    let offer = floor_tick(price * 8 / 10);
    market
        .start_rights(1, &code, 300, offer, now, &rules)
        .unwrap();
    // 보유 비율대로 신주인수권: 대표 400/600 → 200주, 2번 200/600 → 100주.
    assert!(market.rights_cost(2, &code, 101).is_err());
    let cost = market.exercise_rights(2, "U2", &code, 100, now).unwrap();
    assert_eq!(cost, 100 * offer);
    let equity = market.companies[&code].equity;
    market.tick(now + rules.day_ms() + TICK_MS, &rules, &mut rng());
    let company = &market.companies[&code];
    assert_eq!(company.shares, 700, "행사한 만큼만 발행");
    assert_eq!(company.equity, equity + cost);
    assert_eq!(market.accounts[&2].positions[&code].qty, 300);
}

#[test]
fn a_buyback_retires_shares_with_company_cash() {
    let rules = quiet();
    let mut market = market();
    let (code, now) = listed_company(&mut market);
    // 2번이 시장조성자에게 팔아 재고가 생긴다.
    sell(&mut market, 2, &code, 50, None, now).unwrap();
    assert!(market.companies[&code].lp_inventory.unwrap() > 0);
    let shares = market.companies[&code].shares;
    let equity = market.companies[&code].equity;
    market
        .start_buyback(1, &code, 200_000, now, &rules)
        .unwrap();
    assert!(
        sell(&mut market, 1, &code, 1, None, now + rules.day_ms() * 30).is_err(),
        "매입 중 대표 매도 금지"
    );
    market.tick(now + rules.day_ms() + TICK_MS, &rules, &mut rng());
    let company = &market.companies[&code];
    assert!(company.buyback.is_none());
    assert!(company.shares < shares, "산 주식은 소각");
    assert!(company.equity < equity);
}

/// 기관 배정을 켠 규칙 (실제 운영 기본값).
fn with_institutions() -> StockRules {
    StockRules {
        ipo_institution_bp: StockRules::default().ipo_institution_bp,
        ..quiet()
    }
}

#[test]
fn institutions_take_more_of_a_cheaper_ipo() {
    let rules = with_institutions();
    // 공모가가 주당 순자산 이하면 30%, 1.5배면 15%, 2배 이상이면 없다.
    assert_eq!(institution_shares(200, 5_000, 5_000.0, &rules), 60);
    assert_eq!(institution_shares(200, 4_000, 5_000.0, &rules), 60);
    assert_eq!(institution_shares(200, 7_500, 5_000.0, &rules), 30);
    assert_eq!(institution_shares(200, 10_000, 5_000.0, &rules), 0);
    assert_eq!(institution_shares(200, 5_000, 0.0, &rules), 0);
    assert_eq!(institution_shares(200, 5_000, 5_000.0, &quiet()), 0);
}

#[test]
fn an_ipo_leaves_a_public_float_with_the_market_maker() {
    let rules = with_institutions();
    let mut market = market();
    let code = found(&mut market, 1, 2_000_000);
    let now = T0 + rules.day_ms();
    market.start_ipo(1, &code, 5_000, 200, now, &rules).unwrap();
    assert!(
        market
            .news
            .iter()
            .any(|news| news.headline.contains("기관 배정 예정 60주")),
        "청약 공시에 기관 몫을 알린다"
    );
    // 일반 청약분(140주)보다 많이 청약하면 140주를 나눠 받는다.
    market.subscribe(2, "U2", &code, 200, now).unwrap();
    let minted_before = market.stats.lp_sold;
    let now = now + rules.day_ms() + TICK_MS;
    market.tick(now, &rules, &mut rng());
    let company = &market.companies[&code];
    assert_eq!(company.status, CompanyStatus::Listed);
    assert_eq!(company.shares, 600);
    assert_eq!(company.lp_inventory, Some(60));
    assert_eq!(market.accounts[&2].positions[&code].qty, 140);
    // 기관 납입금(60주 × 5,000)은 새로 생긴 코인으로 회사 현금이 되고, 공모 수수료는 전체 대금에서 뗀다.
    let raised = 200 * 5_000;
    assert_eq!(market.stats.lp_sold - minted_before, 60 * 5_000);
    assert_eq!(
        company.equity,
        2_000_000 + raised - bp_part(raised, rules.ipo_fee_bp)
    );
    assert_eq!(
        balances(&market)[&2],
        -140 * 5_000,
        "배정 안 된 증거금은 돌려받는다"
    );
    assert!(
        market
            .news
            .iter()
            .any(|news| news.kind == NewsKind::Listing && news.headline.contains("기관 배정 60주"))
    );
    // 이제 상장 직후에도 매도 호가가 있어 다른 사람이 살 수 있다.
    let book = market.book(&code, &rules).unwrap();
    assert!(!book.asks.is_empty());
    let bought = buy(&mut market, 3, &code, 5, None, 1_000_000, now).unwrap();
    assert_eq!(bought.filled, 5);
    assert_eq!(market.companies[&code].lp_inventory, Some(55));
}

#[test]
fn a_buyback_buys_the_float_through_the_day_and_lifts_the_price() {
    let rules = with_institutions();
    let mut market = market();
    let code = found(&mut market, 1, 2_000_000);
    let now = T0 + rules.day_ms();
    market.start_ipo(1, &code, 5_000, 200, now, &rules).unwrap();
    market.subscribe(2, "U2", &code, 200, now).unwrap();
    let now = now + rules.day_ms() + TICK_MS;
    market.tick(now, &rules, &mut rng());
    let (shares, equity, price) = {
        let company = &market.companies[&code];
        (company.shares, company.equity, company.price)
    };
    market
        .start_buyback(1, &code, 200_000, now, &rules)
        .unwrap();
    assert!(
        market.companies[&code].pending_news > 0.0,
        "매입 공시에 주가가 반응한다"
    );
    // 예산을 하루에 고르게 쓴다: 반나절이면 대략 절반.
    let half = now + rules.day_ms() / 2;
    market.tick(half, &rules, &mut rng());
    let buyback = market.companies[&code].buyback.clone().unwrap();
    let used = buyback.total - buyback.budget;
    assert!(
        used > 60_000 && used < 140_000,
        "반나절에 {used} 사용 (한꺼번에 쓰지 않는다)"
    );
    market.tick(now + rules.day_ms() + TICK_MS, &rules, &mut rng());
    let company = &market.companies[&code];
    assert!(company.buyback.is_none());
    let retired = shares - company.shares;
    assert!(retired >= 30, "유통 물량에서 {retired}주를 사서 소각");
    assert_eq!(company.lp_inventory, Some(60 - retired));
    assert!(equity - company.equity >= 190_000, "예산을 거의 다 쓴다");
    assert!(
        company.price > price,
        "매수세로 주가가 오른다 ({price} → {})",
        company.price
    );
    assert!(market.news.iter().any(|news| {
        news.headline
            .contains(&format!("자사주 매입 완료: {retired}주 소각"))
    }));
}

#[test]
fn holders_can_sell_into_a_buyback_but_the_founder_cannot() {
    let rules = quiet();
    let mut market = market();
    let (code, now) = listed_company(&mut market);
    market
        .start_buyback(1, &code, 300_000, now, &rules)
        .unwrap();
    // 회사의 매수 호가가 호가창 맨 위(현재가)에 보인다.
    let mid = market.companies[&code].quote_mid();
    let book = market.book(&code, &rules).unwrap();
    assert_eq!(book.bids[0].price, mid);
    assert!(book.bids[0].qty >= 50);
    let before = balances(&market)[&2];
    let sold = sell(&mut market, 2, &code, 20, None, now).unwrap();
    assert_eq!(sold.filled, 20);
    assert_eq!(
        sold.avg_price, mid,
        "시장조성자 매수 호가보다 높은 현재가에 판다"
    );
    let company = &market.companies[&code];
    assert_eq!(company.shares, 580, "회사가 산 주식은 바로 소각");
    assert_eq!(company.lp_inventory, Some(0));
    let buyback = company.buyback.clone().unwrap();
    assert_eq!(buyback.bought, 20);
    let notional = 20 * mid;
    assert_eq!(buyback.budget, 300_000 - notional - rules.fee(notional));
    assert_eq!(
        balances(&market)[&2] - before,
        notional - rules.fee(notional) - rules.tax(notional)
    );
    assert!(
        sell(&mut market, 1, &code, 1, None, now + rules.day_ms() * 30).is_err(),
        "대표는 매입 중 팔 수 없다"
    );
}

#[test]
fn resting_sell_orders_fill_against_a_new_buyback() {
    let rules = quiet();
    let mut market = market();
    let (code, now) = listed_company(&mut market);
    // 매입 전에 현재가에 걸어 둔 매도 주문은 시장조성자 매수 호가(더 낮음)와는 체결되지 않고 남는다.
    let mid = market.companies[&code].quote_mid();
    let resting = sell(&mut market, 2, &code, 15, Some(mid), now).unwrap();
    assert_eq!(resting.filled, 0);
    market
        .start_buyback(1, &code, 300_000, now, &rules)
        .unwrap();
    // 다음 틱에 회사의 매수 호가와 체결된다.
    market.tick(now + TICK_MS, &rules, &mut rng());
    let company = &market.companies[&code];
    assert_eq!(company.buyback.as_ref().unwrap().bought, 15);
    assert_eq!(company.shares, 585);
    assert!(market.orders.iter().all(|order| order.user != 2));
}

#[test]
fn risk_and_description_are_the_founders_calls() {
    let mut market = market();
    let code = found(&mut market, 1, 2_000_000);
    assert!(market.set_risk(2, &code, 3, T0).is_err());
    assert!(market.set_risk(1, &code, 6, T0).is_err());
    market.set_risk(1, &code, 5, T0 + 1).unwrap();
    assert!(market.set_risk(1, &code, 4, T0 + 2).is_err(), "1주에 한 번");
    assert!(market.set_risk(1, &code, 4, T0 + 2 + WEEK_MS).is_ok());
    market.set_description(1, &code, "재밌는 회사").unwrap();
    assert!(market.set_description(1, &code, &"가".repeat(121)).is_err());
}

// ------------------------------------------------------------ 시스템 회사·기타

#[test]
fn system_ipos_refill_the_market_and_earnings_pay_dividends() {
    let rules = quiet();
    let mut market = market();
    // 목표보다 적으면 새 회사가 공모에 부쳐진다.
    let removed = market.companies.remove("100120").unwrap();
    // 새 공모는 다음 게임일부터 (하루에 한 번).
    let report = market.tick(T0 + rules.day_ms() + TICK_MS, &rules, &mut rng());
    let new = market
        .companies
        .values()
        .find(|company| matches!(company.status, CompanyStatus::Subscription(_)))
        .expect("신규 공모")
        .code
        .clone();
    assert!(
        report
            .news
            .iter()
            .any(|news| news.kind == NewsKind::Listing)
    );
    assert_ne!(new, removed.code);
    let price = match &market.companies[&new].status {
        CompanyStatus::Subscription(offering) => offering.price,
        _ => unreachable!(),
    };
    market
        .subscribe(5, "U5", &new, 10, T0 + rules.day_ms() + TICK_MS)
        .unwrap();
    market.tick(T0 + 4 * rules.day_ms() + 2 * TICK_MS, &rules, &mut rng());
    assert_eq!(market.companies[&new].status, CompanyStatus::Listed);
    assert_eq!(market.accounts[&5].positions[&new].qty, 10);
    assert_eq!(
        market.stats.ipo_burned,
        price * 10,
        "시스템 공모 대금은 사라진다"
    );

    // 배당주 실적: 이익이 나면 배당하고 배당락.
    let code = "100100";
    let now = T0 + 4 * rules.day_ms() + 2 * TICK_MS;
    buy(&mut market, 6, code, 100, None, 1_000_000_000, now).unwrap();
    market.companies.get_mut(code).unwrap().next_earnings_at = now + TICK_MS;
    let before = balances(&market);
    market.tick(now + 2 * TICK_MS, &rules, &mut rng());
    let result = market.companies[code].quarters.back().cloned().unwrap();
    if result.profit > 0 {
        assert!(result.dividend > 0);
        assert_eq!(balances(&market)[&6] - before[&6], 100 * result.dividend);
    }
}

#[test]
fn allocation_is_half_equal_half_proportional() {
    assert_eq!(
        allocate(&[100, 50], 200),
        vec![100, 50],
        "모자라지 않으면 다 받는다"
    );
    let result = allocate(&[1_000, 10, 10], 100);
    assert_eq!(result.iter().sum::<i64>(), 100);
    assert!(
        result[1] >= 10 && result[2] >= 10,
        "작은 청약자도 균등 배정 {result:?}"
    );
    let result = allocate(&[7, 7, 7], 10);
    assert_eq!(result.iter().sum::<i64>(), 10);
    assert!(result.iter().all(|qty| *qty >= 3));
    assert_eq!(allocate(&[], 10), Vec::<i64>::new());
}

#[test]
fn the_volatility_interruption_halts_a_jumping_stock() {
    let rules = StockRules {
        vol_pct: 0,
        news_pct: 0,
        ..StockRules::default()
    };
    let mut market = StockMarket::new(T0, &rules);
    let code = "100060";
    market.companies.get_mut(code).unwrap().sentiment += 0.2;
    let report = market.tick(T0 + TICK_MS, &rules, &mut rng());
    assert!(report.news.iter().any(|news| news.kind == NewsKind::Halt));
    let company = &market.companies[code];
    assert!(company.halted_until > T0);
    let order = market.place_order(
        &OrderRequest {
            user: 1,
            name: "U1".into(),
            code: code.into(),
            side: Side::Buy,
            qty: 1,
            limit: None,
        },
        1_000_000,
        T0 + TICK_MS + 1,
        &rules,
    );
    assert!(order.unwrap_err().contains("변동성 완화장치"));
}

#[test]
fn portfolio_value_counts_stocks_and_locked_coins() {
    let mut market = market();
    let code = "100070";
    let bought = buy(&mut market, 1, code, 100, None, 1_000_000_000, T0).unwrap();
    let price = market.companies[code].price;
    let limit = tick_down(tick_down(price));
    let resting = buy(&mut market, 1, code, 10, Some(limit), 1_000_000, T0).unwrap();
    let reserved = market.orders[0].reserved;
    assert!(resting.resting.is_some());
    assert_eq!(market.portfolio_value(1, T0), price * 100 + reserved);
    assert!(bought.filled == 100);
    let ranking = market.ranking(T0);
    assert_eq!(ranking[0].user, 1);
    // 사용한 장부를 반영했다고 하면 지운다.
    let last = market.journal.last().unwrap().id;
    market.prune_journal(last);
    assert!(market.journal.is_empty());
}

#[test]
fn the_market_survives_a_json_round_trip() {
    let mut market = market();
    let (code, now) = listed_company(&mut market);
    buy(&mut market, 3, &code, 5, None, 1_000_000, now).unwrap();
    let text = serde_json::to_string(&market).unwrap();
    let loaded: StockMarket = serde_json::from_str(&text).unwrap();
    // 부동소수는 마지막 자리가 다를 수 있어 정수 상태만 같게 본다.
    assert_eq!(loaded.accounts, market.accounts);
    assert_eq!(loaded.journal, market.journal);
    assert_eq!(loaded.orders, market.orders);
    for (code, company) in &market.companies {
        let other = &loaded.companies[code];
        assert_eq!(
            (other.price, other.shares, other.equity, &other.status),
            (
                company.price,
                company.shares,
                company.equity,
                &company.status
            )
        );
        assert!((other.sentiment - company.sentiment).abs() < 1e-12);
    }
}

// ------------------------------------------------------------ 게임일 넘기기

#[test]
fn skipping_game_days_runs_the_calendar_forward() {
    let rules = quiet();
    let mut market = market();
    let code = found(&mut market, 1, 1_000_000);
    let day0 = rules.day_of(market.clock(T0));

    // 설립한 날에는 상장할 수 없다. 하루를 넘기면 된다.
    let mut real_now = T0 + 1_000;
    let item = market.skip_game_days(market.clock(real_now), 1, &rules);
    assert_eq!(item.kind, NewsKind::Market);
    let now = market.clock(real_now);
    assert_eq!(rules.day_of(now), day0 + 1);
    assert_eq!(now % rules.day_ms(), 0, "다음 게임일이 막 시작한 시각");
    market.tick(now, &rules, &mut rng());
    market.start_ipo(1, &code, 5_000, 100, now, &rules).unwrap();
    market.subscribe(2, "U2", &code, 80, now).unwrap();

    // 청약은 1게임일 동안 받는다. 또 하루를 넘기면 마감되고 상장한다.
    real_now += 1_000;
    market.skip_game_days(market.clock(real_now), 1, &rules);
    let now = market.clock(real_now);
    market.tick(now, &rules, &mut rng());
    let company = &market.companies[&code];
    assert_eq!(company.status, CompanyStatus::Listed);
    assert_eq!(rules.day_of(now), day0 + 2);
    assert!(company.listed_at > 0 && company.listed_at <= now);
    assert_eq!(market.accounts[&2].positions[&code].qty, 80);

    // 한 번에 넘길 수 있는 날에는 상한이 있다.
    let before = market.time_shift_ms;
    market.skip_game_days(market.clock(real_now), 1_000, &rules);
    assert!(market.time_shift_ms - before <= MAX_SKIP_DAYS * rules.day_ms());
}

// ------------------------------------------------------------ 운영 기록

#[test]
fn market_activity_is_logged_for_the_log_channel() {
    let mut market = market();
    let _ = market.take_logs();
    buy(&mut market, 1, "100010", 3, None, 10_000_000, T0 + 10).unwrap();
    let low = floor_tick(market.companies["100010"].price * 95 / 100);
    let resting = buy(&mut market, 1, "100010", 2, Some(low), 10_000_000, T0 + 20).unwrap();
    let id = resting.resting.expect("아래 가격의 지정가는 호가에 남는다");
    market.cancel_order(1, id).unwrap();
    let logs = market.take_logs();
    assert!(
        logs.iter()
            .any(|line| line.contains("U1") && line.contains("3주 매수 체결")),
        "{logs:?}"
    );
    assert!(
        logs.iter().any(|line| line.contains("지정가 매수 2주")),
        "{logs:?}"
    );
    assert!(
        logs.iter()
            .any(|line| line.contains(&format!("주문 #{id} 주문 취소"))),
        "{logs:?}"
    );
    assert!(market.take_logs().is_empty(), "꺼낸 기록은 비운다");

    found(&mut market, 5, 1_000_000);
    assert!(
        market
            .take_logs()
            .iter()
            .any(|line| line.contains("U5") && line.contains("설립"))
    );
}

#[test]
fn skipped_time_reads_naturally() {
    assert_eq!(duration_text(12_000), "12초");
    assert_eq!(duration_text(40 * MINUTE_MS), "40분");
    assert_eq!(duration_text(2 * HOUR_MS), "2시간");
    assert_eq!(duration_text(2 * HOUR_MS + 5 * MINUTE_MS), "2시간 5분");
}

// ------------------------------------------------------------ 뉴스 채널

#[test]
fn every_news_item_waits_for_the_news_channel() {
    let rules = quiet();
    let mut market = market();
    let opening = market.take_news_outbox();
    assert!(
        opening.iter().any(|item| item.headline.contains("개장")),
        "개장 소식도 올린다"
    );

    // 명령으로 생긴 공시 (설립·공모 시작·관리자 거래정지)도 틱을 기다리지 않고 채널로 간다.
    let code = found(&mut market, 1, 1_000_000);
    let founded = market.take_news_outbox();
    assert!(
        founded
            .iter()
            .any(|item| item.kind == NewsKind::Disclosure && item.headline.contains("설립"))
    );
    let now = T0 + rules.day_ms();
    market.tick(now, &rules, &mut rng());
    let _ = market.take_news_outbox();
    market.start_ipo(1, &code, 5_000, 100, now, &rules).unwrap();
    assert!(
        market
            .take_news_outbox()
            .iter()
            .any(|item| item.headline.contains("공모주 청약 시작"))
    );
    market.set_admin_halt("100010", true, now).unwrap();
    let halted = market.take_news_outbox();
    assert!(
        halted
            .iter()
            .any(|item| item.kind == NewsKind::Disclosure && item.headline.contains("거래정지")),
        "관리자 거래정지는 공시로 올린다"
    );

    // 회사가 사라지는 것도 올라간다 (비상장 회사 해산).
    let other = market
        .found_company(3, "U3", "사라질상사", Sector::Bio, 1_000_000, now, &rules)
        .unwrap();
    let _ = market.take_news_outbox();
    market.dissolve(3, &other, now, &rules).unwrap();
    assert!(
        market
            .take_news_outbox()
            .iter()
            .any(|item| item.kind == NewsKind::Delisting && item.headline.contains("해산"))
    );
    assert!(market.take_news_outbox().is_empty(), "꺼낸 뉴스는 비운다");
}

// stocks/market.rs — 시장 만들기, 5초마다 가격 갱신(팩터 모형), 게임일 넘기기, 지수, 뉴스, 장부

use super::catalog::CATALOG;
use super::model::*;
use super::news::{company_news, macro_news};
use super::price::*;
use rand::RngCore;
use std::collections::{BTreeMap, VecDeque};

/// 한 번에 따라잡을 최대 틱 수 (봇이 오래 꺼져 있었을 때 1주치까지만 흉내 낸다).
const MAX_CATCHUP_TICKS: i64 = WEEK_MS / TICK_MS;
/// 잡음이 절반으로 돌아가는 데 걸리는 게임일.
const NOISE_HALF_LIFE_DAYS: f64 = 3.0;
/// 투자 심리가 절반으로 돌아가는 데 걸리는 실제 시간.
const SENTIMENT_HALF_LIFE_MS: f64 = 2.0 * WEEK_MS as f64;
/// 뉴스 충격이 절반쯤 반영되는 시간.
const NEWS_HALF_LIFE_MS: f64 = 10.0 * MINUTE_MS as f64;
/// 주문 일시 충격이 절반으로 사라지는 시간.
const IMPACT_HALF_LIFE_MS: f64 = 3.0 * MINUTE_MS as f64;
/// 업종 팩터가 절반으로 돌아가는 시간.
const SECTOR_HALF_LIFE_MS: f64 = 6.0 * WEEK_MS as f64;
/// 시장 팩터가 절반으로 돌아가는 시간 (장기 방향은 순자산 성장이 정한다).
const MARKET_HALF_LIFE_MS: f64 = 12.0 * WEEK_MS as f64;
/// 변동성 완화장치 발동 시 거래정지 시간.
pub const VI_HALT_MS: i64 = 2 * MINUTE_MS;
/// 서킷브레이커 거래정지 시간.
pub const BREAKER_HALT_MS: i64 = 5 * MINUTE_MS;
/// 서킷브레이커 기준 (지수 하락 만분율).
const BREAKER_BP: i64 = 800;
/// "2시간 5분", "40분", "12초".
pub fn duration_text(ms: i64) -> String {
    let seconds = ms.max(0) / 1000;
    let (hours, minutes) = (seconds / 3600, seconds % 3600 / 60);
    if hours > 0 && minutes > 0 {
        format!("{hours}시간 {minutes}분")
    } else if hours > 0 {
        format!("{hours}시간")
    } else if minutes > 0 {
        format!("{minutes}분")
    } else {
        format!("{seconds}초")
    }
}

/// 봇이 가져가지 않은 운영 기록을 이만큼까지만 둔다.
const LOG_LIMIT: usize = 2_000;
/// 관리자가 한 번에 넘길 수 있는 게임일 (따라잡기 계산을 한정한다).
pub const MAX_SKIP_DAYS: i64 = 24;
/// 시장 뉴스(경제 소식) 빈도: 실제 하루 3건.
const MACRO_NEWS_PER_DAY: f64 = 3.0;
/// 기업 뉴스 빈도: 종목당 실제 하루 0.8건.
const COMPANY_NEWS_PER_DAY: f64 = 0.8;
/// 플레이어 회사 뉴스 빈도: 종목당 실제 하루 3건 (시스템 회사 12개 사이에서도 눈에 띄게).
const PLAYER_COMPANY_NEWS_PER_DAY: f64 = 3.0;

/// 한 번의 갱신에서 생긴 일 (봇이 Discord·웹에 알린다).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TickReport {
    pub news: Vec<NewsItem>,
    /// 호가에 남아 있던 주문이 체결된 것 (사용자, 기록).
    pub fills: Vec<(u64, FillRecord)>,
    pub changed: bool,
}

impl StockMarket {
    /// 처음 시장: 목록의 12개 회사를 상장한 상태로 만든다.
    pub fn new(now: i64, rules: &StockRules) -> Self {
        let day = rules.day_of(now);
        let mut companies = BTreeMap::new();
        let count = CATALOG.len() as i64;
        for (index, entry) in CATALOG.iter().enumerate() {
            let equity = (entry.price as f64 * entry.shares as f64 / entry.sector.pbr()) as i64;
            // 실적 발표는 한 주에 고르게 흩어 둔다.
            let slot = WEEK_MS / count * (index as i64 + 1);
            let company = Company {
                code: entry.code.to_string(),
                name: entry.name.to_string(),
                sector: entry.sector,
                kind: CompanyKind::System,
                status: CompanyStatus::Listed,
                description: entry.description.to_string(),
                shares: entry.shares,
                paid_in: equity * 6 / 10,
                equity,
                founded_at: now,
                listed_at: now,
                price: entry.price,
                mid: entry.price,
                prev_close: entry.price,
                open: entry.price,
                high: entry.price,
                low: entry.price,
                volume: 0,
                turnover: 0,
                day,
                first_day_band: None,
                beta: entry.beta,
                daily_vol: entry.daily_vol,
                sentiment: 0.0,
                pending_news: 0.0,
                noise: 0.0,
                impact: 0.0,
                garch: 1.0,
                last_shock: 0.0,
                volume_carry: 0.0,
                adv: entry.adv,
                depth: (entry.adv / 40).max(1),
                spread_bp: entry.spread_bp,
                lp_inventory: None,
                dividend_yield_bp: entry.dividend_yield_bp,
                next_earnings_at: now + slot,
                consensus: None,
                quarter: 0,
                quarters: VecDeque::new(),
                risk: 2,
                risk_changed_at: 0,
                managed_since: None,
                halted_until: 0,
                vi_ref: entry.price,
                admin_halt: false,
                buyback: None,
                rights: None,
                pending_dividend: None,
                activity_mark: Activity::default(),
            };
            companies.insert(company.code.clone(), company);
        }
        let caps = companies
            .values()
            .map(|company| company.market_cap() as f64)
            .sum::<f64>();
        let mut market = Self {
            version: 0,
            companies,
            accounts: BTreeMap::new(),
            orders: Vec::new(),
            subscriptions: Vec::new(),
            news: VecDeque::new(),
            rumors: Vec::new(),
            journal: Vec::new(),
            next_transfer_id: 1,
            next_order_id: 1,
            next_news_id: 1,
            macro_state: MacroState {
                market: 0.0,
                regime: Regime::Calm,
                sectors: Sector::ALL
                    .into_iter()
                    .map(|sector| (sector, 0.0))
                    .collect(),
            },
            index: IndexState {
                divisor: (caps / 1_000.0).max(1.0),
                value: 1_000.0,
                prev_close: 1_000.0,
                open: 1_000.0,
                high: 1_000.0,
                low: 1_000.0,
                day,
                breaker_day: -1,
            },
            stats: MarketStats::default(),
            halted_until: 0,
            last_tick: now,
            last_system_ipo_day: day,
            activity: Activity::default(),
            time_shift_ms: 0,
            logs: Vec::new(),
            news_outbox: Vec::new(),
            candles: CandleStore::default(),
        };
        market.push_news(
            now,
            NewsKind::Market,
            None,
            "마피아 증권거래소 개장: 12개 종목 상장, 마피아 종합지수 1,000으로 출발".to_string(),
            0,
        );
        market
    }

    // ------------------------------------------------------------ 장부·뉴스

    /// 코인 이동을 장부에 남긴다 (금액 0은 남기지 않는다).
    pub fn transfer(&mut self, user: u64, name: &str, amount: i64, reason: impl Into<String>) {
        if amount == 0 {
            return;
        }
        let id = self.next_transfer_id.max(1);
        self.next_transfer_id = id + 1;
        self.journal.push(Transfer {
            id,
            user,
            name: name.to_string(),
            amount,
            reason: reason.into(),
        });
    }

    /// 수수료·세금 등 복지 금고로 가는 코인.
    pub fn to_treasury(&mut self, amount: i64, reason: impl Into<String>) {
        if amount > 0 {
            self.transfer(TREASURY_USER, "복지 금고", amount, reason);
        }
    }

    /// `applied` 이하의 장부를 지운다 (통계 파일에 이미 반영했다).
    pub fn prune_journal(&mut self, applied: u64) {
        self.journal.retain(|transfer| transfer.id > applied);
    }

    pub fn push_news(
        &mut self,
        at: i64,
        kind: NewsKind,
        code: Option<&str>,
        headline: String,
        tone: i8,
    ) -> NewsItem {
        let id = self.next_news_id.max(1);
        self.next_news_id = id + 1;
        let item = NewsItem {
            id,
            at,
            kind,
            code: code.map(str::to_string),
            headline,
            body: String::new(),
            tone,
        };
        self.news.push_back(item.clone());
        while self.news.len() > NEWS_LIMIT {
            self.news.pop_front();
        }
        self.news_outbox.push(item.clone());
        if self.news_outbox.len() > NEWS_LIMIT {
            let excess = self.news_outbox.len() - NEWS_LIMIT;
            self.news_outbox.drain(..excess);
        }
        item
    }

    /// 뉴스 채널에 올릴 새 뉴스·공시를 꺼낸다.
    pub fn take_news_outbox(&mut self) -> Vec<NewsItem> {
        std::mem::take(&mut self.news_outbox)
    }

    pub fn account_mut(&mut self, user: u64, name: &str) -> &mut Account {
        let account = self.accounts.entry(user).or_default();
        if !name.is_empty() {
            account.name = name.to_string();
        }
        account
    }

    // ------------------------------------------------------------ 가격 모형

    /// 회사의 내재가치 (로그). 순자산 × 업종 PBR × 시장·업종 팩터 × 투자 심리.
    pub fn fair_log(&self, company: &Company, now: i64) -> f64 {
        let bvps = company.bvps().max(0.01);
        let (pbr, floor) = if company.is_player() {
            // 플레이어 회사의 가치는 회사 현금에 묶여 있다 (현금보다 한참 싸거나 비싸게 거래되지 않는다).
            (1.0, Some(bvps * 0.95))
        } else {
            (company.sector.pbr(), None)
        };
        let sector = self
            .macro_state
            .sectors
            .get(&company.sector)
            .copied()
            .unwrap_or(0.0);
        let mut log =
            (bvps * pbr).ln() + company.beta * self.macro_state.market + sector + company.sentiment;
        if company.is_player() {
            // 플레이어 회사는 시장·업종 흐름과 심리의 영향을 절반만 받는다.
            log = (bvps * pbr).ln() + 0.5 * (log - (bvps * pbr).ln());
        }
        if let Some(floor) = floor {
            log = log.max(floor.max(0.01).ln());
        }
        if let CompanyStatus::Liquidating {
            until,
            trading: true,
            ..
        } = company.status
        {
            // 정리매매: 상장폐지까지 남은 시간이 줄수록 가치가 사라진다 (플레이어 회사는 청산가치로).
            let remaining = (until - now).max(0) as f64;
            let total = (6 * HOUR_MS) as f64;
            let fade = (remaining / total).clamp(0.0, 1.0);
            let terminal = if company.is_player() {
                bvps.max(1.0).ln()
            } else {
                0.0
            };
            log = terminal + (log - terminal) * fade;
        }
        if log.is_finite() { log } else { 0.0 }
    }

    /// 오늘의 가격 범위 (상장 첫날 범위, 정리매매는 무제한).
    pub fn limits(&self, company: &Company, rules: &StockRules) -> (i64, i64) {
        if let Some(band) = company.first_day_band {
            return band;
        }
        if matches!(
            company.status,
            CompanyStatus::Liquidating { trading: true, .. }
        ) {
            return (1, i64::MAX / 4);
        }
        price_limits(company.prev_close, rules.limit_bp)
    }

    /// 거래를 받을 수 있는지 (거래정지·서킷브레이커 확인).
    pub fn trading_open(
        &self,
        company: &Company,
        rules: &StockRules,
        now: i64,
    ) -> Result<(), String> {
        if !rules.enabled {
            return Err("지금은 주식 시장이 닫혀 있습니다.".to_string());
        }
        if !company.status.is_tradable() {
            return Err(format!(
                "{}은(는) 지금 거래할 수 없습니다 ({}).",
                company.name,
                company.status.label()
            ));
        }
        if company.admin_halt {
            return Err(format!("{}은(는) 거래정지 중입니다.", company.name));
        }
        if now < self.halted_until {
            return Err("서킷브레이커로 모든 거래가 잠시 멈췄습니다.".to_string());
        }
        if now < company.halted_until {
            return Err(format!(
                "{}은(는) 변동성 완화장치로 잠시 거래가 멈췄습니다.",
                company.name
            ));
        }
        Ok(())
    }

    // ------------------------------------------------------------ 운영 기록

    /// 운영 기록 한 줄. 봇이 가져가지 않아도 넘치지 않게 오래된 것부터 버린다.
    pub(super) fn log(&mut self, text: String) {
        self.logs.push(text);
        if self.logs.len() > LOG_LIMIT {
            let excess = self.logs.len() - LOG_LIMIT;
            self.logs.drain(..excess);
        }
    }

    /// 쌓인 운영 기록을 꺼낸다.
    pub fn take_logs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.logs)
    }

    /// 로그용 종목 이름: "하늘반도체(100010)".
    pub(super) fn label(&self, code: &str) -> String {
        match self.companies.get(code) {
            Some(company) => format!("{}({code})", company.name),
            None => code.to_string(),
        }
    }

    // ------------------------------------------------------------ 시계

    /// 시장 시각 (실제 시각 + 관리자가 넘긴 시간).
    pub fn clock(&self, real_now: i64) -> i64 {
        real_now.saturating_add(self.time_shift_ms.max(0))
    }

    /// 관리자: 게임일을 `days`일 넘긴다. 시장 시계를 그만큼 뒤 게임일의 시작으로 옮기고, 알릴 뉴스를
    /// 돌려준다. 그 사이의 시세·주문·청약·배당 등은 다음 `tick`이 5초 단위로 따라잡는다.
    pub fn skip_game_days(&mut self, market_now: i64, days: i64, rules: &StockRules) -> NewsItem {
        let days = days.clamp(1, MAX_SKIP_DAYS);
        let day_ms = rules.day_ms();
        let target = (rules.day_of(market_now) + days) * day_ms;
        let skipped = (target - market_now).max(0);
        self.time_shift_ms = self.time_shift_ms.saturating_add(skipped);
        self.push_news(
            market_now,
            NewsKind::Market,
            None,
            format!(
                "관리자가 게임일을 {days}일 넘겼습니다. 시장 시계가 {} 앞당겨졌습니다.",
                duration_text(skipped)
            ),
            0,
        )
    }

    // ------------------------------------------------------------ 갱신

    /// 지난 갱신 뒤로 5초마다 한 틱씩 시장을 돌린다.
    pub fn tick(&mut self, now: i64, rules: &StockRules, rng: &mut dyn RngCore) -> TickReport {
        let mut report = TickReport::default();
        if !rules.enabled {
            self.last_tick = now;
            return report;
        }
        let mut at = self.last_tick.max(now - MAX_CATCHUP_TICKS * TICK_MS);
        while at + TICK_MS <= now {
            at += TICK_MS;
            self.step(at, rules, rng, &mut report);
        }
        self.last_tick = at;
        if report.changed {
            self.version += 1;
        }
        report
    }

    fn step(
        &mut self,
        at: i64,
        rules: &StockRules,
        rng: &mut dyn RngCore,
        report: &mut TickReport,
    ) {
        report.changed = true;
        self.step_macro(at, rules, rng, report);
        let codes = self.companies.keys().cloned().collect::<Vec<_>>();
        for code in &codes {
            self.step_company(code, at, rules, rng, report);
        }
        self.match_resting(at, rules, report);
        self.expire_orders(at);
        self.run_buybacks(at, rules, report);
        self.run_corporate_schedule(at, rules, rng, report);
        for code in &codes {
            self.maybe_company_news(code, at, rules, rng, report);
        }
        self.update_index(at, rules, report);
        self.maybe_system_ipo(at, rules, rng, report);
    }

    fn step_macro(
        &mut self,
        at: i64,
        rules: &StockRules,
        rng: &mut dyn RngCore,
        report: &mut TickReport,
    ) {
        let dt_week = TICK_MS as f64 / WEEK_MS as f64;
        let vol_scale = rules.vol_pct.max(0) as f64 / 100.0;
        // 국면 전환: 평온 3주, 격동 1주 정도씩 머문다.
        let switch = match self.macro_state.regime {
            Regime::Calm => dt_week / 3.0,
            Regime::Turbulent => dt_week / 1.0,
        };
        if uniform(rng) < switch {
            self.macro_state.regime = match self.macro_state.regime {
                Regime::Calm => Regime::Turbulent,
                Regime::Turbulent => Regime::Calm,
            };
        }
        let (drift, vol) = match self.macro_state.regime {
            Regime::Calm => (0.002, 0.03),
            Regime::Turbulent => (-0.006, 0.08),
        };
        let market = &mut self.macro_state.market;
        *market += drift * dt_week + vol * vol_scale * dt_week.sqrt() * fat_tail(rng);
        *market *= decay(TICK_MS as f64, MARKET_HALF_LIFE_MS);
        for sector in Sector::ALL {
            let value = self.macro_state.sectors.entry(sector).or_insert(0.0);
            *value = *value * decay(TICK_MS as f64, SECTOR_HALF_LIFE_MS)
                + sector.weekly_vol() * vol_scale * dt_week.sqrt() * fat_tail(rng);
        }
        let news_scale = rules.news_pct.max(0) as f64 / 100.0;
        let chance = MACRO_NEWS_PER_DAY * news_scale * TICK_MS as f64 / DAY_MS as f64;
        if uniform(rng) < chance {
            let draft = macro_news(rng);
            self.macro_state.market += draft.market_jump;
            if let Some((sector, jump)) = draft.sector {
                *self.macro_state.sectors.entry(sector).or_insert(0.0) += jump;
            }
            let item = self.push_news(at, NewsKind::Macro, None, draft.headline, draft.tone);
            report.news.push(item);
        }
    }

    fn step_company(
        &mut self,
        code: &str,
        at: i64,
        rules: &StockRules,
        rng: &mut dyn RngCore,
        report: &mut TickReport,
    ) {
        let day_ms = rules.day_ms();
        let day = rules.day_of(at);
        let Some(company) = self.companies.get(code) else {
            return;
        };
        if !company.status.is_active() {
            return;
        }
        let quoted = matches!(
            company.status,
            CompanyStatus::Listed | CompanyStatus::Liquidating { trading: true, .. }
        );
        // 게임일이 바뀌면 전일 종가를 넘긴다.
        if company.day != day {
            let company = self.companies.get_mut(code).expect("company exists");
            company.day = day;
            company.prev_close = company.price;
            company.open = company.price;
            company.high = company.price;
            company.low = company.price;
            company.volume = 0;
            company.turnover = 0;
            company.vi_ref = company.price;
            company.first_day_band = None;
        }
        if !quoted {
            return;
        }
        let fair = {
            let company = &self.companies[code];
            self.fair_log(company, at)
        };
        let (lower, upper) = {
            let company = &self.companies[code];
            self.limits(company, rules)
        };
        let halted = at < self.halted_until || {
            let company = &self.companies[code];
            at < company.halted_until || company.admin_halt
        };
        let vol_scale = rules.vol_pct.max(0) as f64 / 100.0;
        let dt_day = TICK_MS as f64 / day_ms as f64;
        let dt_week = TICK_MS as f64 / WEEK_MS as f64;
        let within_day = at.rem_euclid(day_ms) as f64 / day_ms as f64;
        // 장중 변동성은 게임일의 시작과 끝에 크고 한가운데에 작다 (U자형).
        let intraday = 0.7 + 1.1 * (2.0 * within_day - 1.0).powi(2);
        let shock = fat_tail(rng);
        let sentiment_shock = fat_tail(rng);
        let volume_noise = uniform(rng);
        let company = self.companies.get_mut(code).expect("company exists");
        company.garch =
            (0.001 + 0.004 * company.last_shock.powi(2) + 0.995 * company.garch).clamp(0.2, 6.0);
        company.last_shock = shock;
        let sigma = company.daily_vol * vol_scale * company.garch.sqrt() * intraday * dt_day.sqrt();
        let kappa = std::f64::consts::LN_2 / NOISE_HALF_LIFE_DAYS;
        company.noise = (company.noise * (1.0 - kappa * dt_day) + sigma * shock).clamp(-2.0, 2.0);
        let sentiment_vol = if company.sector == Sector::Bio {
            0.08
        } else {
            0.04
        };
        company.sentiment = company.sentiment * decay(TICK_MS as f64, SENTIMENT_HALF_LIFE_MS)
            + sentiment_vol * vol_scale * dt_week.sqrt() * sentiment_shock;
        let absorbed = company.pending_news * (1.0 - decay(TICK_MS as f64, NEWS_HALF_LIFE_MS));
        company.pending_news -= absorbed;
        company.sentiment = (company.sentiment + absorbed).clamp(-5.0, 5.0);
        company.impact *= decay(TICK_MS as f64, IMPACT_HALF_LIFE_MS);
        if halted {
            return;
        }
        let target = round_tick(price_from_log(fair + company.noise + company.impact));
        let price = target.clamp(lower, upper);
        let old = company.price;
        company.price = price;
        company.mid = price;
        company.high = company.high.max(price);
        company.low = company.low.min(price);
        // 시뮬레이션 거래량 (차트용): 평균 거래량 × 장중 패턴 × 움직임. 플레이어 회사는 주식이 적어
        // 평균 거래량(발행 주식의 2%, 가격 영향·호가 수량용)으로는 거래가 없는 종목처럼 보이므로,
        // 발행 주식의 30%쯤이 하루에 오가는 것으로 본다. 틱마다 1주에 못 미치는 몫은 버리지 않고
        // 다음 틱으로 넘긴다 (버리면 거래량이 적은 종목은 늘 0주였다).
        let daily = if company.is_player() {
            (company.shares.saturating_mul(3) / 10).max(company.adv)
        } else {
            company.adv
        };
        let expected =
            daily as f64 * dt_day * intraday * (1.0 + shock.abs()) * (0.5 + volume_noise)
                + company.volume_carry;
        let volume = expected.floor().max(0.0) as i64;
        company.volume_carry = expected - volume as f64;
        company.volume = company.volume.saturating_add(volume);
        company.turnover = company
            .turnover
            .saturating_add(volume.saturating_mul(price));
        let vi = rules.vi_bp > 0
            && company.vi_ref > 0
            && (price - company.vi_ref).abs().saturating_mul(BP)
                >= rules.vi_bp.saturating_mul(company.vi_ref);
        let name = company.name.clone();
        if vi {
            company.halted_until = at + VI_HALT_MS;
            company.vi_ref = price;
        }
        let series = self.candles.series.entry(code.to_string()).or_default();
        series.record(at, day_ms, price, volume);
        if vi {
            let direction = if price > old { "급등" } else { "급락" };
            let item = self.push_news(
                at,
                NewsKind::Halt,
                Some(code),
                format!("{name} {direction}으로 변동성 완화장치 발동, 2분간 거래정지"),
                0,
            );
            report.news.push(item);
        }
    }

    fn maybe_company_news(
        &mut self,
        code: &str,
        at: i64,
        rules: &StockRules,
        rng: &mut dyn RngCore,
        report: &mut TickReport,
    ) {
        let Some(company) = self.companies.get(code) else {
            return;
        };
        if company.status != CompanyStatus::Listed {
            return;
        }
        let news_scale = rules.news_pct.max(0) as f64 / 100.0;
        let per_day = if company.is_player() {
            PLAYER_COMPANY_NEWS_PER_DAY
        } else {
            COMPANY_NEWS_PER_DAY
        };
        let chance = per_day * news_scale * TICK_MS as f64 / DAY_MS as f64;
        if uniform(rng) >= chance {
            return;
        }
        let rumor_rate = if company.market_cap() < 60_000_000_000 {
            0.25
        } else {
            0.1
        };
        let (name, sector) = (company.name.clone(), company.sector);
        let draft = company_news(&name, sector, rumor_rate, rng);
        let company = self.companies.get_mut(code).expect("company exists");
        // 발표 순간 40%를 반영하고, 나머지는 몇 분에 걸쳐 따라간다.
        company.sentiment += draft.jump * 0.4;
        company.pending_news += draft.jump * 0.6;
        let kind = if let Some((truth, rest)) = draft.rumor {
            let hours = 1 + (uniform(rng) * 6.0) as i64;
            self.rumors.push(PendingRumor {
                code: code.to_string(),
                resolves_at: at + hours * rules.day_ms(),
                truth,
                jump: rest,
                applied: draft.jump,
                topic: draft.topic.clone(),
            });
            NewsKind::Rumor
        } else {
            NewsKind::News
        };
        let item = self.push_news(at, kind, Some(code), draft.headline, draft.tone);
        report.news.push(item);
    }

    /// 루머 결과를 공시한다.
    pub(super) fn resolve_rumors(&mut self, at: i64, report: &mut TickReport) {
        let due = self
            .rumors
            .iter()
            .filter(|rumor| rumor.resolves_at <= at)
            .cloned()
            .collect::<Vec<_>>();
        if due.is_empty() {
            return;
        }
        self.rumors.retain(|rumor| rumor.resolves_at > at);
        for rumor in due {
            let Some(company) = self.companies.get_mut(&rumor.code) else {
                continue;
            };
            if company.status != CompanyStatus::Listed {
                continue;
            }
            let name = company.name.clone();
            let (headline, tone) = if rumor.truth {
                company.pending_news += rumor.jump;
                (
                    format!("{name}, {} 관련 소문 사실로 확인 (공시)", rumor.topic),
                    if rumor.jump >= 0.0 { 1 } else { -1 },
                )
            } else {
                company.pending_news -= rumor.applied;
                (
                    format!("{name}, {} 관련 소문은 사실무근 (해명 공시)", rumor.topic),
                    if rumor.applied >= 0.0 { -1 } else { 1 },
                )
            };
            let item = self.push_news(at, NewsKind::Disclosure, Some(&rumor.code), headline, tone);
            report.news.push(item);
        }
    }

    /// 상장 종목 시가총액 합.
    pub fn listed_cap_sum(&self) -> f64 {
        self.companies
            .values()
            .filter(|company| company.status.is_tradable())
            .map(|company| company.market_cap() as f64)
            .sum()
    }

    /// 종목 구성이 바뀐 뒤(상장·폐지·증자) 지수가 튀지 않게 나눗수를 맞춘다.
    pub fn rebase_index(&mut self, before: f64) {
        let after = self.listed_cap_sum();
        if before > 0.0 && after > 0.0 {
            self.index.divisor *= after / before;
        } else if after > 0.0 {
            self.index.divisor = after / self.index.value.max(1.0);
        }
    }

    fn update_index(&mut self, at: i64, rules: &StockRules, report: &mut TickReport) {
        let day = rules.day_of(at);
        let caps = self.listed_cap_sum();
        let value = if self.index.divisor > 0.0 {
            caps / self.index.divisor
        } else {
            self.index.value
        };
        let index = &mut self.index;
        if index.day != day {
            index.day = day;
            index.prev_close = index.value;
            index.open = index.value;
            index.high = index.value;
            index.low = index.value;
        }
        if value.is_finite() && value > 0.0 {
            index.value = value;
            index.high = index.high.max(value);
            index.low = index.low.min(value);
        }
        let scaled = (index.value * 100.0).round() as i64;
        self.candles.index.record(at, rules.day_ms(), scaled, 0);
        let fall = (self.index.prev_close - self.index.value) / self.index.prev_close.max(1.0);
        if fall * BP as f64 >= BREAKER_BP as f64 && self.index.breaker_day != day {
            self.index.breaker_day = day;
            self.halted_until = at + BREAKER_HALT_MS;
            let item = self.push_news(
                at,
                NewsKind::Halt,
                None,
                "마피아 종합지수 8% 넘게 급락, 서킷브레이커 발동 (5분간 모든 거래 정지)"
                    .to_string(),
                -1,
            );
            report.news.push(item);
        }
    }

    /// 만료된 지정가 주문을 취소한다.
    fn expire_orders(&mut self, at: i64) {
        let expired = self
            .orders
            .iter()
            .filter(|order| order.expires_at <= at)
            .map(|order| order.id)
            .collect::<Vec<_>>();
        for id in expired {
            self.close_order(id, "주문 기간 만료");
        }
    }
}

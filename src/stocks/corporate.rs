// stocks/corporate.rs — 기업 활동: 실적·배당, 공모(청약·배정·상장), 플레이어 회사 설립과 경영
// (배당·유상증자·자사주·위험도·청산), 관리종목·파산·정리매매·상장폐지, 시스템 회사 신규 상장

use super::catalog::{random_company_name, valid_company_name};
use super::market::TickReport;
use super::model::*;
use super::price::*;
use rand::{Rng, RngCore};
use std::collections::VecDeque;

/// 게임 연동 종목 코드.
pub const MAFIA_GAMES_CODE: &str = "100050";
pub const CASINO_LEISURE_CODE: &str = "100080";
/// 연동 종목의 기준 활동량 (실제 1주).
const BASE_MAFIA_GAMES: f64 = 20.0;
const BASE_CASINO_HANDS: f64 = 300.0;
/// 정리매매 기간 (게임일).
pub const LIQUIDATION_DAYS: i64 = 6;
/// 플레이어 회사 위험도별 분기 이익률 변동성.
const RISK_VOL: [f64; 5] = [0.02, 0.05, 0.10, 0.18, 0.30];

impl StockMarket {
    // ------------------------------------------------------------ 일정

    /// 틱마다: 루머 결과, 실적 예상·발표, 배당, 유상증자·공모 마감, 상장폐지.
    pub(super) fn run_corporate_schedule(
        &mut self,
        at: i64,
        rules: &StockRules,
        rng: &mut dyn RngCore,
        report: &mut TickReport,
    ) {
        self.resolve_rumors(at, report);
        let codes = self.companies.keys().cloned().collect::<Vec<_>>();
        for code in codes {
            let Some(company) = self.companies.get(&code) else {
                continue;
            };
            if !company.status.is_active() {
                continue;
            }
            let publish_consensus = company.consensus.is_none()
                && at >= company.next_earnings_at - DAY_MS
                && at < company.next_earnings_at;
            let earnings_due = at >= company.next_earnings_at;
            let dividend_due = company
                .pending_dividend
                .as_ref()
                .is_some_and(|dividend| at >= dividend.pay_at);
            let rights_due = company
                .rights
                .as_ref()
                .is_some_and(|rights| at >= rights.until);
            let ipo_due =
                matches!(&company.status, CompanyStatus::Subscription(ipo) if at >= ipo.closes_at);
            let liquidation_due =
                matches!(company.status, CompanyStatus::Liquidating { until, .. } if at >= until);
            if publish_consensus {
                self.publish_consensus(&code, at, rules, report);
            }
            if earnings_due {
                self.announce_earnings(&code, at, rules, rng, report);
            }
            if dividend_due {
                self.pay_declared_dividend(&code, at, report);
            }
            if rights_due {
                self.finish_rights(&code, at, report);
            }
            if ipo_due {
                self.close_ipo(&code, at, rules, report);
            }
            if liquidation_due {
                self.finish_liquidation(&code, at, report);
            }
        }
    }

    /// 자사주 매입 (실제처럼 장내 매수): 예산을 기간에 고르게 나눠, 지금까지 쓸 수 있는 몫만큼 파는
    /// 쪽(시장조성자 유통 물량, 대표가 아닌 주주의 매도 주문)에서 산다. 1주 값에 못 미치는 몫은 모아
    /// 다음 틱에 산다. 주주는 회사가 현재가에 낸 매수 호가에 바로 팔 수도 있다 (`take`).
    pub(super) fn run_buybacks(&mut self, at: i64, rules: &StockRules, report: &mut TickReport) {
        let active = self
            .companies
            .values()
            .filter_map(|company| {
                company
                    .buyback
                    .as_ref()
                    .map(|buyback| (company.code.clone(), buyback.clone()))
            })
            .collect::<Vec<_>>();
        for (code, buyback) in active {
            let total = if buyback.total > 0 {
                buyback.total
            } else {
                buyback.budget
            };
            let started = if buyback.started_at > 0 {
                buyback.started_at
            } else {
                buyback.until - rules.day_ms()
            };
            let duration = (buyback.until - started).max(TICK_MS);
            let elapsed = (at - started).clamp(0, duration);
            let spent_before = (total - buyback.budget).max(0);
            let allowance = (i128::from(total) * i128::from(elapsed) / i128::from(duration)) as i64
                - spent_before;
            let (qty, spent) = if allowance > 0 {
                self.buy_back(&code, allowance.min(buyback.budget), at, rules)
            } else {
                (0, 0)
            };
            let Some(company) = self.companies.get_mut(&code) else {
                continue;
            };
            let name = company.name.clone();
            let finished = {
                let Some(current) = company.buyback.as_mut() else {
                    continue;
                };
                current.budget = (current.budget - spent).max(0);
                current.bought += qty;
                at >= current.until || current.budget <= 0
            };
            if finished {
                let (bought, left) = company
                    .buyback
                    .take()
                    .map_or((0, 0), |buyback| (buyback.bought, buyback.budget));
                let used = (total - left).max(0);
                let result = if bought > 0 {
                    format!(
                        "자사주 매입 완료: {bought}주 소각 (예산 {} 중 {} 사용)",
                        format_amount(total),
                        format_amount(used)
                    )
                } else {
                    "자사주 매입 종료: 파는 주식이 없어 사지 못함".to_string()
                };
                self.log(format!("🏢 {name}({code}) {result}"));
                let item = self.push_news(
                    at,
                    NewsKind::Disclosure,
                    Some(&code),
                    format!("{name}, {result}"),
                    i8::from(bought > 0),
                );
                report.news.push(item);
            }
        }
    }

    // ------------------------------------------------------------ 실적

    /// 분기 기대 이익률 (시장 기대 수익률 + 배당 몫).
    fn expected_roe(&self, company: &Company, rules: &StockRules) -> f64 {
        let drift = rules.drift_bp_week as f64 / BP as f64;
        if company.is_player() {
            drift
        } else {
            drift + company.dividend_yield_bp as f64 / BP as f64 / 4.0
        }
    }

    fn publish_consensus(
        &mut self,
        code: &str,
        at: i64,
        rules: &StockRules,
        report: &mut TickReport,
    ) {
        let Some(company) = self.companies.get(code) else {
            return;
        };
        let expected = (company.equity.max(0) as f64 * self.expected_roe(company, rules)) as i64;
        let (name, listed) = (
            company.name.clone(),
            company.status == CompanyStatus::Listed,
        );
        if let Some(company) = self.companies.get_mut(code) {
            company.consensus = Some(expected);
        }
        if listed {
            let item = self.push_news(
                at,
                NewsKind::Earnings,
                Some(code),
                format!(
                    "{name} 실적 발표 하루 전, 순이익 예상치 {}",
                    format_amount(expected)
                ),
                0,
            );
            report.news.push(item);
        }
    }

    fn announce_earnings(
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
        let expected = self.expected_roe(company, rules);
        let vol = if company.is_player() {
            RISK_VOL[usize::from(company.risk.clamp(1, 5)) - 1]
        } else {
            company.sector.earnings_vol()
        };
        let mut roe = expected + vol * fat_tail(rng);
        // 게임 연동 종목: 이번 주 서버 활동이 기준보다 많으면 실적이 좋아진다 (영향에는 상한이 있다).
        let mark = company.activity_mark;
        let activity = self.activity;
        if company.code == MAFIA_GAMES_CODE {
            let games = (activity.mafia_games - mark.mafia_games).max(0) as f64;
            roe += 0.02
                * ((games + 1.0) / (BASE_MAFIA_GAMES + 1.0))
                    .ln()
                    .clamp(-1.5, 1.5);
        }
        if company.code == CASINO_LEISURE_CODE {
            let hands = (activity.casino_hands - mark.casino_hands).max(0) as f64;
            let house = (activity.casino_house - mark.casino_house) as f64;
            roe += 0.015
                * ((hands + 1.0) / (BASE_CASINO_HANDS + 1.0))
                    .ln()
                    .clamp(-1.5, 1.5);
            roe += 0.01 * (house / 1_000_000.0).clamp(-1.0, 1.0);
        }
        let equity_before = company.equity.max(0);
        let profit = (equity_before as f64 * roe).round() as i64;
        let consensus = company
            .consensus
            .unwrap_or((equity_before as f64 * expected) as i64);
        let surprise = if equity_before > 0 {
            (profit - consensus) as f64 / equity_before as f64
        } else {
            0.0
        };
        let player = company.is_player();
        let listed = company.status == CompanyStatus::Listed;
        let name = company.name.clone();
        let quarter = company.quarter + 1;
        let yield_q = company.dividend_yield_bp as f64 / BP as f64 / 4.0;
        let price = company.price;
        let before_cap = self.listed_cap_sum();
        {
            let company = self.companies.get_mut(code).expect("company exists");
            company.activity_mark = activity;
            company.quarter = quarter;
            company.consensus = None;
            company.next_earnings_at += WEEK_MS;
            company.equity = company.equity.saturating_add(profit);
            company.sentiment =
                (company.sentiment + (3.0 * surprise).clamp(-0.3, 0.3)).clamp(-5.0, 5.0);
        }
        if player {
            self.stats.company_earnings = self.stats.company_earnings.saturating_add(profit);
            self.log(format!(
                "📊 {} {quarter}분기 실적 {}{} (회사 현금 {})",
                self.label(code),
                if profit > 0 { "+" } else { "" },
                format_amount(profit),
                format_amount(self.companies[code].equity)
            ));
        }
        // 시스템 회사 배당: 이익이 나면 배당수익률만큼 (배당락으로 주가도 그만큼 내려간다).
        let mut dividend = 0;
        if !player && listed && profit > 0 && yield_q > 0.0 {
            let shares = self.companies[code].shares.max(1);
            let per_share = ((price as f64 * yield_q) as i64).min(profit / shares);
            if per_share > 0 {
                self.pay_dividend(code, per_share, at);
                dividend = per_share;
            }
        }
        let equity = self.companies[code].equity;
        {
            let company = self.companies.get_mut(code).expect("company exists");
            company.quarters.push_back(QuarterResult {
                at,
                quarter,
                profit,
                consensus,
                dividend,
                equity,
            });
            while company.quarters.len() > QUARTER_LIMIT {
                company.quarters.pop_front();
            }
        }
        self.rebase_index(before_cap);
        if listed {
            let gap = if consensus != 0 {
                format!(
                    " (예상 대비 {:+.1}%)",
                    (profit - consensus) as f64 / consensus.unsigned_abs() as f64 * 100.0
                )
            } else {
                String::new()
            };
            let dividend_text = if dividend > 0 {
                format!(", 주당 {} 배당", format_amount(dividend))
            } else {
                String::new()
            };
            let tone = if profit >= consensus { 1 } else { -1 };
            let item = self.push_news(
                at,
                NewsKind::Earnings,
                Some(code),
                format!(
                    "{name} {quarter}분기 실적: 순이익 {}{gap}{dividend_text}",
                    format_amount(profit)
                ),
                tone,
            );
            report.news.push(item);
        }
        self.check_solvency(code, at, rules, report);
    }

    /// 자본잠식·파산 판단: 자본이 0 이하면 파산(정리매매 뒤 상장폐지), 납입 자본의 절반 아래면
    /// 관리종목 지정, 관리종목이 다음 분기에도 그대로면 상장폐지.
    fn check_solvency(&mut self, code: &str, at: i64, rules: &StockRules, report: &mut TickReport) {
        let Some(company) = self.companies.get(code) else {
            return;
        };
        let name = company.name.clone();
        let listed = company.status == CompanyStatus::Listed;
        let private = company.status == CompanyStatus::Private;
        let liquidation_until = at + LIQUIDATION_DAYS * rules.day_ms();
        if company.equity <= 0 {
            if private {
                self.companies.get_mut(code).expect("company exists").equity = 0;
                self.delist(code, at, "파산", 0, report);
                return;
            }
            if listed {
                let company = self.companies.get_mut(code).expect("company exists");
                company.equity = 0;
                company.status = CompanyStatus::Liquidating {
                    until: liquidation_until,
                    reason: "파산".to_string(),
                    trading: true,
                };
                self.log(format!(
                    "⚠️ {name}({code}) 파산: 정리매매 {LIQUIDATION_DAYS}게임일 뒤 상장폐지"
                ));
                self.close_orders_for(code, "파산으로 주문 취소");
                let item = self.push_news(
                    at,
                    NewsKind::Delisting,
                    Some(code),
                    format!("{name} 파산: 정리매매 뒤 상장폐지 ({LIQUIDATION_DAYS}게임일)"),
                    -1,
                );
                report.news.push(item);
            }
            return;
        }
        if !listed {
            return;
        }
        let impaired = company.equity.saturating_mul(2) < company.paid_in;
        match (impaired, company.managed_since) {
            (true, None) => {
                self.companies
                    .get_mut(code)
                    .expect("company exists")
                    .managed_since = Some(at);
                self.log(format!(
                    "⚠️ {name}({code}) 관리종목 지정 (자본잠식률 50% 초과)"
                ));
                let item = self.push_news(
                    at,
                    NewsKind::Disclosure,
                    Some(code),
                    format!("{name}, 자본잠식률 50% 넘어 관리종목 지정"),
                    -1,
                );
                report.news.push(item);
            }
            (true, Some(_)) => {
                let company = self.companies.get_mut(code).expect("company exists");
                company.status = CompanyStatus::Liquidating {
                    until: liquidation_until,
                    reason: "자본잠식 지속".to_string(),
                    trading: true,
                };
                self.log(format!(
                    "⚠️ {name}({code}) 자본잠식 지속으로 상장폐지 결정: 정리매매 {LIQUIDATION_DAYS}게임일"
                ));
                self.close_orders_for(code, "상장폐지 결정으로 주문 취소");
                let item = self.push_news(
                    at,
                    NewsKind::Delisting,
                    Some(code),
                    format!(
                        "{name}, 자본잠식 지속으로 상장폐지 결정: 정리매매 {LIQUIDATION_DAYS}게임일"
                    ),
                    -1,
                );
                report.news.push(item);
            }
            (false, Some(_)) => {
                self.companies
                    .get_mut(code)
                    .expect("company exists")
                    .managed_since = None;
                self.log(format!("✅ {name}({code}) 관리종목 해제"));
                let item = self.push_news(
                    at,
                    NewsKind::Disclosure,
                    Some(code),
                    format!("{name}, 자본잠식 해소로 관리종목 해제"),
                    1,
                );
                report.news.push(item);
            }
            (false, None) => {}
        }
    }

    // ------------------------------------------------------------ 배당·분배

    /// 주주에게 주당 `per_share`씩 준다: 플레이어는 코인으로, 시장조성자 몫은 금고로. 준 총액을 돌려준다.
    fn pay_holders(&mut self, code: &str, per_share: i64, reason: &str) -> i64 {
        if per_share <= 0 {
            return 0;
        }
        let holders = self
            .accounts
            .iter()
            .filter_map(|(user, account)| {
                account
                    .positions
                    .get(code)
                    .filter(|position| position.qty > 0)
                    .map(|position| (*user, account.name.clone(), position.qty))
            })
            .collect::<Vec<_>>();
        let holder_count = holders.len();
        let mut paid = 0_i64;
        for (user, name, qty) in holders {
            let amount = qty.saturating_mul(per_share);
            self.transfer(user, &name, amount, format!("{code} {reason}"));
            paid = paid.saturating_add(amount);
        }
        let lp = self
            .companies
            .get(code)
            .and_then(|company| company.lp_inventory)
            .unwrap_or(0);
        if lp > 0 {
            let amount = lp.saturating_mul(per_share);
            self.to_treasury(amount, format!("{code} 시장조성자 몫 {reason}"));
            paid = paid.saturating_add(amount);
        }
        if paid > 0 {
            self.log(format!(
                "💰 {} {reason} 지급: 주당 {}, 주주 {holder_count}명, 총 {}",
                self.label(code),
                format_amount(per_share),
                format_amount(paid)
            ));
        }
        paid
    }

    /// 배당: 주주에게 주고 자본에서 뺀 뒤, 주가(내재가치)가 배당금만큼 내려가게 한다 (배당락).
    fn pay_dividend(&mut self, code: &str, per_share: i64, at: i64) {
        let Some(company) = self.companies.get(code) else {
            return;
        };
        let target = (company.price - per_share).max(1);
        let total = per_share.saturating_mul(company.shares);
        let player = company.is_player();
        let paid = self.pay_holders(code, per_share, "배당금");
        if !player {
            self.stats.dividends = self.stats.dividends.saturating_add(paid);
        }
        if let Some(company) = self.companies.get_mut(code) {
            company.equity = (company.equity - total).max(0);
        }
        self.move_fair_to(code, target, at);
    }

    /// 투자 심리를 조정해 내재가치가 `target`원이 되게 한다 (배당락·권리락처럼 장부가 바뀔 때 주가가 튀지 않게).
    fn move_fair_to(&mut self, code: &str, target: i64, at: i64) {
        let Some(company) = self.companies.get(code) else {
            return;
        };
        let current = self.fair_log(company, at);
        let wanted = (target.max(1) as f64).ln();
        let scale = if company.is_player() { 0.5 } else { 1.0 };
        if let Some(company) = self.companies.get_mut(code) {
            company.sentiment = (company.sentiment + (wanted - current) / scale).clamp(-5.0, 5.0);
            company.price = floor_tick(target.max(1));
            company.mid = company.price;
            company.impact = 0.0;
        }
    }

    fn pay_declared_dividend(&mut self, code: &str, at: i64, report: &mut TickReport) {
        let Some(dividend) = self
            .companies
            .get_mut(code)
            .and_then(|company| company.pending_dividend.take())
        else {
            return;
        };
        let Some(company) = self.companies.get(code) else {
            return;
        };
        let name = company.name.clone();
        // 결정 뒤 회사 현금이 줄었으면 줄 수 있는 만큼만 준다.
        let per_share = dividend
            .per_share
            .min(company.equity.max(0) / company.shares.max(1));
        if per_share <= 0 || company.status != CompanyStatus::Listed {
            return;
        }
        self.pay_dividend(code, per_share, at);
        let item = self.push_news(
            at,
            NewsKind::Dividend,
            Some(code),
            format!("{name}, 주당 {} 현금배당 지급", format_amount(per_share)),
            1,
        );
        report.news.push(item);
    }

    // ------------------------------------------------------------ 상장폐지

    /// 정리매매·청산 대기가 끝났다: 남은 자본을 주주에게 나누고(플레이어 회사) 상장폐지한다.
    fn finish_liquidation(&mut self, code: &str, at: i64, report: &mut TickReport) {
        let Some(company) = self.companies.get(code) else {
            return;
        };
        let CompanyStatus::Liquidating { reason, .. } = company.status.clone() else {
            return;
        };
        let per_share = if company.is_player() && company.equity > 0 {
            company.equity / company.shares.max(1)
        } else {
            0
        };
        self.delist(code, at, &reason, per_share, report);
    }

    /// 상장폐지·해산: 주문을 닫고, 주당 `per_share`를 나눠 준 뒤, 주식을 없앤다.
    fn delist(
        &mut self,
        code: &str,
        at: i64,
        reason: &str,
        per_share: i64,
        report: &mut TickReport,
    ) {
        self.close_orders_for(code, "상장폐지로 주문 취소");
        let refunds = self
            .subscriptions
            .iter()
            .filter(|subscription| subscription.code == code)
            .cloned()
            .collect::<Vec<_>>();
        self.subscriptions
            .retain(|subscription| subscription.code != code);
        for subscription in refunds {
            self.transfer(
                subscription.user,
                &subscription.name,
                subscription.deposit,
                format!("{code} 청약 증거금 반환"),
            );
        }
        // 행사한 신주인수권 대금은 돌려준다 (신주가 나오지 않았다).
        if let Some(rights) = self
            .companies
            .get_mut(code)
            .and_then(|company| company.rights.take())
        {
            for (user, qty) in rights.exercised {
                let name = self
                    .accounts
                    .get(&user)
                    .map(|account| account.name.clone())
                    .unwrap_or_default();
                self.transfer(
                    user,
                    &name,
                    qty.saturating_mul(rights.price),
                    format!("{code} 유상증자 대금 반환"),
                );
            }
        }
        let distributed = self.pay_holders(code, per_share, "청산 분배금");
        let before = self.listed_cap_sum();
        for account in self.accounts.values_mut() {
            account.positions.remove(code);
        }
        let Some(company) = self.companies.get_mut(code) else {
            return;
        };
        let name = company.name.clone();
        let was_private = company.status == CompanyStatus::Private;
        company.equity = (company.equity - distributed).max(0);
        company.status = CompanyStatus::Delisted {
            at,
            reason: reason.to_string(),
        };
        company.lp_inventory = company.lp_inventory.map(|_| 0);
        company.buyback = None;
        company.pending_dividend = None;
        self.rebase_index(before);
        let tail = if per_share > 0 {
            format!(", 주당 {} 분배", format_amount(per_share))
        } else {
            String::new()
        };
        let headline = if was_private {
            format!("{name} 해산 ({reason}){tail}")
        } else {
            format!("{name} 상장폐지 ({reason}){tail}")
        };
        self.log(format!("⛔ {headline} [{code}]"));
        let item = self.push_news(at, NewsKind::Delisting, Some(code), headline, -1);
        report.news.push(item);
    }

    // ------------------------------------------------------------ 공모

    /// 청약 마감: 배정하고 상장한다 (플레이어 회사는 청약이 모자라면 무산).
    fn close_ipo(&mut self, code: &str, at: i64, rules: &StockRules, report: &mut TickReport) {
        let Some(company) = self.companies.get(code) else {
            return;
        };
        let CompanyStatus::Subscription(offering) = company.status.clone() else {
            return;
        };
        let player = company.is_player();
        let name = company.name.clone();
        let bvps_before = company.bvps();
        let orders = self
            .subscriptions
            .iter()
            .filter(|subscription| subscription.code == code)
            .cloned()
            .collect::<Vec<_>>();
        self.subscriptions
            .retain(|subscription| subscription.code != code);
        let requested = orders.iter().map(|order| order.qty).sum::<i64>();
        if player
            && requested.saturating_mul(BP) < offering.shares.saturating_mul(offering.min_fill_bp)
        {
            for order in &orders {
                self.transfer(
                    order.user,
                    &order.name,
                    order.deposit,
                    format!("{code} 공모 무산 증거금 반환"),
                );
            }
            self.companies.get_mut(code).expect("company exists").status = CompanyStatus::Private;
            self.log(format!(
                "🧾 {} 공모 무산: 청약 {requested}주, {}명에게 증거금 반환",
                self.label(code),
                orders.len()
            ));
            let item = self.push_news(
                at,
                NewsKind::Disclosure,
                Some(code),
                format!("{name} 공모 무산: 청약 {requested}주로 최소 물량에 못 미침"),
                -1,
            );
            report.news.push(item);
            return;
        }
        // 기관 배정 (실제 공모처럼): 공모가가 싸면 기관이 공모 주식의 일부를 받아 가고, 플레이어는
        // 나머지(일반 청약분)를 나눠 받는다. 기관 몫은 시장조성자가 유통 물량으로 들고 있다가 시장에 판다.
        let institutions = if player {
            institution_shares(offering.shares, offering.price, bvps_before, rules)
        } else {
            0
        };
        let retail = offering.shares - institutions;
        let supply = retail.min(requested);
        let allocations = allocate(
            &orders.iter().map(|order| order.qty).collect::<Vec<_>>(),
            supply,
        );
        let mut proceeds = 0_i64;
        for (order, alloc) in orders.iter().zip(&allocations) {
            let cost = alloc.saturating_mul(offering.price);
            proceeds = proceeds.saturating_add(cost);
            let refund = order.deposit - cost;
            self.log(format!(
                "🧾 {} · {} 공모 {alloc}주 배정 (청약 {}주, 납입 {}, 환불 {})",
                order.name,
                self.label(code),
                order.qty,
                format_amount(cost),
                format_amount(refund.max(0))
            ));
            if refund > 0 {
                self.transfer(
                    order.user,
                    &order.name,
                    refund,
                    format!("{code} 청약 미배정 증거금 반환"),
                );
            }
            if *alloc > 0 {
                let account = self.account_mut(order.user, &order.name);
                let position = account.positions.entry(code.to_string()).or_default();
                position.qty += alloc;
                position.cost = position.cost.saturating_add(cost);
            }
        }
        let day_ms = rules.day_ms();
        if player {
            // 기관 납입금은 게임 밖에서 들어오는 돈이라 새로 생기는 코인이다 (시장조성자가 주식을 살 때와 같다).
            let institutional = institutions.saturating_mul(offering.price);
            if institutions > 0 {
                self.stats.lp_sold = self.stats.lp_sold.saturating_add(institutional);
                self.log(format!(
                    "🏛️ {} 공모 기관 배정 {institutions}주 (납입 {}, 시장조성자 유통 물량)",
                    self.label(code),
                    format_amount(institutional)
                ));
            }
            let raised = proceeds.saturating_add(institutional);
            let fee = bp_part(raised, rules.ipo_fee_bp);
            self.to_treasury(fee, format!("{code} 공모 수수료"));
            let founder = self.companies[code].founder();
            let company = self.companies.get_mut(code).expect("company exists");
            company.equity = company.equity.saturating_add(raised - fee);
            company.paid_in = company.paid_in.saturating_add(raised - fee);
            company.shares = company.shares.saturating_add(supply + institutions);
            company.lp_inventory = Some(institutions);
            company.adv = (company.shares / 50).max(10);
            company.depth = (company.adv / 20).max(1);
            // 설립자 지분 보호예수.
            if let Some(founder) = founder
                && let Some(position) = self
                    .accounts
                    .get_mut(&founder)
                    .and_then(|account| account.positions.get_mut(code))
            {
                position.lockup_qty = position.qty;
                position.lockup_until = at + rules.lockup_days.max(0) * day_ms;
            }
        } else {
            self.stats.ipo_burned = self.stats.ipo_burned.saturating_add(proceeds);
        }
        let before = self.listed_cap_sum();
        // 청약 경쟁률은 플레이어가 나눠 받는 일반 청약분 기준.
        let ratio = if retail > 0 {
            requested as f64 / retail as f64
        } else {
            0.0
        };
        let band = (
            ceil_tick(offering.price.saturating_mul(6) / 10),
            floor_tick(offering.price.saturating_mul(4)),
        );
        {
            let company = self.companies.get_mut(code).expect("company exists");
            // 청약 경쟁이 뜨거울수록 첫날 수요가 몰린다.
            let pop = if player { 0.05 } else { 0.1 } * (1.0 + ratio).ln().min(5.0);
            company.sentiment += pop;
            company.status = CompanyStatus::Listed;
            company.listed_at = at;
            company.day = rules.day_of(at);
            company.prev_close = offering.price;
            company.first_day_band = Some(band);
            company.noise = 0.0;
            company.impact = 0.0;
        }
        let fair = {
            let company = &self.companies[code];
            self.fair_log(company, at)
        };
        let open = round_tick(price_from_log(fair)).clamp(band.0, band.1);
        {
            let company = self.companies.get_mut(code).expect("company exists");
            company.price = open;
            company.mid = open;
            company.open = open;
            company.high = open;
            company.low = open;
            company.vi_ref = open;
            company.volume = 0;
            company.turnover = 0;
        }
        self.rebase_index(before);
        let institution_text = if institutions > 0 {
            format!(", 기관 배정 {institutions}주")
        } else {
            String::new()
        };
        let item = self.push_news(
            at,
            NewsKind::Listing,
            Some(code),
            format!(
                "{name} 신규 상장: 공모가 {}, 시초가 {} (청약 경쟁률 {:.1}:1{institution_text})",
                format_amount(offering.price),
                format_amount(open),
                ratio
            ),
            if open >= offering.price { 1 } else { -1 },
        );
        report.news.push(item);
    }

    /// 시스템 회사가 목표보다 적으면 새 회사를 공모에 부친다 (게임일마다 최대 한 번).
    pub(super) fn maybe_system_ipo(
        &mut self,
        at: i64,
        rules: &StockRules,
        rng: &mut dyn RngCore,
        report: &mut TickReport,
    ) {
        let day = rules.day_of(at);
        if self.last_system_ipo_day == day {
            return;
        }
        let active = self
            .companies
            .values()
            .filter(|company| !company.is_player() && company.status.is_active())
            .count() as i64;
        if active >= rules.system_companies {
            return;
        }
        self.last_system_ipo_day = day;
        let sector = Sector::ALL[rng.random_range(0..Sector::ALL.len())];
        let mut name = random_company_name(sector, rng);
        for _ in 0..8 {
            if !self.name_taken(&name) {
                break;
            }
            name = random_company_name(sector, rng);
        }
        if self.name_taken(&name) {
            return;
        }
        let code = self.next_code(200_000);
        // 가격은 5천~15만원 사이 로그 균등, 시가총액 300억~3,000억.
        let price = round_tick((5_000.0_f64.ln() + uniform(rng) * (30.0_f64).ln()).exp());
        let cap = (30_000_000_000.0_f64.ln() + uniform(rng) * 10.0_f64.ln()).exp();
        let shares = ((cap / price as f64) as i64).max(10_000);
        let equity = (price as f64 * shares as f64 / sector.pbr()) as i64;
        let adv = (shares / 300).max(100);
        let offer_price = floor_tick(price.saturating_mul(8) / 10);
        let offered = (shares / 50).max(100);
        let daily_vol = 0.02 + sector.earnings_vol() * 0.2;
        let company = Company {
            code: code.clone(),
            name: name.clone(),
            sector,
            kind: CompanyKind::System,
            status: CompanyStatus::Subscription(IpoOffering {
                price: offer_price,
                shares: offered,
                opens_at: at,
                closes_at: at + 3 * rules.day_ms(),
                min_fill_bp: 0,
            }),
            description: format!("새로 상장하는 {} 기업.", sector.name()),
            shares,
            paid_in: equity * 6 / 10,
            equity,
            founded_at: at,
            listed_at: 0,
            price: offer_price,
            mid: offer_price,
            prev_close: offer_price,
            open: offer_price,
            high: offer_price,
            low: offer_price,
            volume: 0,
            turnover: 0,
            day,
            first_day_band: None,
            beta: 0.8 + uniform(rng) * 0.6,
            daily_vol,
            sentiment: 0.0,
            pending_news: 0.0,
            noise: 0.0,
            impact: 0.0,
            garch: 1.0,
            last_shock: 0.0,
            volume_carry: 0.0,
            adv,
            depth: (adv / 40).max(1),
            spread_bp: 15,
            lp_inventory: None,
            dividend_yield_bp: if uniform(rng) < 0.4 {
                100 + (uniform(rng) * 300.0) as i64
            } else {
                0
            },
            next_earnings_at: at + WEEK_MS,
            consensus: None,
            quarter: 0,
            quarters: VecDeque::new(),
            risk: 2,
            risk_changed_at: 0,
            managed_since: None,
            halted_until: 0,
            vi_ref: offer_price,
            admin_halt: false,
            buyback: None,
            rights: None,
            pending_dividend: None,
            activity_mark: self.activity,
        };
        self.companies.insert(code.clone(), company);
        let item = self.push_news(
            at,
            NewsKind::Listing,
            Some(&code),
            format!(
                "공모주 청약 시작: {name}({}), 공모가 {}, {}주, 청약 {}게임일",
                sector.name(),
                format_amount(offer_price),
                offered,
                3
            ),
            1,
        );
        report.news.push(item);
    }

    /// 쓰지 않은 종목코드 (`base`부터).
    fn next_code(&self, base: u32) -> String {
        let mut number = base + 10;
        loop {
            let code = format!("{number:06}");
            if !self.companies.contains_key(&code) {
                return code;
            }
            number += 10;
        }
    }

    /// 활동 중인 회사가 쓰는 이름인지 (대소문자·공백 무시).
    pub fn name_taken(&self, name: &str) -> bool {
        let key = normalize_name(name);
        self.companies
            .values()
            .any(|company| company.status.is_active() && normalize_name(&company.name) == key)
    }

    // ------------------------------------------------------------ 플레이어 회사

    /// 설립 비용 (자본금 + 설립 수수료).
    pub fn founding_cost(capital: i64, rules: &StockRules) -> i64 {
        capital.saturating_add(bp_part(capital, rules.found_fee_bp))
    }

    /// 회사를 세운다. 봇은 `founding_cost`만큼 코인이 있는지 먼저 확인한다.
    #[allow(clippy::too_many_arguments)]
    pub fn found_company(
        &mut self,
        user: u64,
        user_name: &str,
        company_name: &str,
        sector: Sector,
        capital: i64,
        now: i64,
        rules: &StockRules,
    ) -> Result<String, String> {
        if !rules.enabled {
            return Err("지금은 주식 시장이 닫혀 있습니다.".to_string());
        }
        let company_name = valid_company_name(company_name)?;
        if self.name_taken(&company_name) {
            return Err("이미 있는 회사 이름입니다.".to_string());
        }
        let owned = self
            .companies
            .values()
            .filter(|company| company.founder() == Some(user) && company.status.is_active())
            .count() as i64;
        if owned >= rules.max_companies.max(0) {
            return Err(format!(
                "한 사람이 운영할 수 있는 회사는 {}개까지입니다.",
                rules.max_companies
            ));
        }
        if capital < rules.found_min_capital {
            return Err(format!(
                "자본금은 {} 이상이어야 합니다.",
                format_amount(rules.found_min_capital)
            ));
        }
        let fee = bp_part(capital, rules.found_fee_bp);
        let shares = (capital / PAR_VALUE).max(100);
        let price = round_tick(capital as f64 / shares as f64);
        let code = self.next_code(700_000);
        let risk = 2_u8;
        let company = Company {
            code: code.clone(),
            name: company_name.clone(),
            sector,
            kind: CompanyKind::Player {
                founder: user,
                founder_name: user_name.to_string(),
            },
            status: CompanyStatus::Private,
            description: String::new(),
            shares,
            paid_in: capital,
            equity: capital,
            founded_at: now,
            listed_at: 0,
            price,
            mid: price,
            prev_close: price,
            open: price,
            high: price,
            low: price,
            volume: 0,
            turnover: 0,
            day: rules.day_of(now),
            first_day_band: None,
            beta: 1.0,
            daily_vol: player_daily_vol(risk),
            sentiment: 0.0,
            pending_news: 0.0,
            noise: 0.0,
            impact: 0.0,
            garch: 1.0,
            last_shock: 0.0,
            volume_carry: 0.0,
            adv: (shares / 50).max(10),
            depth: ((shares / 50).max(10) / 20).max(1),
            spread_bp: 100,
            lp_inventory: Some(0),
            dividend_yield_bp: 0,
            next_earnings_at: now + WEEK_MS,
            consensus: None,
            quarter: 0,
            quarters: VecDeque::new(),
            risk,
            risk_changed_at: 0,
            managed_since: None,
            halted_until: 0,
            vi_ref: price,
            admin_halt: false,
            buyback: None,
            rights: None,
            pending_dividend: None,
            activity_mark: self.activity,
        };
        self.companies.insert(code.clone(), company);
        self.transfer(
            user,
            user_name,
            -(capital + fee),
            format!("{code} 회사 설립 자본금·수수료"),
        );
        self.to_treasury(fee, format!("{code} 회사 설립 수수료"));
        self.log(format!(
            "🏢 {user_name} · {company_name}({code}) 설립: 자본금 {}, 수수료 {}",
            format_amount(capital),
            format_amount(fee)
        ));
        let account = self.account_mut(user, user_name);
        account.positions.insert(
            code.clone(),
            Position {
                qty: shares,
                locked: 0,
                cost: capital,
                lockup_qty: 0,
                lockup_until: 0,
            },
        );
        self.push_news(
            now,
            NewsKind::Disclosure,
            Some(&code),
            format!(
                "{user_name}님, {company_name}({}) 설립: 자본금 {}",
                sector.name(),
                format_amount(capital)
            ),
            0,
        );
        self.version += 1;
        Ok(code)
    }

    fn founder_company(&self, user: u64, code: &str) -> Result<&Company, String> {
        let company = self
            .companies
            .get(code)
            .ok_or_else(|| "그런 회사가 없습니다.".to_string())?;
        if company.founder() != Some(user) {
            return Err("회사 대표(설립자)만 할 수 있습니다.".to_string());
        }
        Ok(company)
    }

    /// 공모 청약을 연다 (비상장 → 청약 1게임일 → 상장).
    pub fn start_ipo(
        &mut self,
        user: u64,
        code: &str,
        price: i64,
        new_shares: i64,
        now: i64,
        rules: &StockRules,
    ) -> Result<IpoOffering, String> {
        let company = self.founder_company(user, code)?;
        if company.status != CompanyStatus::Private {
            return Err("비상장 회사만 공모할 수 있습니다.".to_string());
        }
        if now - company.founded_at < rules.day_ms() {
            return Err("설립하고 1게임일이 지나야 상장할 수 있습니다.".to_string());
        }
        if company.equity < rules.listing_min_equity {
            return Err(format!(
                "자본총계가 {} 이상이어야 상장할 수 있습니다.",
                format_amount(rules.listing_min_equity)
            ));
        }
        let bvps = company.bvps();
        let low = ceil_tick((bvps * 0.8).ceil() as i64);
        let high = floor_tick((bvps * 3.0) as i64);
        let price = floor_tick(price);
        if price < low || price > high {
            return Err(format!(
                "공모가는 주당 순자산의 0.8~3배({}~{})여야 합니다.",
                format_amount(low),
                format_amount(high)
            ));
        }
        let min_new = (company.shares / 10).max(1);
        if new_shares < min_new || new_shares > company.shares {
            return Err(format!(
                "공모 주식은 {min_new}~{}주여야 합니다 (대표 지분 50% 이상 유지).",
                company.shares
            ));
        }
        let name = company.name.clone();
        let institutions = institution_shares(new_shares, price, bvps, rules);
        let institution_text = if institutions > 0 {
            format!(" (기관 배정 예정 {institutions}주)")
        } else {
            String::new()
        };
        let offering = IpoOffering {
            price,
            shares: new_shares,
            opens_at: now,
            closes_at: now + rules.day_ms(),
            min_fill_bp: 5_000,
        };
        self.companies.get_mut(code).expect("company exists").status =
            CompanyStatus::Subscription(offering.clone());
        self.log(format!(
            "🏢 {} 공모 청약 시작: {new_shares}주{institution_text} @{} (1게임일)",
            self.label(code),
            format_amount(price)
        ));
        self.push_news(
            now,
            NewsKind::Listing,
            Some(code),
            format!(
                "공모주 청약 시작: {name}, 공모가 {}, {}주{institution_text}, 청약 1게임일",
                format_amount(price),
                new_shares
            ),
            0,
        );
        self.version += 1;
        Ok(offering)
    }

    /// 청약 증거금.
    pub fn subscription_cost(&self, code: &str, qty: i64) -> Result<i64, String> {
        let company = self
            .companies
            .get(code)
            .ok_or_else(|| "그런 회사가 없습니다.".to_string())?;
        let CompanyStatus::Subscription(offering) = &company.status else {
            return Err("지금 청약을 받는 회사가 아닙니다.".to_string());
        };
        if qty <= 0 || qty > offering.shares {
            return Err(format!("청약 수량은 1~{}주입니다.", offering.shares));
        }
        qty.checked_mul(offering.price)
            .ok_or_else(|| "청약 금액이 너무 큽니다.".to_string())
    }

    /// 공모 청약. 봇은 `subscription_cost`만큼 코인이 있는지 먼저 확인한다. 같은 공모에 다시 청약하면 더한다.
    pub fn subscribe(
        &mut self,
        user: u64,
        name: &str,
        code: &str,
        qty: i64,
        now: i64,
    ) -> Result<i64, String> {
        let deposit = self.subscription_cost(code, qty)?;
        let company = &self.companies[code];
        let CompanyStatus::Subscription(offering) = &company.status else {
            return Err("지금 청약을 받는 회사가 아닙니다.".to_string());
        };
        if now >= offering.closes_at {
            return Err("청약이 마감됐습니다.".to_string());
        }
        if company.founder() == Some(user) {
            return Err("대표는 자기 회사 공모에 청약할 수 없습니다.".to_string());
        }
        let limit = offering.shares;
        let existing = self
            .subscriptions
            .iter()
            .find(|subscription| subscription.user == user && subscription.code == code)
            .map_or(0, |subscription| subscription.qty);
        if existing + qty > limit {
            return Err(format!(
                "한 사람은 공모 주식 수({limit}주)까지만 청약할 수 있습니다."
            ));
        }
        self.transfer(user, name, -deposit, format!("{code} 청약 증거금"));
        match self
            .subscriptions
            .iter_mut()
            .find(|subscription| subscription.user == user && subscription.code == code)
        {
            Some(subscription) => {
                subscription.qty += qty;
                subscription.deposit += deposit;
            }
            None => self.subscriptions.push(Subscription {
                user,
                name: name.to_string(),
                code: code.to_string(),
                qty,
                deposit,
                at: now,
            }),
        }
        self.log(format!(
            "🧾 {name} · {} 공모 {qty}주 청약 (증거금 {})",
            self.label(code),
            format_amount(deposit)
        ));
        self.version += 1;
        Ok(deposit)
    }

    /// 청약 취소 (마감 전).
    pub fn cancel_subscription(&mut self, user: u64, code: &str) -> Result<i64, String> {
        let index = self
            .subscriptions
            .iter()
            .position(|subscription| subscription.user == user && subscription.code == code)
            .ok_or_else(|| "청약한 내역이 없습니다.".to_string())?;
        let subscription = self.subscriptions.remove(index);
        self.transfer(
            user,
            &subscription.name,
            subscription.deposit,
            format!("{code} 청약 취소"),
        );
        self.log(format!(
            "🧾 {} · {} 청약 취소 (환불 {})",
            subscription.name,
            self.label(code),
            format_amount(subscription.deposit)
        ));
        self.version += 1;
        Ok(subscription.deposit)
    }

    /// 현금배당 결정 (다음 게임일 시작에 그때 주주에게 지급).
    pub fn declare_dividend(
        &mut self,
        user: u64,
        code: &str,
        per_share: i64,
        now: i64,
        rules: &StockRules,
    ) -> Result<i64, String> {
        let company = self.founder_company(user, code)?;
        if company.status != CompanyStatus::Listed {
            return Err("상장된 회사만 배당할 수 있습니다.".to_string());
        }
        if company.pending_dividend.is_some() {
            return Err("이미 결정한 배당이 지급을 기다리고 있습니다.".to_string());
        }
        let total = per_share
            .checked_mul(company.shares)
            .ok_or_else(|| "배당금이 너무 큽니다.".to_string())?;
        if per_share <= 0 || total > company.equity {
            return Err(format!(
                "주당 배당금은 1원 이상, 총액은 회사 현금({}) 이하여야 합니다.",
                format_amount(company.equity)
            ));
        }
        let name = company.name.clone();
        let pay_at = (rules.day_of(now) + 1) * rules.day_ms();
        self.companies
            .get_mut(code)
            .expect("company exists")
            .pending_dividend = Some(PendingDividend { per_share, pay_at });
        self.log(format!(
            "💰 {} 배당 결정: 주당 {}, 총 {} (다음 게임일 지급)",
            self.label(code),
            format_amount(per_share),
            format_amount(total)
        ));
        self.push_news(
            now,
            NewsKind::Dividend,
            Some(code),
            format!(
                "{name}, 주당 {} 현금배당 결정 (다음 게임일 시작 때 주주에게 지급)",
                format_amount(per_share)
            ),
            1,
        );
        self.version += 1;
        Ok(total)
    }

    /// 유상증자 (주주배정): 지금 주주에게 보유 비율대로 신주인수권을 주고, 1게임일 동안 행사를 받는다.
    pub fn start_rights(
        &mut self,
        user: u64,
        code: &str,
        new_shares: i64,
        price: i64,
        now: i64,
        rules: &StockRules,
    ) -> Result<(), String> {
        let company = self.founder_company(user, code)?;
        if company.status != CompanyStatus::Listed {
            return Err("상장된 회사만 유상증자할 수 있습니다.".to_string());
        }
        if company.rights.is_some() {
            return Err("진행 중인 유상증자가 있습니다.".to_string());
        }
        if new_shares < 1 || new_shares > company.shares {
            return Err(format!("신주는 1~{}주여야 합니다.", company.shares));
        }
        let price = floor_tick(price);
        let low = ceil_tick(company.price.saturating_mul(7) / 10);
        if price < low || price > company.price {
            return Err(format!(
                "발행가는 현재가의 70~100%({}~{})여야 합니다.",
                format_amount(low),
                format_amount(company.price)
            ));
        }
        let shares = company.shares.max(1);
        let name = company.name.clone();
        let rights = self
            .accounts
            .iter()
            .filter_map(|(holder, account)| {
                let qty = account
                    .positions
                    .get(code)
                    .map_or(0, |position| position.qty);
                let granted =
                    (i128::from(new_shares) * i128::from(qty) / i128::from(shares)) as i64;
                (granted > 0).then_some((*holder, granted))
            })
            .collect();
        self.companies.get_mut(code).expect("company exists").rights = Some(RightsOffering {
            price,
            shares: new_shares,
            until: now + rules.day_ms(),
            rights,
            exercised: Default::default(),
        });
        self.log(format!(
            "🏢 {} 유상증자 결정: 신주 {new_shares}주 @{} (1게임일)",
            self.label(code),
            format_amount(price)
        ));
        self.push_news(
            now,
            NewsKind::Disclosure,
            Some(code),
            format!(
                "{name}, 주주배정 유상증자: 신주 {new_shares}주, 발행가 {} (1게임일 동안 청약)",
                format_amount(price)
            ),
            -1,
        );
        self.version += 1;
        Ok(())
    }

    /// 신주인수권 행사 대금.
    pub fn rights_cost(&self, user: u64, code: &str, qty: i64) -> Result<i64, String> {
        let rights = self
            .companies
            .get(code)
            .and_then(|company| company.rights.as_ref())
            .ok_or_else(|| "진행 중인 유상증자가 없습니다.".to_string())?;
        let granted = rights.rights.get(&user).copied().unwrap_or(0);
        let used = rights.exercised.get(&user).copied().unwrap_or(0);
        if qty <= 0 || qty > granted - used {
            return Err(format!(
                "행사할 수 있는 신주인수권은 {}주입니다.",
                granted - used
            ));
        }
        qty.checked_mul(rights.price)
            .ok_or_else(|| "금액이 너무 큽니다.".to_string())
    }

    /// 신주인수권 행사. 봇은 `rights_cost`만큼 코인이 있는지 먼저 확인한다.
    pub fn exercise_rights(
        &mut self,
        user: u64,
        name: &str,
        code: &str,
        qty: i64,
        now: i64,
    ) -> Result<i64, String> {
        let cost = self.rights_cost(user, code, qty)?;
        let rights = self
            .companies
            .get_mut(code)
            .and_then(|company| company.rights.as_mut())
            .ok_or_else(|| "진행 중인 유상증자가 없습니다.".to_string())?;
        if now >= rights.until {
            return Err("유상증자 청약이 마감됐습니다.".to_string());
        }
        *rights.exercised.entry(user).or_default() += qty;
        self.transfer(user, name, -cost, format!("{code} 유상증자 대금"));
        self.log(format!(
            "🧾 {name} · {} 신주 {qty}주 인수 (납입 {})",
            self.label(code),
            format_amount(cost)
        ));
        self.version += 1;
        Ok(cost)
    }

    fn finish_rights(&mut self, code: &str, at: i64, report: &mut TickReport) {
        let Some(rights) = self
            .companies
            .get_mut(code)
            .and_then(|company| company.rights.take())
        else {
            return;
        };
        let total = rights.exercised.values().sum::<i64>();
        let proceeds = total.saturating_mul(rights.price);
        let before = self.listed_cap_sum();
        for (user, qty) in &rights.exercised {
            let name = self
                .accounts
                .get(user)
                .map(|account| account.name.clone())
                .unwrap_or_default();
            let account = self.account_mut(*user, &name);
            let position = account.positions.entry(code.to_string()).or_default();
            position.qty += qty;
            position.cost = position
                .cost
                .saturating_add(qty.saturating_mul(rights.price));
        }
        let Some(company) = self.companies.get_mut(code) else {
            return;
        };
        let name = company.name.clone();
        company.shares = company.shares.saturating_add(total);
        company.equity = company.equity.saturating_add(proceeds);
        company.paid_in = company.paid_in.saturating_add(proceeds);
        company.adv = (company.shares / 50).max(10);
        company.depth = (company.adv / 20).max(1);
        self.rebase_index(before);
        self.log(format!(
            "🏢 {name}({code}) 유상증자 완료: 신주 {total}주, {} 조달",
            format_amount(proceeds)
        ));
        let item = self.push_news(
            at,
            NewsKind::Disclosure,
            Some(code),
            format!(
                "{name}, 유상증자 완료: 신주 {total}주 발행, {} 조달",
                format_amount(proceeds)
            ),
            0,
        );
        report.news.push(item);
    }

    /// 자사주 매입 (1게임일 동안 회사 현금으로 조금씩 사서 소각).
    pub fn start_buyback(
        &mut self,
        user: u64,
        code: &str,
        budget: i64,
        now: i64,
        rules: &StockRules,
    ) -> Result<(), String> {
        let company = self.founder_company(user, code)?;
        if company.status != CompanyStatus::Listed {
            return Err("상장된 회사만 자사주를 살 수 있습니다.".to_string());
        }
        if company.buyback.is_some() {
            return Err("진행 중인 자사주 매입이 있습니다.".to_string());
        }
        if budget < 1 || budget > company.equity / 2 {
            return Err(format!(
                "매입 예산은 회사 현금의 절반({}) 이하여야 합니다.",
                format_amount(company.equity / 2)
            ));
        }
        if self
            .orders
            .iter()
            .any(|order| order.user == user && order.code == code && order.side == Side::Sell)
        {
            return Err(
                "대표의 매도 주문을 먼저 취소해 주세요 (자사주 매입 중에는 대표가 팔 수 없습니다)."
                    .to_string(),
            );
        }
        let name = company.name.clone();
        // 실제처럼 매입 공시에 주가가 반응한다: 시가총액 대비 매입 규모만큼 (투자 심리 최대 +0.08,
        // 몇 분에 걸쳐 반영). 사들이는 동안에는 매수세로 더 오른다.
        let signal = (0.3 * budget as f64 / company.market_cap().max(1) as f64).min(0.08);
        let company = self.companies.get_mut(code).expect("company exists");
        company.pending_news += signal;
        company.buyback = Some(Buyback {
            budget,
            until: now + rules.day_ms(),
            bought: 0,
            total: budget,
            started_at: now,
        });
        self.log(format!(
            "🏢 {} 자사주 매입 시작: 예산 {} (1게임일)",
            self.label(code),
            format_amount(budget)
        ));
        self.push_news(
            now,
            NewsKind::Disclosure,
            Some(code),
            format!(
                "{name}, {} 규모 자사주 매입·소각 결정 (1게임일)",
                format_amount(budget)
            ),
            1,
        );
        self.version += 1;
        Ok(())
    }

    /// 사업 위험도 (1~5). 실제 1주에 한 번 바꿀 수 있다.
    pub fn set_risk(&mut self, user: u64, code: &str, risk: u8, now: i64) -> Result<(), String> {
        let company = self.founder_company(user, code)?;
        if !(1..=5).contains(&risk) {
            return Err("위험도는 1~5입니다.".to_string());
        }
        if company.risk_changed_at > 0 && now - company.risk_changed_at < WEEK_MS {
            return Err("위험도는 1주에 한 번만 바꿀 수 있습니다.".to_string());
        }
        let name = company.name.clone();
        let company = self.companies.get_mut(code).expect("company exists");
        company.risk = risk;
        company.risk_changed_at = now;
        company.daily_vol = player_daily_vol(risk);
        self.log(format!("🏢 {name}({code}) 사업 위험도 {risk}단계로 변경"));
        self.push_news(
            now,
            NewsKind::Disclosure,
            Some(code),
            format!("{name}, 사업 위험도를 {risk}단계로 변경"),
            0,
        );
        self.version += 1;
        Ok(())
    }

    /// 회사 소개 문구.
    pub fn set_description(&mut self, user: u64, code: &str, text: &str) -> Result<(), String> {
        self.founder_company(user, code)?;
        let text = text.trim();
        if text.chars().count() > 120 {
            return Err("소개는 120자까지입니다.".to_string());
        }
        self.companies
            .get_mut(code)
            .expect("company exists")
            .description = text.to_string();
        self.log(format!("🏢 {} 회사 소개 변경: {text}", self.label(code)));
        self.version += 1;
        Ok(())
    }

    /// 해산·청산: 비상장이면 바로 자본을 대표에게 돌려주고, 상장사면 대표가 지분 50% 이상일 때
    /// 1게임일 거래정지 뒤 남은 자본을 주주에게 나누고 상장폐지한다.
    pub fn dissolve(
        &mut self,
        user: u64,
        code: &str,
        now: i64,
        rules: &StockRules,
    ) -> Result<(), String> {
        let company = self.founder_company(user, code)?;
        let name = company.name.clone();
        match company.status {
            CompanyStatus::Private => {
                let per_share = company.equity.max(0) / company.shares.max(1);
                let mut report = TickReport::default();
                self.delist(code, now, "해산", per_share, &mut report);
                // 나누고 남은 우수리도 대표에게 준다.
                let left = self.companies.get(code).map_or(0, |company| company.equity);
                if left > 0 {
                    let founder_name = self
                        .accounts
                        .get(&user)
                        .map(|account| account.name.clone())
                        .unwrap_or_default();
                    self.transfer(user, &founder_name, left, format!("{code} 해산 잔여 자본"));
                    if let Some(company) = self.companies.get_mut(code) {
                        company.equity = 0;
                    }
                }
            }
            CompanyStatus::Listed => {
                let owned = self
                    .accounts
                    .get(&user)
                    .and_then(|account| account.positions.get(code))
                    .map_or(0, |position| position.qty);
                if owned.saturating_mul(2) < company.shares {
                    return Err("대표 지분이 50% 이상이어야 청산할 수 있습니다.".to_string());
                }
                let company = self.companies.get_mut(code).expect("company exists");
                company.status = CompanyStatus::Liquidating {
                    until: now + rules.day_ms(),
                    reason: "청산".to_string(),
                    trading: false,
                };
                company.buyback = None;
                self.log(format!(
                    "🏢 {name}({code}) 해산 결정: 1게임일 거래정지 뒤 남은 현금을 주주에게 분배"
                ));
                self.close_orders_for(code, "청산 결정으로 주문 취소");
                self.push_news(
                    now,
                    NewsKind::Delisting,
                    Some(code),
                    format!("{name}, 해산·청산 결정: 1게임일 거래정지 뒤 남은 자본을 주주에게 분배하고 상장폐지"),
                    -1,
                );
            }
            _ => return Err("지금 상태에서는 해산할 수 없습니다.".to_string()),
        }
        self.version += 1;
        Ok(())
    }

    /// 관리자 거래정지·재개.
    pub fn set_admin_halt(&mut self, code: &str, halted: bool, now: i64) -> Result<String, String> {
        let company = self
            .companies
            .get_mut(code)
            .ok_or_else(|| "그런 종목이 없습니다.".to_string())?;
        company.admin_halt = halted;
        let name = company.name.clone();
        let (text, tone) = if halted {
            (format!("{name}, 거래소 결정으로 거래정지"), -1)
        } else {
            (format!("{name}, 거래 재개"), 0)
        };
        self.push_news(now, NewsKind::Disclosure, Some(code), text, tone);
        self.version += 1;
        Ok(name)
    }

    /// 서버 활동 기록 (게임 연동 종목의 실적 재료).
    pub fn record_activity(&mut self, mafia_games: i64, casino_hands: i64, casino_house: i64) {
        self.activity.mafia_games = self.activity.mafia_games.saturating_add(mafia_games);
        self.activity.casino_hands = self.activity.casino_hands.saturating_add(casino_hands);
        self.activity.casino_house = self.activity.casino_house.saturating_add(casino_house);
    }
}

fn player_daily_vol(risk: u8) -> f64 {
    0.012 + 0.006 * f64::from(risk.clamp(1, 5))
}

/// 공모 기관 배정 수량 (수요예측): 공모가가 주당 순자산 이하면 공모 주식의 `ipo_institution_bp`만큼,
/// 비쌀수록 줄어 순자산의 2배 이상이면 받아 가지 않는다.
pub fn institution_shares(offered: i64, price: i64, bvps: f64, rules: &StockRules) -> i64 {
    if offered <= 0 || price <= 0 || bvps.is_nan() || bvps <= 0.0 {
        return 0;
    }
    let appetite = (2.0 - price as f64 / bvps).clamp(0.0, 1.0);
    let share = rules.ipo_institution_bp.clamp(0, BP) as f64 / BP as f64;
    ((offered as f64 * share * appetite).floor() as i64).clamp(0, offered)
}

fn normalize_name(name: &str) -> String {
    name.chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

/// 공모 배정: 공급의 절반은 청약자에게 고르게(균등), 나머지는 남은 청약 수량에 비례해 나눈다.
pub fn allocate(requests: &[i64], supply: i64) -> Vec<i64> {
    let total = requests.iter().map(|qty| qty.max(&0)).sum::<i64>();
    if supply >= total {
        return requests.iter().map(|qty| (*qty).max(0)).collect();
    }
    let mut result = vec![0_i64; requests.len()];
    let active = requests.iter().filter(|qty| **qty > 0).count() as i64;
    if active == 0 || supply <= 0 {
        return result;
    }
    let mut left = supply;
    // 균등 배정.
    let equal = (supply / 2) / active;
    for (slot, qty) in result.iter_mut().zip(requests) {
        let give = equal.min((*qty).max(0));
        *slot = give;
        left -= give;
    }
    // 비례 배정.
    let demand = requests
        .iter()
        .zip(&result)
        .map(|(qty, given)| (qty - given).max(0))
        .collect::<Vec<_>>();
    let total_demand = demand.iter().sum::<i64>();
    if total_demand > 0 && left > 0 {
        let pool = left;
        for (slot, want) in result.iter_mut().zip(&demand) {
            let give = (i128::from(pool) * i128::from(*want) / i128::from(total_demand)) as i64;
            let give = give.min(*want);
            *slot += give;
            left -= give;
        }
    }
    // 우수리는 청약 순서대로 한 주씩 (더 받을 사람이 없으면 멈춘다).
    while left > 0 {
        let mut progressed = false;
        for (slot, qty) in result.iter_mut().zip(requests) {
            if left > 0 && *slot < *qty {
                *slot += 1;
                left -= 1;
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
    result
}

/// "12,345" 표기.
pub fn format_amount(amount: i64) -> String {
    let digits = amount.unsigned_abs().to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    if amount < 0 {
        format!("-{grouped}")
    } else {
        grouped
    }
}

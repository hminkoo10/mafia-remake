// stocks/view.rs — 화면·명령어용 조회 (시세 요약, 종목 상세, 계좌, 평가액, 순위)

use super::model::*;
use super::trading::Book;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CompanySummary {
    pub code: String,
    pub name: String,
    pub sector: &'static str,
    pub player: bool,
    pub founder_name: Option<String>,
    pub status: &'static str,
    pub price: i64,
    pub prev_close: i64,
    pub change_bp: i64,
    pub volume: i64,
    pub market_cap: i64,
    pub managed: bool,
    pub halted: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MarketSummary {
    pub index: f64,
    pub index_prev: f64,
    pub index_open: f64,
    pub index_high: f64,
    pub index_low: f64,
    pub halted_until: i64,
    pub day_ms: i64,
    pub companies: Vec<CompanySummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RightsView {
    pub price: i64,
    pub shares: i64,
    pub until: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HolderView {
    pub name: String,
    pub qty: i64,
    /// 지분율 (만분율).
    pub share_bp: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompanyDetail {
    pub summary: CompanySummary,
    pub description: String,
    pub shares: i64,
    pub equity: i64,
    pub paid_in: i64,
    pub bvps: i64,
    pub open: i64,
    pub high: i64,
    pub low: i64,
    pub turnover: i64,
    pub lower_limit: i64,
    pub upper_limit: i64,
    pub dividend_yield_bp: i64,
    pub next_earnings_at: i64,
    pub consensus: Option<i64>,
    pub quarters: Vec<QuarterResult>,
    pub risk: u8,
    pub book: Book,
    pub ipo: Option<IpoOffering>,
    pub ipo_requested: i64,
    pub rights: Option<RightsView>,
    pub buyback: Option<Buyback>,
    pub pending_dividend: Option<PendingDividend>,
    pub liquidation: Option<(i64, String)>,
    pub delisted: Option<(i64, String)>,
    pub holders: Vec<HolderView>,
    pub founded_at: i64,
    pub listed_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PositionView {
    pub code: String,
    pub name: String,
    pub qty: i64,
    pub available: i64,
    pub avg_price: i64,
    pub price: i64,
    pub value: i64,
    pub pnl: i64,
    pub pnl_bp: i64,
    pub lockup_qty: i64,
    pub lockup_until: i64,
    pub status: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderView {
    pub id: u64,
    pub code: String,
    pub name: String,
    pub side: Side,
    pub limit: i64,
    pub remaining: i64,
    pub original: i64,
    pub reserved: i64,
    pub created_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubscriptionView {
    pub code: String,
    pub name: String,
    pub qty: i64,
    pub deposit: i64,
    pub closes_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RightsClaimView {
    pub code: String,
    pub name: String,
    pub granted: i64,
    pub exercised: i64,
    pub price: i64,
    pub until: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountView {
    pub positions: Vec<PositionView>,
    pub orders: Vec<OrderView>,
    pub subscriptions: Vec<SubscriptionView>,
    pub rights: Vec<RightsClaimView>,
    /// 보유 주식 평가액.
    pub stock_value: i64,
    /// 주문 증거금·청약 증거금·유상증자 대금 (아직 시장에 묶인 코인).
    pub pending: i64,
    pub unrealized: i64,
    pub realized: i64,
    pub fees: i64,
    pub fills: Vec<FillRecord>,
    /// 내가 대표인 회사 코드.
    pub companies: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RankingEntry {
    pub user: u64,
    pub name: String,
    /// 주식 평가액 + 묶인 코인.
    pub value: i64,
    /// 평가 손익 + 실현 손익.
    pub profit: i64,
}

impl StockMarket {
    pub fn summary_of(&self, company: &Company, now: i64) -> CompanySummary {
        CompanySummary {
            code: company.code.clone(),
            name: company.name.clone(),
            sector: company.sector.name(),
            player: company.is_player(),
            founder_name: match &company.kind {
                CompanyKind::Player { founder_name, .. } => Some(founder_name.clone()),
                CompanyKind::System => None,
            },
            status: company.status.label(),
            price: company.price,
            prev_close: company.prev_close,
            change_bp: company.change_bp(),
            volume: company.volume,
            market_cap: company.market_cap(),
            managed: company.managed_since.is_some(),
            halted: company.admin_halt || now < company.halted_until || now < self.halted_until,
        }
    }

    /// 시세 요약 (상장폐지된 회사는 뺀다).
    pub fn market_summary(&self, now: i64, rules: &StockRules) -> MarketSummary {
        let mut companies = self
            .companies
            .values()
            .filter(|company| company.status.is_active())
            .map(|company| self.summary_of(company, now))
            .collect::<Vec<_>>();
        companies.sort_by(|left, right| {
            left.player
                .cmp(&right.player)
                .then(right.market_cap.cmp(&left.market_cap))
        });
        MarketSummary {
            index: self.index.value,
            index_prev: self.index.prev_close,
            index_open: self.index.open,
            index_high: self.index.high,
            index_low: self.index.low,
            halted_until: self.halted_until,
            day_ms: rules.day_ms(),
            companies,
        }
    }

    /// 종목 코드나 이름으로 찾는다 (정확히 같은 이름, 코드, 이름 일부 순).
    pub fn find_company(&self, query: &str) -> Option<&Company> {
        let query = query.trim();
        if query.is_empty() {
            return None;
        }
        if let Some(company) = self.companies.get(query) {
            return Some(company);
        }
        let active = || {
            self.companies
                .values()
                .filter(|company| company.status.is_active())
        };
        active()
            .find(|company| company.name == query)
            .or_else(|| active().find(|company| company.name.contains(query)))
            .or_else(|| {
                self.companies
                    .values()
                    .find(|company| company.name == query)
            })
    }

    pub fn company_detail(
        &self,
        code: &str,
        now: i64,
        rules: &StockRules,
    ) -> Option<CompanyDetail> {
        let company = self.companies.get(code)?;
        let (lower_limit, upper_limit) = self.limits(company, rules);
        let upper_limit = upper_limit.min(i64::MAX / 8);
        let book = self.book(code, rules).unwrap_or_default();
        let mut holders = self
            .accounts
            .values()
            .filter_map(|account| {
                account
                    .positions
                    .get(code)
                    .filter(|position| position.qty > 0)
                    .map(|position| HolderView {
                        name: account.name.clone(),
                        qty: position.qty,
                        share_bp: position.qty.saturating_mul(BP) / company.shares.max(1),
                    })
            })
            .collect::<Vec<_>>();
        holders.sort_by_key(|holder| std::cmp::Reverse(holder.qty));
        holders.truncate(10);
        let ipo_requested = self
            .subscriptions
            .iter()
            .filter(|subscription| subscription.code == code)
            .map(|subscription| subscription.qty)
            .sum();
        Some(CompanyDetail {
            summary: self.summary_of(company, now),
            description: company.description.clone(),
            shares: company.shares,
            equity: company.equity,
            paid_in: company.paid_in,
            bvps: company.bvps() as i64,
            open: company.open,
            high: company.high,
            low: company.low,
            turnover: company.turnover,
            lower_limit,
            upper_limit,
            dividend_yield_bp: company.dividend_yield_bp,
            next_earnings_at: company.next_earnings_at,
            consensus: company.consensus,
            quarters: company.quarters.iter().cloned().collect(),
            risk: company.risk,
            book,
            ipo: match &company.status {
                CompanyStatus::Subscription(offering) => Some(offering.clone()),
                _ => None,
            },
            ipo_requested,
            rights: company.rights.as_ref().map(|rights| RightsView {
                price: rights.price,
                shares: rights.shares,
                until: rights.until,
            }),
            buyback: company.buyback.clone(),
            pending_dividend: company.pending_dividend.clone(),
            liquidation: match &company.status {
                CompanyStatus::Liquidating { until, reason, .. } => Some((*until, reason.clone())),
                _ => None,
            },
            delisted: match &company.status {
                CompanyStatus::Delisted { at, reason } => Some((*at, reason.clone())),
                _ => None,
            },
            holders,
            founded_at: company.founded_at,
            listed_at: company.listed_at,
        })
    }

    /// 보유 주식의 평가 가격 (비상장은 주당 순자산, 상장폐지는 0).
    fn mark_price(company: &Company) -> i64 {
        match company.status {
            CompanyStatus::Private => company.bvps() as i64,
            CompanyStatus::Delisted { .. } => 0,
            _ => company.price,
        }
    }

    pub fn account_view(&self, user: u64, now: i64) -> AccountView {
        let account = self.accounts.get(&user);
        let mut positions = Vec::new();
        let mut stock_value = 0_i64;
        let mut unrealized = 0_i64;
        if let Some(account) = account {
            for (code, position) in &account.positions {
                let Some(company) = self.companies.get(code) else {
                    continue;
                };
                let price = Self::mark_price(company);
                let value = price.saturating_mul(position.qty);
                let pnl = value - position.cost;
                stock_value = stock_value.saturating_add(value);
                unrealized = unrealized.saturating_add(pnl);
                positions.push(PositionView {
                    code: code.clone(),
                    name: company.name.clone(),
                    qty: position.qty,
                    available: position.available(now),
                    avg_price: if position.qty > 0 {
                        position.cost / position.qty
                    } else {
                        0
                    },
                    price,
                    value,
                    pnl,
                    pnl_bp: if position.cost > 0 {
                        pnl.saturating_mul(BP) / position.cost
                    } else {
                        0
                    },
                    lockup_qty: if now < position.lockup_until {
                        position.lockup_qty
                    } else {
                        0
                    },
                    lockup_until: position.lockup_until,
                    status: company.status.label(),
                });
            }
        }
        positions.sort_by_key(|position| std::cmp::Reverse(position.value));
        let name_of = |code: &str| {
            self.companies
                .get(code)
                .map_or_else(|| code.to_string(), |company| company.name.clone())
        };
        let orders = self
            .orders
            .iter()
            .filter(|order| order.user == user)
            .map(|order| OrderView {
                id: order.id,
                code: order.code.clone(),
                name: name_of(&order.code),
                side: order.side,
                limit: order.limit,
                remaining: order.remaining,
                original: order.original,
                reserved: order.reserved,
                created_at: order.created_at,
                expires_at: order.expires_at,
            })
            .collect::<Vec<_>>();
        let subscriptions = self
            .subscriptions
            .iter()
            .filter(|subscription| subscription.user == user)
            .map(|subscription| SubscriptionView {
                code: subscription.code.clone(),
                name: name_of(&subscription.code),
                qty: subscription.qty,
                deposit: subscription.deposit,
                closes_at: match self
                    .companies
                    .get(&subscription.code)
                    .map(|company| &company.status)
                {
                    Some(CompanyStatus::Subscription(offering)) => offering.closes_at,
                    _ => 0,
                },
            })
            .collect::<Vec<_>>();
        let rights = self
            .companies
            .values()
            .filter_map(|company| {
                let rights = company.rights.as_ref()?;
                let granted = rights.rights.get(&user).copied().unwrap_or(0);
                (granted > 0).then(|| RightsClaimView {
                    code: company.code.clone(),
                    name: company.name.clone(),
                    granted,
                    exercised: rights.exercised.get(&user).copied().unwrap_or(0),
                    price: rights.price,
                    until: rights.until,
                })
            })
            .collect::<Vec<_>>();
        let pending = orders.iter().map(|order| order.reserved).sum::<i64>()
            + subscriptions
                .iter()
                .map(|subscription| subscription.deposit)
                .sum::<i64>()
            + rights
                .iter()
                .map(|claim| claim.exercised.saturating_mul(claim.price))
                .sum::<i64>();
        AccountView {
            positions,
            orders,
            subscriptions,
            rights,
            stock_value,
            pending,
            unrealized,
            realized: account.map_or(0, |account| account.realized),
            fees: account.map_or(0, |account| account.fees),
            fills: account
                .map(|account| account.fills.iter().rev().take(20).cloned().collect())
                .unwrap_or_default(),
            companies: self
                .companies
                .values()
                .filter(|company| company.founder() == Some(user) && company.status.is_active())
                .map(|company| company.code.clone())
                .collect(),
        }
    }

    /// 주식 시장에 있는 이 사용자의 재산 (평가액 + 묶인 코인). 구조금 기준에 코인과 함께 센다.
    pub fn portfolio_value(&self, user: u64, now: i64) -> i64 {
        let view = self.account_view(user, now);
        view.stock_value.saturating_add(view.pending)
    }

    /// 주식 재산 순위.
    pub fn ranking(&self, now: i64) -> Vec<RankingEntry> {
        let mut users = self.accounts.keys().copied().collect::<Vec<_>>();
        users.extend(self.orders.iter().map(|order| order.user));
        users.extend(
            self.subscriptions
                .iter()
                .map(|subscription| subscription.user),
        );
        users.sort_unstable();
        users.dedup();
        let mut entries = users
            .into_iter()
            .map(|user| {
                let view = self.account_view(user, now);
                RankingEntry {
                    user,
                    name: self
                        .accounts
                        .get(&user)
                        .map(|account| account.name.clone())
                        .unwrap_or_default(),
                    value: view.stock_value.saturating_add(view.pending),
                    profit: view.unrealized.saturating_add(view.realized),
                }
            })
            .filter(|entry| entry.value > 0 || entry.profit != 0)
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.value));
        entries
    }
}

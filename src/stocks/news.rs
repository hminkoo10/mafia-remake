// stocks/news.rs — 뉴스·루머·경제 소식 만들기 (제목과 가격 충격)

use super::model::Sector;
use super::price::uniform;
use rand::{Rng, RngCore};

/// 만든 기업 뉴스.
#[derive(Debug, Clone, PartialEq)]
pub struct NewsDraft {
    pub headline: String,
    pub tone: i8,
    /// 투자 심리에 더할 충격 (로그 단위).
    pub jump: f64,
    /// 루머면: (사실인지, 사실로 밝혀지면 더할 충격). 처음에는 `jump`만 반영한다.
    pub rumor: Option<(bool, f64)>,
    pub topic: String,
}

/// 경제 소식: 시장 전체와 한 업종에 주는 충격.
#[derive(Debug, Clone, PartialEq)]
pub struct MacroDraft {
    pub headline: String,
    pub tone: i8,
    pub market_jump: f64,
    pub sector: Option<(Sector, f64)>,
}

fn topics(sector: Sector) -> &'static [&'static str] {
    match sector {
        Sector::Semiconductor => &[
            "고대역폭 메모리",
            "파운드리",
            "AI 서버용 D램",
            "차세대 공정",
        ],
        Sector::Battery => &["전고체 배터리", "양극재", "북미 공장", "배터리 재활용"],
        Sector::Auto => &["전기차", "자율주행", "신차 판매", "하이브리드"],
        Sector::Internet => &["AI 비서", "광고 매출", "구독 서비스", "커머스"],
        Sector::Game => &["신작", "글로벌 퍼블리싱", "이용자 수", "e스포츠"],
        Sector::Bio => &["신약 후보물질", "기술수출", "바이오시밀러", "위탁생산"],
        Sector::Bank => &["순이자마진", "대출 성장", "건전성", "주주환원"],
        Sector::Leisure => &["방문객", "해외 관광객", "리조트 개장", "카지노 매출"],
        Sector::Retail => &["온라인 매출", "물류센터", "명절 특수", "자체 브랜드"],
        Sector::Telecom => &["5G 가입자", "요금제", "데이터센터", "B2B 사업"],
        Sector::Shipbuilding => &["LNG선 수주", "해양플랜트", "선가", "친환경 선박"],
        Sector::Entertainment => &["월드투어", "신인 그룹", "음원 차트", "팬덤 플랫폼"],
    }
}

/// 충격 크기: 작음 70%, 중간 25%, 큼 5% (바이오는 큰 소식이 더 크다).
fn magnitude(sector: Sector, rng: &mut dyn RngCore) -> (f64, u8) {
    let roll = uniform(rng);
    let (low, high, tier) = if roll < 0.70 {
        (0.01, 0.03, 0)
    } else if roll < 0.95 {
        (0.04, 0.09, 1)
    } else if sector == Sector::Bio {
        (0.15, 0.35, 2)
    } else {
        (0.10, 0.20, 2)
    };
    (low + (high - low) * uniform(rng), tier)
}

/// 기업 뉴스 하나 (소형·고변동 종목일수록 루머가 잦다).
pub fn company_news(
    name: &str,
    sector: Sector,
    rumor_rate: f64,
    rng: &mut dyn RngCore,
) -> NewsDraft {
    let list = topics(sector);
    let topic = list[rng.random_range(0..list.len())];
    let good = uniform(rng) < 0.5;
    let (size, tier) = magnitude(sector, rng);
    // 호재·악재의 로그 충격 크기를 같게 한다 (악재를 -size%로 두면 로그로는 더 커서, 뉴스가 쌓일수록
    // 주가가 조금씩 내려갔다).
    let jump = if good { size.ln_1p() } else { -size.ln_1p() };
    if uniform(rng) < rumor_rate {
        let truth = uniform(rng) < 0.5;
        // "[루머]" 표시는 뉴스 종류(NewsKind::Rumor)로 붙는다. 제목에도 넣으면 두 번 나온다.
        let headline = if good {
            format!("{name}, {topic} 관련 대형 호재 임박설")
        } else {
            format!("{name}, {topic} 관련 악재 발생설")
        };
        return NewsDraft {
            headline,
            tone: if good { 1 } else { -1 },
            // 루머는 절반만 먼저 반영되고, 사실이면 나머지가 더해진다.
            jump: jump * 0.5,
            rumor: Some((truth, jump * 0.5)),
            topic: topic.to_string(),
        };
    }
    let headline = if sector == Sector::Bio && tier == 2 {
        if good {
            format!("{name}, 임상 3상 성공… 신약 허가 기대")
        } else {
            format!("{name}, 임상 3상 주평가지표 미달")
        }
    } else {
        let variant = rng.random_range(0..5);
        match (good, variant) {
            (true, 0) => format!("{name}, {topic} 호조로 강세"),
            (true, 1) => format!("{name}, {topic} 관련 대형 계약 체결"),
            (true, 2) => format!("증권가 \"{name} {topic} 성장 본격화\"… 목표주가 상향"),
            (true, 3) => format!("{name}, {topic} 기대감에 외국인 순매수"),
            (true, _) => format!("{name}, {topic}에서 예상 밖 성과"),
            (false, 0) => format!("{name}, {topic} 부진 우려에 약세"),
            (false, 1) => format!("{name}, {topic} 관련 계약 무산 소식"),
            (false, 2) => format!("증권가 \"{name} {topic} 둔화\"… 목표주가 하향"),
            (false, 3) => format!("{name}, {topic} 경쟁 심화에 외국인 매도"),
            (false, _) => format!("{name}, {topic} 관련 규제 위험 부각"),
        }
    };
    NewsDraft {
        headline,
        tone: if good { 1 } else { -1 },
        jump,
        rumor: None,
        topic: topic.to_string(),
    }
}

/// 경제 소식 한 쌍. 호재·악재가 반씩 나오고 악재는 충격의 부호만 뒤집으므로, 소식이 쌓여도 시장·업종
/// 팩터가 한쪽으로 흘러가지 않는다 (예전에는 악재 쪽 시장 충격이 더 크고 업종 소식은 한 방향뿐이어서,
/// 시장은 계속 내리고 2차전지는 내리기만, 게임·레저는 오르기만 했다).
struct MacroEvent {
    good: &'static str,
    bad: &'static str,
    /// 호재일 때 시장 충격 배율.
    market: f64,
    /// 호재일 때 업종 충격 (업종, 배율).
    sector: Option<(Sector, f64)>,
}

const MACRO_EVENTS: [MacroEvent; 11] = [
    MacroEvent {
        good: "한국은행, 기준금리 0.25%p 인하",
        bad: "한국은행, 기준금리 0.25%p 인상",
        market: 1.0,
        sector: Some((Sector::Bank, -1.0)),
    },
    MacroEvent {
        good: "수출 지표 호조… 석 달 연속 증가",
        bad: "수출 지표 부진… 석 달 연속 감소",
        market: 1.0,
        sector: Some((Sector::Semiconductor, 1.0)),
    },
    MacroEvent {
        good: "미국 증시 급등에 투자 심리 회복",
        bad: "미국 증시 급락 여파로 투자 심리 위축",
        market: 1.5,
        sector: None,
    },
    MacroEvent {
        good: "외국인, 대규모 순매수 전환",
        bad: "외국인, 대규모 순매도 전환",
        market: 1.0,
        sector: None,
    },
    MacroEvent {
        good: "소비 심리 회복 조짐",
        bad: "경기 침체 우려 확산",
        market: 1.0,
        sector: Some((Sector::Retail, 1.0)),
    },
    MacroEvent {
        good: "원·달러 환율 급락",
        bad: "원·달러 환율 급등",
        market: 0.5,
        sector: Some((Sector::Auto, -1.0)),
    },
    MacroEvent {
        good: "반도체 업황 회복 신호",
        bad: "반도체 업황 둔화 우려",
        market: 0.5,
        sector: Some((Sector::Semiconductor, 2.0)),
    },
    MacroEvent {
        good: "전기차 보조금 확대 발표",
        bad: "전기차 보조금 축소 발표",
        market: 0.0,
        sector: Some((Sector::Battery, 2.0)),
    },
    MacroEvent {
        good: "정부, 게임·콘텐츠 산업 규제 완화",
        bad: "정부, 게임·콘텐츠 산업 규제 강화",
        market: 0.0,
        sector: Some((Sector::Game, 2.0)),
    },
    MacroEvent {
        good: "국제 유가 급락",
        bad: "국제 유가 급등",
        market: 0.5,
        sector: Some((Sector::Shipbuilding, -1.0)),
    },
    MacroEvent {
        good: "해외 관광객 입국 크게 늘어",
        bad: "해외 관광객 입국 크게 줄어",
        market: 0.3,
        sector: Some((Sector::Leisure, 2.0)),
    },
];

/// 경제 소식 하나.
pub fn macro_news(rng: &mut dyn RngCore) -> MacroDraft {
    let event = &MACRO_EVENTS[rng.random_range(0..MACRO_EVENTS.len())];
    let good = uniform(rng) < 0.5;
    let size = (0.005 + 0.02 * uniform(rng)) * if good { 1.0 } else { -1.0 };
    MacroDraft {
        headline: if good { event.good } else { event.bad }.to_string(),
        tone: if good { 1 } else { -1 },
        market_jump: size * event.market,
        sector: event.sector.map(|(sector, weight)| (sector, size * weight)),
    }
}

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
    /// 루머 결과 공시에서 부르는 이름 ("○○ 관련 소문 사실로 확인").
    pub topic: String,
}

/// 경제 소식: 시장 전체와 몇 업종에 주는 충격.
#[derive(Debug, Clone, PartialEq)]
pub struct MacroDraft {
    pub headline: String,
    pub tone: i8,
    pub market_jump: f64,
    pub sectors: Vec<(Sector, f64)>,
}

fn topics(sector: Sector) -> &'static [&'static str] {
    match sector {
        Sector::Semiconductor => &[
            "고대역폭 메모리",
            "파운드리",
            "AI 서버용 D램",
            "차세대 공정",
            "낸드 플래시",
            "시스템 반도체",
            "첨단 패키징",
            "모바일 D램",
        ],
        Sector::Battery => &[
            "전고체 배터리",
            "양극재",
            "북미 공장",
            "배터리 재활용",
            "음극재",
            "ESS",
            "LFP 배터리",
            "리튬 확보",
        ],
        Sector::Auto => &[
            "전기차",
            "자율주행",
            "신차 판매",
            "하이브리드",
            "수소차",
            "SUV 라인업",
            "미국 판매",
            "로보택시",
        ],
        Sector::Internet => &[
            "AI 비서",
            "광고 매출",
            "구독 서비스",
            "커머스",
            "클라우드",
            "간편결제",
            "웹툰",
            "검색 점유율",
        ],
        Sector::Game => &[
            "신작",
            "글로벌 퍼블리싱",
            "이용자 수",
            "e스포츠",
            "모바일 게임",
            "콘솔 진출",
            "IP 확장",
            "확률형 아이템",
        ],
        Sector::Bio => &[
            "신약 후보물질",
            "기술수출",
            "바이오시밀러",
            "위탁생산",
            "항암제",
            "비만 치료제",
            "임상 2상",
            "유전자 치료제",
        ],
        Sector::Bank => &[
            "순이자마진",
            "대출 성장",
            "건전성",
            "주주환원",
            "디지털 뱅킹",
            "해외 법인",
            "PF 대출",
            "연체율",
        ],
        Sector::Leisure => &[
            "방문객",
            "해외 관광객",
            "리조트 개장",
            "카지노 매출",
            "크루즈",
            "호텔 객실",
            "VIP 고객",
            "면세점",
        ],
        Sector::Retail => &[
            "온라인 매출",
            "물류센터",
            "명절 특수",
            "자체 브랜드",
            "새벽 배송",
            "편의점",
            "해외 직구",
            "멤버십",
        ],
        Sector::Telecom => &[
            "5G 가입자",
            "요금제",
            "데이터센터",
            "B2B 사업",
            "AI 통화",
            "위성 통신",
            "IPTV",
            "알뜰폰",
        ],
        Sector::Shipbuilding => &[
            "LNG선 수주",
            "해양플랜트",
            "선가",
            "친환경 선박",
            "암모니아 추진선",
            "특수선",
            "컨테이너선",
            "방산 함정",
        ],
        Sector::Entertainment => &[
            "월드투어",
            "신인 그룹",
            "음원 차트",
            "팬덤 플랫폼",
            "드라마 제작",
            "해외 오디션",
            "굿즈 매출",
            "멀티 레이블",
        ],
    }
}

// 제목 틀: (호재, 악재). `{name}`에 회사 이름, `{topic}`에 그 업종의 주제가 들어간다.

/// 작은 소식 (수급·시황).
const SMALL_NEWS: [(&str, &str); 7] = [
    (
        "{name}, {topic} 기대감에 강세",
        "{name}, {topic} 우려에 약세",
    ),
    (
        "{name}, {topic} 호조로 오름세",
        "{name}, {topic} 부진에 내림세",
    ),
    (
        "기관, {name} 순매수… {topic} 주목",
        "기관, {name} 순매도… {topic} 불확실성",
    ),
    (
        "외국인, {name} 사흘째 순매수",
        "외국인, {name} 사흘째 순매도",
    ),
    (
        "증권가 \"{name} {topic} 긍정적\"",
        "증권가 \"{name} {topic} 신중해야\"",
    ),
    ("{name}, 저가 매수세 유입", "{name}, 차익 실현 매물에 하락"),
    (
        "{name}, {topic} 관련 신제품 공개",
        "공매도 잔고 늘어난 {name} 약세",
    ),
];

/// 중간 소식 (사업·계약·평가).
const MEDIUM_NEWS: [(&str, &str); 7] = [
    (
        "{name}, {topic} 관련 대형 계약 체결",
        "{name}, {topic} 관련 계약 무산",
    ),
    (
        "증권가 \"{name} {topic} 성장 본격화\"… 목표주가 상향",
        "증권가 \"{name} {topic} 둔화\"… 목표주가 하향",
    ),
    (
        "{name}, {topic} 신사업 진출 발표",
        "{name}, {topic} 사업 적자 확대",
    ),
    (
        "{name}, {topic} 핵심 기술 특허 취득",
        "{name}, {topic} 관련 특허 소송 피소",
    ),
    (
        "{name}, 글로벌 기업과 {topic} 전략적 제휴",
        "{name}, {topic} 핵심 인력 대거 이탈",
    ),
    ("{name}, 신용등급 상향", "{name}, 신용등급 하향 검토"),
    (
        "{name}, 정부 {topic} 지원 사업 선정",
        "{name}, 공정위 현장 조사",
    ),
];

/// 큰 소식 가운데 업종과 상관없는 것.
const BIG_NEWS: [(&str, &str); 2] = [
    (
        "{name}, 사상 최대 실적 전망… 목표주가 줄상향",
        "{name}, 대표 횡령 혐의로 압수수색",
    ),
    (
        "{name}, {topic} 부문 글로벌 1위 등극",
        "{name}, {topic} 사업 철수 검토",
    ),
];

/// 업종별 큰 소식.
fn sector_big_news(sector: Sector) -> &'static [(&'static str, &'static str)] {
    match sector {
        Sector::Semiconductor => &[
            (
                "{name}, 글로벌 빅테크에 HBM 대규모 공급 확정",
                "{name}, 메모리 가격 급락에 재고 부담 확대",
            ),
            (
                "{name}, 차세대 공정 수율 안정화 성공",
                "{name}, 차세대 공정 수율 문제로 양산 지연",
            ),
        ],
        Sector::Battery => &[
            (
                "{name}, 완성차와 10년 장기 공급 계약",
                "{name} 배터리 탑재 차량 화재… 리콜 우려",
            ),
            (
                "{name}, 전고체 배터리 양산 성공",
                "{name}, 북미 공장 가동 중단",
            ),
        ],
        Sector::Auto => &[
            (
                "{name} 신차, 사전계약 역대 최다",
                "{name}, 엔진 결함으로 대규모 리콜",
            ),
            (
                "{name}, 미국 시장 점유율 사상 최고",
                "{name}, 노조 파업으로 공장 가동 중단",
            ),
        ],
        Sector::Internet => &[
            (
                "{name}, 월간 이용자 수 사상 최대",
                "{name}, 대규모 서비스 장애로 이용자 이탈",
            ),
            (
                "{name}, AI 서비스 유료 구독자 폭증",
                "{name}, 개인정보 유출로 과징금 부과",
            ),
        ],
        Sector::Game => &[
            (
                "{name} 신작, 출시 첫 주 매출 1위",
                "{name} 신작, 흥행 부진에 매출 순위 급락",
            ),
            (
                "{name}, 글로벌 대작 판호 획득",
                "{name}, 확률 조작 논란에 이용자 집단 소송",
            ),
        ],
        Sector::Bio => &[
            (
                "{name}, 임상 3상 성공… 신약 허가 기대",
                "{name}, 임상 3상 주평가지표 미달",
            ),
            ("{name}, FDA 품목허가 획득", "{name}, FDA 보완요구서한 수령"),
            (
                "{name}, 글로벌 제약사에 조 단위 기술수출",
                "{name}, 기술수출 계약 해지 통보",
            ),
        ],
        Sector::Bank => &[
            (
                "{name}, 분기 최대 순이익 전망",
                "{name}, 부동산 PF 부실로 대규모 충당금",
            ),
            (
                "{name}, 해외 은행 인수로 성장 발판 마련",
                "{name}, 금융당국 중징계 예고",
            ),
        ],
        Sector::Leisure => &[
            (
                "{name}, 연휴 예약률 사상 최고",
                "{name}, 태풍 피해로 리조트 운영 중단",
            ),
            (
                "{name}, 복합 리조트 사업자 선정",
                "{name}, 카지노 VIP 고객 급감",
            ),
        ],
        Sector::Retail => &[
            (
                "{name}, 온라인 매출 사상 최대",
                "{name}, 오프라인 점포 대규모 구조조정",
            ),
            (
                "{name}, 초대형 물류센터 가동",
                "{name}, 납품 비리로 공정위 과징금",
            ),
        ],
        Sector::Telecom => &[
            (
                "{name}, 초대형 데이터센터 고객 유치",
                "{name}, 해킹으로 가입자 정보 유출",
            ),
            (
                "{name}, 위성 통신 사업자 선정",
                "{name}, 통신 장애로 대규모 보상",
            ),
        ],
        Sector::Shipbuilding => &[
            (
                "{name}, LNG선 10척 수주… 역대 최대",
                "{name}, 해양플랜트 손실 대규모 반영",
            ),
            (
                "{name}, 해외 해군 함정 수주",
                "{name}, 수주 계약 무더기 취소",
            ),
        ],
        Sector::Entertainment => &[
            (
                "{name} 소속 그룹, 빌보드 1위",
                "{name} 소속 아티스트, 전속계약 분쟁",
            ),
            (
                "{name}, 월드투어 전석 매진",
                "{name} 소속 아티스트 사생활 논란",
            ),
        ],
    }
}

/// 루머 한 가지: (제목, 결과 공시에서 부르는 이름).
type Rumor = (&'static str, &'static str);

/// 루머: (호재, 악재).
const RUMORS: [(Rumor, Rumor); 3] = [
    (
        ("{name}, {topic} 관련 대형 호재 임박설", "{topic}"),
        ("{name}, {topic} 관련 악재 발생설", "{topic}"),
    ),
    (
        ("{name}, 대기업과 합작 법인 설립설", "합작 법인"),
        ("{name}, 유동성 위기설", "유동성 위기"),
    ),
    (
        ("{name}, {topic} 대규모 수주 임박설", "{topic} 수주"),
        ("{name}, 회계 처리 의혹설", "회계 처리 의혹"),
    ),
];

/// 충격 크기: 작음 70%, 중간 25%, 큼 5% (바이오는 큰 소식이 더 크다).
fn magnitude(sector: Sector, rng: &mut dyn RngCore) -> (f64, u8) {
    let roll = uniform(rng);
    let (low, high, tier) = if roll < 0.70 {
        (0.015, 0.04, 0)
    } else if roll < 0.95 {
        (0.05, 0.12, 1)
    } else if sector == Sector::Bio {
        (0.20, 0.45, 2)
    } else {
        (0.12, 0.25, 2)
    };
    (low + (high - low) * uniform(rng), tier)
}

/// 기업 뉴스 하나 (소형·고변동 종목일수록 루머가 잦다). 제목은 충격 크기에 맞는 틀에서 고른다.
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
    let tone = if good { 1 } else { -1 };
    // 주제를 먼저 채운다 (주제에는 `{name}`이 없고, 회사 이름에는 중괄호를 쓸 수 없다).
    let fill = |template: &str| template.replace("{topic}", topic).replace("{name}", name);
    let pick = |pair: (&'static str, &'static str)| if good { pair.0 } else { pair.1 };
    if uniform(rng) < rumor_rate {
        let truth = uniform(rng) < 0.5;
        let (good_rumor, bad_rumor) = RUMORS[rng.random_range(0..RUMORS.len())];
        // "[루머]" 표시는 뉴스 종류(NewsKind::Rumor)로 붙는다. 제목에도 넣으면 두 번 나온다.
        let (headline, subject) = if good { good_rumor } else { bad_rumor };
        return NewsDraft {
            headline: fill(headline),
            tone,
            // 루머는 절반만 먼저 반영되고, 사실이면 나머지가 더해진다.
            jump: jump * 0.5,
            rumor: Some((truth, jump * 0.5)),
            topic: fill(subject),
        };
    }
    let template = match tier {
        0 => pick(SMALL_NEWS[rng.random_range(0..SMALL_NEWS.len())]),
        1 => pick(MEDIUM_NEWS[rng.random_range(0..MEDIUM_NEWS.len())]),
        _ => {
            let sector_news = sector_big_news(sector);
            if uniform(rng) < 0.7 {
                pick(sector_news[rng.random_range(0..sector_news.len())])
            } else {
                pick(BIG_NEWS[rng.random_range(0..BIG_NEWS.len())])
            }
        }
    };
    NewsDraft {
        headline: fill(template),
        tone,
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
    sectors: &'static [(Sector, f64)],
}

const MACRO_EVENTS: [MacroEvent; 24] = [
    MacroEvent {
        good: "한국은행, 기준금리 0.25%p 인하",
        bad: "한국은행, 기준금리 0.25%p 인상",
        market: 1.0,
        sectors: &[(Sector::Bank, -1.0)],
    },
    MacroEvent {
        good: "수출 지표 호조… 석 달 연속 증가",
        bad: "수출 지표 부진… 석 달 연속 감소",
        market: 1.0,
        sectors: &[(Sector::Semiconductor, 1.0), (Sector::Auto, 0.5)],
    },
    MacroEvent {
        good: "미국 증시 급등에 투자 심리 회복",
        bad: "미국 증시 급락 여파로 투자 심리 위축",
        market: 1.5,
        sectors: &[],
    },
    MacroEvent {
        good: "외국인, 대규모 순매수 전환",
        bad: "외국인, 대규모 순매도 전환",
        market: 1.0,
        sectors: &[],
    },
    MacroEvent {
        good: "소비 심리 회복 조짐",
        bad: "경기 침체 우려 확산",
        market: 1.0,
        sectors: &[(Sector::Retail, 1.0)],
    },
    MacroEvent {
        good: "원·달러 환율 급락",
        bad: "원·달러 환율 급등",
        market: 0.5,
        sectors: &[(Sector::Auto, -1.0)],
    },
    MacroEvent {
        good: "반도체 업황 회복 신호",
        bad: "반도체 업황 둔화 우려",
        market: 0.5,
        sectors: &[(Sector::Semiconductor, 2.0)],
    },
    MacroEvent {
        good: "전기차 보조금 확대 발표",
        bad: "전기차 보조금 축소 발표",
        market: 0.0,
        sectors: &[(Sector::Battery, 2.0)],
    },
    MacroEvent {
        good: "정부, 게임·콘텐츠 산업 규제 완화",
        bad: "정부, 게임·콘텐츠 산업 규제 강화",
        market: 0.0,
        sectors: &[(Sector::Game, 2.0), (Sector::Entertainment, 1.0)],
    },
    MacroEvent {
        good: "국제 유가 급락",
        bad: "국제 유가 급등",
        market: 0.5,
        sectors: &[(Sector::Shipbuilding, -1.0)],
    },
    MacroEvent {
        good: "해외 관광객 입국 크게 늘어",
        bad: "해외 관광객 입국 크게 줄어",
        market: 0.3,
        sectors: &[(Sector::Leisure, 2.0)],
    },
    MacroEvent {
        good: "소비자물가 상승률 둔화",
        bad: "소비자물가 예상보다 크게 올라",
        market: 1.0,
        sectors: &[(Sector::Retail, 0.5)],
    },
    MacroEvent {
        good: "고용 지표 호조… 취업자 수 크게 늘어",
        bad: "실업률 상승… 고용 시장 냉각",
        market: 0.8,
        sectors: &[],
    },
    MacroEvent {
        good: "미 연준, 금리 인하 시사",
        bad: "미 연준, 금리 인상 시사",
        market: 1.2,
        sectors: &[(Sector::Internet, 1.0), (Sector::Bio, 1.0)],
    },
    MacroEvent {
        good: "중국, 대규모 경기 부양책 발표",
        bad: "중국 경기 둔화 우려 커져",
        market: 0.8,
        sectors: &[(Sector::Leisure, 1.0), (Sector::Shipbuilding, 0.5)],
    },
    MacroEvent {
        good: "남북 긴장 완화 기대감",
        bad: "지정학적 긴장 고조",
        market: 0.8,
        sectors: &[],
    },
    MacroEvent {
        good: "정부, 신약 허가 절차 간소화",
        bad: "정부, 약가 인하 정책 발표",
        market: 0.0,
        sectors: &[(Sector::Bio, 2.0)],
    },
    MacroEvent {
        good: "플랫폼 규제 법안 폐기",
        bad: "플랫폼 규제 법안 국회 통과",
        market: 0.0,
        sectors: &[(Sector::Internet, 2.0)],
    },
    MacroEvent {
        good: "정부, 통신 설비 투자 세액공제 확대",
        bad: "정부, 통신요금 인하 압박",
        market: 0.0,
        sectors: &[(Sector::Telecom, 2.0)],
    },
    MacroEvent {
        good: "중국, 한한령 해제 기대감",
        bad: "중국, 한류 콘텐츠 규제 강화",
        market: 0.0,
        sectors: &[(Sector::Entertainment, 2.0), (Sector::Leisure, 0.5)],
    },
    MacroEvent {
        good: "AI 투자 열풍… 빅테크 투자 확대",
        bad: "AI 거품 논란… 빅테크 투자 축소 우려",
        market: 0.5,
        sectors: &[(Sector::Semiconductor, 1.5), (Sector::Internet, 1.0)],
    },
    MacroEvent {
        good: "부동산 PF 우려 해소",
        bad: "부동산 PF 부실 우려 확산",
        market: 0.5,
        sectors: &[(Sector::Bank, 1.5)],
    },
    MacroEvent {
        good: "글로벌 선박 발주 급증",
        bad: "글로벌 선박 발주 급감",
        market: 0.0,
        sectors: &[(Sector::Shipbuilding, 2.0)],
    },
    MacroEvent {
        good: "공매도 금지 연장",
        bad: "공매도 전면 재개",
        market: 0.8,
        sectors: &[],
    },
];

/// 경제 소식 하나.
pub fn macro_news(rng: &mut dyn RngCore) -> MacroDraft {
    let event = &MACRO_EVENTS[rng.random_range(0..MACRO_EVENTS.len())];
    let good = uniform(rng) < 0.5;
    let size = (0.008 + 0.022 * uniform(rng)) * if good { 1.0 } else { -1.0 };
    MacroDraft {
        headline: if good { event.good } else { event.bad }.to_string(),
        tone: if good { 1 } else { -1 },
        market_jump: size * event.market,
        sectors: event
            .sectors
            .iter()
            .map(|&(sector, weight)| (sector, size * weight))
            .collect(),
    }
}

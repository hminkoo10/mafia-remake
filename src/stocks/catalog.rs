// stocks/catalog.rs — 처음 상장된 시스템 회사 12개와, 새로 상장할 시스템 회사 이름 만들기

use super::model::Sector;
use rand::{Rng, RngCore};

/// 시스템 회사의 처음 설정.
#[derive(Debug, Clone, Copy)]
pub struct CatalogEntry {
    pub code: &'static str,
    pub name: &'static str,
    pub sector: Sector,
    pub price: i64,
    pub shares: i64,
    pub beta: f64,
    /// 게임 하루 변동성.
    pub daily_vol: f64,
    /// 연 배당수익률 (만분율).
    pub dividend_yield_bp: i64,
    /// 게임 하루 평균 거래량 (주).
    pub adv: i64,
    pub spread_bp: i64,
    pub description: &'static str,
}

/// 처음 상장된 12개 회사. 게임 연동 두 종목(마피아게임즈·카지노73레저)의 실적에는 서버 활동이 들어간다.
pub const CATALOG: [CatalogEntry; 12] = [
    CatalogEntry {
        code: "100010",
        name: "하늘반도체",
        sector: Sector::Semiconductor,
        price: 72_000,
        shares: 5_000_000,
        beta: 1.2,
        daily_vol: 0.020,
        dividend_yield_bp: 150,
        adv: 6_000,
        spread_bp: 5,
        description: "메모리 반도체 대표 기업. 시장 흐름을 이끄는 대형주.",
    },
    CatalogEntry {
        code: "100020",
        name: "새빛에너지",
        sector: Sector::Battery,
        price: 185_000,
        shares: 1_200_000,
        beta: 1.4,
        daily_vol: 0.035,
        dividend_yield_bp: 0,
        adv: 1_500,
        spread_bp: 10,
        description: "전기차 배터리 양극재 제조사. 성장 기대가 큰 만큼 뉴스에 크게 흔들린다.",
    },
    CatalogEntry {
        code: "100030",
        name: "대한모터스",
        sector: Sector::Auto,
        price: 98_000,
        shares: 2_000_000,
        beta: 1.0,
        daily_vol: 0.018,
        dividend_yield_bp: 300,
        adv: 3_000,
        spread_bp: 5,
        description: "완성차 제조사. 경기와 환율에 민감하다.",
    },
    CatalogEntry {
        code: "100040",
        name: "모아톡",
        sector: Sector::Internet,
        price: 41_500,
        shares: 3_000_000,
        beta: 1.3,
        daily_vol: 0.028,
        dividend_yield_bp: 50,
        adv: 5_000,
        spread_bp: 10,
        description: "메신저·커뮤니티 플랫폼.",
    },
    CatalogEntry {
        code: "100050",
        name: "마피아게임즈",
        sector: Sector::Game,
        price: 23_800,
        shares: 1_500_000,
        beta: 1.1,
        daily_vol: 0.032,
        dividend_yield_bp: 0,
        adv: 4_000,
        spread_bp: 15,
        description: "마피아 게임 개발사. 이 서버에서 열린 마피아 판 수가 실적에 반영된다.",
    },
    CatalogEntry {
        code: "100060",
        name: "한빛바이오",
        sector: Sector::Bio,
        price: 12_400,
        shares: 2_500_000,
        beta: 0.8,
        daily_vol: 0.050,
        dividend_yield_bp: 0,
        adv: 8_000,
        spread_bp: 20,
        description: "신약 개발 바이오 기업. 임상 결과에 따라 주가가 크게 움직인다.",
    },
    CatalogEntry {
        code: "100070",
        name: "백두은행",
        sector: Sector::Bank,
        price: 8_900,
        shares: 10_000_000,
        beta: 0.7,
        daily_vol: 0.012,
        dividend_yield_bp: 500,
        adv: 20_000,
        spread_bp: 10,
        description: "시중 은행. 낮은 변동성과 높은 배당.",
    },
    CatalogEntry {
        code: "100080",
        name: "카지노73레저",
        sector: Sector::Leisure,
        price: 31_200,
        shares: 1_800_000,
        beta: 0.9,
        daily_vol: 0.022,
        dividend_yield_bp: 200,
        adv: 4_000,
        spread_bp: 10,
        description: "카지노·레저 운영사. 이 서버 카지노의 핸드 수와 하우스 손익이 실적에 반영된다.",
    },
    CatalogEntry {
        code: "100090",
        name: "온누리마트",
        sector: Sector::Retail,
        price: 54_300,
        shares: 1_000_000,
        beta: 0.8,
        daily_vol: 0.015,
        dividend_yield_bp: 250,
        adv: 2_000,
        spread_bp: 10,
        description: "대형마트·온라인 유통.",
    },
    CatalogEntry {
        code: "100100",
        name: "한결텔레콤",
        sector: Sector::Telecom,
        price: 36_700,
        shares: 4_000_000,
        beta: 0.5,
        daily_vol: 0.010,
        dividend_yield_bp: 550,
        adv: 4_000,
        spread_bp: 5,
        description: "이동통신사. 가장 안정적인 배당주.",
    },
    CatalogEntry {
        code: "100110",
        name: "동해중공업",
        sector: Sector::Shipbuilding,
        price: 15_600,
        shares: 6_000_000,
        beta: 1.5,
        daily_vol: 0.030,
        dividend_yield_bp: 0,
        adv: 10_000,
        spread_bp: 10,
        description: "조선·해양플랜트. 수주 소식과 경기에 크게 좌우된다.",
    },
    CatalogEntry {
        code: "100120",
        name: "별빛엔터",
        sector: Sector::Entertainment,
        price: 67_900,
        shares: 800_000,
        beta: 1.2,
        daily_vol: 0.038,
        dividend_yield_bp: 80,
        adv: 1_200,
        spread_bp: 20,
        description: "아이돌 기획사. 소형주라 루머에 민감하다.",
    },
];

/// 새로 상장할 시스템 회사 이름 (업종별 앞말 + 뒷말).
pub fn random_company_name(sector: Sector, rng: &mut dyn RngCore) -> String {
    const PREFIXES: [&str; 24] = [
        "새벽", "푸른", "한울", "누리", "다온", "가람", "미르", "라온", "온새", "해솔", "별하",
        "다솜", "나래", "한결", "아라", "도담", "새롬", "은빛", "금빛", "청솔", "한빛", "백두",
        "태백", "여명",
    ];
    let suffixes: &[&str] = match sector {
        Sector::Semiconductor => &["반도체", "실리콘", "칩스", "테크"],
        Sector::Battery => &["에너지", "배터리", "셀", "파워"],
        Sector::Auto => &["모터스", "오토", "모빌리티", "자동차"],
        Sector::Internet => &["넷", "소프트", "플랫폼", "랩스"],
        Sector::Game => &["게임즈", "스튜디오", "엔터테인먼트", "인터랙티브"],
        Sector::Bio => &["바이오", "제약", "파마", "헬스케어"],
        Sector::Bank => &["은행", "금융", "캐피탈", "증권"],
        Sector::Leisure => &["레저", "리조트", "호텔", "투어"],
        Sector::Retail => &["마트", "쇼핑", "리테일", "유통"],
        Sector::Telecom => &["텔레콤", "통신", "네트웍스", "모바일"],
        Sector::Shipbuilding => &["중공업", "조선", "해양", "엔지니어링"],
        Sector::Entertainment => &["엔터", "미디어", "뮤직", "픽처스"],
    };
    let prefix = PREFIXES[rng.random_range(0..PREFIXES.len())];
    let suffix = suffixes[rng.random_range(0..suffixes.len())];
    format!("{prefix}{suffix}")
}

/// 회사 이름 검사: 2~12자, 한글·영문·숫자와 공백만.
pub fn valid_company_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    let count = name.chars().count();
    if !(2..=12).contains(&count) {
        return Err("회사 이름은 2~12자여야 합니다.".to_string());
    }
    let ok = name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ('가'..='힣').contains(&ch) || ch == ' ');
    if !ok || name.contains("  ") {
        return Err("회사 이름에는 한글·영문·숫자만 쓸 수 있습니다.".to_string());
    }
    Ok(name.to_string())
}

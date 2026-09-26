// stocks.rs — 봇 내장 주식 시장 엔진 (가상 종목, 한국거래소식 규칙).
//
// 이 모듈은 Discord·HTTP·코인 잔액을 모르는 순수 규칙 엔진이다. 코인이 오가는 일(매수 대금,
// 매도 대금, 수수료·세금, 배당, 청약 증거금, 회사 자본)은 모두 `Transfer`로 장부(journal)에
// 남기고, 호출자(봇)가 그 장부를 통계 파일의 코인에 한 번씩 반영한다.
//
// 시간은 호출자가 주는 유닉스 ms이고, 난수는 호출자가 준다 (테스트는 시드를 고정한다).
// 게임 하루는 운영 설정의 길이(기본 실제 1시간)이고, 분기(실적·배당)는 실제 1주다.

mod catalog;
mod corporate;
mod market;
mod model;
mod news;
mod price;
mod trading;
mod view;

pub use self::catalog::*;
pub use self::corporate::*;
pub use self::market::*;
pub use self::model::*;
pub use self::news::*;
pub use self::price::*;
pub use self::trading::*;
pub use self::view::*;

#[cfg(test)]
mod tests;

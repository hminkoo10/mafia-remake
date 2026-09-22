// casino.rs — 봇 내장 카지노 엔진 (텍사스 홀덤·블랙잭). noir-casino의 규칙을 Rust로 옮긴 것.
//
// 이 모듈은 Discord·HTTP를 모르는 순수 규칙 엔진이다. 좌석의 칩(stack)만 다루고,
// 바이인·캐시아웃으로 오가는 코인은 호출자(봇)가 이벤트를 받아 처리한다.
// 카드는 "As", "Td"처럼 랭크+무늬 두 글자 문자열이다.

mod blackjack;
mod cards;
mod holdem;
mod table;
mod view;

pub use self::cards::*;
pub use self::table::*;
pub use self::view::*;

#[cfg(test)]
mod tests;

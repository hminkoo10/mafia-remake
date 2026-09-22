// casino/cards.rs — 셔플, 블랙잭 점수, 포커 족보

use crate::system_random;
use rand::RngCore;

pub const RANKS: &str = "23456789TJQKA";
pub const SUITS: &str = "shdc";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CasinoError {
    pub code: &'static str,
    pub message: String,
}

impl CasinoError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("INVALID_ACTION", message)
    }
}

impl std::fmt::Display for CasinoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CasinoError {}

/// 카드 랭크 값 (2~14, A=14).
pub fn rank_value(card: &str) -> i64 {
    card.chars()
        .next()
        .and_then(|rank| RANKS.find(rank))
        .map_or(0, |index| index as i64 + 2)
}

fn suit_of(card: &str) -> char {
    card.chars().nth(1).unwrap_or(' ')
}

/// 서버 CSPRNG로 섞은 덱 (`decks`벌). 마지막 원소가 다음에 뽑힐 카드다.
pub fn shuffled_deck(decks: usize) -> Vec<String> {
    let mut cards = Vec::with_capacity(52 * decks);
    for _ in 0..decks {
        for suit in SUITS.chars() {
            for rank in RANKS.chars() {
                cards.push(format!("{rank}{suit}"));
            }
        }
    }
    let mut rng = system_random::rng();
    for i in (1..cards.len()).rev() {
        let j = (rng.next_u64() % (i as u64 + 1)) as usize;
        cards.swap(i, j);
    }
    cards
}

pub fn draw(deck: &mut Vec<String>) -> Result<String, CasinoError> {
    deck.pop()
        .ok_or_else(|| CasinoError::new("ENGINE_STATE", "덱이 모두 소진되었습니다."))
}

/// 블랙잭 점수와 소프트 여부 (에이스를 11로 센 상태인가).
pub fn blackjack_value(cards: &[String]) -> (i64, bool) {
    let mut total = 0;
    let mut aces = 0;
    for card in cards {
        if card.starts_with('A') {
            total += 11;
            aces += 1;
        } else {
            total += rank_value(card).min(10);
        }
    }
    while total > 21 && aces > 0 {
        total -= 10;
        aces -= 1;
    }
    (total, aces > 0)
}

/// 블랙잭 카드 값 (A=11, 10·J·Q·K=10).
pub fn card_value(card: &str) -> i64 {
    if card.starts_with('A') {
        11
    } else {
        rank_value(card).min(10)
    }
}

const HAND_NAMES: [&str; 9] = [
    "하이 카드",
    "원 페어",
    "투 페어",
    "트리플",
    "스트레이트",
    "플러시",
    "풀 하우스",
    "포 카드",
    "스트레이트 플러시",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PokerRank {
    pub score: i64,
    pub name: String,
}

fn five(cards: &[&str]) -> PokerRank {
    let mut ranks = cards
        .iter()
        .map(|card| rank_value(card))
        .collect::<Vec<_>>();
    ranks.sort_unstable_by(|left, right| right.cmp(left));
    let mut groups: Vec<(i64, usize)> = Vec::new();
    for rank in &ranks {
        if let Some(group) = groups.iter_mut().find(|(value, _)| value == rank) {
            group.1 += 1;
        } else {
            groups.push((*rank, 1));
        }
    }
    groups.sort_by(|left, right| right.1.cmp(&left.1).then(right.0.cmp(&left.0)));
    let flush = cards.iter().all(|card| suit_of(card) == suit_of(cards[0]));
    let straight = if groups.len() == 5 {
        if ranks[0] - ranks[4] == 4 {
            ranks[0]
        } else if ranks == [14, 5, 4, 3, 2] {
            5
        } else {
            0
        }
    } else {
        0
    };
    let group_ranks = groups.iter().map(|(rank, _)| *rank).collect::<Vec<_>>();
    let (category, kickers): (i64, Vec<i64>) = if straight > 0 && flush {
        (8, vec![straight])
    } else if groups[0].1 == 4 {
        (7, group_ranks)
    } else if groups[0].1 == 3 && groups[1].1 == 2 {
        (6, group_ranks)
    } else if flush {
        (5, ranks.clone())
    } else if straight > 0 {
        (4, vec![straight])
    } else if groups[0].1 == 3 {
        (3, group_ranks)
    } else if groups[0].1 == 2 && groups[1].1 == 2 {
        (2, group_ranks)
    } else if groups[0].1 == 2 {
        (1, group_ranks)
    } else {
        (0, ranks.clone())
    };
    let mut score = category;
    for index in 0..5 {
        score = score * 15 + kickers.get(index).copied().unwrap_or(0);
    }
    let name = if category == 8 && straight == 14 {
        "로열 플러시".to_string()
    } else {
        HAND_NAMES[category as usize].to_string()
    };
    PokerRank { score, name }
}

/// 5~7장 중 가장 강한 5장 조합.
pub fn poker_rank(cards: &[String]) -> Result<PokerRank, CasinoError> {
    let n = cards.len();
    if !(5..=7).contains(&n) {
        return Err(CasinoError::new(
            "ENGINE_STATE",
            "포커 족보는 5~7장으로만 계산합니다.",
        ));
    }
    let refs = cards.iter().map(String::as_str).collect::<Vec<_>>();
    let mut best = PokerRank {
        score: -1,
        name: String::new(),
    };
    for a in 0..n - 4 {
        for b in a + 1..n - 3 {
            for c in b + 1..n - 2 {
                for d in c + 1..n - 1 {
                    for e in d + 1..n {
                        let rank = five(&[refs[a], refs[b], refs[c], refs[d], refs[e]]);
                        if rank.score > best.score {
                            best = rank;
                        }
                    }
                }
            }
        }
    }
    Ok(best)
}

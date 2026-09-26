// stocks/price.rs — 호가 단위, 가격제한폭, 난수 분포 (정규·t분포)

use super::model::BP;
use rand::{Rng, RngCore};

/// 한국거래소 호가 가격 단위 (2023년 개편 기준, 코스피·코스닥 공통).
pub fn tick_size(price: i64) -> i64 {
    if price < 2_000 {
        1
    } else if price < 5_000 {
        5
    } else if price < 20_000 {
        10
    } else if price < 50_000 {
        50
    } else if price < 200_000 {
        100
    } else if price < 500_000 {
        500
    } else {
        1_000
    }
}

/// 호가 단위에 맞게 내림 (최소 1원).
pub fn floor_tick(price: i64) -> i64 {
    let price = price.max(1);
    let tick = tick_size(price);
    ((price / tick) * tick).max(1)
}

/// 호가 단위에 맞게 올림.
pub fn ceil_tick(price: i64) -> i64 {
    let price = price.max(1);
    let down = floor_tick(price);
    if down == price {
        price
    } else {
        // 올린 값이 다음 구간으로 넘어가면 그 구간의 단위로 다시 맞춘다 (예: 1,999.x → 2,000).
        floor_tick(down + tick_size(down)).max(down + 1)
    }
}

/// 가장 가까운 호가로.
pub fn round_tick(price: f64) -> i64 {
    if !price.is_finite() || price < 1.0 {
        return 1;
    }
    let raw = price.min(1e13).round() as i64;
    let down = floor_tick(raw);
    let up = ceil_tick(raw);
    if raw - down <= up - raw { down } else { up }
}

/// 한 호가 위.
pub fn tick_up(price: i64) -> i64 {
    let price = floor_tick(price);
    price + tick_size(price)
}

/// 한 호가 아래 (최소 1원).
pub fn tick_down(price: i64) -> i64 {
    let price = floor_tick(price);
    if price <= 1 {
        return 1;
    }
    let below = price - 1;
    floor_tick(below)
}

/// 기준가에서 ±`bp` 가격제한 (하한은 올림, 상한은 내림).
pub fn price_limits(reference: i64, bp: i64) -> (i64, i64) {
    let reference = reference.max(1);
    let span = i128::from(reference) * i128::from(bp.clamp(0, BP * 100)) / i128::from(BP);
    let span = i64::try_from(span).unwrap_or(i64::MAX / 4);
    let lower = ceil_tick((reference - span).max(1));
    let upper = floor_tick(reference.saturating_add(span)).max(lower);
    (lower, upper)
}

/// 표준정규분포 (Box–Muller).
pub fn normal(rng: &mut dyn RngCore) -> f64 {
    let u1: f64 = rng.random::<f64>().max(f64::MIN_POSITIVE);
    let u2: f64 = rng.random::<f64>();
    (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
}

/// 자유도 4인 t분포를 분산 1로 맞춘 값 (가끔 큰 값이 나오는 두꺼운 꼬리).
pub fn fat_tail(rng: &mut dyn RngCore) -> f64 {
    let z = normal(rng);
    let chi2 = (0..4).map(|_| normal(rng).powi(2)).sum::<f64>();
    let t = z / (chi2 / 4.0).max(1e-9).sqrt();
    // t(4)의 분산은 2다.
    (t / std::f64::consts::SQRT_2).clamp(-12.0, 12.0)
}

/// 0~1 균등분포.
pub fn uniform(rng: &mut dyn RngCore) -> f64 {
    rng.random::<f64>()
}

/// 반감기 `half_life`(같은 단위)의 지수 감쇠 계수 (경과 `elapsed` 뒤 남는 비율).
pub fn decay(elapsed: f64, half_life: f64) -> f64 {
    if half_life <= 0.0 {
        return 0.0;
    }
    (-std::f64::consts::LN_2 * elapsed / half_life).exp()
}

/// 로그 값을 안전한 가격으로.
pub fn price_from_log(log_price: f64) -> f64 {
    if !log_price.is_finite() {
        return 1.0;
    }
    log_price.clamp(0.0, 30.0).exp()
}

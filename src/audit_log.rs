// audit_log.rs — 로그 채널로 보낼 운영 기록 (주식 체결·주문·회사 작업, 카지노 바이인·캐시아웃,
// 출석·구조금 등). 기록이 몰려도 Discord 요청이 많아지지 않게 모았다가 몇 초마다 종류별로 한
// 메시지(넘치면 여러 개)로 보낸다.

use poise::serenity_prelude as serenity;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 모았다가 보내는 간격.
const FLUSH_EVERY: Duration = Duration::from_secs(5);
/// 한 메시지(임베드 설명)의 글자 수 한도 (Discord는 4,096자).
const MESSAGE_LIMIT: usize = 3_800;
/// 보내지 못하고 쌓일 수 있는 줄 수 (넘치면 오래된 것부터 버린다).
const PENDING_LIMIT: usize = 2_000;

pub const STOCKS: &str = "주식 로그";
pub const CASINO: &str = "카지노 로그";
pub const COINS: &str = "코인 로그";

#[derive(Default)]
pub struct AuditLog {
    lines: Mutex<Vec<(&'static str, String)>>,
}

impl AuditLog {
    pub fn push(&self, title: &'static str, line: impl Into<String>) {
        let mut lines = self
            .lines
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        lines.push((title, line.into()));
        if lines.len() > PENDING_LIMIT {
            let excess = lines.len() - PENDING_LIMIT;
            lines.drain(..excess);
        }
    }

    pub fn take(&self) -> Vec<(&'static str, String)> {
        std::mem::take(
            &mut *self
                .lines
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }
}

/// 같은 제목끼리 들어온 순서대로 묶고, 한 메시지가 `limit`자를 넘지 않게 나눈다.
pub fn batches(lines: Vec<(&'static str, String)>, limit: usize) -> Vec<(&'static str, String)> {
    let mut groups: Vec<(&'static str, Vec<String>)> = Vec::new();
    for (title, line) in lines {
        match groups.iter_mut().find(|(existing, _)| *existing == title) {
            Some((_, group)) => group.push(line),
            None => groups.push((title, vec![line])),
        }
    }
    let mut out = Vec::new();
    for (title, group) in groups {
        let mut current = String::new();
        let mut current_chars = 0;
        for line in group {
            // 한 줄이 한도보다 길면 자른다.
            let line = if line.chars().count() > limit {
                line.chars()
                    .take(limit.saturating_sub(1))
                    .collect::<String>()
                    + "…"
            } else {
                line
            };
            let chars = line.chars().count();
            if current_chars > 0 && current_chars + 1 + chars > limit {
                out.push((title, std::mem::take(&mut current)));
                current_chars = 0;
            }
            if current_chars > 0 {
                current.push('\n');
                current_chars += 1;
            }
            current.push_str(&line);
            current_chars += chars;
        }
        if !current.is_empty() {
            out.push((title, current));
        }
    }
    out
}

/// 모인 운영 기록을 로그 채널로 보낸다 (주식 엔진의 기록도 여기서 꺼낸다). 로그 채널이 없으면 버린다.
pub async fn run_audit_relay(http: Arc<serenity::Http>, data: crate::Data) {
    let mut interval = tokio::time::interval(FLUSH_EVERY);
    loop {
        interval.tick().await;
        let mut lines = data.audit.take();
        lines.extend(
            data.stocks
                .take_logs()
                .await
                .into_iter()
                .map(|line| (STOCKS, line)),
        );
        if lines.is_empty() {
            continue;
        }
        let channel = data.config.read().await.log_channel_id;
        if channel == 0 {
            continue;
        }
        for (title, text) in batches(lines, MESSAGE_LIMIT) {
            crate::embed::send_admin_log(&http, channel, title, text).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_grouped_by_title_in_order() {
        let log = AuditLog::default();
        log.push(STOCKS, "a");
        log.push(CASINO, "b");
        log.push(STOCKS, "c");
        let out = batches(log.take(), 100);
        assert_eq!(
            out,
            vec![(STOCKS, "a\nc".to_string()), (CASINO, "b".to_string())]
        );
        assert!(log.take().is_empty(), "꺼낸 기록은 비운다");
    }

    #[test]
    fn long_batches_are_split_and_long_lines_cut() {
        let lines = (0..5)
            .map(|index| (COINS, format!("줄{index} {}", "가".repeat(10))))
            .collect::<Vec<_>>();
        let out = batches(lines, 30);
        assert!(out.len() > 1);
        assert!(out.iter().all(|(_, text)| text.chars().count() <= 30));
        let joined = out
            .iter()
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(joined.lines().count(), 5, "줄은 빠지지 않는다");

        let out = batches(vec![(COINS, "나".repeat(50))], 20);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].1.chars().count(), 20);
        assert!(out[0].1.ends_with('…'));
    }

    #[test]
    fn pending_lines_are_bounded() {
        let log = AuditLog::default();
        for index in 0..(PENDING_LIMIT + 10) {
            log.push(COINS, index.to_string());
        }
        let lines = log.take();
        assert_eq!(lines.len(), PENDING_LIMIT);
        assert_eq!(lines[0].1, "10", "오래된 것부터 버린다");
    }
}

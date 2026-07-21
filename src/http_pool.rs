// 역할: 여러 봇 토큰으로 REST 요청을 분산해 Discord 레이트리밋을 우회한다.
//
// Discord의 레이트리밋 버킷(전역 50/s 포함)은 봇 토큰 단위로 집계된다. 따라서
// 워커 토큰을 N개 추가하면 길드 관리용 REST 호출에 쓸 수 있는 예산이 (N+1)배가
// 된다. 봇 정체성과 무관하게 "길드에 들어와 권한만 있으면 아무 봇이나 수행할 수
// 있는" 쓰기 작업만 이 풀로 우회한다:
//   - 채널 권한 오버라이트 생성/삭제
//   - 채널 생성/삭제/수정(슬로우모드·토픽)
//   - 멤버 역할 부여/회수
//   - 익명 채팅 웹훅 생성
//
// 메인 봇이 "소유"해야 하는 작업은 절대 우회하지 않는다:
//   - 슬래시/컴포넌트 인터랙션 응답(인터랙션 토큰은 메인 앱에 묶임)
//   - 상태 메시지 생성/수정(메시지는 작성한 봇만 수정 가능)
//   - 웹훅 실행(전역 레이트리밋에서 이미 면제 + 웹훅별 버킷)

use poise::serenity_prelude as serenity;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

/// 워커 토큰 HTTP 클라이언트 풀. 각 `Http`는 자체 레이트리미터를 가지므로
/// 토큰마다 독립적인 전역/경로 버킷을 얻는다.
struct HttpPool {
    workers: Vec<Arc<serenity::Http>>,
    cursor: AtomicUsize,
}

static POOL: OnceLock<HttpPool> = OnceLock::new();

impl HttpPool {
    /// 라운드로빈으로 워커 하나를 고른다. 같은 채널에 대한 오버라이트 버스트도
    /// 여러 토큰의 독립 버킷으로 흩어지므로 채널당 처리량까지 늘어난다.
    fn next_worker(&self) -> &Arc<serenity::Http> {
        let index = self.cursor.fetch_add(1, Ordering::Relaxed) % self.workers.len();
        &self.workers[index]
    }
}

fn split_tokens(raw: &str) -> Vec<String> {
    raw.split([',', '\n', '\r', ' ', '\t', ';'])
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `DISCORD_WORKER_TOKENS`(구분자: 쉼표/공백/줄바꿈)에서 워커 토큰을 읽어 풀을
/// 초기화한다. 각 토큰은 `get_current_user`로 검증하고, 실패한 토큰은 버린다.
/// 유효한 토큰이 하나도 없으면 풀은 비어 있고 모든 호출은 메인 토큰으로 폴백된다.
pub async fn init_from_env() {
    let raw = std::env::var("DISCORD_WORKER_TOKENS").unwrap_or_default();
    let tokens = split_tokens(&raw);
    if tokens.is_empty() {
        return;
    }

    let mut workers = Vec::with_capacity(tokens.len());
    for token in tokens {
        let http = Arc::new(serenity::HttpBuilder::new(&token).build());
        match http.get_current_user().await {
            Ok(user) => {
                println!("HTTP 워커 토큰 확인: {} ({})", user.name, user.id.get());
                workers.push(http);
            }
            Err(error) => {
                eprintln!("DISCORD_WORKER_TOKENS 항목 무시(검증 실패): {error}");
            }
        }
    }

    if workers.is_empty() {
        eprintln!("유효한 워커 토큰이 없어 레이트리밋 분산이 비활성화됩니다.");
        return;
    }

    let count = workers.len();
    let _ = POOL.set(HttpPool {
        workers,
        cursor: AtomicUsize::new(0),
    });
    println!(
        "HTTP 워커 풀 준비 완료: 추가 토큰 {count}개 (레이트리밋 예산 약 x{})",
        count + 1
    );
}

/// 우회 가능한 길드 관리 호출을 워커 토큰으로 실행하고, 실패하면 메인 토큰으로
/// 한 번 폴백한다. 워커가 미설정(길드 미초대/권한 부족)이라도 게임은 메인 토큰만
/// 쓰던 기존 동작으로 자연스럽게 강등된다.
///
/// `op`는 최대 두 번(워커 → 메인) 호출될 수 있으므로 `Fn`이어야 하며, 넘겨받은
/// `Arc<Http>`만 사용해 요청을 보내야 한다.
pub async fn with_fallback<T, F, Fut>(
    ctx: &serenity::Context,
    op: F,
) -> Result<T, serenity::Error>
where
    F: Fn(Arc<serenity::Http>) -> Fut,
    Fut: std::future::Future<Output = Result<T, serenity::Error>>,
{
    match POOL.get() {
        Some(pool) => {
            let worker = Arc::clone(pool.next_worker());
            match op(worker).await {
                Ok(value) => Ok(value),
                Err(_) => op(Arc::clone(&ctx.http)).await,
            }
        }
        None => op(Arc::clone(&ctx.http)).await,
    }
}

#[cfg(test)]
mod tests {
    use super::split_tokens;

    #[test]
    fn splits_on_mixed_delimiters() {
        let parsed = split_tokens(" tok_a, tok_b\n tok_c;tok_d\t tok_e ");
        assert_eq!(parsed, ["tok_a", "tok_b", "tok_c", "tok_d", "tok_e"]);
    }

    #[test]
    fn empty_input_yields_no_tokens() {
        assert!(split_tokens("   \n , ; ").is_empty());
    }
}

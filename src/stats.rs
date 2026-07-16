use crate::game::{MafiaGame, PlayerAssignmentHistory};
use crate::model::{Player, Role, Winner};
use anyhow::{Context, Result};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

pub const INITIAL_RATING: i64 = 1000;
const RATING_HISTORY_LIMIT: usize = 20;
const RATING_DELTA_CAP: i64 = 80;
const ROLE_DELTA_CAP: i64 = 14;
const LOSING_RATING_GAIN_CAP: i64 = 5;
const FIRST_DEATH_LOSS_RELIEF_DIVISOR: i64 = 4;
const ROLE_BALANCE_RECENT_GAMES: usize = 20;
const ROLE_STATS_ORDER: &[Role] = &[
    Role::Mafia,
    Role::Police,
    Role::Agent,
    Role::Vigilante,
    Role::Inspector,
    Role::Doctor,
    Role::Nurse,
    Role::Gangster,
    Role::Prophet,
    Role::Psychologist,
    Role::Hypnotist,
    Role::Mercenary,
    Role::Detective,
    Role::Shaman,
    Role::Priest,
    Role::Graverobber,
    Role::Politician,
    Role::Judge,
    Role::Reporter,
    Role::Hacker,
    Role::Terrorist,
    Role::Lover,
    Role::Soldier,
    Role::Spy,
    Role::Contractor,
    Role::Thief,
    Role::Witch,
    Role::Scientist,
    Role::Madam,
    Role::Godfather,
    Role::Villain,
    Role::CultLeader,
    Role::Fanatic,
    Role::Joker,
    Role::Frog,
    Role::Citizen,
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatsFile {
    #[serde(default)]
    pub users: HashMap<String, PlayerStats>,
    #[serde(default)]
    pub role_selection_history: Vec<RoleSelectionHistoryItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleSelectionHistoryItem {
    pub started_at: String,
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerStats {
    pub name: String,
    #[serde(default)]
    pub games: i64,
    #[serde(default)]
    pub wins: i64,
    #[serde(default)]
    pub losses: i64,
    #[serde(default)]
    pub win_streak: i64,
    #[serde(default)]
    pub best_win_streak: i64,
    #[serde(default)]
    pub mafia_team_games: i64,
    #[serde(default)]
    pub play_seconds: i64,
    #[serde(default = "initial_rating")]
    pub rating: i64,
    #[serde(default)]
    pub rating_games: i64,
    #[serde(default = "initial_rating")]
    pub rating_peak: i64,
    #[serde(default)]
    pub rating_history: Vec<RatingHistoryItem>,
    #[serde(default)]
    pub roles: HashMap<String, i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatingHistoryItem {
    pub ended_at: String,
    pub before: i64,
    pub after: i64,
    pub delta: i64,
    pub team_delta: i64,
    pub role_delta: i64,
    #[serde(default)]
    pub streak_delta: i64,
    pub role: String,
    pub team: String,
    pub winner: String,
    pub players: usize,
    pub rating_reasons: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GameRatingLogItem {
    pub user_id: u64,
    pub name: String,
    pub role: String,
    pub before: i64,
    pub after: i64,
    pub delta: i64,
    pub team_delta: i64,
    pub role_delta: i64,
    pub streak_delta: i64,
    pub win_streak: i64,
    pub best_win_streak: i64,
    pub reasons: Vec<String>,
}

impl Default for PlayerStats {
    fn default() -> Self {
        Self {
            name: String::new(),
            games: 0,
            wins: 0,
            losses: 0,
            win_streak: 0,
            best_win_streak: 0,
            mafia_team_games: 0,
            play_seconds: 0,
            rating: INITIAL_RATING,
            rating_games: 0,
            rating_peak: INITIAL_RATING,
            rating_history: Vec::new(),
            roles: HashMap::new(),
        }
    }
}

pub fn load_stats(path: impl AsRef<Path>) -> Result<StatsFile> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(StatsFile::default());
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("stats 파일을 읽지 못했습니다: {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("stats JSON을 파싱하지 못했습니다: {}", path.display()))
}

pub fn save_stats(path: impl AsRef<Path>, stats: &StatsFile) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("stats 디렉터리를 만들지 못했습니다: {}", parent.display()))?;
    }
    let temp_path = path.with_file_name(format!(
        "{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("stats.json")
    ));
    let mut text = serde_json::to_string_pretty(stats)
        .with_context(|| format!("stats JSON을 만들지 못했습니다: {}", path.display()))?;
    text.push('\n');
    fs::write(&temp_path, text)
        .with_context(|| format!("stats 임시 파일을 쓰지 못했습니다: {}", temp_path.display()))?;
    if path.exists() {
        fs::remove_file(path).with_context(|| {
            format!("기존 stats 파일을 교체하지 못했습니다: {}", path.display())
        })?;
    }
    fs::rename(&temp_path, path)
        .with_context(|| format!("stats 파일을 저장하지 못했습니다: {}", path.display()))?;
    Ok(())
}

pub fn record_game_stats(
    stats: &mut StatsFile,
    game: &MafiaGame,
    initial_roles: &HashMap<u64, Role>,
    elapsed_seconds: i64,
    winner: Winner,
) -> Vec<GameRatingLogItem> {
    let mut ratings = HashMap::new();
    for player in &game.players {
        let entry = ensure_player_stats(stats, player.user_id, &player.name);
        ratings.insert(player.user_id, entry.rating);
    }

    let team_by_user_id = game
        .players
        .iter()
        .map(|player| (player.user_id, rating_team_key(game, player).to_string()))
        .collect::<HashMap<_, _>>();
    let rating_changes = game
        .players
        .iter()
        .map(|player| {
            let role = initial_roles
                .get(&player.user_id)
                .copied()
                .unwrap_or(player.role);
            (
                player.user_id,
                rating_change_for_player(
                    game,
                    player,
                    role,
                    stats,
                    &ratings,
                    &team_by_user_id,
                    winner,
                ),
            )
        })
        .collect::<HashMap<_, _>>();
    let ended_at = Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
    let mut rating_log = Vec::new();

    for player in &game.players {
        let role = initial_roles
            .get(&player.user_id)
            .copied()
            .unwrap_or(player.role);
        let won = player_won_game(game, player, winner);
        let team = rating_team_key(game, player).to_string();
        let rating_change = rating_changes
            .get(&player.user_id)
            .cloned()
            .unwrap_or_else(|| {
                RatingChange::unchanged(
                    ratings
                        .get(&player.user_id)
                        .copied()
                        .unwrap_or(INITIAL_RATING),
                    won,
                )
            });
        let entry = ensure_player_stats(stats, player.user_id, &player.name);
        entry.games += 1;
        entry.play_seconds += elapsed_seconds.max(0);
        *entry.roles.entry(role.value().to_string()).or_default() += 1;
        if role.is_mafia_team() && (role != Role::Scientist || game.is_mafia_team(player)) {
            entry.mafia_team_games += 1;
        }
        if won {
            entry.wins += 1;
            entry.win_streak += 1;
            entry.best_win_streak = entry.best_win_streak.max(entry.win_streak);
        } else {
            entry.losses += 1;
            entry.win_streak = 0;
        }
        entry.rating = rating_change.after;
        entry.rating_games += 1;
        entry.rating_peak = entry.rating_peak.max(entry.rating);
        rating_log.push(GameRatingLogItem {
            user_id: player.user_id,
            name: player.name.clone(),
            role: role.value().to_string(),
            before: rating_change.before,
            after: rating_change.after,
            delta: rating_change.delta,
            team_delta: rating_change.team_delta,
            role_delta: rating_change.role_delta,
            streak_delta: rating_change.streak_delta,
            win_streak: entry.win_streak,
            best_win_streak: entry.best_win_streak,
            reasons: rating_change.reasons.clone(),
        });
        entry.rating_history.push(RatingHistoryItem {
            ended_at: ended_at.clone(),
            before: rating_change.before,
            after: rating_change.after,
            delta: rating_change.delta,
            team_delta: rating_change.team_delta,
            role_delta: rating_change.role_delta,
            streak_delta: rating_change.streak_delta,
            role: role.value().to_string(),
            team,
            winner: winner.value().to_string(),
            players: game.players.len(),
            rating_reasons: rating_change.reasons,
        });
        let overflow = entry
            .rating_history
            .len()
            .saturating_sub(RATING_HISTORY_LIMIT);
        if overflow > 0 {
            entry.rating_history.drain(..overflow);
        }
    }
    rating_log.sort_by_key(|item| item.name.to_lowercase());
    rating_log
}

pub fn game_rating_log_chunks(logs: &[GameRatingLogItem], max_chars: usize) -> Vec<String> {
    if logs.is_empty() {
        return vec!["레이팅 변동 기록이 없습니다.".to_string()];
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    for item in logs {
        let reasons = if item.reasons.is_empty() {
            "사유 없음".to_string()
        } else {
            item.reasons.join(", ")
        };
        let streak_text = if item.streak_delta == 0 {
            String::new()
        } else {
            format!(" / 연승 {:+}", item.streak_delta)
        };
        let line = format!(
            "- {} ({}) {} -> {} ({:+}) [팀 {:+} / 직업 {:+}{}]\n  사유: {}\n",
            item.name,
            item.role,
            item.before,
            item.after,
            item.delta,
            item.team_delta,
            item.role_delta,
            streak_text,
            reasons
        );
        if !current.is_empty() && current.len() + line.len() > max_chars {
            chunks.push(current.trim_end().to_string());
            current.clear();
        }
        current.push_str(&line);
    }
    if !current.is_empty() {
        chunks.push(current.trim_end().to_string());
    }
    chunks
}

pub fn game_rank_change_chunks(logs: &[GameRatingLogItem], max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for item in logs {
        let before_rank = rating_rank(item.before);
        let after_rank = rating_rank(item.after);
        if before_rank == after_rank {
            continue;
        }
        let direction = if item.after > item.before {
            "승급"
        } else {
            "강등"
        };
        let line = format!(
            "- {} ({}) {}: {} -> {} / {} -> {} ({:+})\n",
            item.name,
            item.role,
            direction,
            before_rank,
            after_rank,
            item.before,
            item.after,
            item.delta
        );
        if !current.is_empty() && current.len() + line.len() > max_chars {
            chunks.push(current.trim_end().to_string());
            current.clear();
        }
        current.push_str(&line);
    }
    if !current.is_empty() {
        chunks.push(current.trim_end().to_string());
    }
    chunks
}

pub fn record_role_selection(stats: &mut StatsFile, roles: impl IntoIterator<Item = Role>) {
    let mut roles = roles
        .into_iter()
        .map(|role| role.value().to_string())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    roles.sort_by_key(|role| role_order_index(role));
    stats.role_selection_history.push(RoleSelectionHistoryItem {
        started_at: Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        roles,
    });
    let overflow = stats
        .role_selection_history
        .len()
        .saturating_sub(ROLE_BALANCE_RECENT_GAMES);
    if overflow > 0 {
        stats.role_selection_history.drain(..overflow);
    }
}

pub fn role_appearance_counts(stats: &StatsFile) -> HashMap<Role, i64> {
    if !stats.role_selection_history.is_empty() {
        let mut counts = HashMap::new();
        for (index, history) in stats
            .role_selection_history
            .iter()
            .rev()
            .take(ROLE_BALANCE_RECENT_GAMES)
            .enumerate()
        {
            let appeared_roles = history
                .roles
                .iter()
                .map(String::as_str)
                .collect::<HashSet<_>>();
            add_recent_role_scores(&mut counts, &appeared_roles, index);
        }
        return counts;
    }

    let mut recent_games = HashMap::<&str, HashSet<&str>>::new();
    for entry in stats.users.values() {
        for history in &entry.rating_history {
            recent_games
                .entry(history.ended_at.as_str())
                .or_default()
                .insert(history.role.as_str());
        }
    }
    if recent_games.is_empty() {
        return lifetime_role_appearance_counts(stats);
    }

    // Lifetime totals can starve older roles after a new role is introduced.
    let mut recent_games = recent_games.into_iter().collect::<Vec<_>>();
    recent_games.sort_unstable_by(|(left, _), (right, _)| right.cmp(left));
    let mut counts = HashMap::new();
    for (index, (_, appeared_roles)) in recent_games
        .into_iter()
        .take(ROLE_BALANCE_RECENT_GAMES)
        .enumerate()
    {
        add_recent_role_scores(&mut counts, &appeared_roles, index);
    }
    counts
}

fn add_recent_role_scores(
    counts: &mut HashMap<Role, i64>,
    appeared_roles: &HashSet<&str>,
    index: usize,
) {
    let recency_weight = match index {
        0 => 64,
        1 => 32,
        2 => 16,
        3 => 8,
        4 => 4,
        5 => 2,
        _ => 1,
    };
    for role in ROLE_STATS_ORDER {
        if appeared_roles.contains(role.value()) {
            *counts.entry(*role).or_default() += recency_weight;
        }
    }
}

pub fn player_assignment_histories(
    stats: &StatsFile,
    user_ids: &[u64],
) -> HashMap<u64, PlayerAssignmentHistory> {
    let mut histories = HashMap::new();
    for user_id in user_ids {
        let Some(entry) = stats.users.get(&user_id.to_string()) else {
            continue;
        };
        let role_counts = ROLE_STATS_ORDER
            .iter()
            .filter_map(|role| {
                let count = entry.roles.get(role.value()).copied().unwrap_or(0);
                (count > 0).then_some((*role, count))
            })
            .collect::<HashMap<_, _>>();
        let recorded_games = role_counts.values().copied().sum::<i64>();
        let mafia_role_games = role_counts
            .iter()
            .filter(|(role, _)| role.is_mafia_team())
            .map(|(_, count)| *count)
            .sum();
        let mut recent_history = entry.rating_history.iter().collect::<Vec<_>>();
        recent_history.sort_unstable_by(|left, right| right.ended_at.cmp(&left.ended_at));
        let recent_roles = recent_history
            .into_iter()
            .filter_map(|history| role_from_value(&history.role))
            .take(3)
            .collect();
        histories.insert(
            *user_id,
            PlayerAssignmentHistory {
                games: entry.games.max(recorded_games),
                mafia_role_games,
                role_counts,
                recent_roles,
            },
        );
    }
    histories
}

fn role_from_value(value: &str) -> Option<Role> {
    ROLE_STATS_ORDER
        .iter()
        .copied()
        .find(|role| role.value() == value)
}

fn lifetime_role_appearance_counts(stats: &StatsFile) -> HashMap<Role, i64> {
    let mut counts = HashMap::new();
    for entry in stats.users.values() {
        for role in ROLE_STATS_ORDER {
            let count = entry.roles.get(role.value()).copied().unwrap_or(0);
            if count > 0 {
                *counts.entry(*role).or_default() += count;
            }
        }
    }
    counts
}

fn ensure_player_stats<'a>(
    stats: &'a mut StatsFile,
    user_id: u64,
    name: &str,
) -> &'a mut PlayerStats {
    let entry = stats.users.entry(user_id.to_string()).or_default();
    entry.name = name.to_string();
    entry
}

#[derive(Debug, Clone)]
struct RatingChange {
    before: i64,
    after: i64,
    delta: i64,
    team_delta: i64,
    role_delta: i64,
    streak_delta: i64,
    reasons: Vec<String>,
}

impl RatingChange {
    fn unchanged(before: i64, won: bool) -> Self {
        Self {
            before,
            after: before,
            delta: 0,
            team_delta: 0,
            role_delta: 0,
            streak_delta: 0,
            reasons: vec![if won {
                "소속 진영 승리".to_string()
            } else {
                "소속 진영 패배".to_string()
            }],
        }
    }
}

fn rating_change_for_player(
    game: &MafiaGame,
    player: &Player,
    initial_role: Role,
    stats: &StatsFile,
    ratings: &HashMap<u64, i64>,
    team_by_user_id: &HashMap<u64, String>,
    winner: Winner,
) -> RatingChange {
    let old_rating = ratings
        .get(&player.user_id)
        .copied()
        .unwrap_or(INITIAL_RATING);
    let won = player_won_game(game, player, winner);
    let score = if won { 1.0 } else { 0.0 };
    let opponent_average = opponent_average_rating(game, player, ratings, team_by_user_id);
    let entry = stats.users.get(&player.user_id.to_string());
    let rating_multiplier = rating_progression_multiplier(old_rating, won);
    let base_delta =
        rating_k(entry) as f64 * (score - expected_score(old_rating, opponent_average));
    let team_delta = clamp(
        (base_delta * rating_multiplier * player_count_multiplier(game.players.len())).round()
            as i64,
        -RATING_DELTA_CAP,
        RATING_DELTA_CAP,
    );
    let (role_delta, mut role_reasons) = role_rating_adjustment(game, player, initial_role, won);
    let streak_delta = win_streak_rating_bonus(entry, won);
    let combined_delta = clamp(
        team_delta + role_delta + streak_delta,
        -RATING_DELTA_CAP,
        RATING_DELTA_CAP,
    );
    let raw_final_delta = final_rating_delta(team_delta, role_delta + streak_delta, won);
    let first_death_relief = first_death_loss_relief(game, player, raw_final_delta, won);
    let final_delta = raw_final_delta + first_death_relief;
    let after = (old_rating + final_delta).max(0);
    let mut reasons = vec![if won {
        "소속 진영 승리".to_string()
    } else {
        "소속 진영 패배".to_string()
    }];
    reasons.append(&mut role_reasons);
    if streak_delta > 0 {
        let next_streak = entry.map_or(1, |entry| entry.win_streak.saturating_add(1));
        reasons.push(format!("{next_streak}연승 보너스 +{streak_delta}"));
    }
    if (rating_multiplier - 1.0).abs() > f64::EPSILON {
        reasons.push(format!("레이팅 구간 보정 x{rating_multiplier:.2}"));
    }
    if combined_delta != team_delta + role_delta + streak_delta {
        reasons.push("전체 레이팅 변동 상한 적용".to_string());
    }
    if !won && raw_final_delta != combined_delta {
        reasons.push("패배팀 상승 제한 적용".to_string());
    }
    if first_death_relief > 0 {
        reasons.push(format!("첫 사망 패배 완화 +{first_death_relief}"));
    }
    RatingChange {
        before: old_rating,
        after,
        delta: after - old_rating,
        team_delta,
        role_delta,
        streak_delta,
        reasons,
    }
}

fn win_streak_rating_bonus(entry: Option<&PlayerStats>, won: bool) -> i64 {
    if !won {
        return 0;
    }
    let next_streak = entry.map_or(1, |entry| entry.win_streak.saturating_add(1));
    if next_streak <= 1 {
        0
    } else {
        ((next_streak - 1) * 2).min(16)
    }
}

fn role_rating_adjustment(
    game: &MafiaGame,
    player: &Player,
    role: Role,
    won: bool,
) -> (i64, Vec<String>) {
    let mut points = 0;
    let mut reasons = Vec::new();
    let (role_points, role_reason) = role_specific_rating_adjustment(player, role, won);
    if role_points != 0 {
        points += role_points;
        reasons.push(format!("{} {:+}", role_reason, role_points));
    }
    for event in game
        .rating_events
        .get(&player.user_id)
        .into_iter()
        .flatten()
    {
        points += event.points;
        reasons.push(format!("{} {:+}", event.reason, event.points));
    }
    let action_count = game
        .rating_action_counts
        .get(&player.user_id)
        .copied()
        .unwrap_or(0);
    if action_count == 0 && player.alive && game.day_number >= 2 && role_has_core_action(role) {
        points -= 2;
        reasons.push("핵심 능력 미사용 -2".to_string());
    }
    let role_delta = clamp(points, -ROLE_DELTA_CAP, ROLE_DELTA_CAP);
    if role_delta != points {
        reasons.push("직업 보정 상한 적용".to_string());
    }
    (role_delta, reasons)
}

fn role_specific_rating_adjustment(player: &Player, role: Role, won: bool) -> (i64, &'static str) {
    let alive_win_points = if player.alive { 2 } else { 1 };
    match role {
        Role::Citizen => (
            if won { alive_win_points } else { 0 },
            "시민 생존/추론 기여",
        ),
        Role::Police => (
            if won { alive_win_points } else { 0 },
            "경찰 조사 유지 기여",
        ),
        Role::Agent => (
            if won { alive_win_points } else { 0 },
            "요원 정보 압박 기여",
        ),
        Role::Vigilante => (
            if won { alive_win_points } else { 0 },
            "자경단원 처형 압박 기여",
        ),
        Role::Inspector => (
            if won { alive_win_points } else { 0 },
            "형사 수사 정보 공유 기여",
        ),
        Role::Doctor => (if won { 1 } else { 0 }, "의사 보호 운영 기여"),
        Role::Nurse => (if won { 1 } else { 0 }, "간호사 보조 보호 기여"),
        Role::Gangster => (if won { 1 } else { 0 }, "건달 투표 제어 기여"),
        Role::Prophet => (if won { 1 } else { 0 }, "예언자 장기 생존 기여"),
        Role::Psychologist => (
            if won { alive_win_points } else { 0 },
            "심리학자 관계 분석 기여",
        ),
        Role::Hypnotist => (
            if won { alive_win_points } else { 0 },
            "최면술사 누적 판별 기여",
        ),
        Role::Mercenary => (
            if won { alive_win_points } else { 0 },
            "용병 의뢰 수행 기여",
        ),
        Role::Detective => (
            if won { alive_win_points } else { 0 },
            "탐정 행동 추적 기여",
        ),
        Role::Shaman => (if won { 1 } else { 0 }, "영매 사망 정보 연결 기여"),
        Role::Priest => (if won { 1 } else { 0 }, "성직자 정화 판단 기여"),
        Role::Graverobber => (if won { 1 } else { 0 }, "도굴꾼 역할 전환 기여"),
        Role::Politician => (if won { 1 } else { 0 }, "정치인 찬반 운영 기여"),
        Role::Judge => (if won { 1 } else { 0 }, "판사 처형 판정 기여"),
        Role::Reporter => (if won { 1 } else { 0 }, "기자 공개 정보 기여"),
        Role::Hacker => (if won { 1 } else { 0 }, "해커 행동 정보 기여"),
        Role::Terrorist => (
            if won { alive_win_points } else { 0 },
            "테러리스트 교환 압박 기여",
        ),
        Role::Lover => (if won { 1 } else { 0 }, "연인 생존 연계 기여"),
        Role::Soldier => (if won { 1 } else { 0 }, "군인 방탄 생존 기여"),
        Role::Mafia => (
            if won { alive_win_points } else { 0 },
            "마피아 처형 운영 기여",
        ),
        Role::Spy => (if won { 1 } else { 0 }, "스파이 접선/교란 기여"),
        Role::Contractor => (if won { 1 } else { 0 }, "청부업자 표적 압박 기여"),
        Role::Thief => (if won { 1 } else { 0 }, "도둑 능력 탈취 기여"),
        Role::Witch => (if won { 1 } else { 0 }, "마녀 저주 교란 기여"),
        Role::Scientist => (if won { 1 } else { 0 }, "과학자 부활 변수 기여"),
        Role::Madam => (if won { 1 } else { 0 }, "마담 접대/접선 기여"),
        Role::Godfather => (
            if won { alive_win_points } else { 0 },
            "대부 지휘/은폐 기여",
        ),
        Role::Villain => (if won { 1 } else { 0 }, "악인 마피아팀 보조 기여"),
        Role::CultLeader => (
            if won { alive_win_points } else { 0 },
            "교주 포교 운영 기여",
        ),
        Role::Fanatic => (if won { 1 } else { 0 }, "광신도 교주팀 보조 기여"),
        Role::Joker => (if won { 6 } else { 0 }, "조커 단독 승리 달성"),
        Role::Frog => (if won { 1 } else { 0 }, "개구리 상태 생존 기여"),
    }
}

fn role_has_core_action(role: Role) -> bool {
    matches!(
        role,
        Role::Mafia
            | Role::Doctor
            | Role::Nurse
            | Role::Gangster
            | Role::Police
            | Role::Vigilante
            | Role::Inspector
            | Role::Reporter
            | Role::Hacker
            | Role::Psychologist
            | Role::Hypnotist
            | Role::Mercenary
            | Role::Detective
            | Role::Shaman
            | Role::Priest
            | Role::Spy
            | Role::Contractor
            | Role::Thief
            | Role::Witch
            | Role::Godfather
            | Role::Terrorist
            | Role::CultLeader
            | Role::Fanatic
    )
}

fn final_rating_delta(team_delta: i64, role_delta: i64, won: bool) -> i64 {
    let combined_delta = clamp(team_delta + role_delta, -RATING_DELTA_CAP, RATING_DELTA_CAP);
    if won {
        combined_delta
    } else {
        combined_delta.min(LOSING_RATING_GAIN_CAP)
    }
}

fn first_death_loss_relief(game: &MafiaGame, player: &Player, final_delta: i64, won: bool) -> i64 {
    if won || final_delta >= 0 || game.death_order.first() != Some(&player.user_id) {
        return 0;
    }
    ((-final_delta + FIRST_DEATH_LOSS_RELIEF_DIVISOR - 1) / FIRST_DEATH_LOSS_RELIEF_DIVISOR).max(1)
}

fn player_won_game(game: &MafiaGame, player: &Player, winner: Winner) -> bool {
    match winner {
        Winner::Mafia => game.is_mafia_team(player),
        Winner::Cult => game.is_cult_team(player),
        Winner::Joker => game
            .joker_winner_id
            .map_or(player.role == Role::Joker, |winner_id| {
                player.user_id == winner_id
            }),
        Winner::Citizen => game.is_citizen_team(player),
    }
}

fn rating_team_key(game: &MafiaGame, player: &Player) -> &'static str {
    if player.role == Role::Joker {
        "joker"
    } else if game.is_cult_team(player) {
        "cult"
    } else if game.is_mafia_team(player) {
        "mafia"
    } else {
        "citizen"
    }
}

fn opponent_average_rating(
    game: &MafiaGame,
    player: &Player,
    ratings: &HashMap<u64, i64>,
    team_by_user_id: &HashMap<u64, String>,
) -> f64 {
    let player_team = rating_team_key(game, player);
    let mut candidates = game
        .players
        .iter()
        .filter(|candidate| {
            let team = team_by_user_id
                .get(&candidate.user_id)
                .map(String::as_str)
                .unwrap_or("citizen");
            match player_team {
                "citizen" => team == "mafia",
                "mafia" => team == "citizen",
                _ => team != player_team,
            }
        })
        .map(|candidate| candidate.user_id)
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        candidates = game
            .players
            .iter()
            .filter(|candidate| candidate.user_id != player.user_id)
            .map(|candidate| candidate.user_id)
            .collect();
    }
    if candidates.is_empty() {
        return ratings
            .get(&player.user_id)
            .copied()
            .unwrap_or(INITIAL_RATING) as f64;
    }
    candidates
        .iter()
        .map(|user_id| ratings.get(user_id).copied().unwrap_or(INITIAL_RATING))
        .sum::<i64>() as f64
        / candidates.len() as f64
}

fn rating_k(entry: Option<&PlayerStats>) -> i64 {
    let rating_games = entry.map_or(0, |entry| entry.rating_games);
    if rating_games < 10 {
        56
    } else if rating_games < 30 {
        48
    } else if rating_games < 70 {
        40
    } else {
        34
    }
}

fn rating_progression_multiplier(rating: i64, won: bool) -> f64 {
    if won {
        if rating < 950 {
            1.45
        } else if rating < 1125 {
            1.15
        } else if rating < 1325 {
            0.95
        } else if rating < 1575 {
            0.78
        } else if rating < 1900 {
            0.62
        } else {
            0.48
        }
    } else if rating < 950 {
        0.60
    } else if rating < 1125 {
        0.78
    } else if rating < 1325 {
        0.95
    } else if rating < 1575 {
        1.12
    } else if rating < 1900 {
        1.32
    } else {
        1.55
    }
}

fn player_count_multiplier(player_count: usize) -> f64 {
    if player_count <= 3 {
        0.6
    } else if player_count <= 6 {
        0.85
    } else if player_count <= 10 {
        1.0
    } else {
        1.1
    }
}

fn expected_score(player_rating: i64, opponent_average: f64) -> f64 {
    1.0 / (1.0 + 10_f64.powf((opponent_average - player_rating as f64) / 400.0))
}

fn clamp(value: i64, low: i64, high: i64) -> i64 {
    value.max(low).min(high)
}

pub fn win_rate_text(wins: i64, games: i64) -> String {
    if games <= 0 {
        return "0.0%".to_string();
    }
    format!("{:.1}%", wins as f64 / games as f64 * 100.0)
}

pub fn rating_rank(rating: i64) -> &'static str {
    if rating < 950 {
        "C"
    } else if rating < 1100 {
        "B"
    } else if rating < 1300 {
        "A"
    } else if rating < 1550 {
        "S"
    } else if rating < 1850 {
        "SS"
    } else {
        "X"
    }
}

pub fn role_stats_text(entry: &PlayerStats) -> String {
    if entry.roles.is_empty() {
        return "없음".to_string();
    }
    let mut items = entry.roles.iter().collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .1
            .cmp(left.1)
            .then_with(|| role_order_index(left.0).cmp(&role_order_index(right.0)))
            .then_with(|| left.0.cmp(right.0))
    });
    items
        .into_iter()
        .map(|(role, count)| format!("{role} {count}회"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn leaderboard_text(stats: &StatsFile, metric: &str) -> String {
    let entries = leaderboard_entries(stats, metric, 10);

    if entries.is_empty() {
        return "아직 기록된 게임 전적이 없습니다.".to_string();
    }

    let mut lines = vec![format!("기준: **{}**", leaderboard_metric_name(metric))];
    for (index, (_user_id, entry)) in entries.into_iter().enumerate() {
        lines.push(format!(
            "{}. **{}** - {}승 {}패 / {}판 / 승률 {} / 현재 {}연승 / 최고 {}연승 / 마피아팀 {}회 / 게임시간 {} / 레이팅 {}점 ({})",
            index + 1,
            if entry.name.is_empty() {
                "알 수 없음"
            } else {
                &entry.name
            },
            entry.wins,
            entry.losses,
            entry.games,
            win_rate_text(entry.wins, entry.games),
            entry.win_streak,
            entry.best_win_streak,
            entry.mafia_team_games,
            play_duration_text(entry.play_seconds),
            entry.rating,
            rating_rank(entry.rating)
        ));
    }
    lines.join("\n")
}

pub fn leaderboard_entries(
    stats: &StatsFile,
    metric: &str,
    limit: usize,
) -> Vec<(String, PlayerStats)> {
    let mut entries = stats
        .users
        .iter()
        .filter(|(_user_id, entry)| entry.games > 0)
        .map(|(user_id, entry)| (user_id.clone(), entry.clone()))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        let left_value = leaderboard_value(&left.1, metric);
        let right_value = leaderboard_value(&right.1, metric);
        right_value
            .total_cmp(&left_value)
            .then_with(|| right.1.wins.cmp(&left.1.wins))
            .then_with(|| right.1.games.cmp(&left.1.games))
            .then_with(|| left.1.name.cmp(&right.1.name))
    });
    entries.truncate(limit);
    entries
}

pub fn rating_log_text(
    stats: &StatsFile,
    user_id: u64,
    fallback_name: &str,
    limit: usize,
) -> String {
    let Some(entry) = stats.users.get(&user_id.to_string()) else {
        return "아직 기록된 레이팅 로그가 없습니다.".to_string();
    };
    if entry.rating_history.is_empty() {
        return "아직 기록된 레이팅 로그가 없습니다.".to_string();
    }
    let name = if entry.name.is_empty() {
        fallback_name
    } else {
        &entry.name
    };
    let mut lines = vec![format!("{name} 님의 최근 레이팅 로그")];
    for item in entry.rating_history.iter().rev().take(limit) {
        let sign = if item.delta >= 0 { "+" } else { "" };
        let detail = item
            .rating_reasons
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        let streak_text = if item.streak_delta == 0 {
            String::new()
        } else {
            format!(", 연승 {:+}", item.streak_delta)
        };
        let detail = if detail.is_empty() {
            format!(
                "팀 {:+}, 직업 {:+}{}",
                item.team_delta, item.role_delta, streak_text
            )
        } else {
            format!(
                "팀 {:+}, 직업 {:+}{} / {detail}",
                item.team_delta, item.role_delta, streak_text
            )
        };
        lines.push(format!(
            "- {}: {} -> {} ({}{}) / {} / 승자 {} / {}",
            short_time_text(&item.ended_at),
            item.before,
            item.after,
            sign,
            item.delta,
            item.role,
            item.winner,
            detail
        ));
    }
    lines.join("\n")
}

pub fn leaderboard_value(entry: &PlayerStats, metric: &str) -> f64 {
    match metric {
        "winrate" => {
            if entry.games > 0 {
                entry.wins as f64 / entry.games as f64
            } else {
                0.0
            }
        }
        "games" => entry.games as f64,
        "streak" => entry.win_streak as f64,
        "mafia" => entry.mafia_team_games as f64,
        "playtime" => entry.play_seconds as f64,
        "rating" => entry.rating as f64,
        _ => entry.wins as f64,
    }
}

pub fn leaderboard_metric_name(metric: &str) -> &'static str {
    match metric {
        "winrate" => "승률",
        "games" => "판수",
        "streak" => "연승",
        "mafia" => "마피아팀 플레이",
        "playtime" => "게임시간",
        "rating" => "레이팅",
        _ => "승리수",
    }
}

pub fn play_duration_text(seconds: i64) -> String {
    let minutes = seconds.max(0) / 60;
    if minutes <= 0 {
        "1분 미만".to_string()
    } else {
        format!("{minutes}분")
    }
}

fn role_order_index(role_name: &str) -> usize {
    ROLE_STATS_ORDER
        .iter()
        .position(|role| role.value() == role_name)
        .unwrap_or(999)
}

fn short_time_text(value: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(value).map_or_else(
        |_| {
            if value.is_empty() {
                "날짜 없음".to_string()
            } else {
                value.to_string()
            }
        },
        |time| time.format("%m/%d %H:%M").to_string(),
    )
}

const fn initial_rating() -> i64 {
    INITIAL_RATING
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rating_test_game() -> MafiaGame {
        MafiaGame::new(
            vec![
                (1, "Alpha".to_string()),
                (2, "Beta".to_string()),
                (3, "Gamma".to_string()),
                (4, "Delta".to_string()),
            ],
            1,
            1,
            1,
            Vec::new(),
        )
        .unwrap()
    }

    fn initial_roles(game: &MafiaGame) -> HashMap<u64, Role> {
        game.players
            .iter()
            .map(|player| (player.user_id, player.role))
            .collect()
    }

    #[test]
    fn win_rate_handles_zero_games() {
        assert_eq!(win_rate_text(0, 0), "0.0%");
        assert_eq!(win_rate_text(3, 4), "75.0%");
    }

    #[test]
    fn old_stats_without_role_selection_history_still_loads() {
        let stats: StatsFile = serde_json::from_str(r#"{"users":{}}"#).unwrap();

        assert!(stats.role_selection_history.is_empty());
    }

    #[test]
    fn role_balance_falls_back_to_lifetime_counts_without_history() {
        let mut stats = StatsFile::default();
        stats.users.insert(
            "1".to_string(),
            PlayerStats {
                roles: HashMap::from([(Role::Shaman.value().to_string(), 3)]),
                ..Default::default()
            },
        );

        let counts = role_appearance_counts(&stats);

        assert_eq!(counts.get(&Role::Shaman).copied(), Some(3));
    }

    #[test]
    fn started_role_history_overrides_old_lifetime_counts() {
        let mut stats = StatsFile::default();
        stats.users.insert(
            "1".to_string(),
            PlayerStats {
                roles: HashMap::from([(Role::Shaman.value().to_string(), 100)]),
                ..Default::default()
            },
        );

        record_role_selection(&mut stats, [Role::Mafia, Role::Detective, Role::Detective]);
        let counts = role_appearance_counts(&stats);

        assert_eq!(
            stats.role_selection_history[0].roles,
            vec![
                Role::Mafia.value().to_string(),
                Role::Detective.value().to_string()
            ]
        );
        assert_eq!(counts.get(&Role::Detective).copied(), Some(64));
        assert!(!counts.contains_key(&Role::Shaman));
    }

    #[test]
    fn role_selection_history_is_bounded() {
        let mut stats = StatsFile::default();
        for _ in 0..(ROLE_BALANCE_RECENT_GAMES + 5) {
            record_role_selection(&mut stats, [Role::Detective]);
        }

        assert_eq!(
            stats.role_selection_history.len(),
            ROLE_BALANCE_RECENT_GAMES
        );
    }

    #[test]
    fn role_balance_penalizes_recent_appearances_more() {
        let history = |role: Role, ended_at: &str| RatingHistoryItem {
            ended_at: ended_at.to_string(),
            before: 1000,
            after: 1000,
            delta: 0,
            team_delta: 0,
            role_delta: 0,
            streak_delta: 0,
            role: role.value().to_string(),
            team: "citizen".to_string(),
            winner: Winner::Citizen.value().to_string(),
            players: 8,
            rating_reasons: Vec::new(),
        };
        let mut stats = StatsFile::default();
        stats.users.insert(
            "1".to_string(),
            PlayerStats {
                rating_history: vec![history(Role::Detective, "2026-01-01T00:00:00+09:00")],
                ..Default::default()
            },
        );
        stats.users.insert(
            "2".to_string(),
            PlayerStats {
                rating_history: vec![history(Role::Shaman, "2026-01-02T00:00:00+09:00")],
                ..Default::default()
            },
        );

        let scores = role_appearance_counts(&stats);

        assert!(scores[&Role::Shaman] > scores[&Role::Detective]);
    }

    #[test]
    fn assignment_history_counts_special_mafia_roles() {
        let mut stats = StatsFile::default();
        stats.users.insert(
            "7".to_string(),
            PlayerStats {
                games: 6,
                roles: HashMap::from([
                    (Role::Mafia.value().to_string(), 1),
                    (Role::Spy.value().to_string(), 2),
                    (Role::Citizen.value().to_string(), 3),
                ]),
                rating_history: vec![RatingHistoryItem {
                    ended_at: "2026-01-02T00:00:00+09:00".to_string(),
                    before: 1000,
                    after: 1000,
                    delta: 0,
                    team_delta: 0,
                    role_delta: 0,
                    streak_delta: 0,
                    role: Role::Spy.value().to_string(),
                    team: "citizen".to_string(),
                    winner: Winner::Citizen.value().to_string(),
                    players: 8,
                    rating_reasons: Vec::new(),
                }],
                ..Default::default()
            },
        );

        let histories = player_assignment_histories(&stats, &[7]);
        let history = &histories[&7];

        assert_eq!(history.games, 6);
        assert_eq!(history.mafia_role_games, 3);
        assert_eq!(history.recent_roles, vec![Role::Spy]);
    }

    #[test]
    fn rating_rank_maps_rating_bands() {
        assert_eq!(rating_rank(949), "C");
        assert_eq!(rating_rank(950), "B");
        assert_eq!(rating_rank(1100), "A");
        assert_eq!(rating_rank(1300), "S");
        assert_eq!(rating_rank(1550), "SS");
        assert_eq!(rating_rank(1850), "X");
    }

    #[test]
    fn rank_change_log_only_lists_rank_crossings() {
        let logs = vec![
            GameRatingLogItem {
                user_id: 1,
                name: "Alpha".to_string(),
                role: Role::Doctor.value().to_string(),
                before: 1090,
                after: 1110,
                delta: 20,
                team_delta: 15,
                role_delta: 5,
                streak_delta: 0,
                win_streak: 1,
                best_win_streak: 3,
                reasons: vec![],
            },
            GameRatingLogItem {
                user_id: 2,
                name: "Beta".to_string(),
                role: Role::Mafia.value().to_string(),
                before: 1000,
                after: 1030,
                delta: 30,
                team_delta: 29,
                role_delta: 1,
                streak_delta: 0,
                win_streak: 2,
                best_win_streak: 2,
                reasons: vec![],
            },
        ];

        let chunks = game_rank_change_chunks(&logs, 3500);

        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("Alpha"));
        assert!(chunks[0].contains("B -> A"));
        assert!(!chunks[0].contains("Beta"));
    }

    #[test]
    fn rating_progression_helps_lower_ratings_more() {
        assert!(
            rating_progression_multiplier(850, true) > rating_progression_multiplier(1450, true)
        );
        assert!(
            rating_progression_multiplier(850, false) < rating_progression_multiplier(1450, false)
        );
        assert!(rating_progression_multiplier(1000, false) < 1.0);
        assert!(rating_progression_multiplier(1700, false) > 1.0);
    }

    #[test]
    fn leaderboard_sorts_by_rating() {
        let mut stats = StatsFile::default();
        stats.users.insert(
            "1".to_string(),
            PlayerStats {
                name: "Alpha".to_string(),
                games: 3,
                wins: 1,
                losses: 2,
                rating: 980,
                ..Default::default()
            },
        );
        stats.users.insert(
            "2".to_string(),
            PlayerStats {
                name: "Beta".to_string(),
                games: 2,
                wins: 2,
                losses: 0,
                rating: 1120,
                ..Default::default()
            },
        );

        let text = leaderboard_text(&stats, "rating");
        assert!(text.starts_with("기준: **레이팅**\n1. **Beta**"));
        assert!(text.contains("2. **Alpha**"));
    }

    #[test]
    fn win_streak_updates_and_sorts() {
        let game = rating_test_game();
        let roles = initial_roles(&game);
        let citizen_id = game
            .players
            .iter()
            .find(|player| game.is_citizen_team(player))
            .map(|player| player.user_id)
            .unwrap();
        let mut stats = StatsFile::default();

        record_game_stats(&mut stats, &game, &roles, 120, Winner::Citizen);
        record_game_stats(&mut stats, &game, &roles, 120, Winner::Citizen);

        let entry = stats.users.get(&citizen_id.to_string()).unwrap();
        assert_eq!(entry.win_streak, 2);
        assert_eq!(entry.best_win_streak, 2);

        record_game_stats(&mut stats, &game, &roles, 120, Winner::Mafia);

        let entry = stats.users.get(&citizen_id.to_string()).unwrap();
        assert_eq!(entry.win_streak, 0);
        assert_eq!(entry.best_win_streak, 2);

        let mut ranking = StatsFile::default();
        ranking.users.insert(
            "1".to_string(),
            PlayerStats {
                name: "Alpha".to_string(),
                games: 5,
                wins: 4,
                win_streak: 1,
                best_win_streak: 4,
                ..Default::default()
            },
        );
        ranking.users.insert(
            "2".to_string(),
            PlayerStats {
                name: "Beta".to_string(),
                games: 4,
                wins: 3,
                win_streak: 3,
                best_win_streak: 3,
                ..Default::default()
            },
        );

        let entries = leaderboard_entries(&ranking, "streak", 10);
        assert_eq!(entries[0].0, "2");
        assert_eq!(leaderboard_metric_name("streak"), "연승");
    }

    #[test]
    fn win_streak_bonus_increases_rating_gain() {
        let game = rating_test_game();
        let roles = initial_roles(&game);
        let citizen = game
            .players
            .iter()
            .find(|player| game.is_citizen_team(player))
            .unwrap();

        let mut baseline = StatsFile::default();
        baseline.users.insert(
            citizen.user_id.to_string(),
            PlayerStats {
                name: citizen.name.clone(),
                games: 4,
                wins: 3,
                losses: 1,
                win_streak: 0,
                best_win_streak: 3,
                rating_games: 4,
                ..Default::default()
            },
        );
        let baseline_log = record_game_stats(&mut baseline, &game, &roles, 120, Winner::Citizen);
        let baseline_item = baseline_log
            .iter()
            .find(|item| item.name == citizen.name)
            .unwrap();

        let mut streaking = StatsFile::default();
        streaking.users.insert(
            citizen.user_id.to_string(),
            PlayerStats {
                name: citizen.name.clone(),
                games: 4,
                wins: 4,
                losses: 0,
                win_streak: 4,
                best_win_streak: 4,
                rating_games: 4,
                ..Default::default()
            },
        );
        let streak_log = record_game_stats(&mut streaking, &game, &roles, 120, Winner::Citizen);
        let streak_item = streak_log
            .iter()
            .find(|item| item.name == citizen.name)
            .unwrap();

        assert!(streak_item.delta > baseline_item.delta);
        assert!(streak_item.streak_delta > baseline_item.streak_delta);
        assert_eq!(streak_item.win_streak, 5);
        assert_eq!(streak_item.best_win_streak, 5);
        assert!(
            streak_item
                .reasons
                .iter()
                .any(|reason| reason.contains("연승 보너스"))
        );
    }

    #[test]
    fn play_duration_formats_short_and_long_values() {
        assert_eq!(play_duration_text(12), "1분 미만");
        assert_eq!(play_duration_text(72), "1분");
        assert_eq!(play_duration_text(3700), "61분");
    }

    #[test]
    fn successful_role_event_is_recorded_in_rating_history() {
        let mut game = rating_test_game();
        let doctor = game
            .players
            .iter()
            .find(|player| player.role == Role::Doctor)
            .cloned()
            .unwrap();
        game.record_rating_event(doctor.user_id, 5, "마피아 공격 치료 성공");
        let mut stats = StatsFile::default();

        record_game_stats(
            &mut stats,
            &game,
            &initial_roles(&game),
            120,
            Winner::Citizen,
        );

        let history = stats
            .users
            .get(&doctor.user_id.to_string())
            .unwrap()
            .rating_history
            .last()
            .unwrap();
        assert!(history.role_delta >= 5);
        assert!(
            history
                .rating_reasons
                .iter()
                .any(|reason| reason.contains("치료 성공"))
        );
    }

    #[test]
    fn scientist_stats_switch_to_mafia_team_after_first_death() {
        let mut game = rating_test_game();
        let scientist_id = game.players[0].user_id;
        game.get_player_mut(scientist_id).unwrap().role = Role::Scientist;
        game.scientist_contacted.remove(&scientist_id);
        let roles = initial_roles(&game);
        let mut stats = StatsFile::default();

        record_game_stats(&mut stats, &game, &roles, 120, Winner::Citizen);
        let citizen_entry = stats.users.get(&scientist_id.to_string()).unwrap();
        assert_eq!(citizen_entry.wins, 1);
        assert_eq!(citizen_entry.mafia_team_games, 0);

        game.scientist_contacted.insert(scientist_id);
        record_game_stats(&mut stats, &game, &roles, 120, Winner::Mafia);
        let mafia_entry = stats.users.get(&scientist_id.to_string()).unwrap();
        assert_eq!(mafia_entry.wins, 2);
        assert_eq!(mafia_entry.mafia_team_games, 1);
    }

    #[test]
    fn role_rating_adjustment_is_capped() {
        let mut game = rating_test_game();
        let doctor = game
            .players
            .iter()
            .find(|player| player.role == Role::Doctor)
            .cloned()
            .unwrap();
        game.record_rating_event(doctor.user_id, 9, "첫 번째 기여");
        game.record_rating_event(doctor.user_id, 8, "두 번째 기여");

        let (role_delta, reasons) = role_rating_adjustment(&game, &doctor, Role::Doctor, true);

        assert_eq!(role_delta, ROLE_DELTA_CAP);
        assert!(reasons.iter().any(|reason| reason == "직업 보정 상한 적용"));
    }

    #[test]
    fn inactive_surviving_role_receives_small_penalty() {
        let mut game = rating_test_game();
        game.day_number = 2;
        let doctor = game
            .players
            .iter()
            .find(|player| player.role == Role::Doctor)
            .cloned()
            .unwrap();

        let (role_delta, reasons) = role_rating_adjustment(&game, &doctor, Role::Doctor, false);

        assert_eq!(role_delta, -2);
        assert!(reasons.iter().any(|reason| reason.contains("미사용")));
    }

    #[test]
    fn every_role_has_role_specific_rating_element() {
        let game = rating_test_game();
        let player = game.players.first().unwrap().clone();
        let roles = [
            Role::Mafia,
            Role::Doctor,
            Role::Nurse,
            Role::Police,
            Role::Agent,
            Role::Vigilante,
            Role::Inspector,
            Role::Reporter,
            Role::Hacker,
            Role::Detective,
            Role::Shaman,
            Role::Priest,
            Role::Soldier,
            Role::Gangster,
            Role::Prophet,
            Role::Psychologist,
            Role::Hypnotist,
            Role::Mercenary,
            Role::Spy,
            Role::Contractor,
            Role::Thief,
            Role::Witch,
            Role::Scientist,
            Role::Madam,
            Role::Graverobber,
            Role::Godfather,
            Role::Joker,
            Role::Politician,
            Role::Judge,
            Role::Terrorist,
            Role::Lover,
            Role::CultLeader,
            Role::Fanatic,
            Role::Frog,
            Role::Villain,
            Role::Citizen,
        ];

        for role in roles {
            let (points, reason) = role_specific_rating_adjustment(&player, role, true);
            assert!(points > 0, "{role:?} should have a positive win element");
            assert!(
                !reason.trim().is_empty(),
                "{role:?} should have a visible reason"
            );
        }
    }

    #[test]
    fn first_dead_losing_player_loses_less_rating() {
        let game = rating_test_game();
        let roles = initial_roles(&game);
        let loser = game
            .players
            .iter()
            .find(|player| game.is_citizen_team(player))
            .cloned()
            .unwrap();
        let other_id = game
            .players
            .iter()
            .find(|player| player.user_id != loser.user_id)
            .map(|player| player.user_id)
            .unwrap();

        let mut first_dead_game = game.clone();
        first_dead_game.get_player_mut(loser.user_id).unwrap().alive = false;
        first_dead_game.death_order.push(loser.user_id);

        let mut later_dead_game = game.clone();
        later_dead_game.get_player_mut(loser.user_id).unwrap().alive = false;
        later_dead_game.death_order.push(other_id);
        later_dead_game.death_order.push(loser.user_id);

        let mut first_stats = StatsFile::default();
        let first_log = record_game_stats(
            &mut first_stats,
            &first_dead_game,
            &roles,
            120,
            Winner::Mafia,
        );
        let first_item = first_log
            .iter()
            .find(|item| item.name == loser.name)
            .unwrap();

        let mut later_stats = StatsFile::default();
        let later_log = record_game_stats(
            &mut later_stats,
            &later_dead_game,
            &roles,
            120,
            Winner::Mafia,
        );
        let later_item = later_log
            .iter()
            .find(|item| item.name == loser.name)
            .unwrap();

        assert!(first_item.delta > later_item.delta);
        assert!(first_item.delta <= 0);
        assert!(
            first_item
                .reasons
                .iter()
                .any(|reason| reason.contains("첫 사망 패배 완화"))
        );
        assert!(
            !later_item
                .reasons
                .iter()
                .any(|reason| reason.contains("첫 사망 패배 완화"))
        );
    }

    #[test]
    fn losing_team_positive_gain_is_capped() {
        assert_eq!(final_rating_delta(-2, 10, false), LOSING_RATING_GAIN_CAP);
        assert_eq!(final_rating_delta(-40, 10, false), -30);
    }
}

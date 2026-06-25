use crate::game::MafiaGame;
use crate::model::{Player, Role, Winner};
use anyhow::{Context, Result};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::Path};

pub const INITIAL_RATING: i64 = 1000;
const RATING_HISTORY_LIMIT: usize = 20;
const RATING_DELTA_CAP: i64 = 80;
const ROLE_DELTA_CAP: i64 = 14;
const LOSING_RATING_GAIN_CAP: i64 = 5;
const ROLE_STATS_ORDER: &[Role] = &[
    Role::Mafia,
    Role::Police,
    Role::Agent,
    Role::Vigilante,
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
    Role::CultLeader,
    Role::Fanatic,
    Role::Joker,
    Role::Citizen,
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatsFile {
    #[serde(default)]
    pub users: HashMap<String, PlayerStats>,
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
    pub role: String,
    pub team: String,
    pub winner: String,
    pub players: usize,
    pub rating_reasons: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GameRatingLogItem {
    pub name: String,
    pub role: String,
    pub before: i64,
    pub after: i64,
    pub delta: i64,
    pub team_delta: i64,
    pub role_delta: i64,
    pub reasons: Vec<String>,
}

impl Default for PlayerStats {
    fn default() -> Self {
        Self {
            name: String::new(),
            games: 0,
            wins: 0,
            losses: 0,
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
        if role.is_mafia_team() {
            entry.mafia_team_games += 1;
        }
        if won {
            entry.wins += 1;
        } else {
            entry.losses += 1;
        }
        entry.rating = rating_change.after;
        entry.rating_games += 1;
        entry.rating_peak = entry.rating_peak.max(entry.rating);
        rating_log.push(GameRatingLogItem {
            name: player.name.clone(),
            role: role.value().to_string(),
            before: rating_change.before,
            after: rating_change.after,
            delta: rating_change.delta,
            team_delta: rating_change.team_delta,
            role_delta: rating_change.role_delta,
            reasons: rating_change.reasons.clone(),
        });
        entry.rating_history.push(RatingHistoryItem {
            ended_at: ended_at.clone(),
            before: rating_change.before,
            after: rating_change.after,
            delta: rating_change.delta,
            team_delta: rating_change.team_delta,
            role_delta: rating_change.role_delta,
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
        let line = format!(
            "- {} ({}) {} -> {} ({:+}) [팀 {:+} / 직업 {:+}]\n  사유: {}\n",
            item.name,
            item.role,
            item.before,
            item.after,
            item.delta,
            item.team_delta,
            item.role_delta,
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

pub fn role_appearance_counts(stats: &StatsFile) -> HashMap<Role, i64> {
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
    let (role_delta, mut role_reasons) = role_rating_adjustment(game, player, initial_role);
    let combined_delta = clamp(
        team_delta + role_delta,
        -RATING_DELTA_CAP,
        RATING_DELTA_CAP,
    );
    let final_delta = final_rating_delta(team_delta, role_delta, won);
    let after = (old_rating + final_delta).max(0);
    let mut reasons = vec![if won {
        "소속 진영 승리".to_string()
    } else {
        "소속 진영 패배".to_string()
    }];
    reasons.append(&mut role_reasons);
    if (rating_multiplier - 1.0).abs() > f64::EPSILON {
        reasons.push(format!("레이팅 구간 보정 x{rating_multiplier:.2}"));
    }
    if combined_delta != team_delta + role_delta {
        reasons.push("전체 레이팅 변동 상한 적용".to_string());
    }
    if !won && final_delta != combined_delta {
        reasons.push("패배팀 상승 제한 적용".to_string());
    }
    RatingChange {
        before: old_rating,
        after,
        delta: after - old_rating,
        team_delta,
        role_delta,
        reasons,
    }
}

fn role_rating_adjustment(game: &MafiaGame, player: &Player, role: Role) -> (i64, Vec<String>) {
    let mut points = 0;
    let mut reasons = Vec::new();
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
    if action_count == 0
        && player.alive
        && game.day_number >= 2
        && role_has_core_action(role)
    {
        points -= 2;
        reasons.push("핵심 능력 미사용 -2".to_string());
    }
    let role_delta = clamp(points, -ROLE_DELTA_CAP, ROLE_DELTA_CAP);
    if role_delta != points {
        reasons.push("직업 보정 상한 적용".to_string());
    }
    (role_delta, reasons)
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
    let combined_delta = clamp(
        team_delta + role_delta,
        -RATING_DELTA_CAP,
        RATING_DELTA_CAP,
    );
    if won {
        combined_delta
    } else {
        combined_delta.min(LOSING_RATING_GAIN_CAP)
    }
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
        64
    } else if rating_games < 30 {
        52
    } else if rating_games < 70 {
        44
    } else {
        36
    }
}

fn rating_progression_multiplier(rating: i64, won: bool) -> f64 {
    if won {
        if rating < 900 {
            1.70
        } else if rating < 1100 {
            1.35
        } else if rating < 1300 {
            1.10
        } else if rating < 1500 {
            0.85
        } else if rating < 1700 {
            0.70
        } else {
            0.55
        }
    } else if rating < 900 {
        0.55
    } else if rating < 1100 {
        0.75
    } else if rating < 1300 {
        0.95
    } else if rating < 1500 {
        1.15
    } else if rating < 1700 {
        1.35
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
    if rating < 900 {
        "C"
    } else if rating < 1100 {
        "B"
    } else if rating < 1300 {
        "A"
    } else if rating < 1500 {
        "S"
    } else if rating < 1700 {
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
            "{}. **{}** - {}승 {}패 / {}판 / 승률 {} / 마피아팀 {}회 / 게임시간 {} / 레이팅 {}점 ({})",
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
        let detail = if detail.is_empty() {
            format!("팀 {:+}, 직업 {:+}", item.team_delta, item.role_delta)
        } else {
            format!(
                "팀 {:+}, 직업 {:+} / {detail}",
                item.team_delta, item.role_delta
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
    fn rating_rank_maps_rating_bands() {
        assert_eq!(rating_rank(899), "C");
        assert_eq!(rating_rank(900), "B");
        assert_eq!(rating_rank(1100), "A");
        assert_eq!(rating_rank(1300), "S");
        assert_eq!(rating_rank(1500), "SS");
        assert_eq!(rating_rank(1700), "X");
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

        record_game_stats(&mut stats, &game, &initial_roles(&game), 120, Winner::Citizen);

        let history = stats
            .users
            .get(&doctor.user_id.to_string())
            .unwrap()
            .rating_history
            .last()
            .unwrap();
        assert_eq!(history.role_delta, 5);
        assert!(history.rating_reasons.iter().any(|reason| reason.contains("치료 성공")));
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

        let (role_delta, reasons) = role_rating_adjustment(&game, &doctor, Role::Doctor);

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

        let (role_delta, reasons) = role_rating_adjustment(&game, &doctor, Role::Doctor);

        assert_eq!(role_delta, -2);
        assert!(reasons.iter().any(|reason| reason.contains("미사용")));
    }

    #[test]
    fn losing_team_positive_gain_is_capped() {
        assert_eq!(final_rating_delta(-2, 10, false), LOSING_RATING_GAIN_CAP);
        assert_eq!(final_rating_delta(-40, 10, false), -30);
    }
}

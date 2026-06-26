use mafia_remake::{
    game::{MafiaGame, majority_required},
    model::{Phase, Role},
};

#[test]
fn half_of_even_players_meets_the_majority_threshold() {
    assert_eq!(majority_required(5), 3);
    assert_eq!(majority_required(6), 3);
}

#[test]
fn confirmation_majority_uses_submitted_vote_count() {
    let mut game = MafiaGame::new(
        (1..=7).map(|id| (id, format!("p{id}"))).collect(),
        1,
        0,
        0,
        vec![],
    )
    .unwrap();
    let target_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Citizen)
        .map(|player| player.user_id)
        .unwrap();
    let voter_ids = game
        .players
        .iter()
        .filter(|player| player.user_id != target_id)
        .take(5)
        .map(|player| player.user_id)
        .collect::<Vec<_>>();
    game.phase = Phase::ConfirmVote;
    for (index, voter_id) in voter_ids.into_iter().enumerate() {
        game.submit_confirmation_vote(voter_id, index < 3).unwrap();
    }

    let result = game.resolve_confirmation_vote(target_id).unwrap();

    assert!(result.approved);
    assert_eq!(
        result.executed.map(|player| player.user_id),
        Some(target_id)
    );
}

#[test]
fn confirmation_tie_requires_one_more_yes_than_no() {
    let mut game = MafiaGame::new(
        (1..=9).map(|id| (id, format!("p{id}"))).collect(),
        1,
        0,
        0,
        vec![],
    )
    .unwrap();
    let target_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Citizen)
        .map(|player| player.user_id)
        .unwrap();
    let voter_ids = game
        .players
        .iter()
        .filter(|player| player.user_id != target_id)
        .take(8)
        .map(|player| player.user_id)
        .collect::<Vec<_>>();
    game.phase = Phase::ConfirmVote;
    for (index, voter_id) in voter_ids.into_iter().enumerate() {
        game.submit_confirmation_vote(voter_id, index < 4).unwrap();
    }

    let result = game.resolve_confirmation_vote(target_id).unwrap();

    assert!(!result.approved);
    assert!(result.executed.is_none());
    assert!(result.tied);
}

use mafia_remake::{
    game::{MafiaGame, majority_required},
    model::{Phase, Role},
};

#[test]
fn half_of_even_players_meets_the_majority_threshold() {
    assert_eq!(majority_required(5), 3);
    assert_eq!(majority_required(6), 3);

    let mut game = MafiaGame::new(
        (1..=6).map(|id| (id, format!("p{id}"))).collect(),
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
        .map(|player| player.user_id)
        .collect::<Vec<_>>();
    game.phase = Phase::ConfirmVote;
    for (index, voter_id) in voter_ids.into_iter().enumerate() {
        game.submit_confirmation_vote(voter_id, index < 3).unwrap();
    }

    let result = game.resolve_confirmation_vote(target_id).unwrap();

    assert!(result.approved);
    assert_eq!(result.executed.map(|player| player.user_id), Some(target_id));
}

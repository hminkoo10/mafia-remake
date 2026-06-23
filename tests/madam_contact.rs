use mafia_remake::{
    game::MafiaGame,
    model::{Phase, Role, VoteResult},
};

fn resolve_madam_vote(game: &mut MafiaGame, madam_id: u64, target_id: u64) -> VoteResult {
    game.start_vote().unwrap();
    game.submit_day_vote(madam_id, Some(target_id)).unwrap();
    let voter_ids = game
        .alive_players()
        .into_iter()
        .filter(|player| player.user_id != madam_id)
        .map(|player| player.user_id)
        .collect::<Vec<_>>();
    for voter_id in voter_ids {
        game.submit_day_vote(voter_id, None).unwrap();
    }
    game.resolve_nomination_vote().unwrap()
}

#[test]
fn madam_contact_event_only_reports_new_mafia_contact() {
    let players = (1..=6)
        .map(|id| (id, format!("player-{id}")))
        .collect::<Vec<_>>();
    let mut game = MafiaGame::new(players, 1, 1, 0, vec![Role::Madam]).unwrap();
    let madam_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Madam)
        .unwrap()
        .user_id;
    let mafia_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Mafia)
        .unwrap()
        .user_id;
    let citizen_id = game
        .players
        .iter()
        .find(|player| player.role == Role::Citizen)
        .unwrap()
        .user_id;

    game.phase = Phase::Day;
    let first_vote = resolve_madam_vote(&mut game, madam_id, citizen_id);
    assert!(first_vote.madam_newly_contacted.is_empty());

    game.resolve_night().unwrap();
    let contact_vote = resolve_madam_vote(&mut game, madam_id, mafia_id);
    assert_eq!(
        contact_vote
            .madam_newly_contacted
            .iter()
            .map(|player| player.user_id)
            .collect::<Vec<_>>(),
        vec![madam_id]
    );

    game.resolve_night().unwrap();
    let later_citizen_vote = resolve_madam_vote(&mut game, madam_id, citizen_id);
    assert!(later_citizen_vote.madam_newly_contacted.is_empty());
}

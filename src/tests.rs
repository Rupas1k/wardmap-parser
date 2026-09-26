use crate::observers::vision::metrics::{Observation, PlayerObservation, Sight, VisionMetrics};

fn player(state: Observation) -> PlayerObservation {
    PlayerObservation {
        slot: 7,
        steam_id: Some(76561197960265735),
        hero_name: "npc_dota_hero_axe".to_owned(),
        team: 3,
        position: Some([1.0, 2.0, 3.0]),
        state,
    }
}

#[test]
fn added_vision_is_split_between_observer_wards() {
    let mut metrics = VisionMetrics::default();
    let wards = [(10, 2), (11, 2)];
    let sight = || {
        Observation::Alive(Sight {
            wards: vec![10, 11],
            non_ward: false,
        })
    };

    metrics.sample(0.0, vec![player(sight())], &wards);
    metrics.sample(0.2, vec![player(sight())], &wards);

    assert!((metrics.measurement(10).added_vision_seconds - 0.1).abs() < 1e-9);
    assert!((metrics.measurement(11).added_vision_seconds - 0.1).abs() < 1e-9);
    let measurement = metrics.measurement(10);
    assert_eq!(measurement.intervals.len(), 1);
    assert_eq!(measurement.intervals[0].observer_handles, vec![10, 11]);
    assert!((measurement.intervals[0].added_seconds - measurement.added_vision_seconds).abs() < 1e-9);
}

#[test]
fn new_ward_does_not_inherit_previous_interval_coverage() {
    let mut metrics = VisionMetrics::default();
    metrics.sample(0.0, vec![player(Observation::Dead)], &[]);
    metrics.sample(0.2, vec![player(Observation::Dead)], &[(10, 2)]);
    assert_eq!(metrics.measurement(10).vision_measured_seconds, 0.0);
    metrics.sample(0.4, vec![player(Observation::Dead)], &[(10, 2)]);
    assert!((metrics.measurement(10).vision_measured_seconds - 0.2).abs() < 1e-9);
}

#[test]
fn recovery_from_unknown_does_not_backfill_coverage() {
    let mut metrics = VisionMetrics::default();
    metrics.sample(0.0, vec![player(Observation::Unknown)], &[(10, 2)]);
    metrics.sample(0.2, vec![player(Observation::Dead)], &[(10, 2)]);
    assert_eq!(metrics.measurement(10).vision_measured_seconds, 0.0);
}

#[test]
fn gap_cannot_create_a_fresh_sighting() {
    let mut metrics = VisionMetrics::default();
    metrics.sample(0.0, vec![player(Observation::Alive(Sight::default()))], &[(10, 2)]);
    metrics.sample(
        6.0,
        vec![player(Observation::Alive(Sight {
            wards: vec![10],
            non_ward: false,
        }))],
        &[(10, 2)],
    );
    assert_eq!(metrics.measurement(10).fresh_sightings, 0.0);
    assert_eq!(metrics.measurement(10).vision_measured_seconds, 0.0);
}

#[test]
fn unknown_target_time_reduces_coverage_without_becoming_zero_impact() {
    let mut metrics = VisionMetrics::default();
    let wards = [(10, 2)];

    metrics.sample(0.0, vec![player(Observation::Unknown)], &wards);
    metrics.sample(0.2, vec![player(Observation::Unknown)], &wards);

    let measurement = metrics.measurement(10);
    assert!((measurement.vision_possible_seconds - 0.2).abs() < 1e-9);
    assert_eq!(measurement.vision_measured_seconds, 0.0);
    assert_eq!(measurement.vision_coverage, Some(0.0));
}

#[test]
fn fresh_sighting_requires_five_known_hidden_seconds_and_splits_credit() {
    let mut metrics = VisionMetrics::default();
    let wards = [(10, 2), (11, 2)];
    let hidden = || Observation::Alive(Sight::default());

    metrics.sample(0.0, vec![player(hidden())], &wards);
    for sample in 1..=25 {
        metrics.sample(sample as f64 * 0.2, vec![player(hidden())], &wards);
    }
    metrics.sample(
        5.2,
        vec![player(Observation::Alive(Sight {
            wards: vec![10, 11],
            non_ward: false,
        }))],
        &wards,
    );
    metrics.sample(
        5.4,
        vec![player(Observation::Alive(Sight {
            wards: vec![11],
            non_ward: false,
        }))],
        &wards,
    );

    assert_eq!(metrics.measurement(10).fresh_sightings, 0.5);
    assert_eq!(metrics.measurement(11).fresh_sightings, 0.5);
    let measurement = metrics.measurement(10);
    assert_eq!(measurement.sightings.len(), 1);
    assert_eq!(measurement.sightings[0].target_player_slot, 7);
    assert_eq!(measurement.sightings[0].target_steam_id.as_deref(), Some("76561197960265735"));
    assert_eq!(measurement.sightings[0].credit, measurement.fresh_sightings);
    assert!((measurement.sightings[0].segments[0].visible_seconds.unwrap() - 0.2).abs() < 1e-9);
    assert_eq!(measurement.sightings[0].segments[0].lost_position, Some([1.0, 2.0, 3.0]));
    assert_eq!(metrics.measurement(11).sightings[0].segments[0].visible_seconds, None);

    metrics.sample(5.6, vec![player(hidden())], &wards);
    assert!((metrics.measurement(11).sightings[0].segments[0].visible_seconds.unwrap() - 0.4).abs() < 1e-9);
}

#[test]
fn short_gap_creates_resighting_segment_without_new_fresh_sighting() {
    let mut metrics = VisionMetrics::default();
    let wards = [(10, 2)];
    let hidden = || Observation::Alive(Sight::default());
    let visible = || {
        Observation::Alive(Sight {
            wards: vec![10],
            non_ward: false,
        })
    };

    metrics.sample(0.0, vec![player(hidden())], &wards);
    for sample in 1..=25 {
        metrics.sample(sample as f64 * 0.2, vec![player(hidden())], &wards);
    }
    metrics.sample(5.2, vec![player(visible())], &wards);
    metrics.sample(5.4, vec![player(hidden())], &wards);
    metrics.sample(5.6, vec![player(hidden())], &wards);
    metrics.sample(5.8, vec![player(hidden())], &wards);
    metrics.sample(6.0, vec![player(visible())], &wards);
    metrics.sample(6.2, vec![player(hidden())], &wards);

    let measurement = metrics.measurement(10);
    assert_eq!(measurement.fresh_sightings, 1.0);
    assert_eq!(measurement.sightings.len(), 1);
    assert_eq!(measurement.sightings[0].segments.len(), 2);
    assert_eq!(measurement.sightings[0].segments[0].gap_seconds, None);
    assert!((measurement.sightings[0].segments[1].gap_seconds.unwrap() - 0.8).abs() < 1e-9);
}

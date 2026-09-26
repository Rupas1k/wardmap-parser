use std::collections::{HashMap, HashSet};

use serde::Serialize;

pub const SCOUTING_VERSION: i16 = 1;
pub const DISCOVERY_TAU_SECONDS: f64 = 10.0;
pub const SAMPLE_INTERVAL_TICKS: i32 = 6;
pub const MEASUREMENT_VERSION: u16 = 2;
pub const FRESH_SIGHTING_THRESHOLD_SECONDS: f64 = 5.0;
const MAX_INTERVAL_SECONDS: f64 = SAMPLE_INTERVAL_TICKS as f64 / 30.0 + 1e-6;
const ROUTE_SAMPLE_SECONDS: f64 = 0.25;
const ROUTE_SAMPLE_DISTANCE: f32 = 128.0;

#[derive(Debug, Clone, Serialize)]
pub struct SightingRoutePoint {
    pub time: f64,
    pub position: [f32; 3],
}

#[derive(Debug, Clone, Serialize)]
pub struct SightingSegmentEvidence {
    pub time: f64,
    pub gap_seconds: Option<f64>,
    pub start_position: [f32; 3],
    pub visible_seconds: Option<f64>,
    pub lost_position: Option<[f32; 3]>,
    pub route: Vec<SightingRoutePoint>,
}

#[derive(Debug, Default, Clone)]
pub struct WardVisionMetrics {
    pub enemy_hero_seconds: f64,
    pub unique_enemy_hero_seconds: f64,
    pub heroes_spotted: u8,
    pub hero_reveal_events: u16,
    pub unique_hero_reveal_events: u16,
    pub scouting_tracking_seconds: f64,
    pub scouting_discovery_seconds: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SightingEvidence {
    pub sample_tick: i32,
    pub time: f64,
    pub target_player_slot: u8,
    pub target_steam_id: Option<String>,
    pub target_hero_name: String,
    pub target_is_radiant: bool,
    pub target_position: [f32; 3],
    pub hidden_seconds: f64,
    pub segments: Vec<SightingSegmentEvidence>,
    pub observer_handles: Vec<u32>,
    pub credit: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct VisionInterval {
    pub start: f64,
    pub end: f64,
    pub target_player: String,
    pub observer_handles: Vec<u32>,
    pub non_ward: bool,
    pub added_seconds: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WardMeasurement {
    pub intervals: Vec<VisionInterval>,
    pub sightings: Vec<SightingEvidence>,
    pub version: u16,
    pub added_vision_seconds: f64,
    pub fresh_sightings: f64,
    pub fresh_sighting_threshold_seconds: f64,
    pub vision_possible_seconds: f64,
    pub vision_measured_seconds: f64,
    pub vision_coverage: Option<f64>,
}

impl WardVisionMetrics {
    pub fn scouting_score(&self) -> f64 {
        self.scouting_tracking_seconds + self.scouting_discovery_seconds
    }
}

#[derive(Default)]
struct WardAccumulator {
    intervals: Vec<VisionInterval>,
    last_interval: HashMap<u8, usize>,
    sightings: Vec<SightingEvidence>,
    metrics: WardVisionMetrics,
    heroes: HashSet<u8>,
    fresh_sightings: f64,
    eligible_target_seconds: f64,
    known_target_seconds: f64,
}

#[derive(Default, Clone)]
pub struct Sight {
    pub wards: Vec<u32>,
    pub non_ward: bool,
}

impl Sight {
    fn visible(&self) -> bool {
        self.non_ward || !self.wards.is_empty()
    }
}

#[derive(Clone)]
pub enum Observation {
    Alive(Sight),
    Dead,
    Unknown,
}

#[derive(Clone)]
pub struct PlayerObservation {
    pub slot: u8,
    pub steam_id: Option<u64>,
    pub hero_name: String,
    pub team: i32,
    pub position: Option<[f32; 3]>,
    pub state: Observation,
}

struct TargetState {
    sight: Sight,
    unseen_since: Option<f64>,
    position: Option<[f32; 3]>,
}

#[derive(Clone, Copy)]
struct OpenSighting {
    sighting: usize,
    segment: usize,
}

#[derive(Clone, Copy)]
struct RecentSighting {
    sighting: usize,
    lost_at: f64,
}

#[derive(Default)]
pub struct VisionMetrics {
    previous_time: Option<f64>,
    targets: HashMap<u8, TargetState>,
    wards: HashMap<u32, WardAccumulator>,
    previous_known: HashSet<u8>,
    previous_active: Vec<(u32, i32)>,
    open_sightings: HashMap<(u8, u32), OpenSighting>,
    recent_sightings: HashMap<(u8, u32), RecentSighting>,
}

impl VisionMetrics {
    pub fn ward(&self, handle: u32) -> WardVisionMetrics {
        self.wards.get(&handle).map(|ward| ward.metrics.clone()).unwrap_or_default()
    }

    pub fn measurement(&self, handle: u32) -> WardMeasurement {
        let ward = self.wards.get(&handle);
        let eligible = ward.map_or(0.0, |ward| ward.eligible_target_seconds);
        let known = ward.map_or(0.0, |ward| ward.known_target_seconds);

        WardMeasurement {
            intervals: ward.map_or_else(Vec::new, |ward| ward.intervals.clone()),
            sightings: ward.map_or_else(Vec::new, |ward| ward.sightings.clone()),
            version: MEASUREMENT_VERSION,
            added_vision_seconds: ward.map_or(0.0, |ward| ward.metrics.scouting_tracking_seconds),
            fresh_sightings: ward.map_or(0.0, |ward| ward.fresh_sightings),
            fresh_sighting_threshold_seconds: FRESH_SIGHTING_THRESHOLD_SECONDS,
            vision_possible_seconds: eligible,
            vision_measured_seconds: known,
            vision_coverage: (eligible > 0.0).then_some((known / eligible).clamp(0.0, 1.0)),
        }
    }

    pub fn sample_at_tick(
        &mut self,
        sample_tick: i32,
        time: f64,
        observations: Vec<PlayerObservation>,
        active_wards: &[(u32, i32)],
    ) {
        let elapsed = self.previous_time.map(|previous| time - previous);
        self.previous_time = Some(time);

        let elapsed = match elapsed {
            Some(elapsed) if (0.0..=MAX_INTERVAL_SECONDS).contains(&elapsed) => elapsed,
            _ => {
                self.targets.clear();
                self.previous_known.clear();
                self.open_sightings.clear();
                self.recent_sightings.clear();
                0.0
            }
        };
        let mut previous = std::mem::take(&mut self.targets);
        let previous_known = std::mem::take(&mut self.previous_known);
        let previous_active = std::mem::replace(&mut self.previous_active, active_wards.to_vec());

        for observation in observations {
            let player = observation.slot;
            let steam_id = observation.steam_id;
            let hero_name = observation.hero_name;
            let team = observation.team;
            let position = observation.position;
            let state = observation.state;
            let old = previous.remove(&player);

            if elapsed > 0.0 {
                for (handle, _) in previous_active.iter().filter(|(_, ward_team)| *ward_team != team) {
                    let ward = self.wards.entry(*handle).or_default();
                    ward.eligible_target_seconds += elapsed;
                    if previous_known.contains(&player) && !matches!(state, Observation::Unknown) {
                        ward.known_target_seconds += elapsed;
                    }
                }
            }

            if matches!(state, Observation::Unknown) {
                self.open_sightings.retain(|(slot, _), _| *slot != player);
                continue;
            }

            self.previous_known.insert(player);

            if let Some(old) = &old {
                self.integrate(&old.sight, elapsed, time, player);
            }

            let Observation::Alive(mut sight) = state else {
                self.finish_missing_sightings(player, time, time - elapsed, old.as_ref().and_then(|old| old.position), &[]);
                continue;
            };

            sight.wards.sort_unstable();
            sight.wards.dedup();

            let visible = sight.visible();

            self.finish_missing_sightings(
                player,
                time,
                time - elapsed,
                old.as_ref().and_then(|old| old.position),
                &sight.wards,
            );

            for handle in &sight.wards {
                let ward = self.wards.entry(*handle).or_default();
                ward.heroes.insert(player);
                ward.metrics.heroes_spotted = ward.heroes.len().try_into().unwrap_or(u8::MAX);
            }

            if visible && !sight.non_ward {
                if let Some(since) = old.as_ref().and_then(|old| old.unseen_since) {
                    let hidden_seconds = time - since;
                    let discovery = discovery_bonus(hidden_seconds) / sight.wards.len() as f64;
                    for handle in &sight.wards {
                        let ward = self.wards.entry(*handle).or_default();
                        if hidden_seconds >= FRESH_SIGHTING_THRESHOLD_SECONDS {
                            ward.fresh_sightings += 1.0 / sight.wards.len() as f64;
                            let index = ward.sightings.len();
                            ward.sightings.push(SightingEvidence {
                                sample_tick,
                                time,
                                target_player_slot: player,
                                target_steam_id: steam_id.map(|id| id.to_string()),
                                target_hero_name: hero_name.clone(),
                                target_is_radiant: team == 2,
                                target_position: position.unwrap_or_default(),
                                hidden_seconds,
                                segments: vec![SightingSegmentEvidence {
                                    time,
                                    gap_seconds: None,
                                    start_position: position.unwrap_or_default(),
                                    visible_seconds: None,
                                    lost_position: None,
                                    route: vec![SightingRoutePoint {
                                        time,
                                        position: position.unwrap_or_default(),
                                    }],
                                }],
                                observer_handles: sight.wards.clone(),
                                credit: 1.0 / sight.wards.len() as f64,
                            });
                            self.open_sightings.insert(
                                (player, *handle),
                                OpenSighting {
                                    sighting: index,
                                    segment: 0,
                                },
                            );
                            self.recent_sightings.remove(&(player, *handle));
                        }
                        let metrics = &mut ward.metrics;
                        metrics.scouting_discovery_seconds += discovery;
                        metrics.hero_reveal_events = metrics.hero_reveal_events.saturating_add(1);
                        if sight.wards.len() == 1 {
                            metrics.unique_hero_reveal_events = metrics.unique_hero_reveal_events.saturating_add(1);
                        }
                    }
                }
            }

            for handle in &sight.wards {
                let key = (player, *handle);
                let newly_visible = old.as_ref().is_some_and(|old| !old.sight.wards.contains(handle));
                if !newly_visible || self.open_sightings.contains_key(&key) {
                    continue;
                }

                let Some(recent) = self.recent_sightings.get(&key).copied() else {
                    continue;
                };
                let gap_seconds = time - recent.lost_at;
                if !(0.0..FRESH_SIGHTING_THRESHOLD_SECONDS).contains(&gap_seconds) {
                    self.recent_sightings.remove(&key);
                    continue;
                }

                let ward = self.wards.entry(*handle).or_default();
                let Some(sighting) = ward.sightings.get_mut(recent.sighting) else {
                    self.recent_sightings.remove(&key);
                    continue;
                };
                let segment = sighting.segments.len();
                let position = position.unwrap_or_default();
                sighting.segments.push(SightingSegmentEvidence {
                    time,
                    gap_seconds: Some(gap_seconds),
                    start_position: position,
                    visible_seconds: None,
                    lost_position: None,
                    route: vec![SightingRoutePoint { time, position }],
                });
                self.open_sightings.insert(
                    key,
                    OpenSighting {
                        sighting: recent.sighting,
                        segment,
                    },
                );
                self.recent_sightings.remove(&key);
            }

            self.extend_sighting_routes(player, time, position, &sight.wards);

            let unseen_since = if visible {
                None
            } else {
                Some(old.and_then(|old| old.unseen_since).unwrap_or(time))
            };

            self.targets.insert(
                player,
                TargetState {
                    sight,
                    unseen_since,
                    position,
                },
            );
        }
    }

    fn extend_sighting_routes(&mut self, player: u8, time: f64, position: Option<[f32; 3]>, ward_handles: &[u32]) {
        let Some(position) = position else {
            return;
        };

        for &handle in ward_handles {
            let Some(&open) = self.open_sightings.get(&(player, handle)) else {
                continue;
            };

            let Some(segment) = self
                .wards
                .get_mut(&handle)
                .and_then(|ward| ward.sightings.get_mut(open.sighting))
                .and_then(|sighting| sighting.segments.get_mut(open.segment))
            else {
                continue;
            };

            let Some(previous) = segment.route.last() else {
                continue;
            };

            let distance = previous
                .position
                .iter()
                .zip(position)
                .map(|(left, right)| (left - right).powi(2))
                .sum::<f32>()
                .sqrt();

            if time - previous.time >= ROUTE_SAMPLE_SECONDS || distance >= ROUTE_SAMPLE_DISTANCE {
                segment.route.push(SightingRoutePoint { time, position });
            }
        }
    }

    fn finish_missing_sightings(
        &mut self,
        player: u8,
        time: f64,
        last_visible_time: f64,
        position: Option<[f32; 3]>,
        visible_wards: &[u32],
    ) {
        let sightings = self
            .open_sightings
            .iter()
            .filter(|((slot, handle), _)| *slot == player && !visible_wards.contains(handle))
            .map(|(&(slot, handle), &open)| (slot, handle, open))
            .collect::<Vec<_>>();

        for (slot, handle, open) in sightings {
            self.open_sightings.remove(&(slot, handle));

            let Some(segment) = self
                .wards
                .get_mut(&handle)
                .and_then(|ward| ward.sightings.get_mut(open.sighting))
                .and_then(|sighting| sighting.segments.get_mut(open.segment))
            else {
                continue;
            };

            segment.visible_seconds = Some((time - segment.time).max(0.0));
            segment.lost_position = position;

            if let Some(position) = position {
                let duplicate = segment.route.last().is_some_and(|point| point.position == position);
                if !duplicate {
                    segment.route.push(SightingRoutePoint {
                        time: last_visible_time,
                        position,
                    });
                }
            }

            self.recent_sightings.insert(
                (slot, handle),
                RecentSighting {
                    sighting: open.sighting,
                    lost_at: last_visible_time,
                },
            );
        }
    }

    fn integrate(&mut self, sight: &Sight, elapsed: f64, time: f64, player: u8) {
        for handle in &sight.wards {
            let ward = self.wards.entry(*handle).or_default();
            let added = if sight.non_ward {
                0.0
            } else {
                elapsed / sight.wards.len() as f64
            };

            if elapsed > 0.0 {
                let previous = ward.last_interval.get(&player).copied();
                let merge = previous.filter(|&index| {
                    let interval = &ward.intervals[index];
                    (interval.end - (time - elapsed)).abs() < 1e-6
                        && interval.observer_handles == sight.wards
                        && interval.non_ward == sight.non_ward
                });

                if let Some(index) = merge {
                    let interval = &mut ward.intervals[index];
                    interval.end = time;
                    interval.added_seconds += added;
                } else {
                    ward.last_interval.insert(player, ward.intervals.len());
                    ward.intervals.push(VisionInterval {
                        start: time - elapsed,
                        end: time,
                        target_player: player.to_string(),
                        observer_handles: sight.wards.clone(),
                        non_ward: sight.non_ward,
                        added_seconds: added,
                    });
                }
            }

            let metrics = &mut ward.metrics;
            metrics.enemy_hero_seconds += elapsed;

            if !sight.non_ward {
                metrics.scouting_tracking_seconds += elapsed / sight.wards.len() as f64;
                if sight.wards.len() == 1 {
                    metrics.unique_enemy_hero_seconds += elapsed;
                }
            }
        }
    }
}

fn discovery_bonus(unseen_seconds: f64) -> f64 {
    -DISCOVERY_TAU_SECONDS * (-unseen_seconds.max(0.0) / DISCOVERY_TAU_SECONDS).exp_m1()
}

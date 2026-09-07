use std::collections::{HashMap, HashSet};

pub const SCOUTING_VERSION: i16 = 1;
pub const DISCOVERY_TAU_SECONDS: f64 = 10.0;
pub const SAMPLE_INTERVAL_TICKS: i32 = 6;
const MAX_INTERVAL_SECONDS: f64 = SAMPLE_INTERVAL_TICKS as f64 / 30.0 + 1e-6;

#[derive(Debug, Default, Clone)]
pub struct WardVisionMetrics {
    pub enemy_hero_seconds: f64,
    pub unique_enemy_hero_seconds: f64,
    pub ward_only_enemy_hero_seconds: f64,
    pub heroes_spotted: u8,
    pub hero_reveal_events: u16,
    pub unique_hero_reveal_events: u16,
    pub scouting_tracking_seconds: f64,
    pub scouting_discovery_seconds: f64,
}

impl WardVisionMetrics {
    pub fn scouting_score(&self) -> f64 {
        self.scouting_tracking_seconds + self.scouting_discovery_seconds
    }
}

#[derive(Default)]
struct WardAccumulator {
    metrics: WardVisionMetrics,
    heroes: HashSet<u64>,
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

pub enum Observation {
    Alive(Sight),
    Dead,
    Unknown,
}

struct TargetState {
    sight: Sight,
    unseen_since: Option<f64>,
}

#[derive(Default)]
pub struct VisionMetrics {
    previous_time: Option<f64>,
    targets: HashMap<u64, TargetState>,
    wards: HashMap<u32, WardAccumulator>,
}

impl VisionMetrics {
    pub fn ward(&self, handle: u32) -> WardVisionMetrics {
        self.wards.get(&handle).map(|ward| ward.metrics.clone()).unwrap_or_default()
    }

    pub fn sample(&mut self, time: f64, observations: Vec<(u64, Observation)>) {
        let elapsed = self.previous_time.map(|previous| time - previous);
        self.previous_time = Some(time);

        let elapsed = match elapsed {
            Some(elapsed) if (0.0..=MAX_INTERVAL_SECONDS).contains(&elapsed) => elapsed,
            _ => {
                self.targets.clear();
                0.0
            }
        };
        let mut previous = std::mem::take(&mut self.targets);

        for (player, observation) in observations {
            let old = previous.remove(&player);
            if matches!(observation, Observation::Unknown) {
                continue;
            }

            if let Some(old) = &old {
                self.integrate(&old.sight, elapsed);
            }

            let Observation::Alive(mut sight) = observation else {
                continue;
            };
            sight.wards.sort_unstable();
            sight.wards.dedup();

            let visible = sight.visible();
            for handle in &sight.wards {
                let ward = self.wards.entry(*handle).or_default();
                ward.heroes.insert(player);
                ward.metrics.heroes_spotted = ward.heroes.len().try_into().unwrap_or(u8::MAX);
            }

            if visible && !sight.non_ward {
                if let Some(since) = old.as_ref().and_then(|old| old.unseen_since) {
                    let discovery = discovery_bonus(time - since) / sight.wards.len() as f64;
                    for handle in &sight.wards {
                        let metrics = &mut self.wards.entry(*handle).or_default().metrics;
                        metrics.scouting_discovery_seconds += discovery;
                        metrics.hero_reveal_events = metrics.hero_reveal_events.saturating_add(1);
                        if sight.wards.len() == 1 {
                            metrics.unique_hero_reveal_events = metrics.unique_hero_reveal_events.saturating_add(1);
                        }
                    }
                }
            }

            let unseen_since = if visible {
                None
            } else {
                Some(old.and_then(|old| old.unseen_since).unwrap_or(time))
            };
            self.targets.insert(player, TargetState { sight, unseen_since });
        }
    }

    fn integrate(&mut self, sight: &Sight, elapsed: f64) {
        for handle in &sight.wards {
            let metrics = &mut self.wards.entry(*handle).or_default().metrics;
            metrics.enemy_hero_seconds += elapsed;
            if !sight.non_ward {
                metrics.ward_only_enemy_hero_seconds += elapsed;
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

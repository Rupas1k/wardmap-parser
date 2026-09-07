use std::cell::RefCell;
use std::collections::{BTreeSet, HashSet};
use std::rc::Rc;

use source2_demo::prelude::*;
use source2_demo::proto::DotaCombatlogTypes;

mod grid;
use grid::VisionGrid;
mod metrics;
use metrics::{Observation, Sight, VisionMetrics, SAMPLE_INTERVAL_TICKS};
pub use metrics::{WardVisionMetrics, DISCOVERY_TAU_SECONDS, SCOUTING_VERSION};

use crate::observers::game_time::GameTime;
use crate::observers::players::Players;
use crate::VisionSample;

const RADIANT: i32 = 2;
const DIRE: i32 = 3;
const WATCHER_VISION_RANGE: i32 = 800;

#[derive(Default)]
pub struct Vision {
    game_time: Rc<RefCell<GameTime>>,
    players: Rc<RefCell<Players>>,
    grid: VisionGrid,
    provider_handles: HashSet<u32>,
    next_sample_tick: Option<i32>,
    metrics: VisionMetrics,
    providers_changed: bool,
    previous_sample_tick: Option<i32>,
    active_observers: HashSet<u32>,
    incomplete_wards: HashSet<u32>,
    smoked_heroes: HashSet<u64>,
    collect_samples: bool,
    pub samples: Vec<VisionSample>,
}

impl Vision {
    pub fn set_dependencies(&mut self, game_time: Rc<RefCell<GameTime>>, players: Rc<RefCell<Players>>) {
        self.game_time = game_time;
        self.players = players;
    }

    pub fn enable_samples(&mut self) {
        self.collect_samples = true;
    }

    pub fn ward_metrics(&self, handle: u32) -> WardVisionMetrics {
        self.metrics.ward(handle)
    }

    pub fn scouting_complete(&self, handle: u32) -> bool {
        !self.incomplete_wards.contains(&handle)
    }

    fn sample(&mut self, ctx: &Context, tick: i32, start_time: Option<f32>) {
        let is_day = self.is_day(ctx);
        let providers_known = self.provider_handles.iter().all(|handle| {
            ctx.entities().get_by_handle(*handle as usize).is_ok_and(|entity| {
                entity_position(entity).is_some_and(|position| self.grid.contains(position))
                    && (entity.class().name() == "CDOTA_BaseNPC_Watch_Tower"
                        || (try_property!(entity, i32, "m_iDayTimeVisionRange").is_some()
                            && try_property!(entity, i32, "m_iNightTimeVisionRange").is_some()))
            })
        });
        let providers = self
            .provider_handles
            .iter()
            .filter_map(|handle| ctx.entities().get_by_handle(*handle as usize).ok())
            .filter_map(|entity| ActiveProvider::from_entity(entity, is_day))
            .collect::<Vec<_>>();
        let active_observers = self
            .provider_handles
            .iter()
            .filter(|handle| {
                ctx.entities()
                    .get_by_handle(**handle as usize)
                    .is_ok_and(|entity| entity.class().name() == "CDOTA_NPC_Observer_Ward")
            })
            .copied()
            .collect::<HashSet<_>>();
        let players = self.players.borrow();
        let game_time_seconds = start_time.map(|start| tick as f64 / 30.0 - start as f64);
        let mut observations = Vec::with_capacity(players.players.len());

        for player in &players.players {
            let player = player.borrow();
            let Ok(hero) = ctx.entities().get_by_handle(player.handle) else {
                observations.push((player.id, Observation::Unknown));
                continue;
            };

            let is_alive = is_alive(hero);
            let position = entity_position(hero);
            let enemy_team = if player.team == RADIANT { DIRE } else { RADIANT };
            let mut provider_kinds = BTreeSet::new();
            let mut provider_count = 0_u16;
            let mut ward_providers = Vec::new();
            let mut has_non_ward_provider = false;

            if is_alive && !self.smoked_heroes.contains(&player.id) {
                if let Some(target_position) = position {
                    for provider in providers.iter().filter(|provider| provider.team == enemy_team) {
                        if self.grid.can_see(provider.position, target_position, provider.range) {
                            provider_count = provider_count.saturating_add(1);
                            provider_kinds.insert(provider.kind.to_string());

                            if provider.kind == "observer_ward" {
                                ward_providers.push(provider.handle);
                            } else {
                                has_non_ward_provider = true;
                            }
                        }
                    }
                }
            }

            ward_providers.sort_unstable();
            let visibility_known =
                !is_alive || (providers_known && position.is_some_and(|position| self.grid.contains(position)));
            let observation = if !is_alive {
                Observation::Dead
            } else if !visibility_known {
                Observation::Unknown
            } else {
                Observation::Alive(Sight {
                    wards: ward_providers.clone(),
                    non_ward: has_non_ward_provider,
                })
            };
            observations.push((player.id, observation));

            if let Some(game_time_seconds) = game_time_seconds.filter(|_| self.collect_samples) {
                self.samples.push(VisionSample {
                    time: game_time_seconds as i32,
                    game_time_seconds,
                    target_player_steam_id: player.id,
                    target_is_radiant: player.team == RADIANT,
                    observing_is_radiant: enemy_team == RADIANT,
                    is_alive,
                    visible_by_enemy: provider_count > 0,
                    provider_count,
                    provider_kinds: provider_kinds.into_iter().collect(),
                    ward_provider_handles: ward_providers,
                    has_non_ward_provider,
                    visibility_known,
                });
            }
        }
        let has_gap = self
            .previous_sample_tick
            .is_some_and(|previous| tick < previous || tick - previous > SAMPLE_INTERVAL_TICKS);
        if has_gap || !providers_known || observations.iter().any(|(_, state)| matches!(state, Observation::Unknown)) {
            self.incomplete_wards.extend(&self.active_observers);
            self.incomplete_wards.extend(&active_observers);
        }
        self.previous_sample_tick = Some(tick);
        self.active_observers = active_observers;
        self.metrics.sample(tick as f64 / 30.0, observations);
    }

    fn is_day(&self, ctx: &Context) -> bool {
        let Ok(game_rules) = ctx.entities().get_by_class_name("CDOTAGamerulesProxy") else {
            return true;
        };

        if game_rules
            .get_property("m_pGameRules.m_bIsTemporaryDay")
            .is_ok_and(|x| x.bool())
        {
            return true;
        };

        if game_rules
            .get_property("m_pGameRules.m_bIsTemporaryNight")
            .is_ok_and(|x| x.bool())
        {
            return false;
        };

        if game_rules
            .get_property("m_pGameRules.m_bIsNightstalkerNight")
            .is_ok_and(|x| x.bool())
        {
            return false;
        };

        let Ok(FieldValue::Signed32(net_time)) = game_rules.get_property("m_pGameRules.m_iNetTimeOfDay") else {
            return false;
        };

        let daytime_start = try_property!(game_rules, f32, "m_pGameRules.m_flDaytimeStart").unwrap_or(0.25);
        let nighttime_start = try_property!(game_rules, f32, "m_pGameRules.m_flNighttimeStart").unwrap_or(0.75);
        let time_of_day = *net_time as f32 / u16::MAX as f32;

        time_of_day >= daytime_start && time_of_day < nighttime_start
    }
}

#[observer]
impl Vision {
    #[on_entity]
    fn on_entity(&mut self, event: EntityEvents, entity: &Entity) -> ObserverResult {
        if entity.class().name() == "CDOTA_NPC_Observer_Ward" && !self.players.borrow().init {
            self.incomplete_wards.insert(entity.handle());
        }
        if event == EntityEvents::Deleted {
            self.providers_changed |= self.provider_handles.remove(&entity.handle());
            return Ok(());
        }

        if is_provider_candidate(entity) && is_alive(entity) {
            self.providers_changed |= self.provider_handles.insert(entity.handle());
        } else {
            self.providers_changed |= self.provider_handles.remove(&entity.handle());
        }

        Ok(())
    }

    #[on_tick_end]
    fn on_tick_end(&mut self, ctx: &Context) -> ObserverResult {
        if !self.players.borrow().init {
            return Ok(());
        }

        let Ok(tick) = self.game_time.borrow().tick(ctx) else {
            return Ok(());
        };

        let start_time = self.game_time.borrow().start_time().ok();

        if !self.providers_changed && self.next_sample_tick.is_some_and(|next| tick < next) {
            return Ok(());
        }

        self.next_sample_tick = Some(tick + SAMPLE_INTERVAL_TICKS);
        self.providers_changed = false;
        self.sample(ctx, tick, start_time);

        Ok(())
    }

    #[on_stop]
    fn on_stop(&mut self, ctx: &Context) -> ObserverResult {
        self.providers_changed = true;
        self.on_tick_end(ctx)
    }

    #[on_combat_log]
    fn on_combat_log(&mut self, combat_log: &CombatLogEntry) -> ObserverResult {
        let event = combat_log.r#type();
        if event != DotaCombatlogTypes::DotaCombatlogModifierAdd && event != DotaCombatlogTypes::DotaCombatlogModifierRemove {
            return Ok(());
        }

        if combat_log.inflictor_name().ok() != Some("modifier_smoke_of_deceit") {
            return Ok(());
        }

        let Ok(target_name) = combat_log.target_name() else {
            return Ok(());
        };

        let Some(player) = self.players.borrow().hero_to_player.get(target_name).cloned() else {
            return Ok(());
        };

        let player_id = player.borrow().id;

        if event == DotaCombatlogTypes::DotaCombatlogModifierAdd {
            self.smoked_heroes.insert(player_id);
        } else {
            self.smoked_heroes.remove(&player_id);
        }
        self.providers_changed = true;

        Ok(())
    }
}

struct ActiveProvider<'a> {
    handle: u32,
    team: i32,
    range: i32,
    position: [f32; 3],
    kind: &'a str,
}

impl<'a> ActiveProvider<'a> {
    fn from_entity(entity: &'a Entity, is_day: bool) -> Option<Self> {
        if !is_alive(entity) {
            return None;
        }

        let team = try_property!(entity, i32, "m_iTeamNum")?;
        if team != RADIANT && team != DIRE {
            return None;
        }

        let class_name = entity.class().name();
        let range = if class_name == "CDOTA_BaseNPC_Watch_Tower" {
            WATCHER_VISION_RANGE
        } else if is_day {
            try_property!(entity, i32, "m_iDayTimeVisionRange")?
        } else {
            try_property!(entity, i32, "m_iNightTimeVisionRange")?
        };

        if range <= 0 {
            return None;
        }

        Some(Self {
            handle: entity.handle(),
            team,
            range,
            position: entity_position(entity)?,
            kind: provider_kind(class_name),
        })
    }
}

fn is_provider_candidate(entity: &Entity) -> bool {
    let team = try_property!(entity, i32, "m_iTeamNum");
    if team != Some(RADIANT) && team != Some(DIRE) {
        return false;
    }

    if entity.class().name() == "CDOTA_BaseNPC_Watch_Tower" {
        return true;
    }

    let day_range = try_property!(entity, i32, "m_iDayTimeVisionRange").unwrap_or(0);
    let night_range = try_property!(entity, i32, "m_iNightTimeVisionRange").unwrap_or(0);

    day_range > 0 || night_range > 0
}

fn is_alive(entity: &Entity) -> bool {
    try_property!(entity, i32, "m_lifeState").is_none_or(|state| state == 0)
        && try_property!(entity, i32, "m_iHealth").is_none_or(|health| health > 0)
}

fn entity_position(entity: &Entity) -> Option<[f32; 3]> {
    let cell_x = try_property!(entity, u16, "CBodyComponent.m_cellX")?;
    let cell_y = try_property!(entity, u16, "CBodyComponent.m_cellY")?;
    let cell_z = try_property!(entity, u16, "CBodyComponent.m_cellZ")?;
    let offset_x = try_property!(entity, f32, "CBodyComponent.m_vecX")?;
    let offset_y = try_property!(entity, f32, "CBodyComponent.m_vecY")?;
    let offset_z = try_property!(entity, f32, "CBodyComponent.m_vecZ")?;

    Some([
        cell_x as f32 * 128.0 + offset_x,
        cell_y as f32 * 128.0 + offset_y,
        cell_z as f32 * 128.0 + offset_z,
    ])
}

fn provider_kind(class_name: &str) -> &'static str {
    if class_name.starts_with("CDOTA_Unit_Hero_") {
        "hero"
    } else if matches!(class_name, "CDOTA_BaseNPC_Creep_Lane" | "CDOTA_BaseNPC_Creep_Siege") {
        "lane_creep"
    } else if class_name == "CDOTA_BaseNPC_Creep_Neutral" {
        "controlled_neutral"
    } else if class_name == "CDOTA_NPC_Observer_Ward" {
        "observer_ward"
    } else if class_name == "CDOTA_BaseNPC_Tower" {
        "tower"
    } else if class_name == "CDOTA_BaseNPC_Watch_Tower" {
        "watcher"
    } else if class_name == "CDOTA_Unit_Courier" {
        "courier"
    } else if class_name == "CDOTA_Unit_Roshans_Banner" {
        "roshan_banner"
    } else if matches!(
        class_name,
        "CDOTA_BaseNPC_Barracks"
            | "CDOTA_BaseNPC_Effigy_Statue"
            | "CDOTA_BaseNPC_Filler"
            | "CDOTA_BaseNPC_Fort"
            | "CDOTA_NPC_BaseBlocker"
            | "CDOTA_Unit_Fountain"
    ) {
        "building"
    } else {
        "controlled_unit"
    }
}

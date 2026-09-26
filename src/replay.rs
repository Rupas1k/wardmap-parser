use anyhow::{anyhow, bail};
use hashbrown::HashMap;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use source2_demo::prelude::*;

use crate::observers::game_time::*;
use crate::observers::players::*;
use crate::observers::vision::*;
use crate::observers::wards::*;
use crate::output::{Output, Replay, ReplayMetadata, ReplayPlayer};

#[derive(Debug, Copy, Clone)]
struct WardEntry {
    hero_handle: usize,
    placed_tick: i32,
    is_radiant: bool,
    is_observer: bool,
    radiant_networth: i32,
    dire_networth: i32,
}

#[derive(Debug, Clone)]
struct ResultEvidence {
    handle: u32,
    placed_at_seconds: f64,
    ended_at_seconds: f64,
    outcome: &'static str,
    outcome_reason: &'static str,
}

fn ended_at_expected_lifetime(entry: WardEntry, tick: i32) -> bool {
    let lifetime = if entry.is_observer { 360.0 } else { 420.0 };
    let age = (tick - entry.placed_tick) as f64 / 30.0;

    (age - lifetime).abs() <= 0.25
}

fn shift_time(value: &mut serde_json::Value, start: f64) {
    if let Some(time) = value.as_f64() {
        *value = (time - start).into();
    }
}

fn shift_sighting_times(sighting: &mut serde_json::Value, start: f64) {
    shift_time(&mut sighting["time"], start);

    let Some(segments) = sighting.get_mut("segments").and_then(serde_json::Value::as_array_mut) else {
        return;
    };

    for segment in segments {
        shift_time(&mut segment["time"], start);

        if let Some(route) = segment.get_mut("route").and_then(serde_json::Value::as_array_mut) {
            for point in route {
                shift_time(&mut point["time"], start);
            }
        }
    }
}

#[derive(Default)]
struct App {
    game_time: Rc<RefCell<GameTime>>,
    players: Rc<RefCell<Players>>,
    vision: Rc<RefCell<Vision>>,

    handle_to_entry: HashMap<u32, WardEntry>,
    pending_entries: VecDeque<(Entity, i32, WardEvent, bool)>,
    result: Vec<Output>,
    result_evidence: Vec<ResultEvidence>,
}

#[observer]
impl App {
    #[on_tick_end]
    fn tick_end(&mut self, ctx: &Context) -> ObserverResult {
        let Ok(start_time) = self.game_time.borrow().start_time() else {
            return Ok(());
        };

        while let Some((ward, tick, event, post_game)) = self.pending_entries.pop_front() {
            let handle = ward.handle();
            let entry = self
                .handle_to_entry
                .get(&handle)
                .copied()
                .ok_or_else(|| anyhow!("Ward handle {handle} has no placement entry"))?;
            let duration = (((tick - entry.placed_tick) as f32) / 30.0).round() as i32;
            let time_placed = (entry.placed_tick as f32 / 30.0 - start_time).round() as i32;
            let hero_placed = ctx.entities().get_by_handle(entry.hero_handle)?.class().name();

            let vision_metrics = self.vision.borrow().ward_metrics(handle);
            let player_destroyed_steam_id = match &event {
                WardEvent::Killed(killer) => self
                    .players
                    .borrow()
                    .hero_to_player
                    .get(killer)
                    .map(|player| player.borrow().id),
                _ => None,
            };

            let expected_expiry = ended_at_expected_lifetime(entry, tick);
            let (outcome, outcome_reason) = match &event {
                _ if expected_expiry => ("expired", "expected_lifetime"),
                WardEvent::Killed(killer) => self
                    .players
                    .borrow()
                    .hero_to_player
                    .get(killer)
                    .map(|player| player.borrow().team == if entry.is_radiant { 2 } else { 3 })
                    .map_or(("unknown", "unknown_killer"), |allied| {
                        if allied {
                            ("allied_removed", "combat_log")
                        } else {
                            ("dewarded", "combat_log")
                        }
                    }),
                WardEvent::Expired if post_game => ("match_ended", "match_end"),
                WardEvent::Expired => ("unknown", "entity_removal"),
                WardEvent::Placed => ("unknown", "entity_event"),
            };

            let npc_killed = match &event {
                WardEvent::Killed(killer) => Some(killer.to_string()),
                _ => None,
            };

            self.result.push(Output {
                time_placed,
                duration,
                is_obs: entry.is_observer,
                is_radiant: entry.is_radiant,
                event: event.to_string(),
                post_game,
                player_placed_steam_id: self
                    .players
                    .borrow()
                    .hero_to_player
                    .get(hero_placed)
                    .ok_or_else(|| anyhow!("Hero {hero_placed} has no player entry"))?
                    .borrow()
                    .id,
                player_destroyed_steam_id,
                npc_killed,
                x: property!(ward, "CBodyComponent.m_cellX"),
                y: property!(ward, "CBodyComponent.m_cellY"),
                z: property!(ward, "CBodyComponent.m_cellZ"),
                vec_x: property!(ward, "CBodyComponent.m_vecX"),
                vec_y: property!(ward, "CBodyComponent.m_vecY"),
                vec_z: property!(ward, "CBodyComponent.m_vecZ"),
                radiant_networth: entry.radiant_networth,
                dire_networth: entry.dire_networth,
                enemy_hero_vision_seconds: vision_metrics.enemy_hero_seconds as f32,
                unique_enemy_hero_vision_seconds: vision_metrics.unique_enemy_hero_seconds as f32,
                heroes_spotted: vision_metrics.heroes_spotted,
                hero_reveal_events: vision_metrics.hero_reveal_events,
                unique_hero_reveal_events: vision_metrics.unique_hero_reveal_events,
                scouting_tracking_seconds: None,
                scouting_discovery_seconds: None,
                scouting_score: None,
                scouting_version: None,
                scouting_tau_seconds: None,
                scouting_complete: None,
                measurement_json: None,
            });

            self.result_evidence.push(ResultEvidence {
                handle,
                placed_at_seconds: entry.placed_tick as f64 / 30.0 - start_time as f64,
                ended_at_seconds: tick as f64 / 30.0 - start_time as f64,
                outcome,
                outcome_reason,
            });
        }

        Ok(())
    }
}

impl WardsObserver for App {
    fn on_ward(
        &mut self,
        ctx: &Context,
        ward_class: WardClass,
        event: WardEvent,
        post_game: bool,
        ward: &Entity,
    ) -> ObserverResult {
        match event {
            WardEvent::Placed => self.track_placement(ctx, ward_class, ward)?,
            WardEvent::Killed(killer) => self.queue_removal(ctx, ward, WardEvent::Killed(killer), post_game)?,
            WardEvent::Expired => self.queue_removal(ctx, ward, WardEvent::Expired, post_game)?,
        }

        Ok(())
    }
}

impl App {
    fn track_placement(&mut self, ctx: &Context, ward_class: WardClass, ward: &Entity) -> ObserverResult {
        let owner_handle: usize = property!(ward, "m_hOwnerEntity");
        let owner = ctx.entities().get_by_handle(owner_handle)?;
        let mut player_slot: usize = if let Some(slot) = try_property!(owner, "m_nPlayerID") {
            slot
        } else if let Some(slot) = try_property!(owner, "m_iPlayerID") {
            slot
        } else {
            bail!("Couldn't get player slot from ward entity")
        };

        player_slot >>= 1;

        let player = self
            .players
            .borrow()
            .players
            .get(player_slot)
            .cloned()
            .ok_or_else(|| anyhow!("Player slot {player_slot} is unavailable"))?;
        let player = player.borrow();
        let radiant_networth: i32 = property!(
            ctx.entities().get_by_class_name("CDOTA_DataRadiant")?,
            "m_vecDataTeam.0002.m_iNetWorth"
        );
        let dire_networth: i32 = property!(
            ctx.entities().get_by_class_name("CDOTA_DataDire")?,
            "m_vecDataTeam.0003.m_iNetWorth"
        );

        self.handle_to_entry.insert(
            ward.handle(),
            WardEntry {
                hero_handle: player.handle,
                placed_tick: self.game_time.borrow().tick(ctx)?,
                is_radiant: player.team == 2,
                is_observer: ward_class == WardClass::Observer,
                radiant_networth,
                dire_networth,
            },
        );

        Ok(())
    }

    fn queue_removal(&mut self, ctx: &Context, ward: &Entity, event: WardEvent, post_game: bool) -> ObserverResult {
        self.pending_entries
            .push_back((ward.clone(), self.game_time.borrow().tick(ctx)?, event, post_game));

        Ok(())
    }
}

pub fn parse_replay(data: &[u8]) -> anyhow::Result<Replay> {
    let mut parser = Parser::new(data)?;

    let game_time = parser.register_observer::<GameTime>();
    let players = parser.register_observer::<Players>();
    let wards = parser.register_observer::<Wards>();
    let vision = parser.register_observer::<Vision>();
    let app = parser.register_observer::<App>();

    wards.borrow_mut().register_observer(app.clone());
    vision.borrow_mut().set_dependencies(game_time.clone(), players.clone());

    app.borrow_mut().game_time = game_time;
    app.borrow_mut().players = players;
    app.borrow_mut().vision = vision;

    parser.run_to_end()?;
    app.borrow_mut().tick_end(parser.context())?;

    let metadata = replay_metadata(parser.replay_info(), parser.context());

    let app = app.borrow();
    let vision = app.vision.borrow();
    let mut result = app.result.clone();
    for (output, evidence) in result.iter_mut().zip(&app.result_evidence) {
        let metrics = vision.ward_metrics(evidence.handle);
        output.enemy_hero_vision_seconds = metrics.enemy_hero_seconds as f32;
        output.unique_enemy_hero_vision_seconds = metrics.unique_enemy_hero_seconds as f32;
        output.heroes_spotted = metrics.heroes_spotted;
        output.hero_reveal_events = metrics.hero_reveal_events;
        output.unique_hero_reveal_events = metrics.unique_hero_reveal_events;
        if output.is_obs {
            output.scouting_tracking_seconds = Some(metrics.scouting_tracking_seconds);
            output.scouting_discovery_seconds = Some(metrics.scouting_discovery_seconds);
            output.scouting_score = Some(metrics.scouting_score());
            output.scouting_version = Some(SCOUTING_VERSION);
            output.scouting_tau_seconds = Some(DISCOVERY_TAU_SECONDS);
            output.scouting_complete = Some(vision.scouting_complete(evidence.handle));
        }

        let mut measurement = serde_json::to_value(vision.measurement(evidence.handle))?;
        let object = measurement
            .as_object_mut()
            .ok_or_else(|| anyhow!("Ward measurement did not serialize as an object"))?;
        object.insert("revision".to_owned(), 2.into());
        if let Some(intervals) = object.get_mut("intervals").and_then(serde_json::Value::as_array_mut) {
            let start = app.game_time.borrow().start_time()? as f64;
            for interval in intervals {
                for key in ["start", "end"] {
                    if let Some(time) = interval[key].as_f64() {
                        interval[key] = (time - start).into();
                    }
                }
            }
        }
        if let Some(sightings) = object.get_mut("sightings").and_then(serde_json::Value::as_array_mut) {
            let start = app.game_time.borrow().start_time()? as f64;
            for sighting in sightings {
                shift_sighting_times(sighting, start);
            }
        }
        object.insert("placed_at_seconds".to_owned(), evidence.placed_at_seconds.into());
        let ended_at = metadata
            .game_duration_seconds
            .map(f64::from)
            .map_or(evidence.ended_at_seconds, |game_end| evidence.ended_at_seconds.min(game_end));
        object.insert("ended_at_seconds".to_owned(), ended_at.into());
        if output.is_obs {
            let eligible = (ended_at - evidence.placed_at_seconds).max(0.0) * 5.0;
            let known = object
                .get("vision_measured_seconds")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0)
                .min(eligible);
            object.insert("vision_possible_seconds".to_owned(), eligible.into());
            object.insert("vision_measured_seconds".to_owned(), known.into());
            object.insert(
                "vision_coverage".to_owned(),
                if eligible > 0.0 {
                    (known / eligible).into()
                } else {
                    serde_json::Value::Null
                },
            );
        }
        let ambiguous = app.result.iter().zip(&app.result_evidence).any(|(other, other_evidence)| {
            other_evidence.handle != evidence.handle
                && other.is_obs == output.is_obs
                && other.is_radiant == output.is_radiant
                && (other_evidence.ended_at_seconds - evidence.ended_at_seconds).abs() <= 0.5
        });
        let (outcome, outcome_reason) = if metadata.game_duration_seconds.is_none() && evidence.outcome == "match_ended" {
            ("replay_ended", "replay_end")
        } else if metadata
            .game_duration_seconds
            .is_some_and(|game_end| evidence.ended_at_seconds > f64::from(game_end))
        {
            ("match_ended", "match_end")
        } else if ambiguous && matches!(evidence.outcome, "dewarded" | "allied_removed") {
            ("unknown", "ambiguous_removal")
        } else {
            (evidence.outcome, evidence.outcome_reason)
        };
        object.insert("outcome".to_owned(), outcome.into());
        object.insert("outcome_reason".to_owned(), outcome_reason.into());
        object.insert(
            "vision_complete".to_owned(),
            (output.is_obs
                && object
                    .get("vision_coverage")
                    .and_then(serde_json::Value::as_f64)
                    .is_some_and(|coverage| coverage >= 1.0 - 1e-9))
            .into(),
        );
        if !output.is_obs {
            object.insert("added_vision_seconds".to_owned(), serde_json::Value::Null);
            object.insert("fresh_sightings".to_owned(), serde_json::Value::Null);
            object.insert("vision_coverage".to_owned(), serde_json::Value::Null);
        }
        output.measurement_json = Some(serde_json::to_string(&measurement)?);
    }

    Ok(Replay { metadata, wards: result })
}

#[cfg(test)]
mod outcome_tests {
    use super::*;

    fn ward_entry(is_observer: bool) -> WardEntry {
        WardEntry {
            hero_handle: 0,
            placed_tick: 1_000,
            is_radiant: true,
            is_observer,
            radiant_networth: 0,
            dire_networth: 0,
        }
    }

    #[test]
    fn recognizes_expected_observer_lifetime() {
        let entry = ward_entry(true);

        assert!(ended_at_expected_lifetime(entry, 1_000 + 360 * 30));
        assert!(!ended_at_expected_lifetime(entry, 1_000 + 359 * 30));
    }

    #[test]
    fn recognizes_expected_sentry_lifetime() {
        let entry = ward_entry(false);

        assert!(ended_at_expected_lifetime(entry, 1_000 + 420 * 30));
        assert!(!ended_at_expected_lifetime(entry, 1_000 + 419 * 30));
    }

    #[test]
    fn shifts_all_sighting_times_to_game_time() {
        let mut sighting = serde_json::json!({
            "time": 1000.0,
            "segments": [{
                "time": 1000.0,
                "route": [
                    {"time": 1000.0},
                    {"time": 1001.5}
                ]
            }]
        });

        shift_sighting_times(&mut sighting, 900.0);

        assert_eq!(sighting["time"], 100.0);
        assert_eq!(sighting["segments"][0]["time"], 100.0);
        assert_eq!(sighting["segments"][0]["route"][0]["time"], 100.0);
        assert_eq!(sighting["segments"][0]["route"][1]["time"], 101.5);
    }
}

fn replay_metadata(info: &source2_demo::proto::CDemoFileInfo, ctx: &Context) -> ReplayMetadata {
    let game = info.game_info.as_ref().and_then(|info| info.dota.as_ref());
    let game_duration_seconds = ctx
        .entities()
        .get_by_class_name("CDOTAGamerulesProxy")
        .ok()
        .and_then(|rules| {
            let start: f32 = try_property!(rules, "m_pGameRules.m_flGameStartTime")?;
            let end: f32 = try_property!(rules, "m_pGameRules.m_flGameEndTime")?;

            (start.is_finite() && end.is_finite() && end > 0.0 && end >= start).then_some(end - start)
        });
    let players = game
        .into_iter()
        .flat_map(|game| &game.player_info)
        .map(|player| ReplayPlayer {
            steam_id: player.steamid,
            name: player
                .player_name
                .as_deref()
                .map(|name| String::from_utf8_lossy(name).trim_end_matches('\0').to_owned()),
            hero_name: player.hero_name.clone(),
            team: player.game_team,
            is_fake_client: player.is_fake_client,
        })
        .collect();

    ReplayMetadata {
        playback_time_seconds: info.playback_time,
        game_duration_seconds,
        playback_ticks: info.playback_ticks,
        playback_frames: info.playback_frames,
        match_id: game.and_then(|game| game.match_id),
        game_mode: game.and_then(|game| game.game_mode),
        game_winner: game.and_then(|game| game.game_winner),
        league_id: game.and_then(|game| game.leagueid),
        radiant_team_id: game.and_then(|game| game.radiant_team_id),
        dire_team_id: game.and_then(|game| game.dire_team_id),
        radiant_team_tag: game.and_then(|game| game.radiant_team_tag.clone()),
        dire_team_tag: game.and_then(|game| game.dire_team_tag.clone()),
        end_time: game.and_then(|game| game.end_time),
        players,
    }
}

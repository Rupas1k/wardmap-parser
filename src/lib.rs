use anyhow::bail;
use hashbrown::HashMap;
use pyo3::prelude::*;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use source2_demo::prelude::*;

use crate::observers::game_time::*;
use crate::observers::players::*;
use crate::observers::wards::*;

mod observers;

pub mod utils;

#[derive(Debug, Copy, Clone)]
pub struct WardEntry {
    pub hero_handle: usize,
    pub placed_tick: i32,
    pub is_radiant: bool,
    pub is_observer: bool,
}

#[pyclass(get_all, set_all)]
#[derive(Clone)]
pub struct Output {
    pub time_placed: i32,
    pub duration: i32,
    pub is_obs: bool,
    pub is_radiant: bool,
    pub event: String,
    pub post_game: bool,
    pub player_placed_steam_id: u64,
    pub player_destroyed_steam_id: Option<u64>,
    pub npc_killed: Option<String>,
    pub x: u16,
    pub y: u16,
    pub z: u16,
    pub vec_x: f32,
    pub vec_y: f32,
    pub vec_z: f32,
    pub radiant_networth: i32,
    pub dire_networth: i32,
}

#[derive(Default)]
struct App {
    game_time: Rc<RefCell<GameTime>>,
    players: Rc<RefCell<Players>>,

    handle_to_entry: HashMap<u32, WardEntry>,
    pending_entries: VecDeque<(Entity, i32, WardEvent)>,
    result: Vec<Output>,
}

#[observer]
impl App {
    #[on_tick_end]
    fn tick_end(&mut self, ctx: &Context) -> ObserverResult {
        if let Ok(start_time) = self.game_time.borrow().start_time() {
            while let Some((ward, tick, event)) = self.pending_entries.pop_front() {
                let handle = ward.handle();
                let output = Output {
                    time_placed: (self.handle_to_entry[&handle].placed_tick as f32 / 30.0 - start_time) as i32,
                    duration: (((tick - self.handle_to_entry[&handle].placed_tick) as f32) / 30.0) as i32,
                    is_obs: self.handle_to_entry[&handle].is_observer,
                    is_radiant: self.handle_to_entry[&handle].is_radiant,
                    event: match event {
                        WardEvent::Killed(_) => "killed".to_string(),
                        WardEvent::Expired => "expired".to_string(),
                        _ => unreachable!(),
                    },
                    post_game: false,
                    player_placed_steam_id: self.players.borrow().hero_to_player[ctx.entities().get_by_handle(self.handle_to_entry[&handle].hero_handle)?.class().name()].borrow().id,
                    player_destroyed_steam_id: if let WardEvent::Killed(killer) = &event {
                        self.players.borrow().hero_to_player.get(killer).map(|x| x.borrow().id)
                    } else {
                        None
                    },
                    npc_killed: if let WardEvent::Killed(killer) = &event {
                        Some(killer.to_string())
                    } else {
                        None
                    },
                    x: property!(ward, "CBodyComponent.m_cellX"),
                    y: property!(ward, "CBodyComponent.m_cellY"),
                    z: property!(ward, "CBodyComponent.m_cellZ"),
                    vec_x: property!(ward, "CBodyComponent.m_vecX"),
                    vec_y: property!(ward, "CBodyComponent.m_vecY"),
                    vec_z: property!(ward, "CBodyComponent.m_vecZ"),
                    radiant_networth: property!(
                        ctx.entities().get_by_class_name("CDOTA_DataRadiant")?,
                        "m_vecDataTeam.0002.m_iNetWorth"
                    ),
                    dire_networth: property!(
                        ctx.entities().get_by_class_name("CDOTA_DataDire")?,
                        "m_vecDataTeam.0003.m_iNetWorth"
                    ),
                };
                self.result.push(output);
            }
        }
        Ok(())
    }
}

impl WardsObserver for App {
    fn on_ward(&mut self, ctx: &Context, ward_class: WardClass, event: WardEvent, ward: &Entity) -> ObserverResult {
        match event {
            WardEvent::Placed => {
                let owner_handle: usize = property!(ward, "m_hOwnerEntity");
                let owner = ctx.entities().get_by_handle(owner_handle)?;
                let mut player_slot: usize = if let Some(x) = try_property!(owner, "m_nPlayerID") {
                    x
                } else if let Some(x) = try_property!(owner, "m_iPlayerID") {
                    x
                } else {
                    bail!("Couldn't get player slot from ward entity")
                };
                player_slot >>= 1;

                let player = &self.players.borrow().players[player_slot];
                let hero_handle = player.borrow().handle;

                self.handle_to_entry.insert(
                    ward.handle(),
                    WardEntry {
                        hero_handle,
                        placed_tick: self.game_time.borrow().tick(ctx)?,
                        is_radiant: player.borrow().team == 2,
                        is_observer: ward_class == WardClass::Observer,
                    },
                );
            }
            WardEvent::Killed(killer) => {
                self.pending_entries.push_back((
                    ward.clone(),
                    self.game_time.borrow().tick(ctx)?,
                    WardEvent::Killed(killer),
                ));
            }
            WardEvent::Expired => {
                self.pending_entries
                    .push_back((ward.clone(), self.game_time.borrow().tick(ctx)?, WardEvent::Expired));
            }
        }
        Ok(())
    }
}

#[pyfunction]
pub fn parse_replay(data: &[u8]) -> PyResult<Vec<Output>> {
    std::panic::catch_unwind(|| {
        let mut parser = Parser::new(data)?;

        let game_time = parser.register_observer::<GameTime>();
        let players = parser.register_observer::<Players>();
        let wards = parser.register_observer::<Wards>();
        let app = parser.register_observer::<App>();

        wards.borrow_mut().register_observer(app.clone());

        app.borrow_mut().game_time = game_time;
        app.borrow_mut().players = players;

        parser.run_to_end()?;

        app.borrow_mut().tick_end(parser.context())?;

        let x = Ok(app.borrow_mut().result.clone());
        x
    })
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyException, _>(format!("Panic while parsing\n{e:?}")))
        .and_then(|x| {
            x.map_err(|e: anyhow::Error| {
                PyErr::new::<pyo3::exceptions::PyException, _>(format!("Error while parsing\n{e}"))
            })
        })
}

#[pymodule]
#[pyo3(name = "d2wm_parser")]
fn d2wm_parser(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Output>()?;
    module.add_function(wrap_pyfunction!(parse_replay, module)?)
}

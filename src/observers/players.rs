use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use source2_demo::prelude::*;
use source2_demo::proto::*;

use crate::utils::class_to_combat_log;

#[derive(Debug)]
#[allow(dead_code)]
pub struct Player {
    pub id: u64,
    pub team: i32,
    pub slot: i32,
    pub hero: Box<str>,
    pub handle: usize,
    pub controller_handle: usize,
}

#[derive(Default)]
pub struct Players {
    pub init: bool,
    pub players: Vec<Rc<RefCell<Player>>>,
    pub steam_id_to_player: HashMap<u64, Rc<RefCell<Player>>>,
    pub hero_to_player: HashMap<Box<str>, Rc<RefCell<Player>>>,
    pub owner_to_hero: HashMap<usize, usize>,

    is_pre_game: bool,
    pre_game_tick: Option<u32>,
}

#[observer]
impl Players {
    #[on_tick_start]
    fn on_tick_start(&mut self, ctx: &Context) -> ObserverResult {
        if !self.init && self.is_pre_game && self.pre_game_tick.is_some() && (self.pre_game_tick.unwrap() + 30) < ctx.tick() {
            if let Ok(pr) = ctx.entities().get_by_class_name("CDOTA_PlayerResource") {
                let mut added = 0;
                while added < 10 {
                    let id: u64 = property!(pr, "m_vecPlayerData.{added:04}.m_iPlayerSteamID");
                    let team: i32 = property!(pr, "m_vecPlayerData.{added:04}.m_iPlayerTeam");
                    let slot: i32 = property!(pr, "m_vecPlayerTeamData.{added:04}.m_iTeamSlot");
                    let handle: usize = property!(pr, "m_vecPlayerTeamData.{added:04}.m_hSelectedHero");

                    let controller_handle: usize = property!(ctx.entities().get_by_handle(handle)?, "m_hOwnerEntity");

                    let hero_str = ctx
                        .entities()
                        .get_by_handle(handle)?
                        .class()
                        .name()
                        .to_string()
                        .into_boxed_str();

                    let player = Rc::new(RefCell::new(Player {
                        id,
                        team,
                        slot,
                        hero: hero_str.clone(),
                        handle,
                        controller_handle,
                    }));

                    self.players.push(player.clone());

                    self.hero_to_player.insert(hero_str.clone(), player.clone());
                    for name in class_to_combat_log(&hero_str) {
                        self.hero_to_player.insert(name, player.clone());
                    }

                    self.steam_id_to_player.insert(id, player.clone());
                    self.owner_to_hero.insert(controller_handle, handle);

                    added += 1;
                }

                self.init = true;
            }
        }
        Ok(())
    }

    #[on_combat_log]
    fn on_combat_log(&mut self, ctx: &Context, combat_log: &CombatLogEntry) -> ObserverResult {
        if !self.is_pre_game
            && combat_log.r#type() == DotaCombatlogTypes::DotaCombatlogGameState
            && combat_log.value().unwrap() == DotaGameState::DotaGamerulesStatePreGame as u32
        {
            self.is_pre_game = true;
            self.pre_game_tick = Some(ctx.tick());
        }
        Ok(())
    }
}

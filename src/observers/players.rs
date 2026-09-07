use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use source2_demo::prelude::*;
use source2_demo::proto::*;

use crate::utils::class_to_combat_log;

#[derive(Debug)]
pub struct Player {
    pub id: u64,
    pub team: i32,
    pub handle: usize,
}

#[derive(Default)]
pub struct Players {
    pub init: bool,
    pub players: Vec<Rc<RefCell<Player>>>,
    pub hero_to_player: HashMap<Box<str>, Rc<RefCell<Player>>>,

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
                    let handle: usize = property!(pr, "m_vecPlayerTeamData.{added:04}.m_hSelectedHero");

                    let hero_str = ctx
                        .entities()
                        .get_by_handle(handle)?
                        .class()
                        .name()
                        .to_string()
                        .into_boxed_str();

                    let player = Rc::new(RefCell::new(Player { id, team, handle }));

                    self.players.push(player.clone());

                    self.hero_to_player.insert(hero_str.clone(), player.clone());
                    for name in class_to_combat_log(&hero_str) {
                        self.hero_to_player.insert(name, player.clone());
                    }

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

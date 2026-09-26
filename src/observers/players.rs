use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use source2_demo::prelude::*;
use source2_demo::proto::*;

const PLAYER_SLOTS: usize = 10;
const INITIALIZATION_DELAY_TICKS: u32 = 30;

#[derive(Debug)]
pub struct Player {
    pub id: u64,
    pub slot: u8,
    pub team: i32,
    pub handle: usize,
}

fn class_to_combat_log(class: &str) -> HashSet<Box<str>> {
    let hero_name = class.strip_prefix("CDOTA_Unit_Hero_").unwrap_or(class);
    let direct_name = format!("npc_dota_hero_{}", hero_name.to_lowercase());
    let snake_case = hero_name
        .chars()
        .enumerate()
        .flat_map(|(index, character)| {
            let separator = (index > 0 && character.is_ascii_uppercase()).then_some('_');
            separator.into_iter().chain(character.to_lowercase())
        })
        .collect::<String>();
    let snake_case_name = format!("npc_dota_hero_{snake_case}");

    HashSet::from([direct_name.into_boxed_str(), snake_case_name.into_boxed_str()])
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
        let Some(pre_game_tick) = self.pre_game_tick else {
            return Ok(());
        };

        if self.init {
            return Ok(());
        }

        if !self.is_pre_game {
            return Ok(());
        }

        if ctx.tick() <= pre_game_tick.saturating_add(INITIALIZATION_DELAY_TICKS) {
            return Ok(());
        }

        let Ok(player_resource) = ctx.entities().get_by_class_name("CDOTA_PlayerResource") else {
            return Ok(());
        };

        let mut loaded_players = Vec::with_capacity(PLAYER_SLOTS);

        for slot in 0..PLAYER_SLOTS {
            let id: u64 = property!(player_resource, "m_vecPlayerData.{slot:04}.m_iPlayerSteamID");
            let team: i32 = property!(player_resource, "m_vecPlayerData.{slot:04}.m_iPlayerTeam");
            let handle: usize = property!(player_resource, "m_vecPlayerTeamData.{slot:04}.m_hSelectedHero");
            let hero = ctx.entities().get_by_handle(handle)?;
            let hero_name = hero.class().name().to_owned().into_boxed_str();

            loaded_players.push((slot, id, team, handle, hero_name));
        }

        for (slot, id, team, handle, hero_name) in loaded_players {
            let player = Rc::new(RefCell::new(Player {
                id,
                slot: slot as u8,
                team,
                handle,
            }));

            self.players.push(Rc::clone(&player));

            self.hero_to_player.insert(hero_name.clone(), Rc::clone(&player));
            for name in class_to_combat_log(&hero_name) {
                self.hero_to_player.insert(name, Rc::clone(&player));
            }
        }

        self.init = true;
        Ok(())
    }

    #[on_combat_log]
    fn on_combat_log(&mut self, ctx: &Context, combat_log: &CombatLogEntry) -> ObserverResult {
        if self.is_pre_game || combat_log.r#type() != DotaCombatlogTypes::DotaCombatlogGameState {
            return Ok(());
        }

        if combat_log
            .value()
            .is_ok_and(|value| value == DotaGameState::DotaGamerulesStatePreGame as u32)
        {
            self.is_pre_game = true;
            self.pre_game_tick = Some(ctx.tick());
        }

        Ok(())
    }
}

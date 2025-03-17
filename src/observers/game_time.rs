use anyhow::{anyhow, bail, Result};

use source2_demo::prelude::*;

#[derive(Default)]
pub struct GameTime {
    start_time: Option<f32>,
}

#[observer]
impl GameTime {
    pub fn start_time(&self) -> Result<f32> {
        self.start_time
            .ok_or_else(|| anyhow!("Game has not started yet."))
    }

    pub fn tick(&self, ctx: &Context) -> Result<i32> {
        if let Ok(game_rules) = ctx.entities().get_by_class_name("CDOTAGamerulesProxy") {
            let is_paused: bool = property!(game_rules, "m_pGameRules.m_bGamePaused");
            let time_tick: i32 = match is_paused {
                true => property!(game_rules, "m_pGameRules.m_nPauseStartTick"),
                false => ctx.net_tick() as i32,
            };
            let paused_ticks: i32 = property!(game_rules, "m_pGameRules.m_nTotalPausedTicks");
            return Ok(time_tick - paused_ticks);
        }
        bail!("No CDOTAGamerulesProxy.")
    }

    #[on_tick_start]
    fn on_tick_start(&mut self, ctx: &Context) -> ObserverResult {
        if self.start_time.is_none() {
            if let Ok(game_rules) = ctx.entities().get_by_class_name("CDOTAGamerulesProxy") {
                let start_time: f32 = property!(game_rules, "m_pGameRules.m_flGameStartTime");
                if start_time > 0.0 {
                    self.start_time = Some(start_time);
                }
            }
        }
        Ok(())
    }
}
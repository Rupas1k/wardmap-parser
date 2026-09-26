#[derive(Clone, Debug)]
pub struct Replay {
    pub metadata: ReplayMetadata,
    pub wards: Vec<Output>,
}

#[derive(Clone, Debug)]
pub struct ReplayMetadata {
    pub playback_time_seconds: Option<f32>,
    pub game_duration_seconds: Option<f32>,
    pub playback_ticks: Option<i32>,
    pub playback_frames: Option<i32>,
    pub match_id: Option<u64>,
    pub game_mode: Option<i32>,
    pub game_winner: Option<i32>,
    pub league_id: Option<u32>,
    pub radiant_team_id: Option<u32>,
    pub dire_team_id: Option<u32>,
    pub radiant_team_tag: Option<String>,
    pub dire_team_tag: Option<String>,
    pub end_time: Option<u32>,
    pub players: Vec<ReplayPlayer>,
}

#[derive(Clone, Debug)]
pub struct ReplayPlayer {
    pub steam_id: Option<u64>,
    pub name: Option<String>,
    pub hero_name: Option<String>,
    pub team: Option<i32>,
    pub is_fake_client: Option<bool>,
}

#[derive(Clone, Debug)]
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
    pub enemy_hero_vision_seconds: f32,
    pub unique_enemy_hero_vision_seconds: f32,
    pub heroes_spotted: u8,
    pub hero_reveal_events: u16,
    pub unique_hero_reveal_events: u16,
    pub scouting_tracking_seconds: Option<f64>,
    pub scouting_discovery_seconds: Option<f64>,
    pub scouting_score: Option<f64>,
    pub scouting_version: Option<i16>,
    pub scouting_tau_seconds: Option<f64>,
    pub scouting_complete: Option<bool>,
    pub measurement_json: Option<String>,
}

const GRID_SIZE: f32 = 64.0;
const NETWORK_COORDINATE_BIAS: f32 = 128.0 * 128.0;
const EXPORTED_WORLD_MIN: f32 = -8923.0;
const GRID_WORLD_ORIGIN: f32 = NETWORK_COORDINATE_BIAS + EXPORTED_WORLD_MIN;
const WORLD_Z_ORIGIN: f32 = NETWORK_COORDINATE_BIAS;

pub const CURRENT_MAP_VERSION: u8 = 2;

#[derive(Debug, Clone, Copy)]
pub struct MapData {
    version: u8,
    elevations: &'static [u8],
}

impl MapData {
    const fn new(version: u8, elevations: &'static [u8]) -> Self {
        Self { version, elevations }
    }

    pub const fn version(self) -> u8 {
        self.version
    }

    pub const fn elevations(self) -> &'static [u8] {
        self.elevations
    }

    pub const fn grid_size(self) -> f32 {
        GRID_SIZE
    }

    pub const fn world_origin(self) -> f32 {
        GRID_WORLD_ORIGIN
    }

    pub const fn z_origin(self) -> f32 {
        WORLD_Z_ORIGIN
    }
}

#[cfg(feature = "map-0")]
const MAP_0: MapData = MapData::new(0, include_bytes!("../assets/elevations/0.bin"));
#[cfg(feature = "map-1")]
const MAP_1: MapData = MapData::new(1, include_bytes!("../assets/elevations/1.bin"));
#[cfg(feature = "map-2")]
const MAP_2: MapData = MapData::new(2, include_bytes!("../assets/elevations/2.bin"));

pub fn map_data(version: u8) -> Option<MapData> {
    match version {
        #[cfg(feature = "map-0")]
        0 => Some(MAP_0),
        #[cfg(feature = "map-1")]
        1 => Some(MAP_1),
        #[cfg(feature = "map-2")]
        2 => Some(MAP_2),
        _ => None,
    }
}

pub fn elevation_data(version: u8) -> Option<&'static [u8]> {
    map_data(version).map(MapData::elevations)
}

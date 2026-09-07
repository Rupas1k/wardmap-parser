const GRID_SIZE: f32 = 64.0;
const NETWORK_COORDINATE_BIAS: f32 = 128.0 * 128.0;
const EXPORTED_WORLD_MIN: f32 = -8923.0;
const GRID_WORLD_ORIGIN: f32 = NETWORK_COORDINATE_BIAS + EXPORTED_WORLD_MIN;
const WORLD_Z_ORIGIN: f32 = NETWORK_COORDINATE_BIAS;

const MAP_DATA: &[u8] = include_bytes!("../../../assets/elevations/2.bin");

pub struct VisionGrid {
    rows: usize,
    columns: usize,
    cells: Vec<i16>,
}

impl Default for VisionGrid {
    fn default() -> Self {
        let rows = u16::from_be_bytes([MAP_DATA[0], MAP_DATA[1]]) as usize;
        let columns = u16::from_be_bytes([MAP_DATA[2], MAP_DATA[3]]) as usize;
        let cells = MAP_DATA[4..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|bytes| i16::from_be_bytes(*bytes))
            .collect::<Vec<_>>();

        debug_assert_eq!(cells.len(), rows * columns);

        Self { rows, columns, cells }
    }
}

impl VisionGrid {
    pub fn contains(&self, position: [f32; 3]) -> bool {
        if !position.iter().all(|coordinate| coordinate.is_finite()) {
            return false;
        }
        let column = ((position[0] - GRID_WORLD_ORIGIN) / GRID_SIZE).round() as i32;
        let row = ((position[1] - GRID_WORLD_ORIGIN) / GRID_SIZE).round() as i32;
        self.cell(column, row).is_some()
    }

    pub fn can_see(&self, source: [f32; 3], target: [f32; 3], radius: i32) -> bool {
        if radius <= 0 {
            return false;
        }

        let source_x = source[0] - GRID_WORLD_ORIGIN;
        let source_y = source[1] - GRID_WORLD_ORIGIN;
        let target_x = target[0] - GRID_WORLD_ORIGIN;
        let target_y = target[1] - GRID_WORLD_ORIGIN;
        let source_column = (source_x / GRID_SIZE).round() as i32;
        let source_row = (source_y / GRID_SIZE).round() as i32;
        let target_column = (target_x / GRID_SIZE).round() as i32;
        let target_row = (target_y / GRID_SIZE).round() as i32;

        if self.cell(source_column, source_row).is_none() || self.cell(target_column, target_row).is_none() {
            return false;
        }

        let delta_x = (target_column - source_column) as f32 * GRID_SIZE;
        let delta_y = (target_row - source_row) as f32 * GRID_SIZE;
        if delta_x.hypot(delta_y) > radius as f32 {
            return false;
        }

        self.has_line_of_sight(
            source_column,
            source_row,
            target_column,
            target_row,
            source[2] - WORLD_Z_ORIGIN,
        )
    }

    fn has_line_of_sight(
        &self,
        origin_column: i32,
        origin_row: i32,
        target_column: i32,
        target_row: i32,
        eye_height: f32,
    ) -> bool {
        let delta_column = target_column - origin_column;
        let delta_row = target_row - origin_row;
        let column_steps = delta_column.abs();
        let row_steps = delta_row.abs();
        let column_direction = delta_column.signum();
        let row_direction = delta_row.signum();
        let mut column = origin_column;
        let mut row = origin_row;
        let mut completed_columns = 0;
        let mut completed_rows = 0;

        while completed_columns < column_steps || completed_rows < row_steps {
            let decision = (1 + 2 * completed_columns) * row_steps - (1 + 2 * completed_rows) * column_steps;

            if decision == 0 {
                if !self.cell_is_clear(column + column_direction, row, eye_height)
                    || !self.cell_is_clear(column, row + row_direction, eye_height)
                {
                    return false;
                }

                column += column_direction;
                row += row_direction;
                completed_columns += 1;
                completed_rows += 1;
            } else if decision < 0 {
                column += column_direction;
                completed_columns += 1;
            } else {
                row += row_direction;
                completed_rows += 1;
            }

            if !self.cell_is_clear(column, row, eye_height) {
                return false;
            }
        }

        true
    }

    fn cell_is_clear(&self, column: i32, row: i32, eye_height: f32) -> bool {
        self.cell(column, row)
            .is_some_and(|value| !cell_blocks_vision(value, eye_height))
    }

    fn cell(&self, column: i32, row: i32) -> Option<i16> {
        let row = usize::try_from(row).ok()?;
        let column = usize::try_from(column).ok()?;

        if row >= self.rows || column >= self.columns {
            return None;
        }

        self.cells.get(row * self.columns + column).copied()
    }
}

fn cell_blocks_vision(value: i16, eye_height: f32) -> bool {
    let cell_height = value >> 1;
    let has_tree = value & 1 == 1;
    let high_ground_blocks = cell_height as f32 > eye_height + 64.0;
    let tree_canopy_blocks = has_tree && cell_height as f32 + 160.0 > eye_height;

    high_ground_blocks || tree_canopy_blocks
}

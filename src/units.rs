use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Meters(pub f64);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SquareMeters(pub f64);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SquareKilometers(pub f64);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Celsius(pub f64);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Degrees(pub f64);

impl Meters {
    pub fn to_cells(self, cell_size_m: f64) -> u32 {
        if cell_size_m <= 0.0 {
            return 0;
        }
        (self.0 / cell_size_m).round().max(0.0) as u32
    }

    pub fn to_cells_usize(self, cell_size_m: f64) -> usize {
        self.to_cells(cell_size_m) as usize
    }
}

impl SquareMeters {
    pub fn to_cell_count(self, cell_size_m: f64) -> usize {
        if cell_size_m <= 0.0 {
            return 0;
        }
        let area = cell_size_m * cell_size_m;
        (self.0 / area).round().max(0.0) as usize
    }
}

impl SquareKilometers {
    pub fn to_square_meters(self) -> SquareMeters {
        SquareMeters(self.0 * 1_000_000.0)
    }

    pub fn to_cell_count(self, cell_size_m: f64) -> usize {
        self.to_square_meters().to_cell_count(cell_size_m)
    }
}

impl Degrees {
    pub fn to_radians(self) -> f64 {
        self.0.to_radians()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meters_to_cells_default_grid() {
        assert_eq!(Meters(200.0).to_cells(20.0), 10);
        assert_eq!(Meters(120.0).to_cells(20.0), 6);
        assert_eq!(Meters(100.0).to_cells(20.0), 5);
    }

    #[test]
    fn area_to_cells_default_grid() {
        assert_eq!(SquareMeters(9600.0).to_cell_count(20.0), 24);
        assert_eq!(SquareKilometers(0.01536).to_cell_count(20.0), 38);
    }
}

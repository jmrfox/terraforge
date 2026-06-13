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

/// Map extent wavelength to noise frequency (cycles per normalized map width).
pub fn wavelength_to_frequency(map_extent_m: f64, wavelength_m: Meters) -> f64 {
    if wavelength_m.0 <= 0.0 {
        return 1.0;
    }
    map_extent_m / wavelength_m.0
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

    #[test]
    fn wavelength_frequency_calibration() {
        let map_w = 512.0 * 20.0;
        assert!((wavelength_to_frequency(map_w, Meters(5120.0)) - 2.0).abs() < 0.01);
        assert!((wavelength_to_frequency(map_w, Meters(1024.0)) - 10.0).abs() < 0.01);
    }
}

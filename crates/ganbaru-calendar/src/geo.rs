//! Event coordinates.
//!
//! Calendar stores a latitude and longitude as integer millionths of a degree, which is finer
//! than any iCalendar GEO value needs and keeps replicated values free of floating point.
//! Commands and the native projection carry degrees as JSON numbers; this module is the single
//! conversion between the two.

use serde::Deserialize;

/// Stored units per degree of latitude or longitude.
pub const GEO_UNITS_PER_DEGREE: i64 = 1_000_000;

/// Largest stored latitude magnitude, 90 degrees.
pub const MAX_GEO_LAT_UNITS: i64 = 90 * GEO_UNITS_PER_DEGREE;

/// Largest stored longitude magnitude, 180 degrees.
pub const MAX_GEO_LNG_UNITS: i64 = 180 * GEO_UNITS_PER_DEGREE;

/// Coordinates in degrees as commands and imports carry them.
#[derive(Deserialize)]
struct GeoPayload {
    lat: f64,
    lng: f64,
}

/// Stored coordinates in millionths of a degree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeoUnits {
    /// Latitude in millionths of a degree.
    pub lat: i64,
    /// Longitude in millionths of a degree.
    pub lng: i64,
}

impl GeoUnits {
    /// Converts degrees to stored units, rounding to the nearest unit.
    ///
    /// # Errors
    ///
    /// Returns an error when either coordinate is not finite or lies outside geographic bounds.
    pub fn from_degrees(lat: f64, lng: f64) -> Result<Self, String> {
        if !lat.is_finite() || !lng.is_finite() {
            return Err("geo coordinates must be finite".to_string());
        }
        let lat = degrees_to_units(lat);
        let lng = degrees_to_units(lng);
        if !(-MAX_GEO_LAT_UNITS..=MAX_GEO_LAT_UNITS).contains(&lat)
            || !(-MAX_GEO_LNG_UNITS..=MAX_GEO_LNG_UNITS).contains(&lng)
        {
            return Err("Calendar coordinates are outside geographic bounds".to_string());
        }
        Ok(Self { lat, lng })
    }

    /// Latitude in degrees.
    pub fn lat_degrees(self) -> f64 {
        units_to_degrees(self.lat)
    }

    /// Longitude in degrees.
    pub fn lng_degrees(self) -> f64 {
        units_to_degrees(self.lng)
    }
}

/// Parses an optional `{"lat": .., "lng": ..}` payload in degrees into stored units.
///
/// # Errors
///
/// Returns an error when the payload is not valid coordinates or lies outside geographic bounds.
pub fn parse_geo(value: &Option<String>) -> Result<Option<GeoUnits>, String> {
    let Some(json) = value else {
        return Ok(None);
    };
    let geo = serde_json::from_str::<GeoPayload>(json)
        .map_err(|e| format!("geo is not valid coordinates: {e}"))?;
    GeoUnits::from_degrees(geo.lat, geo.lng).map(Some)
}

/// Serializes stored units as the `{"lat": .., "lng": ..}` payload in degrees.
///
/// # Errors
///
/// Returns an error when serialization fails.
pub fn geo_json(units: GeoUnits) -> Result<String, String> {
    serde_json::to_string(&serde_json::json!({
        "lat": units.lat_degrees(),
        "lng": units.lng_degrees(),
    }))
    .map_err(|e| format!("serialize geo: {e}"))
}

/// Rounds degrees to stored units; the cast saturates, so out-of-range input stays out of range
/// for the caller's bound check.
fn degrees_to_units(degrees: f64) -> i64 {
    (degrees * GEO_UNITS_PER_DEGREE as f64).round() as i64
}

/// Converts stored units to degrees; units stay within 180 million, which f64 holds exactly.
fn units_to_degrees(units: i64) -> f64 {
    units as f64 / GEO_UNITS_PER_DEGREE as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degrees_round_trip_through_stored_units() {
        let units = GeoUnits::from_degrees(37.386_013, -122.082_932).unwrap();
        assert_eq!(
            units,
            GeoUnits {
                lat: 37_386_013,
                lng: -122_082_932
            }
        );
        assert_eq!(units.lat_degrees(), 37.386_013);
        assert_eq!(units.lng_degrees(), -122.082_932);
    }

    #[test]
    fn finer_precision_rounds_to_the_nearest_unit() {
        let units = GeoUnits::from_degrees(0.000_000_6, -0.000_000_4).unwrap();
        assert_eq!(units, GeoUnits { lat: 1, lng: 0 });
    }

    #[test]
    fn bounds_and_non_finite_values_are_refused() {
        assert!(GeoUnits::from_degrees(90.0, 180.0).is_ok());
        assert!(GeoUnits::from_degrees(-90.0, -180.0).is_ok());
        assert!(GeoUnits::from_degrees(90.000_001, 0.0).is_err());
        assert!(GeoUnits::from_degrees(0.0, -180.000_001).is_err());
        assert!(GeoUnits::from_degrees(f64::NAN, 0.0).is_err());
        assert!(GeoUnits::from_degrees(0.0, f64::INFINITY).is_err());
        assert!(parse_geo(&Some("{\"lat\":1}".into())).is_err());
        assert_eq!(parse_geo(&None).unwrap(), None);
    }
}

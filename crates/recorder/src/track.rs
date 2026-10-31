use std::path::Path;

use sdrmm_wire::{Attitude, PositionFix};

use crate::{SigmfError, SigmfGeolocation, SigmfMeta, Stored, collection::lane_stem, read_meta};

/// Where a recording was made, by sample. A capture without a geolocation marks a lost
/// position; a geolocation in the global object stands for the whole recording.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecordedTrack {
    points: Vec<Point>,
    unreadable: usize,
}

#[derive(Clone, Debug, PartialEq)]
struct Point {
    sample: u64,
    place: Option<Place>,
}

#[derive(Clone, Debug, PartialEq)]
struct Place {
    latitude: f64,
    longitude: f64,
    altitude_m: Option<f64>,
    time: Option<String>,
}

impl RecordedTrack {
    pub fn read(stem: &Path) -> Result<Self, SigmfError> {
        let meta = match Stored::at(stem) {
            Stored::Recording(_) => read_meta(stem)?,
            Stored::Collection(_) => read_meta(&lane_stem(stem, 0))?,
        };
        Ok(Self::of(&meta))
    }

    #[must_use]
    pub fn of(meta: &SigmfMeta) -> Self {
        let started = meta
            .captures
            .first()
            .and_then(|capture| capture.datetime.as_deref());
        let mut track = Self::default();
        if let Some(standing) = &meta.global.geolocation {
            track.push(0, Some(standing), started);
        }
        if meta
            .captures
            .iter()
            .any(|capture| capture.geolocation.is_some())
        {
            for capture in &meta.captures {
                let time = capture.datetime.as_deref().or(started);
                track.push(capture.sample_start, capture.geolocation.as_ref(), time);
            }
        }
        track.points.sort_by_key(|point| point.sample);
        track
    }

    fn push(&mut self, sample: u64, geolocation: Option<&SigmfGeolocation>, time: Option<&str>) {
        let place = geolocation.and_then(|geolocation| Place::of(geolocation, time));
        if geolocation.is_some() && place.is_none() {
            self.unreadable += 1;
        }
        self.points.retain(|point| point.sample != sample);
        self.points.push(Point { sample, place });
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.points.iter().all(|point| point.place.is_none())
    }

    /// Geolocations that are not a valid GeoJSON point and so never reach a map.
    #[must_use]
    pub fn unreadable(&self) -> usize {
        self.unreadable
    }

    /// The point in force at `sample`, so a caller can tell when playback crosses into the next.
    #[must_use]
    pub fn index_at(&self, sample: u64) -> Option<usize> {
        self.points
            .partition_point(|point| point.sample <= sample)
            .checked_sub(1)
    }

    /// The fix of a point, dated `undated` when the recording does not say when it was taken.
    #[must_use]
    pub fn fix(&self, index: usize, undated: &str) -> Option<PositionFix> {
        let place = self.points.get(index)?.place.as_ref()?;
        Some(PositionFix {
            latitude: place.latitude,
            longitude: place.longitude,
            altitude_m: place.altitude_m,
            accuracy_m: None,
            speed_mps: None,
            track_deg: None,
            time: place.time.as_deref().unwrap_or(undated).to_owned(),
            attitude: Attitude::default(),
        })
    }
}

impl Place {
    fn of(geolocation: &SigmfGeolocation, time: Option<&str>) -> Option<Self> {
        if geolocation.kind != "Point" {
            return None;
        }
        let (longitude, latitude, altitude_m) = match geolocation.coordinates[..] {
            [longitude, latitude] => (longitude, latitude, None),
            [longitude, latitude, altitude] => (longitude, latitude, Some(altitude)),
            _ => return None,
        };
        let on_earth = latitude.is_finite()
            && longitude.is_finite()
            && (-90.0..=90.0).contains(&latitude)
            && (-180.0..=180.0).contains(&longitude)
            && altitude_m.is_none_or(f64::is_finite);
        on_earth.then(|| Self {
            latitude,
            longitude,
            altitude_m,
            time: time
                .filter(|time| time.parse::<jiff::Timestamp>().is_ok())
                .map(str::to_owned),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SigmfWriter;

    const UNDATED: &str = "2026-01-01T00:00:00Z";

    fn fix(latitude: f64, time: &str) -> PositionFix {
        PositionFix {
            latitude,
            longitude: 13.4,
            altitude_m: Some(40.0),
            accuracy_m: None,
            speed_mps: None,
            track_deg: None,
            time: time.to_owned(),
            attitude: Attitude::default(),
        }
    }

    fn samples(n: usize) -> Vec<num_complex::Complex<f32>> {
        vec![num_complex::Complex::new(0.0, 0.0); n]
    }

    fn latitude_at(track: &RecordedTrack, sample: u64) -> Option<f64> {
        let index = track.index_at(sample)?;
        track.fix(index, UNDATED).map(|fix| fix.latitude)
    }

    #[test]
    fn playback_finds_the_place_recorded_for_each_sample() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut writer =
            SigmfWriter::create(&dir.path().join("drive"), 48_000.0, 1e6, "hw").unwrap();
        writer.write_block(&samples(100)).unwrap();
        writer.set_position(Some(&fix(52.0, "2026-08-14T12:00:00Z")));
        writer.write_block(&samples(100)).unwrap();
        writer.add_capture(2e6);
        writer.write_block(&samples(100)).unwrap();
        writer.set_position(Some(&fix(53.0, "2026-08-14T12:00:01Z")));
        writer.write_block(&samples(100)).unwrap();
        writer.set_position(None);
        let track = RecordedTrack::of(&writer.finalize().unwrap());

        assert_eq!(latitude_at(&track, 50), None);
        assert_eq!(latitude_at(&track, 100), Some(52.0));
        assert_eq!(
            latitude_at(&track, 250),
            Some(52.0),
            "a retune keeps the place"
        );
        assert_eq!(latitude_at(&track, 399), Some(53.0));
        assert_eq!(latitude_at(&track, 400), None, "a lost fix stays lost");
        let at = track.index_at(300).unwrap();
        assert_eq!(track.fix(at, UNDATED).unwrap().time, "2026-08-14T12:00:01Z");
        assert_eq!(track.fix(at, UNDATED).unwrap().altitude_m, Some(40.0));
    }

    #[test]
    fn a_recording_without_positions_has_an_empty_track() {
        let dir = tempfile::TempDir::new().unwrap();
        let stem = dir.path().join("still");
        let mut writer = SigmfWriter::create(&stem, 48_000.0, 1e6, "hw").unwrap();
        writer.write_block(&samples(10)).unwrap();
        writer.add_capture(2e6);
        writer.finalize().unwrap();

        let track = RecordedTrack::read(&stem).unwrap();
        assert!(track.is_empty());
        assert_eq!(latitude_at(&track, 5), None);
    }

    #[test]
    fn a_global_geolocation_places_the_whole_recording() {
        let meta: SigmfMeta = serde_json::from_value(serde_json::json!({
            "global": {
                "core:datatype": "cf32_le",
                "core:version": "1.2.6",
                "core:geolocation": {"type": "Point", "coordinates": [13.4, 52.5]}
            },
            "captures": [{"core:sample_start": 0}, {"core:sample_start": 500}]
        }))
        .unwrap();
        let track = RecordedTrack::of(&meta);

        assert_eq!(latitude_at(&track, 900), Some(52.5));
        let fix = track.fix(track.index_at(0).unwrap(), UNDATED).unwrap();
        assert_eq!(fix.time, UNDATED);
        assert_eq!(fix.validate(), Ok(()));
    }

    #[test]
    fn places_that_are_not_on_earth_are_counted_not_shown() {
        let meta: SigmfMeta = serde_json::from_value(serde_json::json!({
            "global": {"core:datatype": "cf32_le", "core:version": "1.2.6"},
            "captures": [
                {"core:sample_start": 0,
                 "core:geolocation": {"type": "Point", "coordinates": [13.4, 52.5]}},
                {"core:sample_start": 10,
                 "core:geolocation": {"type": "Point", "coordinates": [13.4, 95.0]}},
                {"core:sample_start": 20,
                 "core:geolocation": {"type": "Polygon", "coordinates": [1.0, 2.0]}}
            ]
        }))
        .unwrap();
        let track = RecordedTrack::of(&meta);

        assert_eq!(track.unreadable(), 2);
        assert_eq!(latitude_at(&track, 5), Some(52.5));
        assert_eq!(latitude_at(&track, 15), None);
    }
}

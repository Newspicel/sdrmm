use std::ops::{Add, Mul, Sub};

use sdrmm_wire::{AntennaGeometry, AntennaPoint, AntennaSegmentRole};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub fn norm(self) -> f64 {
        self.dot(self).sqrt()
    }

    pub fn reflected(self) -> Self {
        Self::new(self.x, -self.y, self.z)
    }

    pub fn midpoint(self, other: Self) -> Self {
        (self + other) * 0.5
    }

    fn from_point(point: AntennaPoint, lift_m: f64) -> Self {
        Self::new(point.x_m, point.y_m + lift_m, point.z_m)
    }

    pub fn to_point(self, lift_m: f64) -> AntennaPoint {
        AntennaPoint::new(self.x, self.y - lift_m, self.z)
    }
}

impl Add for Vec3 {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl Sub for Vec3 {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl Mul<f64> for Vec3 {
    type Output = Self;
    fn mul(self, scale: f64) -> Self {
        Self::new(self.x * scale, self.y * scale, self.z * scale)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Segment {
    pub start: Vec3,
    pub end: Vec3,
}

impl Segment {
    pub fn length(&self) -> f64 {
        (self.end - self.start).norm()
    }

    pub fn middle(&self) -> Vec3 {
        self.start.midpoint(self.end)
    }

    pub fn reflected(&self) -> Self {
        Self {
            start: self.start.reflected(),
            end: self.end.reflected(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Half {
    pub segment: usize,
    pub upper: bool,
    pub along: f64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Pulse {
    pub node: Vec3,
    pub from_segment: usize,
    pub to_segment: usize,
    pub halves: [Half; 2],
}

#[derive(Clone, Debug)]
pub(super) struct Wire {
    pub from: Vec3,
    pub to: Vec3,
    pub first_segment: usize,
    pub segments: usize,
}

#[derive(Clone, Debug)]
pub(super) struct Structure {
    pub wires: Vec<Wire>,
    pub segments: Vec<Segment>,
    pub pulses: Vec<Pulse>,
    pub feeds: Vec<usize>,
    pub feed_wire: usize,
    pub feed_segment: usize,
    pub lift_m: f64,
    pub lowest_m: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum StructureError {
    Empty,
    FeedOffAntenna,
}

#[derive(Clone, Copy, Debug)]
struct RawWire {
    from: Vec3,
    to: Vec3,
    hot: bool,
}

impl Structure {
    pub fn build(
        geometry: &AntennaGeometry,
        lift_m: f64,
        max_segment_m: f64,
    ) -> Result<Self, StructureError> {
        let feed = Vec3::from_point(geometry.feed, lift_m);
        let raw = radiating_wires(geometry, lift_m);
        if raw.is_empty() {
            return Err(StructureError::Empty);
        }
        let tolerance = tolerance_for(&raw);
        let wires = split_at_junctions(&raw, feed, tolerance);
        let hot = hot_side(&wires, feed, tolerance)?;
        let mut structure = segmented(&wires, max_segment_m, lift_m);
        structure.connect(feed, hot, tolerance);
        if structure.feeds.is_empty() {
            return Err(StructureError::FeedOffAntenna);
        }
        Ok(structure)
    }

    pub fn length_of(&self, pulse: &Pulse) -> f64 {
        (self.segments[pulse.from_segment].length() + self.segments[pulse.to_segment].length())
            / 2.0
    }

    pub fn half_ends(&self, half: Half) -> (Vec3, Vec3) {
        let segment = self.segments[half.segment];
        let middle = segment.middle();
        let (low, high) = if half.upper {
            (middle, segment.end)
        } else {
            (segment.start, middle)
        };
        if half.along > 0.0 {
            (low, high)
        } else {
            (high, low)
        }
    }

    fn connect(&mut self, feed: Vec3, hot: usize, tolerance: f64) {
        let mut ends: Vec<WireEnd> = Vec::new();
        for (index, wire) in self.wires.iter().enumerate() {
            let last = wire.first_segment + wire.segments - 1;
            for segment in wire.first_segment..last {
                self.pulses.push(pulse_between(
                    &self.segments,
                    segment,
                    segment + 1,
                    true,
                    true,
                ));
            }
            ends.push(WireEnd {
                at: wire.from,
                segment: wire.first_segment,
                is_end: false,
                wire: index,
            });
            ends.push(WireEnd {
                at: wire.to,
                segment: last,
                is_end: true,
                wire: index,
            });
        }
        for mut cluster in clusters(&ends, tolerance) {
            let fed = (ends[cluster[0]].at - feed).norm() <= tolerance;
            if fed && let Some(position) = cluster.iter().position(|&end| ends[end].wire == hot) {
                cluster.swap(0, position);
                self.feed_wire = hot;
                self.feed_segment = ends[cluster[0]].segment - self.wires[hot].first_segment;
            }
            let Some((&base, rest)) = cluster.split_first() else {
                continue;
            };
            for &other in rest {
                if fed {
                    self.feeds.push(self.pulses.len());
                }
                self.pulses.push(pulse_between(
                    &self.segments,
                    ends[base].segment,
                    ends[other].segment,
                    ends[base].is_end,
                    !ends[other].is_end,
                ));
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct WireEnd {
    at: Vec3,
    segment: usize,
    is_end: bool,
    wire: usize,
}

fn radiating_wires(geometry: &AntennaGeometry, lift_m: f64) -> Vec<RawWire> {
    geometry
        .segments
        .iter()
        .filter(|segment| {
            !matches!(
                segment.role,
                AntennaSegmentRole::Feedline | AntennaSegmentRole::Structure
            ) && segment.length_m() > 0.0
        })
        .map(|segment| RawWire {
            from: Vec3::from_point(segment.from, lift_m),
            to: Vec3::from_point(segment.to, lift_m),
            hot: segment.role != AntennaSegmentRole::Radial,
        })
        .collect()
}

fn tolerance_for(wires: &[RawWire]) -> f64 {
    let longest = wires
        .iter()
        .map(|wire| (wire.to - wire.from).norm())
        .fold(0.0, f64::max);
    (longest * 1e-7).max(1e-9)
}

fn split_at_junctions(wires: &[RawWire], feed: Vec3, tolerance: f64) -> Vec<RawWire> {
    let mut marks: Vec<Vec3> = wires.iter().flat_map(|wire| [wire.from, wire.to]).collect();
    marks.push(feed);
    let mut split = Vec::new();
    for wire in wires {
        let span = wire.to - wire.from;
        let length = span.norm();
        let mut cuts: Vec<f64> = marks
            .iter()
            .filter_map(|&mark| {
                let t = (mark - wire.from).dot(span) / (length * length);
                let off = (wire.from + span * t - mark).norm();
                (off <= tolerance && t * length > tolerance && (1.0 - t) * length > tolerance)
                    .then_some(t)
            })
            .collect();
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() * length <= tolerance);
        let mut start = wire.from;
        for t in cuts {
            let point = wire.from + span * t;
            split.push(RawWire {
                from: start,
                to: point,
                hot: wire.hot,
            });
            start = point;
        }
        split.push(RawWire {
            from: start,
            to: wire.to,
            hot: wire.hot,
        });
    }
    split
}

fn hot_side(wires: &[RawWire], feed: Vec3, tolerance: f64) -> Result<usize, StructureError> {
    let touching: Vec<usize> = wires
        .iter()
        .enumerate()
        .filter(|(_, wire)| {
            (wire.from - feed).norm() <= tolerance || (wire.to - feed).norm() <= tolerance
        })
        .map(|(index, _)| index)
        .collect();
    if touching.len() < 2 {
        return Err(StructureError::FeedOffAntenna);
    }
    Ok(touching
        .iter()
        .copied()
        .find(|&index| wires[index].hot)
        .unwrap_or(touching[0]))
}

fn segmented(wires: &[RawWire], max_segment_m: f64, lift_m: f64) -> Structure {
    let mut segments = Vec::new();
    let mut built = Vec::new();
    let mut lowest_m = f64::INFINITY;
    for wire in wires {
        let span = wire.to - wire.from;
        let count = segment_count(span.norm(), max_segment_m);
        let first_segment = segments.len();
        for index in 0..count {
            let start = wire.from + span * (index as f64 / count as f64);
            let end = wire.from + span * ((index + 1) as f64 / count as f64);
            segments.push(Segment { start, end });
        }
        lowest_m = lowest_m.min(wire.from.y).min(wire.to.y);
        built.push(Wire {
            from: wire.from,
            to: wire.to,
            first_segment,
            segments: count,
        });
    }
    Structure {
        wires: built,
        segments,
        pulses: Vec::new(),
        feeds: Vec::new(),
        feed_wire: 0,
        feed_segment: 0,
        lift_m,
        lowest_m,
    }
}

fn segment_count(length_m: f64, max_segment_m: f64) -> usize {
    let count = (length_m / max_segment_m).ceil();
    if count.is_finite() && count >= 1.0 {
        count as usize
    } else {
        1
    }
}

fn pulse_between(
    segments: &[Segment],
    from: usize,
    to: usize,
    from_ends_at_node: bool,
    to_starts_at_node: bool,
) -> Pulse {
    let node = if from_ends_at_node {
        segments[from].end
    } else {
        segments[from].start
    };
    Pulse {
        node,
        from_segment: from,
        to_segment: to,
        halves: [
            Half {
                segment: from,
                upper: from_ends_at_node,
                along: if from_ends_at_node { 1.0 } else { -1.0 },
            },
            Half {
                segment: to,
                upper: !to_starts_at_node,
                along: if to_starts_at_node { 1.0 } else { -1.0 },
            },
        ],
    }
}

fn clusters(ends: &[WireEnd], tolerance: f64) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (index, end) in ends.iter().enumerate() {
        match groups
            .iter_mut()
            .find(|group| (ends[group[0]].at - end.at).norm() <= tolerance)
        {
            Some(group) => group.push(index),
            None => groups.push(vec![index]),
        }
    }
    groups
}

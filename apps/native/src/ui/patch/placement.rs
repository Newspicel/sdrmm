use super::graph;

const ASSUMED_HEIGHT: f64 = 320.0;
const ASSUMED_CHANNEL_HEIGHT: f64 = 600.0;
pub const SCREEN_GAP: f64 = 24.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    #[must_use]
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    #[must_use]
    pub fn encloses(&self, other: &Self) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.x + other.w <= self.x + self.w
            && other.y + other.h <= self.y + self.h
    }

    #[must_use]
    pub fn union(&self, other: &Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Self::new(
            x,
            y,
            (self.x + self.w).max(other.x + other.w) - x,
            (self.y + self.h).max(other.y + other.h) - y,
        )
    }
}

#[must_use]
pub fn drop_size(kind: &str) -> (f64, f64) {
    let natural = graph::natural_size(kind);
    let fallback = if kind == "channel" {
        ASSUMED_CHANNEL_HEIGHT
    } else {
        ASSUMED_HEIGHT
    };
    (
        f64::from(natural.w),
        natural.h.map_or(fallback, f64::from),
    )
}

fn intersects(at: (f64, f64), size: (f64, f64), rect: &Rect, gap: f64) -> bool {
    at.0 < rect.x + rect.w + gap
        && at.0 + size.0 + gap > rect.x
        && at.1 < rect.y + rect.h + gap
        && at.1 + size.1 + gap > rect.y
}

fn candidates(start: f64, span: f64, size: f64, edges: &[(f64, f64)], gap: f64) -> Vec<f64> {
    let inset = gap.min(((span - size) / 2.0).max(0.0));
    let min = start + inset;
    let max = min.max(start + span - size - inset);
    let mut values = vec![start + (span - size) / 2.0, min, max];
    for (edge, extent) in edges {
        values.extend([*edge, edge - size - gap, edge + extent + gap]);
    }
    let mut clamped: Vec<f64> = values.into_iter().map(|v| v.clamp(min, max)).collect();
    clamped.sort_by(f64::total_cmp);
    clamped.dedup();
    clamped
}

fn distance(at: (f64, f64), size: (f64, f64), centre: (f64, f64)) -> f64 {
    let dx = at.0 + size.0 / 2.0 - centre.0;
    let dy = at.1 + size.1 / 2.0 - centre.1;
    dx * dx + dy * dy
}

fn clear_position(
    area: &Rect,
    size: (f64, f64),
    occupied: &[Rect],
    gap: f64,
    centre: (f64, f64),
) -> Option<(f64, f64)> {
    let nearby: Vec<&Rect> = occupied
        .iter()
        .filter(|rect| intersects((area.x, area.y), (area.w, area.h), rect, gap))
        .collect();
    let xs = candidates(
        area.x,
        area.w,
        size.0,
        &nearby.iter().map(|rect| (rect.x, rect.w)).collect::<Vec<_>>(),
        gap,
    );
    let ys = candidates(
        area.y,
        area.h,
        size.1,
        &nearby.iter().map(|rect| (rect.y, rect.h)).collect::<Vec<_>>(),
        gap,
    );
    xs.iter()
        .flat_map(|x| ys.iter().map(move |y| (*x, *y)))
        .filter(|at| nearby.iter().all(|rect| !intersects(*at, size, rect, gap)))
        .min_by(|a, b| {
            distance(*a, size, centre)
                .total_cmp(&distance(*b, size, centre))
                .then(a.1.total_cmp(&b.1))
                .then(a.0.total_cmp(&b.0))
        })
}

fn widened(viewport: &Rect, size: (f64, f64), occupied: &[Rect], gap: f64) -> Rect {
    let margin = (size.0 + 2.0 * gap, size.1 + 2.0 * gap);
    let left = occupied.iter().map(|r| r.x).fold(viewport.x, f64::min) - margin.0;
    let top = occupied.iter().map(|r| r.y).fold(viewport.y, f64::min) - margin.1;
    let right = occupied
        .iter()
        .map(|r| r.x + r.w)
        .fold(viewport.x + viewport.w, f64::max)
        + margin.0;
    let bottom = occupied
        .iter()
        .map(|r| r.y + r.h)
        .fold(viewport.y + viewport.h, f64::max)
        + margin.1;
    Rect::new(left, top, right - left, bottom - top)
}

#[must_use]
pub fn drop_position(
    viewport: &Rect,
    size: (f64, f64),
    occupied: &[Rect],
    gap: f64,
) -> (f64, f64) {
    let centre = (viewport.x + viewport.w / 2.0, viewport.y + viewport.h / 2.0);
    clear_position(viewport, size, occupied, gap, centre)
        .or_else(|| {
            clear_position(
                &widened(viewport, size, occupied, gap),
                size,
                occupied,
                gap,
                centre,
            )
        })
        .unwrap_or((viewport.x, viewport.y))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: Rect = Rect::new(0.0, 0.0, 1200.0, 800.0);

    #[test]
    fn an_empty_view_takes_the_node_at_its_centre() {
        let at = drop_position(&VIEW, (400.0, 300.0), &[], SCREEN_GAP);
        assert_eq!(at, (400.0, 250.0));
    }

    #[test]
    fn a_taken_centre_moves_the_node_beside_what_is_there() {
        let taken = [Rect::new(350.0, 200.0, 500.0, 400.0)];
        let at = drop_position(&VIEW, (300.0, 300.0), &taken, SCREEN_GAP);
        assert!(!intersects(at, (300.0, 300.0), &taken[0], SCREEN_GAP));
    }

    #[test]
    fn a_full_view_finds_room_outside_it() {
        let taken = [Rect::new(-10.0, -10.0, 1220.0, 820.0)];
        let at = drop_position(&VIEW, (300.0, 300.0), &taken, SCREEN_GAP);
        assert!(!intersects(at, (300.0, 300.0), &taken[0], SCREEN_GAP));
        assert!(!VIEW.encloses(&Rect::new(at.0, at.1, 300.0, 300.0)));
    }

    #[test]
    fn a_channel_is_assumed_taller_than_a_readout() {
        assert_eq!(drop_size("channel"), (440.0, 600.0));
        assert_eq!(drop_size("speaker"), (320.0, 320.0));
        assert_eq!(drop_size("scope"), (520.0, 360.0));
    }

    #[test]
    fn union_and_enclosure_follow_the_edges() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 10.0, 10.0);
        assert_eq!(a.union(&b), Rect::new(0.0, 0.0, 15.0, 15.0));
        assert!(a.union(&b).encloses(&b));
        assert!(!a.encloses(&b));
    }
}

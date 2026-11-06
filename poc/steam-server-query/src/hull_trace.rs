//! Bounded point sweep through a static GoldSrc BSP hull.
use crate::map::{Geometry, Plane};
use std::io::{self, Error, ErrorKind};

const SOLID: i32 = -2;
const EMPTY: i32 = -1;
const TRANSLUCENT: i32 = -15;
const EPSILON: f32 = 1.0 / 32.0;
const MAX_DEPTH: usize = 128;
const MAX_WORK: usize = 1_000_000;

fn invalid() -> io::Error {
    Error::new(ErrorKind::InvalidData, "invalid or excessive BSP trace")
}

#[derive(Debug, PartialEq)]
pub struct Trace {
    pub fraction: f32,
    pub end: [f32; 3],
    pub all_solid: bool,
    pub start_solid: bool,
    pub in_open: bool,
    pub in_water: bool,
    /// Collision plane in model-local coordinates, absent without an impact.
    pub plane: Option<([f32; 3], f32)>,
}

#[derive(Clone, Copy)]
struct Segment {
    node: i32,
    first: f32,
    last: f32,
    start: [f32; 3],
    end: [f32; 3],
}

struct Query<'a> {
    geometry: &'a Geometry,
    hull: usize,
    root: i32,
    remaining: usize,
}

impl Query<'_> {
    fn charge(&mut self) -> io::Result<()> {
        self.remaining = self.remaining.checked_sub(1).ok_or_else(invalid)?;
        Ok(())
    }

    fn contents(&self, child: i32) -> io::Result<i32> {
        if self.hull == 0 {
            let index = (-1_i64 - i64::from(child)) as usize;
            return self
                .geometry
                .leaves
                .get(index)
                .map(|leaf| leaf.contents)
                .ok_or_else(invalid);
        }
        Ok(child)
    }

    fn node(&self, index: i32) -> io::Result<(&Plane, [i16; 2])> {
        let (plane, children) = if self.hull == 0 {
            let node = self
                .geometry
                .nodes
                .get(index as usize)
                .ok_or_else(invalid)?;
            (node.plane, node.children)
        } else {
            let node = self
                .geometry
                .clipnodes
                .get(index as usize)
                .ok_or_else(invalid)?;
            (node.plane, node.children)
        };
        Ok((
            self.geometry
                .planes
                .get(plane as usize)
                .ok_or_else(invalid)?,
            children,
        ))
    }

    fn distance(plane: &Plane, point: [f32; 3]) -> io::Result<f32> {
        let distance = if (0..3).contains(&plane.kind) {
            point[plane.kind as usize] - plane.distance
        } else if (3..=5).contains(&plane.kind) {
            plane.normal[0] * point[0] + plane.normal[1] * point[1] + plane.normal[2] * point[2]
                - plane.distance
        } else {
            return Err(invalid());
        };
        if !distance.is_finite() {
            return Err(invalid());
        }
        Ok(distance)
    }

    fn point(&mut self, mut node: i32, point: [f32; 3]) -> io::Result<i32> {
        while node >= 0 {
            self.charge()?;
            let (plane, children) = self.node(node)?;
            node = i32::from(children[usize::from(Self::distance(plane, point)? < 0.0)]);
        }
        self.contents(node)
    }

    fn check(&mut self, segment: Segment, trace: &mut Trace, depth: usize) -> io::Result<bool> {
        self.charge()?;
        if depth > MAX_DEPTH {
            return Err(invalid());
        }
        if segment.node < 0 {
            let contents = self.contents(segment.node)?;
            if contents == SOLID {
                trace.start_solid = true;
            } else {
                trace.all_solid = false;
                if contents == EMPTY {
                    trace.in_open = true;
                } else if contents != TRANSLUCENT {
                    trace.in_water = true;
                }
            }
            return Ok(true);
        }
        let (plane, children) = self.node(segment.node)?;
        let t1 = Self::distance(plane, segment.start)?;
        let t2 = Self::distance(plane, segment.end)?;
        let normal = plane.normal;
        let plane_distance = plane.distance;
        if t1 >= 0.0 && t2 >= 0.0 {
            return self.check(
                Segment {
                    node: i32::from(children[0]),
                    ..segment
                },
                trace,
                depth + 1,
            );
        }
        if t1 < 0.0 && t2 < 0.0 {
            return self.check(
                Segment {
                    node: i32::from(children[1]),
                    ..segment
                },
                trace,
                depth + 1,
            );
        }
        let side = usize::from(t1 < 0.0);
        let denominator = t1 - t2;
        if !denominator.is_finite() {
            return Err(invalid());
        }
        let mut fraction =
            ((t1 + if side == 1 { EPSILON } else { -EPSILON }) / denominator).clamp(0.0, 1.0);
        if !fraction.is_finite() {
            return Err(invalid());
        }
        let mut mid_fraction = segment.first + (segment.last - segment.first) * fraction;
        let mut mid = std::array::from_fn(|i| {
            segment.start[i] + fraction * (segment.end[i] - segment.start[i])
        });
        if !mid.iter().all(|value| value.is_finite()) {
            return Err(invalid());
        }
        if !self.check(
            Segment {
                node: i32::from(children[side]),
                last: mid_fraction,
                end: mid,
                ..segment
            },
            trace,
            depth + 1,
        )? {
            return Ok(false);
        }
        if self.point(i32::from(children[side ^ 1]), mid)? != SOLID {
            return self.check(
                Segment {
                    node: i32::from(children[side ^ 1]),
                    first: mid_fraction,
                    start: mid,
                    ..segment
                },
                trace,
                depth + 1,
            );
        }
        if trace.all_solid {
            return Ok(false);
        }
        let sign = if side == 0 { 1.0 } else { -1.0 };
        trace.plane = Some((normal.map(|value| value * sign), plane_distance * sign));
        while self.point(self.root, mid)? == SOLID {
            fraction -= 0.1;
            if fraction < 0.0 {
                trace.fraction = mid_fraction;
                trace.end = mid;
                return Ok(false);
            }
            mid_fraction = segment.first + (segment.last - segment.first) * fraction;
            mid = std::array::from_fn(|i| {
                segment.start[i] + fraction * (segment.end[i] - segment.start[i])
            });
            if !mid.iter().all(|value| value.is_finite()) {
                return Err(invalid());
            }
        }
        trace.fraction = mid_fraction;
        trace.end = mid;
        Ok(false)
    }
}

/// Trace a point against one static model hull in model-local coordinates.
/// Fails closed on nonfinite arithmetic, invalid graphs, or excessive work/depth.
pub fn trace_point(
    geometry: &Geometry,
    model: usize,
    hull: usize,
    start: [f32; 3],
    end: [f32; 3],
) -> io::Result<Trace> {
    if hull >= 4 || !start.iter().chain(end.iter()).all(|v| v.is_finite()) {
        return Err(invalid());
    }
    let root = geometry.models.get(model).ok_or_else(invalid)?.headnodes[hull];
    let mut trace = Trace {
        fraction: 1.0,
        end,
        all_solid: true,
        start_solid: false,
        in_open: false,
        in_water: false,
        plane: None,
    };
    let mut query = Query {
        geometry,
        hull,
        root,
        remaining: MAX_WORK,
    };
    query.check(
        Segment {
            node: root,
            first: 0.0,
            last: 1.0,
            start,
            end,
        },
        &mut trace,
        0,
    )?;
    Ok(trace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Clipnode, Model};

    fn half_space() -> Geometry {
        Geometry {
            planes: vec![Plane {
                normal: [1.0, 0.0, 0.0],
                distance: 0.0,
                kind: 0,
            }],
            models: vec![Model {
                mins: [-1.0; 3],
                maxs: [1.0; 3],
                origin: [0.0; 3],
                headnodes: [0, 0, -2, -1],
                visleafs: 0,
                first_face: 0,
                face_count: 0,
            }],
            nodes: vec![],
            leaves: vec![],
            clipnodes: vec![Clipnode {
                plane: 0,
                children: [-1, -2],
            }],
            faces: vec![],
            marksurfaces: vec![],
            node_count: 0,
            clipnode_count: 1,
            face_count: 0,
        }
    }

    #[test]
    fn synthetic_half_space_trace_and_failure_cases() {
        let mut geometry = half_space();
        let trace = trace_point(&geometry, 0, 1, [1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]).unwrap();
        assert_eq!(trace.fraction, 0.484375);
        assert_eq!(trace.end, [0.03125, 0.0, 0.0]);
        assert_eq!(trace.plane, Some(([1.0, 0.0, 0.0], 0.0)));
        assert!(!trace.all_solid && !trace.start_solid && trace.in_open);
        let trace = trace_point(&geometry, 0, 1, [-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]).unwrap();
        assert!(trace.start_solid && !trace.all_solid);
        assert_eq!(trace.fraction, 1.0);
        let trace = trace_point(&geometry, 0, 1, [-1.0; 3], [-1.0; 3]).unwrap();
        assert!(trace.start_solid && trace.all_solid);
        assert!(trace_point(&geometry, 0, 1, [f32::NAN, 0.0, 0.0], [0.0; 3]).is_err());
        assert!(trace_point(&geometry, 0, 4, [0.0; 3], [0.0; 3]).is_err());
        geometry.clipnodes[0].children[0] = 0;
        assert!(trace_point(&geometry, 0, 1, [1.0; 3], [1.0; 3]).is_err());
    }

    #[test]
    #[ignore = "requires local HLDS map"]
    fn impact_positions_and_planes_match_hlds() {
        let root =
            std::env::var_os("GOLDSRC_DELTA_REFERENCE_DIR").expect("missing fixture directory");
        let path = std::path::Path::new(&root).join("reference-hlds/cstrike/maps/de_dust2.bsp");
        let geometry = crate::map::load_geometry(&path).unwrap();
        let a = [-500.0, 1000.0, 0.0];
        let b = [0.0; 3];
        let c = [100.0, 100.0, 100.0];
        let segments = [(a, b), (b, a), (a, a), (b, b), (c, a), (a, c)];
        // Captured independently from hlds_hull_trace.gdb, HLDS 10211.
        let hits = [
            (0, 0, [-144.015_63, 288.03125, 0.0], [0.0, 1.0, 0.0], 288.0),
            (
                0,
                1,
                [-383.984_38, 767.96875, 0.0],
                [0.0, -1.0, 0.0],
                -768.0,
            ),
            (
                0,
                4,
                [16.031252, 225.953_13, 86.00521],
                [1.0, 0.0, 0.0],
                16.0,
            ),
            (
                0,
                5,
                [-25.354162, 288.03125, 79.10764],
                [0.0, 1.0, 0.0],
                288.0,
            ),
            (
                1,
                0,
                [-455.85498, 911.70996, 0.0],
                [0.0, 0.24253562, 0.9701425],
                221.0909,
            ),
            (
                1,
                5,
                [-394.05197, 841.078, 17.658],
                [0.0, 0.24253562, 0.9701425],
                221.0909,
            ),
            (
                2,
                0,
                [-450.03412, 900.06824, 0.0],
                [0.0, 0.24253562, 0.9701425],
                218.26736,
            ),
            (
                2,
                5,
                [-380.0819, 820.12286, 19.986347],
                [0.0, 0.24253562, 0.9701425],
                218.26736,
            ),
            (
                3,
                0,
                [-420.92987, 841.85974, 0.0],
                [0.0, 0.24253562, 0.9701425],
                204.14972,
            ),
            (
                3,
                5,
                [-36.020836, 304.03125, 77.329865],
                [0.0, 1.0, 0.0],
                304.0,
            ),
        ];
        for hull in 0..4 {
            for (index, (start, end)) in segments.iter().enumerate() {
                let trace = trace_point(&geometry, 0, hull, *start, *end).unwrap();
                let expected = hits
                    .iter()
                    .find(|(h, i, _, _, _)| *h == hull && *i == index);
                if let Some((_, _, position, normal, distance)) = expected {
                    let (actual_normal, actual_distance) = trace.plane.expect("HLDS impact plane");
                    for axis in 0..3 {
                        assert!(
                            (trace.end[axis] - position[axis]).abs() < 0.001,
                            "hull {hull}, segment {index}, position {axis}: {trace:?}"
                        );
                        assert!(
                            (actual_normal[axis] - normal[axis]).abs() < 0.00001,
                            "hull {hull}, segment {index}, normal {axis}: {trace:?}"
                        );
                    }
                    assert!(
                        (actual_distance - distance).abs() < 0.001,
                        "hull {hull}, segment {index}: {trace:?}"
                    );
                } else {
                    assert_eq!(trace.end, *end, "hull {hull}, segment {index}");
                    assert_eq!(trace.plane, None, "hull {hull}, segment {index}");
                }
            }
        }
    }

    #[test]
    #[ignore = "requires local HLDS map"]
    fn world_hull_traces_match_hlds() {
        let root =
            std::env::var_os("GOLDSRC_DELTA_REFERENCE_DIR").expect("missing fixture directory");
        let path = std::path::Path::new(&root).join("reference-hlds/cstrike/maps/de_dust2.bsp");
        let geometry = crate::map::load_geometry(&path).unwrap();
        let a = [-500.0, 1000.0, 0.0];
        let b = [0.0; 3];
        let segments = [(a, b), (b, a), (a, a), (b, b)];
        let fractions = [
            [0.711_968_8, 0.767_968_7, 1.0, 1.0],
            [0.088_290_01, 1.0, 1.0, 1.0],
            [0.099_931_74, 1.0, 1.0, 1.0],
            [0.158_140_27, 1.0, 1.0, 1.0],
        ];
        let flags = [
            [
                [false, false, true, false],
                [false, true, true, false],
                [false, false, true, false],
                [true, true, false, false],
            ],
            [
                [false, true, true, false],
                [true, true, false, false],
                [true, true, false, false],
                [true, true, false, false],
            ],
            [
                [false, true, true, false],
                [true, true, false, false],
                [true, true, false, false],
                [true, true, false, false],
            ],
            [
                [false, true, true, false],
                [true, true, false, false],
                [true, true, false, false],
                [true, true, false, false],
            ],
        ];
        for hull in 0..4 {
            for (index, (start, end)) in segments.iter().enumerate() {
                let trace = trace_point(&geometry, 0, hull, *start, *end).unwrap();
                assert!(
                    (trace.fraction - fractions[hull][index]).abs() < 0.00001,
                    "hull {hull}, segment {index}: {trace:?}"
                );
                assert_eq!(
                    [
                        trace.all_solid,
                        trace.start_solid,
                        trace.in_open,
                        trace.in_water
                    ],
                    flags[hull][index]
                );
            }
        }
    }
}

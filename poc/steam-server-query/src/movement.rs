//! Bounded collision-only movement primitives; not a complete GoldSrc PMove.
use crate::hull_trace::{select_bsp_hull_for_bounds, trace_bsp_entity};
use crate::map::Geometry;
use std::io::{self, Error, ErrorKind};

#[derive(Debug, PartialEq)]
pub struct SlideMoveResult {
    pub origin: [f32; 3],
    pub velocity: [f32; 3],
    /// Number of BSP impacts observed, capped at two per call.
    pub collision_count: u8,
    pub all_solid: bool,
}

fn invalid_input() -> io::Error {
    Error::new(ErrorKind::InvalidInput, "invalid movement input")
}

fn dot(left: [f32; 3], right: [f32; 3]) -> f32 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn checked_normal(normal: [f32; 3]) -> io::Result<[f32; 3]> {
    let length_squared = dot(normal, normal);
    if !length_squared.is_finite() || (length_squared - 1.0).abs() > 0.001 {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "invalid BSP collision plane normal",
        ));
    }
    Ok(normal)
}

fn validate_fraction(fraction: f32) -> io::Result<()> {
    if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "invalid BSP movement trace fraction",
        ));
    }
    Ok(())
}

fn endpoint(origin: [f32; 3], velocity: [f32; 3], time: f32) -> io::Result<[f32; 3]> {
    let mut end = origin;
    for axis in 0..3 {
        end[axis] += time * velocity[axis];
        if !end[axis].is_finite() {
            return Err(invalid_input());
        }
    }
    Ok(end)
}

/// Move an entity through one unrotated static world BSP using at most two sweeps.
///
/// `origin` and `velocity` are world-space values; `mins` and `maxs` are
/// entity-relative bounds. At the first impact, velocity into the returned
/// plane is removed and the remaining frame time is spent on one more sweep. A
/// second impact stops movement for this call. This small geometric primitive
/// does not apply acceleration, gravity, ground categorization, stair stepping,
/// water, user commands or moving-entity collision. Invalid input or a failed
/// BSP query returns an error instead of accepting an unchecked position.
pub fn slide_move(
    geometry: &Geometry,
    world_model: usize,
    origin: [f32; 3],
    velocity: [f32; 3],
    mins: [f32; 3],
    maxs: [f32; 3],
    frame_time: f32,
) -> io::Result<SlideMoveResult> {
    if !origin
        .iter()
        .chain(velocity.iter())
        .chain(mins.iter())
        .chain(maxs.iter())
        .all(|coordinate| coordinate.is_finite())
        || !frame_time.is_finite()
        || frame_time < 0.0
    {
        return Err(invalid_input());
    }
    geometry
        .models
        .get(world_model)
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "invalid world BSP model"))?;
    select_bsp_hull_for_bounds([0.0; 3], mins, maxs)?;

    let mut result = SlideMoveResult {
        origin,
        velocity,
        collision_count: 0,
        all_solid: false,
    };
    if frame_time == 0.0 || velocity == [0.0; 3] {
        return Ok(result);
    }

    let first_trace = trace_bsp_entity(
        geometry,
        world_model,
        [0.0; 3],
        mins,
        maxs,
        origin,
        endpoint(origin, velocity, frame_time)?,
    )?;
    validate_fraction(first_trace.fraction)?;
    if first_trace.all_solid {
        result.velocity = [0.0; 3];
        result.collision_count = 1;
        result.all_solid = true;
        return Ok(result);
    }
    if first_trace.fraction == 1.0 {
        result.origin = first_trace.end;
        return Ok(result);
    }

    let Some((raw_normal, _)) = first_trace.plane else {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "BSP movement impact has no collision plane",
        ));
    };
    let normal = checked_normal(raw_normal)?;
    result.origin = first_trace.end;
    result.collision_count = 1;

    let into_plane = dot(velocity, normal);
    if !into_plane.is_finite() {
        return Err(invalid_input());
    }
    if into_plane < 0.0 {
        for (velocity_component, normal_component) in result.velocity.iter_mut().zip(normal.iter())
        {
            *velocity_component -= normal_component * into_plane;
            if !velocity_component.is_finite() {
                return Err(invalid_input());
            }
        }
    }

    let remaining_time = frame_time * (1.0 - first_trace.fraction);
    if !remaining_time.is_finite() || remaining_time < 0.0 {
        return Err(invalid_input());
    }
    if remaining_time == 0.0 || result.velocity == [0.0; 3] {
        return Ok(result);
    }

    let second_trace = trace_bsp_entity(
        geometry,
        world_model,
        [0.0; 3],
        mins,
        maxs,
        result.origin,
        endpoint(result.origin, result.velocity, remaining_time)?,
    )?;
    validate_fraction(second_trace.fraction)?;
    if second_trace.all_solid {
        result.velocity = [0.0; 3];
        result.collision_count = 2;
        result.all_solid = true;
        return Ok(result);
    }

    result.origin = second_trace.end;
    if second_trace.fraction < 1.0 {
        result.velocity = [0.0; 3];
        result.collision_count = 2;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Clipnode, Geometry, Model, Plane};

    fn half_space(normal: [f32; 3], kind: i32) -> Geometry {
        Geometry {
            planes: vec![Plane {
                normal,
                distance: 0.0,
                kind,
            }],
            models: vec![Model {
                mins: [-2.0; 3],
                maxs: [2.0; 3],
                origin: [0.0; 3],
                headnodes: [0; 4],
                visleafs: 0,
                first_face: 0,
                face_count: 0,
            }],
            nodes: Vec::new(),
            leaves: Vec::new(),
            clipnodes: vec![Clipnode {
                plane: 0,
                children: [-1, -2],
            }],
            faces: Vec::new(),
            marksurfaces: Vec::new(),
            node_count: 0,
            clipnode_count: 1,
            face_count: 0,
        }
    }

    fn player_bounds() -> ([f32; 3], [f32; 3]) {
        ([-16.0, -16.0, -36.0], [16.0, 16.0, 36.0])
    }

    #[test]
    fn slides_across_the_unblocked_axis_with_two_sweeps_maximum() {
        let geometry = half_space([1.0, 0.0, 0.0], 0);
        let (mins, maxs) = player_bounds();
        let result = slide_move(
            &geometry,
            0,
            [1.0, 0.0, 0.0],
            [-10.0, 2.0, 0.0],
            mins,
            maxs,
            0.2,
        )
        .unwrap();

        assert!((result.origin[0] - 0.03125).abs() < 1.0e-6);
        assert!((result.origin[1] - 0.4).abs() < 1.0e-6);
        assert_eq!(result.origin[2], 0.0);
        assert_eq!(result.velocity, [0.0, 2.0, 0.0]);
        assert_eq!(result.collision_count, 1);
        assert!(!result.all_solid);
    }

    #[test]
    fn stops_when_the_sweep_is_entirely_solid() {
        let geometry = half_space([1.0, 0.0, 0.0], 0);
        let (mins, maxs) = player_bounds();
        let result = slide_move(
            &geometry,
            0,
            [-1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            mins,
            maxs,
            0.1,
        )
        .unwrap();

        assert_eq!(result.origin, [-1.0, 0.0, 0.0]);
        assert_eq!(result.velocity, [0.0; 3]);
        assert_eq!(result.collision_count, 1);
        assert!(result.all_solid);
    }

    #[test]
    fn handles_floor_impacts_without_gameplay_classification() {
        let geometry = half_space([0.0, 0.0, 1.0], 2);
        let (mins, maxs) = player_bounds();
        let result = slide_move(
            &geometry,
            0,
            [0.0, 0.0, 1.0],
            [2.0, 0.0, -10.0],
            mins,
            maxs,
            0.2,
        )
        .unwrap();

        assert!((result.origin[0] - 0.4).abs() < 1.0e-6);
        assert_eq!(result.origin[1], 0.0);
        assert!((result.origin[2] - 0.03125).abs() < 1.0e-6);
        assert_eq!(result.velocity, [2.0, 0.0, 0.0]);
        assert_eq!(result.collision_count, 1);
        assert!(!result.all_solid);
    }

    #[test]
    fn accepts_open_motion_and_rejects_invalid_time_or_state() {
        let geometry = half_space([1.0, 0.0, 0.0], 0);
        let (mins, maxs) = player_bounds();
        let result = slide_move(
            &geometry,
            0,
            [1.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            mins,
            maxs,
            0.25,
        )
        .unwrap();
        assert_eq!(result.origin, [1.5, 0.0, 0.0]);
        assert_eq!(result.velocity, [2.0, 0.0, 0.0]);
        assert_eq!(result.collision_count, 0);
        assert!(!result.all_solid);

        assert!(slide_move(&geometry, 0, [0.0; 3], [0.0; 3], [1.0; 3], [0.0; 3], 0.1).is_err());
        assert!(slide_move(&geometry, 1, [0.0; 3], [0.0; 3], mins, maxs, 0.0).is_err());
        assert!(slide_move(&geometry, 0, [0.0; 3], [0.0; 3], mins, maxs, f32::NAN).is_err());
        assert!(slide_move(&geometry, 0, [0.0; 3], [0.0; 3], mins, maxs, -0.1).is_err());
    }

    #[test]
    #[ignore = "requires the local de_dust2 BSP and the isolated HLDS PM_Move reference"]
    fn matches_local_hlds_pmove_wall_impact_on_horizontal_components() {
        let root = std::env::var_os("GOLDSRC_DELTA_REFERENCE_DIR")
            .expect("missing local HLDS fixture directory");
        let path = std::path::Path::new(&root).join("reference-hlds/cstrike/maps/de_dust2.bsp");
        let geometry = crate::map::load_geometry(&path).unwrap();
        let (mins, maxs) = player_bounds();

        let result = slide_move(
            &geometry,
            0,
            [-832.0, -704.0, 176.0],
            [0.0, 1000.0, 0.0],
            mins,
            maxs,
            0.05,
        )
        .unwrap();

        // PM_Move's local synthetic reference also hits the y=-1 wall plane.
        // Its full result includes gravity, which this collision-only primitive
        // deliberately does not model, so compare the horizontal components.
        assert!((result.origin[0] + 832.0).abs() < 0.001);
        assert!((result.origin[1] + 656.03125).abs() < 0.001);
        assert_eq!(result.velocity[0], 0.0);
        assert_eq!(result.velocity[1], 0.0);
        assert_eq!(result.collision_count, 1);
        assert!(!result.all_solid);
    }
}

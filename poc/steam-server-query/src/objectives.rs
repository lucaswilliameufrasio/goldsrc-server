//! Static CS bomb-target brush references; no objective gameplay is executed.
use crate::map::{MapEntity, Model};
use std::io::{self, Error, ErrorKind};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BombTargetBrush {
    pub model_index: usize,
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    pub model_origin: [f32; 3],
}

fn invalid_bomb_target(reason: &'static str) -> io::Error {
    Error::new(
        ErrorKind::InvalidData,
        format!("invalid CS bomb-target brush: {reason}"),
    )
}

fn is_bomb_target(entity: &MapEntity) -> io::Result<bool> {
    let mut classname_count = 0;
    let mut found = false;
    for (key, value) in &entity.fields {
        if key != b"classname" {
            continue;
        }
        classname_count += 1;
        if value == b"func_bomb_target" {
            if found {
                return Err(invalid_bomb_target("ambiguous classname"));
            }
            found = true;
        }
    }
    if found && classname_count != 1 {
        return Err(invalid_bomb_target("duplicate classnames"));
    }
    Ok(found)
}

fn model_reference(entity: &MapEntity, models: &[Model]) -> io::Result<BombTargetBrush> {
    let mut model_value = None;
    for (key, value) in &entity.fields {
        if key == b"model" {
            if model_value.is_some() {
                return Err(invalid_bomb_target("duplicate model field"));
            }
            model_value = Some(value.as_slice());
        }
    }
    let value = model_value.ok_or_else(|| invalid_bomb_target("missing model field"))?;
    let text = std::str::from_utf8(value)
        .map_err(|_| invalid_bomb_target("model reference is not UTF-8"))?;
    let index = text
        .strip_prefix('*')
        .and_then(|index| index.parse::<usize>().ok())
        .filter(|index| *index > 0)
        .ok_or_else(|| invalid_bomb_target("malformed submodel reference"))?;
    let model = models
        .get(index)
        .ok_or_else(|| invalid_bomb_target("submodel index is out of range"))?;
    for axis in 0..3 {
        if !model.mins[axis].is_finite()
            || !model.maxs[axis].is_finite()
            || !model.origin[axis].is_finite()
            || model.mins[axis] >= model.maxs[axis]
        {
            return Err(invalid_bomb_target("submodel bounds are empty or invalid"));
        }
    }
    Ok(BombTargetBrush {
        model_index: index,
        mins: model.mins,
        maxs: model.maxs,
        model_origin: model.origin,
    })
}

/// Resolve each `func_bomb_target` entity to a non-world BSP submodel.
/// This does not test trigger occupancy, bomb planting/defusal, or round rules.
pub fn extract_bomb_target_brushes(
    entities: &[MapEntity],
    models: &[Model],
) -> io::Result<Vec<BombTargetBrush>> {
    let mut result = Vec::new();
    for (entity_index, entity) in entities.iter().enumerate() {
        let is_target = is_bomb_target(entity)
            .map_err(|error| Error::new(error.kind(), format!("entity {entity_index}: {error}")))?;
        if !is_target {
            continue;
        }
        result.push(model_reference(entity, models).map_err(|error| {
            Error::new(error.kind(), format!("entity {entity_index}: {error}"))
        })?);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(fields: &[(&[u8], &[u8])]) -> MapEntity {
        let mut result = Vec::with_capacity(fields.len());
        for (key, value) in fields {
            result.push((key.to_vec(), value.to_vec()));
        }
        MapEntity { fields: result }
    }

    fn model(index: usize) -> Model {
        let origin = index as f32;
        Model {
            mins: [-origin; 3],
            maxs: [origin; 3],
            origin: [origin; 3],
            headnodes: [0; 4],
            visleafs: 0,
            first_face: 0,
            face_count: 0,
        }
    }

    #[test]
    fn resolves_bomb_target_to_non_world_submodel_bounds() {
        let entities = [
            entity(&[(b"classname", b"worldspawn")]),
            entity(&[(b"classname", b"func_bomb_target"), (b"model", b"*1")]),
            entity(&[
                (b"classname", b"func_buyzone"),
                (b"model", b"not parsed for other entities"),
            ]),
        ];
        let models = [model(0), model(1)];

        assert_eq!(
            extract_bomb_target_brushes(&entities, &models).unwrap(),
            [BombTargetBrush {
                model_index: 1,
                mins: [-1.0; 3],
                maxs: [1.0; 3],
                model_origin: [1.0; 3],
            }]
        );
    }

    #[test]
    fn rejects_missing_ambiguous_and_invalid_model_references() {
        let models = [model(0), model(1)];
        let malformed = [
            entity(&[(b"classname", b"func_bomb_target")]),
            entity(&[(b"classname", b"func_bomb_target"), (b"model", b"world")]),
            entity(&[(b"classname", b"func_bomb_target"), (b"model", b"*0")]),
            entity(&[(b"classname", b"func_bomb_target"), (b"model", b"*2")]),
            entity(&[
                (b"classname", b"func_bomb_target"),
                (b"model", b"*1"),
                (b"model", b"*1"),
            ]),
            entity(&[
                (b"classname", b"worldspawn"),
                (b"classname", b"func_bomb_target"),
                (b"model", b"*1"),
            ]),
        ];

        for candidate in malformed {
            assert!(extract_bomb_target_brushes(&[candidate], &models).is_err());
        }

        let nonfinite_target = entity(&[(b"classname", b"func_bomb_target"), (b"model", b"*1")]);
        let mut nonfinite_models = [model(0), model(1)];
        nonfinite_models[1].mins[0] = f32::NAN;
        assert!(extract_bomb_target_brushes(&[nonfinite_target], &nonfinite_models).is_err());

        let inverted_target = entity(&[(b"classname", b"func_bomb_target"), (b"model", b"*1")]);
        let mut inverted_models = [model(0), model(1)];
        inverted_models[1].mins[0] = 2.0;
        assert!(extract_bomb_target_brushes(&[inverted_target], &inverted_models).is_err());

        let degenerate_target = entity(&[(b"classname", b"func_bomb_target"), (b"model", b"*1")]);
        let mut degenerate_models = [model(0), model(1)];
        degenerate_models[1].maxs[0] = degenerate_models[1].mins[0];
        assert!(extract_bomb_target_brushes(&[degenerate_target], &degenerate_models).is_err());
    }
}

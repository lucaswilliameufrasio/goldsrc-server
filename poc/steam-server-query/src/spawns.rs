//! Stock CS player-spawn entity extraction; this does not spawn a live player.
use crate::map::MapEntity;
use std::io::{self, Error, ErrorKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnEntityKind {
    PlayerStart,
    PlayerDeathmatch,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CsSpawnEntity {
    pub kind: SpawnEntityKind,
    /// Unmodified BSP entity origin in GoldSrc world coordinates.
    pub origin: [f32; 3],
    /// Raw scalar `angle` key if present; no default or view-angle policy applied.
    pub angle: Option<f32>,
}

fn invalid_spawn_entity(reason: &'static str) -> io::Error {
    Error::new(
        ErrorKind::InvalidData,
        format!("invalid CS player spawn entity: {reason}"),
    )
}

fn is_spawn_classname(classname: &[u8]) -> Option<SpawnEntityKind> {
    match classname {
        b"info_player_start" => Some(SpawnEntityKind::PlayerStart),
        b"info_player_deathmatch" => Some(SpawnEntityKind::PlayerDeathmatch),
        _ => None,
    }
}

fn spawn_kind(entity: &MapEntity) -> io::Result<Option<SpawnEntityKind>> {
    let mut classname_count = 0;
    let mut kind = None;
    for (key, value) in &entity.fields {
        if key != b"classname" {
            continue;
        }
        classname_count += 1;
        if let Some(candidate) = is_spawn_classname(value) {
            if kind.is_some() {
                return Err(invalid_spawn_entity("ambiguous spawn classnames"));
            }
            kind = Some(candidate);
        }
    }
    if kind.is_some() && classname_count != 1 {
        return Err(invalid_spawn_entity(
            "spawn entity has duplicate classnames",
        ));
    }
    Ok(kind)
}

fn unique_field<'a>(entity: &'a MapEntity, name: &[u8]) -> io::Result<Option<&'a [u8]>> {
    let mut value = None;
    for (key, candidate) in &entity.fields {
        if key == name {
            if value.is_some() {
                return Err(invalid_spawn_entity("duplicate spawn field"));
            }
            value = Some(candidate.as_slice());
        }
    }
    Ok(value)
}

fn parse_origin(value: &[u8]) -> io::Result<[f32; 3]> {
    let text =
        std::str::from_utf8(value).map_err(|_| invalid_spawn_entity("origin is not UTF-8"))?;
    let mut origin = [0.0; 3];
    let mut count = 0;
    for component in text.split_ascii_whitespace() {
        if count == origin.len() {
            return Err(invalid_spawn_entity(
                "origin has more than three coordinates",
            ));
        }
        let coordinate = component
            .parse::<f32>()
            .map_err(|_| invalid_spawn_entity("origin has a malformed coordinate"))?;
        if !coordinate.is_finite() {
            return Err(invalid_spawn_entity("origin has a non-finite coordinate"));
        }
        origin[count] = coordinate;
        count += 1;
    }
    if count != origin.len() {
        return Err(invalid_spawn_entity("origin must have three coordinates"));
    }
    Ok(origin)
}

fn parse_angle(value: &[u8]) -> io::Result<f32> {
    let text =
        std::str::from_utf8(value).map_err(|_| invalid_spawn_entity("angle is not UTF-8"))?;
    let mut components = text.split_ascii_whitespace();
    let yaw = components
        .next()
        .ok_or_else(|| invalid_spawn_entity("angle is empty"))?
        .parse::<f32>()
        .map_err(|_| invalid_spawn_entity("angle is malformed"))?;
    if components.next().is_some() || !yaw.is_finite() {
        return Err(invalid_spawn_entity("angle must be one finite number"));
    }
    Ok(yaw)
}

/// Extract the two stock CS player-spawn entity classes and their raw origins.
/// Team assignment, orientation, hull offsets and player selection are left to
/// a later simulation slice with HLDS/client evidence.
pub fn extract_cs_spawn_entities(entities: &[MapEntity]) -> io::Result<Vec<CsSpawnEntity>> {
    let mut spawns = Vec::new();
    for (entity_index, entity) in entities.iter().enumerate() {
        let Some(kind) = spawn_kind(entity)
            .map_err(|error| Error::new(error.kind(), format!("entity {entity_index}: {error}")))?
        else {
            continue;
        };
        let origin = unique_field(entity, b"origin")
            .map_err(|error| Error::new(error.kind(), format!("entity {entity_index}: {error}")))?
            .ok_or_else(|| invalid_spawn_entity("recognized spawn entity is missing origin"))?;
        let angle = unique_field(entity, b"angle")
            .map_err(|error| Error::new(error.kind(), format!("entity {entity_index}: {error}")))?
            .map(parse_angle)
            .transpose()
            .map_err(|error| Error::new(error.kind(), format!("entity {entity_index}: {error}")))?;
        spawns.push(CsSpawnEntity {
            kind,
            origin: parse_origin(origin).map_err(|error| {
                Error::new(error.kind(), format!("entity {entity_index}: {error}"))
            })?,
            angle,
        });
    }
    Ok(spawns)
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

    #[test]
    fn extracts_only_stock_player_spawn_classes_and_raw_origins() {
        let entities = [
            entity(&[(b"classname", b"worldspawn")]),
            entity(&[(b"classname", b"worldspawn"), (b"classname", b"func_wall")]),
            entity(&[
                (b"classname", b"info_player_start"),
                (b"origin", b"-10.5 20 30"),
                (b"angle", b"90"),
            ]),
            entity(&[
                (b"classname", b"info_player_deathmatch"),
                (b"origin", b"1e1 2 3"),
            ]),
            entity(&[
                (b"classname", b"info_target"),
                (b"origin", b"not parsed for non-spawn entities"),
            ]),
        ];

        assert_eq!(
            extract_cs_spawn_entities(&entities).unwrap(),
            [
                CsSpawnEntity {
                    kind: SpawnEntityKind::PlayerStart,
                    origin: [-10.5, 20.0, 30.0],
                    angle: Some(90.0),
                },
                CsSpawnEntity {
                    kind: SpawnEntityKind::PlayerDeathmatch,
                    origin: [10.0, 2.0, 3.0],
                    angle: None,
                },
            ]
        );
    }

    #[test]
    fn rejects_ambiguous_or_invalid_spawn_metadata() {
        let malformed = [
            entity(&[(b"classname", b"info_player_start")]),
            entity(&[(b"classname", b"info_player_start"), (b"origin", b"1 2")]),
            entity(&[
                (b"classname", b"info_player_start"),
                (b"origin", b"1 2 3 4"),
            ]),
            entity(&[
                (b"classname", b"info_player_start"),
                (b"origin", b"1 NaN 3"),
            ]),
            entity(&[
                (b"classname", b"info_player_start"),
                (b"classname", b"info_player_deathmatch"),
                (b"origin", b"1 2 3"),
            ]),
            entity(&[
                (b"classname", b"info_player_start"),
                (b"origin", b"1 2 3"),
                (b"origin", b"4 5 6"),
            ]),
            entity(&[
                (b"classname", b"info_player_start"),
                (b"origin", b"1 2 3"),
                (b"angle", b"NaN"),
            ]),
            entity(&[
                (b"classname", b"info_player_start"),
                (b"origin", b"1 2 3"),
                (b"angle", b"90 180"),
            ]),
            entity(&[
                (b"classname", b"info_player_start"),
                (b"origin", b"1 2 3"),
                (b"angle", b"90"),
                (b"angle", b"180"),
            ]),
        ];

        for candidate in malformed {
            assert!(extract_cs_spawn_entities(&[candidate]).is_err());
        }
    }
}

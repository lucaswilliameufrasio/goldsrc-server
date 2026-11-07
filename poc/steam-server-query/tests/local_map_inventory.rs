use std::env;
use std::path::PathBuf;

use steam_server_query_poc::map::{Geometry, MapEntity, load_world};

fn count_classname(entities: &[MapEntity], classname: &[u8]) -> usize {
    let mut count = 0;

    for entity in entities {
        for (key, value) in &entity.fields {
            if key == b"classname" && value == classname {
                count += 1;
            }
        }
    }

    count
}

fn assert_team_spawns(entities: &[MapEntity]) {
    assert!(count_classname(entities, b"info_player_start") > 0);
    assert!(count_classname(entities, b"info_player_deathmatch") > 0);
}

fn assert_geometry_loaded(geometry: &Geometry) {
    assert!(!geometry.models.is_empty());
    assert!(!geometry.planes.is_empty());
    assert!(geometry.node_count > 0);
    assert!(geometry.clipnode_count > 0);
}

#[test]
#[ignore = "requires the owner's local map checkout; no map assets are committed"]
fn local_companion_map_candidates_parse_and_contain_expected_entities() {
    let map_directory = env::var_os("GOLDSRC_COMPANION_MAPS_DIR")
        .map(PathBuf::from)
        .expect("set GOLDSRC_COMPANION_MAPS_DIR to the local maps directory");
    let map_names = [
        "35hp_2",
        "aim_headshot",
        "aim_map",
        "awp_india",
        "cs_deathmatch-final",
        "cs_deathmatch_2005c",
        "de_dust2_2x2",
        "fy_pool_day",
        "fy_snow",
    ];
    let mut non_candidate_geometry_failures = Vec::new();

    for map_name in map_names {
        let map_path = map_directory.join(format!("{map_name}.bsp"));
        let result = load_world(&map_path);

        match map_name {
            "de_dust2_2x2" => {
                let (entities, _, geometry) = result.expect("bomb candidate should parse");
                assert_eq!(count_classname(&entities, b"func_bomb_target"), 1);
                assert_eq!(count_classname(&entities, b"info_player_start"), 20);
                assert_eq!(count_classname(&entities, b"info_player_deathmatch"), 20);
                assert_team_spawns(&entities);
                assert_geometry_loaded(&geometry);
            }
            "fy_pool_day" => {
                let (entities, _, geometry) = result.expect("bomb candidate should parse");
                assert_eq!(count_classname(&entities, b"func_bomb_target"), 1);
                assert_eq!(count_classname(&entities, b"func_buyzone"), 1);
                assert_team_spawns(&entities);
                assert_geometry_loaded(&geometry);
            }
            "cs_deathmatch-final" => {
                let (entities, _, geometry) = result.expect("hostage candidate should parse");
                assert_eq!(count_classname(&entities, b"hostage_entity"), 2);
                assert_eq!(count_classname(&entities, b"info_hostage_rescue"), 2);
                assert_team_spawns(&entities);
                assert_geometry_loaded(&geometry);
            }
            _ => {
                if result.is_err() {
                    non_candidate_geometry_failures.push(map_name);
                }
            }
        }
    }

    println!(
        "non_candidate_geometry_failures={}",
        non_candidate_geometry_failures.join(",")
    );
}

use std::env;
use std::io::Read;
use std::net::UdpSocket;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use steam_server_query_poc::map::{Geometry, MapEntity, load_world};
use steam_server_query_poc::objectives::extract_bomb_target_brushes;
use steam_server_query_poc::spawns::{SpawnEntityKind, extract_cs_spawn_entities};

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

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

fn assert_team_spawns(entities: &[MapEntity], map_name: &str) {
    assert!(count_classname(entities, b"info_player_start") > 0);
    assert!(count_classname(entities, b"info_player_deathmatch") > 0);
    let spawns = extract_cs_spawn_entities(entities)
        .unwrap_or_else(|error| panic!("spawn metadata failed for {map_name}: {error}"));
    assert_eq!(
        spawns
            .iter()
            .filter(|spawn| spawn.kind == SpawnEntityKind::PlayerStart)
            .count(),
        count_classname(entities, b"info_player_start")
    );
    assert_eq!(
        spawns
            .iter()
            .filter(|spawn| spawn.kind == SpawnEntityKind::PlayerDeathmatch)
            .count(),
        count_classname(entities, b"info_player_deathmatch")
    );
    assert!(
        spawns
            .iter()
            .all(|spawn| spawn.origin.iter().all(|coordinate| coordinate.is_finite()))
    );
    assert!(
        spawns
            .iter()
            .filter_map(|spawn| spawn.angle)
            .all(f32::is_finite)
    );
}

fn assert_geometry_loaded(geometry: &Geometry) {
    assert!(!geometry.models.is_empty());
    assert!(!geometry.planes.is_empty());
    assert!(geometry.node_count > 0);
    assert!(geometry.clipnode_count > 0);
}

fn assert_bomb_target_brushes(entities: &[MapEntity], geometry: &Geometry, map_name: &str) {
    let targets = extract_bomb_target_brushes(entities, &geometry.models)
        .unwrap_or_else(|error| panic!("bomb-target metadata failed for {map_name}: {error}"));
    assert_eq!(
        targets.len(),
        count_classname(entities, b"func_bomb_target")
    );
    for target in targets {
        assert!(target.model_index > 0);
        assert!(target.model_index < geometry.models.len());
        assert!(
            target
                .mins
                .iter()
                .chain(target.maxs.iter())
                .chain(target.model_origin.iter())
                .all(|coordinate| coordinate.is_finite())
        );
    }
}

fn assert_map_startup_prepares_metadata(map_path: &std::path::Path, map_name: &str) {
    let reservation = UdpSocket::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);

    let mut command = Command::new(env!("CARGO_BIN_EXE_steam-server-query-poc"));
    command
        .env("LISTEN_ADDRESS", address.to_string())
        .env("GOLDSRC_MAP_BSP", map_path)
        .env_remove("GOLDSRC_PACKET_TRACE")
        .env_remove("GOLDSRC_STEAM_API")
        .env_remove("GOLDSRC_DELTA_LIST")
        .env_remove("GOLDSRC_DELTA_CATALOG")
        .env_remove("GOLDSRC_RESOURCE_MANIFEST")
        .env_remove("GOLDSRC_RESOURCE_CHECKS")
        .env_remove("GOLDSRC_RESOURCE_SPAWN_COUNT")
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut server = Server(command.spawn().unwrap());

    let client = UdpSocket::bind("127.0.0.1:0").unwrap();
    client
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let mut response = [0_u8; 512];
    let mut ready = false;
    for _ in 0..40 {
        assert!(
            server.0.try_wait().unwrap().is_none(),
            "server rejected BSP metadata for {map_name}"
        );
        client
            .send_to(b"\xff\xff\xff\xffgetchallenge steam\n", address)
            .unwrap();
        match client.recv_from(&mut response) {
            Ok((length, peer)) => {
                assert_eq!(peer, address);
                assert!(length > 5);
                assert_eq!(&response[..5], b"\xff\xff\xff\xffA");
                ready = true;
                break;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(error) => panic!("UDP receive failed for {map_name}: {error}"),
        }
    }
    assert!(ready, "server did not listen after preparing {map_name}");

    let query_prefix = b"\xff\xff\xff\xffTSource Engine Query\0";
    client.send_to(query_prefix, address).unwrap();
    let (challenge_length, peer) = client.recv_from(&mut response).unwrap();
    assert_eq!(peer, address);
    assert_eq!(challenge_length, 9);
    assert_eq!(&response[..5], b"\xff\xff\xff\xffA");
    let query_challenge: [u8; 4] = response[5..9].try_into().unwrap();

    let mut info_request = query_prefix.to_vec();
    info_request.extend_from_slice(&query_challenge);
    client.send_to(&info_request, address).unwrap();
    let (info_length, peer) = client.recv_from(&mut response).unwrap();
    assert_eq!(peer, address);
    assert!(info_length > 6);
    assert_eq!(&response[..6], b"\xff\xff\xff\xffI\x30");
    let mut fields = Vec::with_capacity(4);
    let mut position = 6;
    for _ in 0..4 {
        let field_length = response[position..info_length]
            .iter()
            .position(|byte| *byte == 0)
            .expect("A2S string field should be NUL-terminated");
        fields.push(&response[position..position + field_length]);
        position += field_length + 1;
    }
    assert_eq!(fields[1], map_name.as_bytes());

    server.0.kill().unwrap();
    let _ = server.0.wait().unwrap();
    let mut stderr = String::new();
    server
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(stderr.contains("Map metadata prepared:"));
    assert!(stderr.contains("CS spawn points"));
    assert!(stderr.contains("1 bomb-target brushes"));
    assert!(stderr.contains("world simulation and sign-on disabled"));
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
                assert_team_spawns(&entities, map_name);
                assert_geometry_loaded(&geometry);
                assert_bomb_target_brushes(&entities, &geometry, map_name);
            }
            "fy_pool_day" => {
                let (entities, _, geometry) = result.expect("bomb candidate should parse");
                assert_eq!(count_classname(&entities, b"func_bomb_target"), 1);
                assert_eq!(count_classname(&entities, b"func_buyzone"), 1);
                assert_team_spawns(&entities, map_name);
                assert_geometry_loaded(&geometry);
                assert_bomb_target_brushes(&entities, &geometry, map_name);
            }
            "cs_deathmatch-final" => {
                let (entities, _, geometry) = result.expect("hostage candidate should parse");
                assert_eq!(count_classname(&entities, b"hostage_entity"), 2);
                assert_eq!(count_classname(&entities, b"info_hostage_rescue"), 2);
                assert_team_spawns(&entities, map_name);
                assert_geometry_loaded(&geometry);
            }
            _ => {
                if result.is_err() {
                    non_candidate_geometry_failures.push(map_name);
                }
            }
        }
    }

    for map_name in ["de_dust2_2x2", "fy_pool_day"] {
        let map_path = map_directory.join(format!("{map_name}.bsp"));
        assert_map_startup_prepares_metadata(&map_path, map_name);
    }

    assert_eq!(non_candidate_geometry_failures, ["35hp_2"]);
    println!(
        "non_candidate_geometry_failures={}",
        non_candidate_geometry_failures.join(",")
    );
}

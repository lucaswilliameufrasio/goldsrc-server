import json
import socket
import threading
import unittest

from hlds_query_summary import QUERY_PREFIX, QueryError, parse_info_response, query


def cstring(value: bytes) -> bytes:
    return value + b"\0"


def source_response(
    map_name: bytes = b"de_example",
    game_name: bytes = b"Counter-Strike",
    version: bytes = b"1.1.2.7",
) -> bytes:
    return (
        b"\xff" * 4
        + b"I"
        + bytes([48])
        + cstring(b"private-host-name")
        + cstring(map_name)
        + cstring(b"cstrike")
        + cstring(game_name)
        + (10).to_bytes(2, "little")
        + bytes([1, 32, 0, ord("d"), ord("l"), 0, 1])
        + cstring(version)
        + bytes([0x10])
        + (0x1234567890ABCDEF).to_bytes(8, "little")
    )


class QuerySummaryTests(unittest.TestCase):
    def test_source_summary_omits_hostname_and_optional_steam_id(self) -> None:
        summary = parse_info_response(source_response())
        encoded = json.dumps(summary.__dict__)

        self.assertEqual(summary.response_type, "source")
        self.assertEqual(summary.protocol, 48)
        self.assertEqual(summary.map, "de_example")
        self.assertEqual(summary.game_directory, "cstrike")
        self.assertEqual(summary.players, 1)
        self.assertEqual(summary.max_players, 32)
        self.assertEqual(summary.version, "1.1.2.7")
        self.assertNotIn("private-host-name", encoded)
        self.assertNotIn("1234567890ABCDEF", encoded)

    def test_legacy_summary_omits_address_and_hostname(self) -> None:
        packet = (
            b"\xff" * 4
            + b"m"
            + cstring(b"10.0.0.12:27015")
            + cstring(b"private-host-name")
            + cstring(b"de_example")
            + cstring(b"cstrike")
            + cstring(b"Counter-Strike")
            + bytes([1, 32, 48])
        )
        summary = parse_info_response(packet)
        encoded = json.dumps(summary.__dict__)

        self.assertEqual(summary.response_type, "goldsrc_legacy")
        self.assertEqual(summary.protocol, 48)
        self.assertEqual(summary.map, "de_example")
        self.assertEqual(summary.version, "not_included")
        self.assertNotIn("10.0.0.12", encoded)
        self.assertNotIn("private-host-name", encoded)

    def test_redacts_unexpected_map_game_and_version_text(self) -> None:
        summary = parse_info_response(
            source_response(
                map_name=b"76561198000000000",
                game_name=b"steam-id-76561198000000000",
                version=b"76561198000000000",
            )
        )

        self.assertEqual(summary.map, "<redacted>")
        self.assertEqual(summary.game, "<redacted>")
        self.assertEqual(summary.version, "<redacted>")

    def test_rejects_truncated_and_unsafe_targets(self) -> None:
        with self.assertRaises(QueryError):
            parse_info_response(b"\xff\xff\xff\xffI\x30")
        with self.assertRaises(QueryError):
            query("8.8.8.8", 27015)
        with self.assertRaises(QueryError):
            query("2001:db8::1", 27015)

    def test_queries_private_hlds_and_printable_summary_has_no_endpoint(self) -> None:
        listener = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        listener.bind(("127.0.0.1", 0))
        listener.settimeout(2.0)
        port = listener.getsockname()[1]
        challenge = b"\x12\x34\x56\x78"
        errors: list[Exception] = []

        def respond() -> None:
            try:
                first_request, peer = listener.recvfrom(128)
                if first_request != QUERY_PREFIX:
                    raise AssertionError("unexpected first query")
                listener.sendto(b"\xff" * 4 + b"A" + challenge, peer)
                second_request, peer = listener.recvfrom(128)
                if second_request != QUERY_PREFIX + challenge:
                    raise AssertionError("unexpected challenge response")
                listener.sendto(source_response(), peer)
            except Exception as error:
                errors.append(error)
            finally:
                listener.close()

        server_thread = threading.Thread(target=respond)
        server_thread.start()
        summary = query("127.0.0.1", port, timeout=1.0)
        server_thread.join(timeout=2.0)

        self.assertFalse(server_thread.is_alive())
        self.assertEqual(errors, [])
        self.assertTrue(summary.challenge_roundtrip)
        self.assertEqual(
            summary.request_lengths,
            [len(QUERY_PREFIX), len(QUERY_PREFIX) + 4],
        )
        self.assertEqual(summary.response_lengths, [9, len(source_response())])
        self.assertNotIn("127.0.0.1", json.dumps(summary.__dict__))


if __name__ == "__main__":
    unittest.main()

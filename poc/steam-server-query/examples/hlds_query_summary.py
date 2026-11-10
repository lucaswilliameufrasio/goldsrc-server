#!/usr/bin/env python3
"""Query a private GoldSrc server and print only sanitized A2S_INFO fields."""

from __future__ import annotations

import argparse
import ipaddress
import json
import re
import socket
import sys
from dataclasses import asdict, dataclass


QUERY_PREFIX = b"\xff\xff\xff\xffTSource Engine Query\0"
RESPONSE_HEADER = b"\xff\xff\xff\xff"
MAX_DATAGRAM = 4096
MAX_CSTRING = 256
SAFE_MAP = re.compile(r"^[A-Za-z][A-Za-z0-9_-]{0,31}$")
SAFE_DIRECTORY = re.compile(r"^[A-Za-z][A-Za-z0-9_-]{0,31}$")
SAFE_GAME = re.compile(r"^[A-Za-z][A-Za-z -]{0,31}$")
SAFE_VERSION = re.compile(r"^[0-9]{1,3}(?:\.[0-9]{1,3}){1,4}(?:/[A-Za-z]{1,12})?$")
PRIVATE_NETWORKS = (
    ipaddress.ip_network("10.0.0.0/8"),
    ipaddress.ip_network("172.16.0.0/12"),
    ipaddress.ip_network("192.168.0.0/16"),
    ipaddress.ip_network("100.64.0.0/10"),
    ipaddress.ip_network("127.0.0.0/8"),
    ipaddress.ip_network("169.254.0.0/16"),
    ipaddress.ip_network("fc00::/7"),
    ipaddress.ip_network("fe80::/10"),
    ipaddress.ip_network("::1/128"),
)


class QueryError(Exception):
    """A malformed or unsupported query response."""


@dataclass(frozen=True)
class QuerySummary:
    response_type: str
    protocol: int
    map: str
    game_directory: str
    game: str
    players: int
    max_players: int
    version: str
    request_lengths: list[int]
    response_lengths: list[int]
    challenge_roundtrip: bool


class Cursor:
    def __init__(self, data: bytes, offset: int) -> None:
        self.data = data
        self.offset = offset

    def take(self, length: int) -> bytes:
        end = self.offset + length
        if length < 0 or end > len(self.data):
            raise QueryError("truncated A2S_INFO response")
        value = self.data[self.offset:end]
        self.offset = end
        return value

    def byte(self) -> int:
        return self.take(1)[0]

    def skip_cstring(self) -> None:
        search_end = min(len(self.data), self.offset + MAX_CSTRING + 1)
        end = self.data.find(b"\0", self.offset, search_end)
        if end < 0:
            raise QueryError("invalid A2S_INFO string")
        self.offset = end + 1

    def cstring(self, pattern: re.Pattern[str]) -> str:
        search_end = min(len(self.data), self.offset + MAX_CSTRING + 1)
        end = self.data.find(b"\0", self.offset, search_end)
        if end < 0:
            raise QueryError("invalid A2S_INFO string")
        value = self.data[self.offset:end]
        self.offset = end + 1

        decoded = value.decode("utf-8", errors="replace")
        if pattern.fullmatch(decoded) is None:
            return "<redacted>"
        return decoded


def _private_ip(value: str) -> ipaddress.IPv4Address | ipaddress.IPv6Address:
    try:
        address = ipaddress.ip_address(value)
    except ValueError as error:
        raise QueryError("target must be a private IP literal") from error

    is_allowed = False
    for network in PRIVATE_NETWORKS:
        if address.version == network.version and address in network:
            is_allowed = True
            break

    if not is_allowed:
        raise QueryError("target must be a private IP literal")
    return address


def _receive(sock: socket.socket) -> bytes:
    packet = sock.recv(MAX_DATAGRAM + 1)
    if len(packet) > MAX_DATAGRAM:
        raise QueryError("A2S_INFO datagram exceeds the size limit")
    return packet


def _read_info(sock: socket.socket, requests: list[int], responses: list[int]) -> bytes:
    packet = _receive(sock)
    responses.append(len(packet))
    if len(packet) < 5 or packet[:4] != RESPONSE_HEADER:
        raise QueryError("invalid A2S_INFO response header")
    if packet[4] == ord("A"):
        if len(packet) != 9:
            raise QueryError("invalid A2S_INFO challenge")
        challenge_request = QUERY_PREFIX + packet[5:9]
        sock.send(challenge_request)
        requests.append(len(challenge_request))
        packet = _receive(sock)
        responses.append(len(packet))
        if len(packet) < 5 or packet[:4] != RESPONSE_HEADER:
            raise QueryError("invalid A2S_INFO response header")
    return packet


def _parse_source(
    packet: bytes,
    requests: list[int],
    responses: list[int],
    challenged: bool,
) -> QuerySummary:
    cursor = Cursor(packet, 5)
    protocol = cursor.byte()
    cursor.skip_cstring()  # server name may contain private information
    map_name = cursor.cstring(SAFE_MAP)
    game_directory = cursor.cstring(SAFE_DIRECTORY)
    game = cursor.cstring(SAFE_GAME)
    cursor.take(2)  # App ID; not needed in the sanitized summary
    players = cursor.byte()
    max_players = cursor.byte()
    cursor.byte()  # bot count
    cursor.take(2)  # server type and platform
    cursor.take(2)  # password and VAC flags
    version = cursor.cstring(SAFE_VERSION)

    return QuerySummary(
        response_type="source",
        protocol=protocol,
        map=map_name,
        game_directory=game_directory,
        game=game,
        players=players,
        max_players=max_players,
        version=version,
        request_lengths=requests.copy(),
        response_lengths=responses.copy(),
        challenge_roundtrip=challenged,
    )


def _parse_goldsrc(
    packet: bytes,
    requests: list[int],
    responses: list[int],
    challenged: bool,
) -> QuerySummary:
    cursor = Cursor(packet, 5)
    cursor.skip_cstring()  # address may identify a private host
    cursor.skip_cstring()  # server name may contain private information
    map_name = cursor.cstring(SAFE_MAP)
    game_directory = cursor.cstring(SAFE_DIRECTORY)
    game = cursor.cstring(SAFE_GAME)
    players = cursor.byte()
    max_players = cursor.byte()
    protocol = cursor.byte()

    return QuerySummary(
        response_type="goldsrc_legacy",
        protocol=protocol,
        map=map_name,
        game_directory=game_directory,
        game=game,
        players=players,
        max_players=max_players,
        version="not_included",
        request_lengths=requests.copy(),
        response_lengths=responses.copy(),
        challenge_roundtrip=challenged,
    )


def parse_info_response(
    packet: bytes,
    requests: list[int] | None = None,
    responses: list[int] | None = None,
    challenged: bool = False,
) -> QuerySummary:
    request_lengths = requests if requests is not None else [len(QUERY_PREFIX)]
    response_lengths = responses if responses is not None else [len(packet)]
    if len(packet) < 5 or packet[:4] != RESPONSE_HEADER:
        raise QueryError("invalid A2S_INFO response header")
    if packet[4] == ord("I"):
        return _parse_source(packet, request_lengths, response_lengths, challenged)
    if packet[4] == ord("m"):
        return _parse_goldsrc(packet, request_lengths, response_lengths, challenged)
    raise QueryError("unsupported A2S_INFO response type")


def query(host: str, port: int, timeout: float = 2.0) -> QuerySummary:
    address = _private_ip(host)
    if not 1 <= port <= 65535:
        raise QueryError("port is outside the valid range")
    if not 0 < timeout <= 10:
        raise QueryError("timeout must be between 0 and 10 seconds")

    if isinstance(address, ipaddress.IPv6Address):
        family = socket.AF_INET6
    else:
        family = socket.AF_INET
    requests = [len(QUERY_PREFIX)]
    responses: list[int] = []
    with socket.socket(family, socket.SOCK_DGRAM) as sock:
        sock.settimeout(timeout)
        sock.connect((str(address), port))
        sock.send(QUERY_PREFIX)
        packet = _read_info(sock, requests, responses)

    if packet[4] == ord("A"):
        raise QueryError("repeated A2S_INFO challenge")
    if packet[:4] != RESPONSE_HEADER:
        raise QueryError("invalid A2S_INFO response header")
    if packet[4] not in (ord("I"), ord("m")):
        raise QueryError("unsupported or split A2S_INFO response")
    challenged = len(requests) == 2
    return parse_info_response(packet, requests, responses, challenged)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("address", help="loopback/private IP literal; never printed")
    parser.add_argument("--port", type=int, default=27015)
    parser.add_argument("--timeout", type=float, default=2.0)
    args = parser.parse_args()

    try:
        summary = query(args.address, args.port, args.timeout)
    except (OSError, QueryError):
        print(
            "query failed; check the private endpoint and local HLDS health",
            file=sys.stderr,
        )
        return 2

    print(json.dumps(asdict(summary), sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

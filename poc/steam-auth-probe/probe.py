"""Bounded local SDK diagnostic; never accepts or logs player tickets."""

import argparse
import ctypes
import json
import os
from pathlib import Path
import platform
import re
import socket
import subprocess
import sys
import tempfile
import time


INTERFACES = (
    "STEAMUTILS_INTERFACE_VERSION",
    "STEAMNETWORKINGUTILS_INTERFACE_VERSION",
    "STEAMGAMESERVER_INTERFACE_VERSION",
    "STEAMGAMESERVERSTATS_INTERFACE_VERSION",
    "STEAMHTTP_INTERFACE_VERSION",
    "STEAMINVENTORY_INTERFACE_VERSION",
    "STEAMNETWORKING_INTERFACE_VERSION",
    "STEAMNETWORKINGMESSAGES_INTERFACE_VERSION",
    "STEAMNETWORKINGSOCKETS_INTERFACE_VERSION",
    "STEAMUGC_INTERFACE_VERSION",
)


def emit(event, **fields):
    print(json.dumps({"event": event, **fields}), flush=True)


def interface_versions(headers):
    definitions = {}
    for header in headers.glob("*.h"):
        if header.name.startswith("._"):
            continue
        text = header.read_text(encoding="utf-8", errors="replace")
        for name, value in re.findall(r'^#define\s+(\w+)\s+"([^"\r\n]+)"', text, re.M):
            definitions[name] = value
    values = [definitions[name] for name in INTERFACES]
    if definitions["STEAMGAMESERVER_INTERFACE_VERSION"] != "SteamGameServer015":
        raise ValueError("Probe requires SteamGameServer015")
    return ("\0".join(values) + "\0\0").encode("ascii")


def library_dir(machine):
    if machine == "x86_64":
        return "linux64"
    if machine in ("aarch64", "arm64"):
        return "linuxarm64"
    raise ValueError("Probe supports Linux x86-64/AArch64 only")


def validate_library(path, machine):
    expected = {"x86_64": 62, "aarch64": 183, "arm64": 183}[machine]
    with path.open("rb") as source:
        header = source.read(20)
    if (len(header) != 20 or header[:6] != b"\x7fELF\x02\x01"
            or int.from_bytes(header[18:20], "little") != expected):
        raise ValueError(f"Library is not a same-architecture 64-bit Linux ELF: {path}")


class Callback(ctypes.Structure):
    # Linux SDK VALVE_CALLBACK_PACK_SMALL uses 4-byte alignment.
    _pack_ = 4
    _fields_ = [
        ("user", ctypes.c_int32),
        ("identifier", ctypes.c_int32),
        ("data", ctypes.c_void_p),
        ("size", ctypes.c_int32),
    ]


def bind(api, name, result, *arguments):
    function = getattr(api, name)
    function.restype = result
    function.argtypes = list(arguments)
    return function


def native_probe(args):
    sdk = Path(args.sdk)
    api = ctypes.CDLL(str(sdk / "redistributable_bin" / library_dir(platform.machine()) / "libsteam_api.so"))
    emit("library_loaded", architecture=platform.machine())
    init = bind(api, "SteamInternal_GameServer_Init_V2", ctypes.c_int,
                ctypes.c_uint32, ctypes.c_uint16, ctypes.c_uint16, ctypes.c_int,
                ctypes.c_char_p, ctypes.c_char_p, ctypes.c_void_p)
    shutdown = bind(api, "SteamGameServer_Shutdown", None)
    accessor = bind(api, "SteamAPI_SteamGameServer_v015", ctypes.c_void_p)
    advertise = bind(api, "SteamAPI_ISteamGameServer_SetAdvertiseServerActive", None,
                     ctypes.c_void_p, ctypes.c_bool)
    set_product = bind(api, "SteamAPI_ISteamGameServer_SetProduct", None, ctypes.c_void_p, ctypes.c_char_p)
    set_description = bind(api, "SteamAPI_ISteamGameServer_SetGameDescription", None, ctypes.c_void_p, ctypes.c_char_p)
    set_mod = bind(api, "SteamAPI_ISteamGameServer_SetModDir", None, ctypes.c_void_p, ctypes.c_char_p)
    set_dedicated = bind(api, "SteamAPI_ISteamGameServer_SetDedicatedServer", None, ctypes.c_void_p, ctypes.c_bool)
    logon = bind(api, "SteamAPI_ISteamGameServer_LogOnAnonymous", None, ctypes.c_void_p)
    logoff = bind(api, "SteamAPI_ISteamGameServer_LogOff", None, ctypes.c_void_p)
    logged_on = bind(api, "SteamAPI_ISteamGameServer_BLoggedOn", ctypes.c_bool, ctypes.c_void_p)
    identity = bind(api, "SteamAPI_ISteamGameServer_GetSteamID", ctypes.c_uint64, ctypes.c_void_p)
    dispatch_init = bind(api, "SteamAPI_ManualDispatch_Init", None)
    pipe_handle = bind(api, "SteamGameServer_GetHSteamPipe", ctypes.c_int32)
    run_frame = bind(api, "SteamAPI_ManualDispatch_RunFrame", None, ctypes.c_int32)
    get_next = bind(api, "SteamAPI_ManualDispatch_GetNextCallback", ctypes.c_bool,
                    ctypes.c_int32, ctypes.POINTER(Callback))
    free_last = bind(api, "SteamAPI_ManualDispatch_FreeLastCallback", None, ctypes.c_int32)
    error = ctypes.create_string_buffer(1024)
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as reservation:
        reservation.bind(("127.0.0.1", 0))
        query_port = reservation.getsockname()[1]
    # Non-VAC authentication mode, never no-auth mode. No gameplay listener.
    result = init(0x7f000001, 27035, query_port, 2, b"1.1.2.7",
                  interface_versions(sdk / "public" / "steam"), error)
    if result != 0:
        emit("init_failed", result=result, diagnostic=error.value.decode("utf-8", errors="replace"))
        return 2
    server = None
    try:
        server = accessor()
        if not server:
            emit("interface_unavailable")
            return 2
        advertise(server, False)
        set_product(server, b"cstrike")
        set_description(server, b"Local Rust server SDK diagnostic")
        set_mod(server, b"cstrike")
        set_dedicated(server, True)
        emit("initialized", advertising=False, logon_requested=args.logon)
        if not args.logon:
            return 0
        dispatch_init()
        pipe = pipe_handle()
        if pipe == 0:
            emit("callback_pipe_unavailable")
            return 2
        logon(server)
        deadline = time.monotonic() + args.seconds
        callback = Callback()
        while time.monotonic() < deadline:
            run_frame(pipe)
            for _ in range(256):
                if not get_next(pipe, ctypes.byref(callback)):
                    break
                try:
                    if callback.identifier == 101:
                        valid = bool(logged_on(server) and identity(server) != 0)
                        advertise(server, False)
                        emit("server_connected", logged_on=bool(logged_on(server)), identity_valid=valid)
                        return 0 if valid else 3
                    if callback.identifier in (102, 103):
                        if callback.size < 4 or not callback.data:
                            emit("malformed_callback", callback_id=callback.identifier)
                            return 3
                        code = int.from_bytes(ctypes.string_at(callback.data, 4), "little", signed=True)
                        emit("server_connect_failed" if callback.identifier == 102 else "server_disconnected", result=code)
                        return 3
                finally:
                    free_last(pipe)
            time.sleep(0.05)
        emit("logon_timeout", seconds=args.seconds)
        return 4
    finally:
        if server:
            advertise(server, False)
            logoff(server)
        shutdown()
        emit("shutdown")


def main():
    root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sdk", default=str(root / "fixtures/local/steamworks-sdk"))
    parser.add_argument("--runtime", default=str(root / "fixtures/local/reference-hlds/linux64"))
    parser.add_argument("--logon", action="store_true", help="Contact Steam for anonymous server logon")
    parser.add_argument("--seconds", type=int, choices=range(1, 121), default=30, metavar="1..120")
    parser.add_argument("--native-child", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--run-server", help="Run the prebuilt opt-in Rust Steam diagnostic for 120 seconds")
    parser.add_argument("--listen", default="127.0.0.1:27025", help="Rust server private bind address")
    parser.add_argument("--trace", help="New local JSONL trace path for the Rust server")
    args = parser.parse_args()
    if sys.platform != "linux":
        parser.error("Linux only")
    args.sdk = str(Path(args.sdk).resolve())
    args.runtime = str(Path(args.runtime).resolve())
    try:
        architecture = library_dir(platform.machine())
        interface_versions(Path(args.sdk) / "public/steam")
        for path in (Path(args.runtime) / "steamclient.so", Path(args.sdk) / "redistributable_bin" / architecture / "libsteam_api.so"):
            if not path.is_file():
                raise ValueError(f"Required library missing: {path}")
            validate_library(path, platform.machine())
        if args.native_child:
            return native_probe(args)
        local = root / "fixtures/local"
        local.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="steam-probe-", dir=local) as work:
            home = Path(work).resolve()
            (home / ".steam").mkdir()
            (home / ".steam/sdk64").symlink_to(args.runtime, target_is_directory=True)
            env = {
                "PATH": os.defpath, "HOME": str(home), "LANG": "C.UTF-8",
                "SteamAppId": "10", "SteamGameId": "10",
                "LD_LIBRARY_PATH": args.runtime,
            }
            if args.run_server:
                binary = Path(args.run_server).resolve()
                if not binary.is_file():
                    raise ValueError("Build the Rust binary before --run-server")
                versions_path = home / "interfaces.bin"
                versions_path.write_bytes(interface_versions(Path(args.sdk) / "public/steam"))
                env["GOLDSRC_STEAM_API"] = str(Path(args.sdk) / "redistributable_bin" / architecture / "libsteam_api.so")
                env["GOLDSRC_STEAM_INTERFACES"] = str(versions_path)
                env["GOLDSRC_STEAM_SECONDS"] = str(args.seconds)
                env["LISTEN_ADDRESS"] = args.listen
                if args.trace:
                    env["GOLDSRC_PACKET_TRACE"] = str(Path(args.trace).resolve())
                try:
                    return subprocess.run([str(binary)], cwd=home, env=env, timeout=args.seconds + 35, check=False).returncode
                except subprocess.TimeoutExpired:
                    emit("native_timeout")
                    return 5
            command = [sys.executable, str(Path(__file__).resolve()), "--native-child",
                       "--sdk", args.sdk, "--runtime", args.runtime, "--seconds", str(args.seconds)]
            if args.logon:
                command.append("--logon")
            try:
                completed = subprocess.run(command, cwd=home, env=env, timeout=args.seconds + 15, check=False)
                if completed.returncode < 0:
                    emit("native_crash", signal=-completed.returncode)
                return completed.returncode if completed.returncode >= 0 else 5
            except subprocess.TimeoutExpired:
                emit("native_timeout")
                return 5
    except (OSError, ValueError, KeyError, AttributeError) as error:
        emit("probe_error", diagnostic=str(error))
        return 2


if __name__ == "__main__":
    raise SystemExit(main())

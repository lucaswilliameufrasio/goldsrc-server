# Capture only per-client callback results and user-message list deltas.
# Run against the local reference HLDS; no packet or identity data is written.
# This one-shot run follows the client to SV_SendEnts_f and samples only
# sanitized client-state flags; it records no identity or movement values.
python
import os
import re

capture_id = os.environ.get("GOLDSRC_CAPTURE_ID", "")
if not re.fullmatch(r"[A-Za-z0-9._-]{1,40}", capture_id):
    raise RuntimeError("GOLDSRC_CAPTURE_ID must be a short safe identifier")

output_path = f"/out/hlds-client-connect-build10211-{capture_id}.tsv"
if os.path.exists(output_path):
    raise RuntimeError("refusing to overwrite an existing callback capture")

state = {
    "events": [],
    "callback_count": 0,
    "callback_results": [],
    "callback_depth": 0,
    "regusermsg_count": 0,
    "regusermsg_results": [],
    "senduserreg_count": 0,
    "senduserreg_samples": [],
    "senduserreg_overflow": False,
    "new_count": 0,
    "spawn_count": 0,
    "sendres_count": 0,
    "sendresources_count": 0,
    "writes_spawn_count": 0,
    "parsemove_count": 0,
    "sendents_count": 0,
    "sendents_flags": {},
    "capture_started": False,
}
MAX_REGISTRATIONS = 192
MAX_CALLBACKS = 8
MAX_REGUSERMSG_CALLS = 64
MAX_SEND_USER_REG_CALLS = 256
MAX_CAPTURE_BYTES = 32_768

def registrations(symbol):
    cursor = gdb.parse_and_eval(symbol)
    seen = set()
    records = []
    while int(cursor) != 0:
        address = int(cursor)
        if address in seen:
            raise RuntimeError("cycle in local HLDS user-message registry")
        if len(records) >= MAX_REGISTRATIONS:
            raise RuntimeError("local HLDS user-message registry exceeds the bound")
        seen.add(address)
        message = cursor.dereference()
        message_id = int(message["iMsg"])
        message_size = int(message["iSize"])
        if not 64 <= message_id <= 255 or not -1 <= message_size <= 255:
            raise RuntimeError("invalid local HLDS user-message metadata")
        records.append((message_id, message_size))
        cursor = message["next"]
    ids = [record[0] for record in records]
    if len(set(ids)) != len(ids):
        raise RuntimeError("duplicate IDs in local HLDS user-message registry")
    return tuple(records)

def list_delta(current, previous):
    current_by_id = {message_id: size for message_id, size in current}
    previous_by_id = {message_id: size for message_id, size in previous}
    added = sorted(
        (message_id, size)
        for message_id, size in current_by_id.items()
        if message_id not in previous_by_id
    )
    removed = sorted(
        (message_id, size)
        for message_id, size in previous_by_id.items()
        if message_id not in current_by_id
    )
    changed = sorted(
        (message_id, current_by_id[message_id], previous_by_id[message_id])
        for message_id in current_by_id.keys() & previous_by_id.keys()
        if current_by_id[message_id] != previous_by_id[message_id]
    )
    return added, removed, changed

def format_records(records):
    if not records:
        return "-"
    return ",".join(f"{message_id}:{size}" for message_id, size in records)

def append_capture(lines):
    payload = "\n".join(lines) + "\n"
    encoded = payload.encode("ascii")
    current_size = os.path.getsize(output_path)
    if current_size + len(encoded) > MAX_CAPTURE_BYTES:
        raise RuntimeError("sanitized callback capture exceeds the byte bound")
    with open(output_path, "a", encoding="ascii") as capture:
        capture.write(payload)

def begin_capture():
    with open(output_path, "x", encoding="ascii") as capture:
        capture.write("field\tvalue\ncapture_status\tstarted\n")
    state["capture_started"] = True

def format_event(event):
    return (
        "event\t{stage}\tresult={result}\tactive={active_count}"
        "\tpending={pending_count}\tcallback_active={callback_active}"
        "\tactive_added={active_added}"
        "\tactive_removed={active_removed}\tactive_changed={active_changed}"
        "\tpending_added={pending_added}\tpending_removed={pending_removed}"
        "\tpending_changed={pending_changed}\tpromoted={promoted}".format(
            stage=event["stage"],
            result=event["result"],
            active_count=len(event["active"]),
            pending_count=len(event["pending"]),
            callback_active=int(event["callback_active"]),
            active_added=format_records(event["active_added"]),
            active_removed=format_records(event["active_removed"]),
            active_changed=event["active_changed"] or "-",
            pending_added=format_records(event["pending_added"]),
            pending_removed=format_records(event["pending_removed"]),
            pending_changed=event["pending_changed"] or "-",
            promoted=event["promoted"] or "-",
        )
    )

def snapshot(stage, result="-"):
    active = registrations("sv_gpUserMsgs")
    pending = registrations("sv_gpNewUserMsgs")
    previous = state["events"][-1] if state["events"] else None
    if previous is None:
        active_added, active_removed, active_changed = (), (), ()
        pending_added, pending_removed, pending_changed = (), (), ()
        promoted = ()
    else:
        active_added, active_removed, active_changed = list_delta(active, previous["active"])
        pending_added, pending_removed, pending_changed = list_delta(pending, previous["pending"])
        previous_pending_ids = {message_id for message_id, _ in previous["pending"]}
        current_pending_ids = {message_id for message_id, _ in pending}
        current_active_ids = {message_id for message_id, _ in active}
        promoted = sorted(previous_pending_ids & current_active_ids - current_pending_ids)
    event = {
        "stage": stage,
        "result": result,
        "callback_active": state["callback_depth"] > 0,
        "active": active,
        "pending": pending,
        "active_added": active_added,
        "active_removed": active_removed,
        "active_changed": active_changed,
        "pending_added": pending_added,
        "pending_removed": pending_removed,
        "pending_changed": pending_changed,
        "promoted": promoted,
    }
    state["events"].append(event)
    if state["capture_started"]:
        append_capture([format_event(event)])
    return event

def write_capture(status):
    lines = [
        f"capture_status\t{status}",
        f"callback_entries\t{state['callback_count']}",
        "callback_results\t" + (",".join(state["callback_results"]) or "none"),
        f"regusermsg_calls\t{state['regusermsg_count']}",
        "regusermsg_results\t" + (",".join(state["regusermsg_results"]) or "none"),
        f"sv_senduserreg_calls\t{state['senduserreg_count']}",
        f"sv_senduserreg_overflow\t{int(state['senduserreg_overflow'])}",
        "sv_senduserreg_samples\t" + (",".join(state["senduserreg_samples"]) or "none"),
        f"new_command_entries\t{state['new_count']}",
        f"spawn_command_entries\t{state['spawn_count']}",
        f"sendres_command_entries\t{state['sendres_count']}",
        f"sendresources_calls\t{state['sendresources_count']}",
        f"write_spawn_calls\t{state['writes_spawn_count']}",
        f"parsemove_entries\t{state['parsemove_count']}",
        f"sendents_command_entries\t{state['sendents_count']}",
    ]
    for field in ("connected", "spawned", "active", "fully_connected"):
        lines.append(f"host_client_{field}\t{state['sendents_flags'].get(field, 'unknown')}")
    append_capture(lines)

def client_flag(field):
    try:
        value = int(gdb.parse_and_eval(f"host_client->{field}"))
    except Exception:
        return "unknown"
    if value not in {0, 1}:
        return "unknown"
    return str(value)

class ClientConnectReturn(gdb.FinishBreakpoint):
    def __init__(self, frame, callback_number):
        super().__init__(frame, internal=True)
        self.callback_number = callback_number

    def stop(self):
        try:
            value = self.return_value
            result = "unknown" if value is None else str(int(value))
        except Exception:
            result = "unknown"
        state["callback_results"].append(result)
        state["callback_depth"] -= 1
        snapshot(f"clientconnect_return_{self.callback_number}", result)
        return False

class ClientConnectEntry(gdb.Breakpoint):
    def __init__(self):
        super().__init__("ClientConnect", internal=True)

    def stop(self):
        state["callback_count"] += 1
        if state["callback_count"] > MAX_CALLBACKS:
            self.enabled = False
            return False
        state["callback_depth"] += 1
        snapshot(f"clientconnect_entry_{state['callback_count']}")
        ClientConnectReturn(gdb.newest_frame(), state["callback_count"])
        return False

class RegUserMsgReturn(gdb.FinishBreakpoint):
    def __init__(self, frame, call_number):
        super().__init__(frame, internal=True)
        self.call_number = call_number

    def stop(self):
        try:
            value = self.return_value
            result = "unknown" if value is None else str(int(value))
        except Exception:
            result = "unknown"
        state["regusermsg_results"].append(result)
        snapshot(f"regusermsg_return_{self.call_number}", result)
        return False

class RegUserMsgEntry(gdb.Breakpoint):
    def __init__(self):
        super().__init__("RegUserMsg", internal=True)

    def stop(self):
        state["regusermsg_count"] += 1
        if state["regusermsg_count"] > MAX_REGUSERMSG_CALLS:
            self.enabled = False
            return False
        snapshot(f"regusermsg_entry_{state['regusermsg_count']}")
        RegUserMsgReturn(gdb.newest_frame(), state["regusermsg_count"])
        return False

class SendUserRegEntry(gdb.Breakpoint):
    def __init__(self):
        super().__init__("SV_SendUserReg", internal=True)

    def stop(self):
        state["senduserreg_count"] += 1
        if state["senduserreg_count"] > MAX_SEND_USER_REG_CALLS:
            state["senduserreg_overflow"] = True
            self.enabled = False
            return False
        call_number = state["senduserreg_count"]
        if call_number == 1 or call_number & (call_number - 1) == 0:
            state["senduserreg_samples"].append(str(call_number))
            snapshot(f"sv_senduserreg_call_{call_number}")
        return False

class NewCommandReturn(gdb.FinishBreakpoint):
    def stop(self):
        snapshot("sv_new_return")
        return False

class NewCommandEntry(gdb.Breakpoint):
    def __init__(self):
        super().__init__("SV_New_f", internal=True, temporary=True)

    def stop(self):
        state["new_count"] += 1
        snapshot("sv_new_entry")
        NewCommandReturn(gdb.newest_frame(), internal=True)
        return False

class SpawnCommandEntry(gdb.Breakpoint):
    def __init__(self):
        super().__init__("SV_Spawn_f", internal=True, temporary=True)

    def stop(self):
        state["spawn_count"] += 1
        snapshot("sv_spawn_entry")
        SpawnCommandReturn(gdb.newest_frame(), internal=True)
        return False

class SpawnCommandReturn(gdb.FinishBreakpoint):
    def stop(self):
        snapshot("sv_spawn_return")
        ParseMoveEntry()
        return False

class SendResCommandEntry(gdb.Breakpoint):
    def __init__(self):
        super().__init__("SV_SendRes_f", internal=True, temporary=True)

    def stop(self):
        state["sendres_count"] += 1
        snapshot("sv_sendres_entry")
        SendResCommandReturn(gdb.newest_frame(), internal=True)
        return False

class SendResCommandReturn(gdb.FinishBreakpoint):
    def stop(self):
        snapshot("sv_sendres_return")
        return False

class SendResourcesEntry(gdb.Breakpoint):
    def __init__(self):
        super().__init__("SV_SendResources", internal=True, temporary=True)

    def stop(self):
        state["sendresources_count"] += 1
        snapshot("sv_sendresources_entry")
        return False

class WriteSpawnEntry(gdb.Breakpoint):
    def __init__(self):
        super().__init__("SV_WriteSpawn", internal=True, temporary=True)

    def stop(self):
        state["writes_spawn_count"] += 1
        snapshot("sv_writespawn_entry")
        return False

class ParseMoveEntry(gdb.Breakpoint):
    def __init__(self):
        super().__init__("SV_ParseMove", internal=True)

    def stop(self):
        state["parsemove_count"] += 1
        snapshot("sv_parsemove_entry")
        ParseMoveReturn(gdb.newest_frame(), internal=True)
        self.enabled = False
        return False

class ParseMoveReturn(gdb.FinishBreakpoint):
    def stop(self):
        snapshot("sv_parsemove_return")
        return False

class SendEntsEntry(gdb.Breakpoint):
    def __init__(self):
        super().__init__("SV_SendEnts_f", internal=True, temporary=True)

    def stop(self):
        state["sendents_count"] += 1
        snapshot("sv_sendents_entry")
        SendEntsReturn(gdb.newest_frame(), internal=True)
        return False

class SendEntsReturn(gdb.FinishBreakpoint):
    def stop(self):
        snapshot("sv_sendents_return")
        state["sendents_flags"] = {
            field: client_flag(field)
            for field in ("connected", "spawned", "active", "fully_connected")
        }
        status = (
            "client_fully_connected"
            if state["sendents_flags"]["fully_connected"] == "1"
            else "sendents_returned_without_full_connection"
        )
        write_capture(status)
        print(
            "HLDS_CLIENT_FULL_CONNECTION_CAPTURE_COMPLETE "
            f"callbacks={state['callback_count']} spawns={state['spawn_count']} "
            f"sendres={state['sendres_count']} sendents={state['sendents_count']} "
            f"fully_connected={state['sendents_flags']['fully_connected']}"
        )
        return True
end

set pagination off
set confirm off
set inferior-tty /dev/null
set breakpoint pending on
set disable-randomization off
break SV_ActivateServer
run -game cstrike -console -nomaster +maxplayers 2 +ip 0.0.0.0 +sv_lan 1 +map de_dust2 +log off
finish

python
ClientConnectEntry()
RegUserMsgEntry()
SendUserRegEntry()
NewCommandEntry()
SpawnCommandEntry()
SendResCommandEntry()
SendResourcesEntry()
WriteSpawnEntry()
SendEntsEntry()
if os.environ.get("GOLDSRC_CAPTURE_DRY_RUN") == "1":
    gdb.parse_and_eval("&SV_Spawn_f")
    gdb.parse_and_eval("&SV_SendRes_f")
    gdb.parse_and_eval("&SV_SendResources")
    gdb.parse_and_eval("&SV_WriteSpawn")
    gdb.parse_and_eval("&SV_ParseMove")
    gdb.parse_and_eval("&SV_SendEnts_f")
    gdb.parse_and_eval("sizeof(host_client->fully_connected)")
    print("HLDS_CLIENT_CALLBACK_INSTRUMENTATION_READY")
    gdb.execute("quit")
begin_capture()
end

continue
quit

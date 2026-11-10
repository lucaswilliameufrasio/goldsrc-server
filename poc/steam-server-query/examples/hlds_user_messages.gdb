# Capture the startup user-message registry after the reference map is active.
# This is offline metadata only; it does not run a client or invoke ClientConnect.
python
import os
import re

capture_id = os.environ.get("GOLDSRC_CAPTURE_ID", "")
if not re.fullmatch(r"[A-Za-z0-9._-]{1,40}", capture_id):
    raise RuntimeError("GOLDSRC_CAPTURE_ID must be a short safe identifier")

output_path = f"/out/hlds-user-messages-build10211-{capture_id}.tsv"
if os.path.exists(output_path):
    raise RuntimeError("refusing to overwrite an existing user-message capture")
end

set pagination off
set confirm off
set inferior-tty /dev/null
set breakpoint pending on
set disable-randomization off
break Host_Map
run -game cstrike -console +maxplayers 2 +sv_lan 1 +map de_dust2 +log off
finish

python
import gdb

cursor = gdb.parse_and_eval("sv_gpUserMsgs")
seen = set()
rows = []
while int(cursor) != 0:
    address = int(cursor)
    if address in seen:
        raise RuntimeError("cycle in local HLDS user-message registry")
    if len(rows) >= 192:
        raise RuntimeError("local HLDS user-message registry exceeds the bound")
    seen.add(address)
    message = cursor.dereference()
    message_id = int(message["iMsg"])
    message_size = int(message["iSize"])
    name = message["szName"].string()
    if isinstance(name, bytes):
        name = name.decode("ascii", errors="strict")
    if not 64 <= message_id <= 255:
        raise RuntimeError("invalid local HLDS user-message id")
    if not -1 <= message_size <= 255:
        raise RuntimeError("invalid local HLDS user-message size")
    if not 1 <= len(name) <= 15 or not all(33 <= ord(char) <= 126 for char in name):
        raise RuntimeError("invalid local HLDS user-message name")
    rows.append(f"{message_id}\t{message_size}\t{name}")
    cursor = message["next"]

if not rows:
    raise RuntimeError("local HLDS user-message registry is empty")
ids = [int(row.split("\t", 1)[0]) for row in rows]
names = [row.split("\t", 2)[2] for row in rows]
if len(set(ids)) != len(ids) or len(set(names)) != len(names):
    raise RuntimeError("duplicate local HLDS user-message registration")

with open(output_path, "x", encoding="ascii") as capture:
    if rows:
        capture.write("\n".join(rows) + "\n")

print(f"Captured startup user-message registrations: {len(rows)}")
end
quit

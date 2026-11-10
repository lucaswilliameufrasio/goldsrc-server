# Export selected-map metadata and a wire digest from isolated local HLDS.
# Writes no packet bytes, raw memory dump, or client/player identity.
python
import os
import re

map_name = os.environ.get('GOLDSRC_RESOURCE_MAP', '')
if not re.fullmatch(r'(de_dust2_2x2|fy_pool_day)', map_name):
    raise RuntimeError('unsupported selected map')
out = '/out/' + map_name
paths = [out + '-manifest.tsv', out + '-checks.tsv', out + '-wire-summary.tsv']
for path in paths:
    if os.path.exists(path):
        raise RuntimeError('refusing to overwrite local selected-map evidence')
end
set breakpoint pending on
set disable-randomization off
break SV_ActivateServer
python
gdb.execute('run -game cstrike -console +maxplayers 2 +sv_lan 1 +map ' + map_name)
end
finish
set $buf = (unsigned char*)malloc(32)
python
sv = gdb.parse_and_eval('sv')
spawn_count = int(gdb.parse_and_eval('svs.spawncount'))
resource_count = int(sv['num_resources'])
consistency_count = int(sv['num_consistency'])
custom = 0
reserved = 0
size_sentinels = 0
for index in range(resource_count):
    if int(sv['resourcelist'][index]['ucFlags']) & 4:
        custom += 1
if custom != 0:
    raise RuntimeError('selected map uses unsupported custom resource fields')
with open(out + '-manifest.tsv', 'x') as manifest:
    for index in range(resource_count):
        resource = sv['resourcelist'][index]
        name = resource['szFileName'].string()
        flags = int(resource['ucFlags'])
        download_size = int(resource['nDownloadSize'])
        if download_size < -1 or download_size > 0x00ffffff:
            raise RuntimeError('unsupported resource download size')
        if download_size == -1:
            size_sentinels += 1
        reserved_bytes = bytes(int(resource['rguc_reserved'][n]) for n in range(32))
        if any(reserved_bytes):
            reserved += 1
        if '\t' in name or '\n' in name:
            raise RuntimeError('unsupported resource name')
        manifest.write('%s\t%d\t%d\t%d\t%d\t%s\n' % (
            name,
            int(resource['type']),
            int(resource['nIndex']),
            download_size & 0x00ffffff,
            flags,
            reserved_bytes.hex(),
        ))
with open(out + '-checks.tsv', 'x') as checks:
    written = 0
    for index in range(resource_count):
        resource = sv['resourcelist'][index]
        if not (int(resource['ucFlags']) & 128):
            continue
        raw = bytes(int(resource['rguc_reserved'][n]) for n in range(32))
        hash_bytes = bytes(int(resource['rgucMD5_hash'][n]) for n in range(4))
        decoded = raw
        if any(raw):
            buffer = int(gdb.parse_and_eval('$buf'))
            gdb.selected_inferior().write_memory(buffer, raw)
            gdb.execute('call (void)COM_UnMunge($buf,32,svs.spawncount)', to_string=True)
            decoded = bytes(gdb.selected_inferior().read_memory(buffer, 32))
        checks.write('%d\t%s\t%s\t%s\n' % (
            index,
            hash_bytes.hex(),
            raw.hex(),
            decoded.hex(),
        ))
        written += 1
if written != consistency_count:
    raise RuntimeError('HLDS resource consistency count changed during export')
print('HLDS selected map: %s resources=%d consistency=%d reserved=%d size_sentinels=%d spawn=%d' % (
    map_name,
    resource_count,
    consistency_count,
    reserved,
    size_sentinels,
    spawn_count,
))
end
set host_client = &svs.clients[0]
python
if int(gdb.parse_and_eval('host_client->proxy')) != 0:
    raise RuntimeError('synthetic consistency probe must not use a proxy slot')
if int(gdb.parse_and_eval('mp_consistency.value')) != 1:
    raise RuntimeError('HLDS consistency is not enabled for this reference run')
end
set net_message.cursize = 0
call (void)SV_SendResources(&net_message)
python
import hashlib

length = int(gdb.parse_and_eval('net_message.cursize'))
address = int(gdb.parse_and_eval('&net_message.data[0]'))
message = bytes(gdb.selected_inferior().read_memory(address, length))
digest = hashlib.md5(message).hexdigest()
with open(out + '-wire-summary.tsv', 'x') as summary:
    summary.write('%s\t%d\t%d\t%d\t%d\t%d\t%s\n' % (
        map_name,
        int(gdb.parse_and_eval('svs.spawncount')),
        int(gdb.parse_and_eval('sv.num_resources')),
        int(gdb.parse_and_eval('sv.num_consistency')),
        size_sentinels,
        length,
        digest,
    ))
print('HLDS selected-map resource message: length=%d md5=%s' % (length, digest))
end
quit

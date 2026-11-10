# Isolated offline reference: disables consistency only in this disposable process.
python
import os
for path in ['/out/hlds-manifest-10211-v2.tsv', '/out/hlds-manifest-10211-v2.bin']:
    if os.path.exists(path):
        raise RuntimeError('refusing to overwrite local resource evidence')
end
set breakpoint pending on
set disable-randomization off
break SV_ActivateServer
run -game cstrike -console +maxplayers 2 +sv_lan 1 +map de_dust2
finish
python
sv = gdb.parse_and_eval('sv')
count = int(sv['num_resources'])
consistency = int(sv['num_consistency'])
custom = 0
reserved = 0
with open('/out/hlds-manifest-10211-v2.tsv', 'x') as f:
    for i in range(count):
        r = sv['resourcelist'][i]
        name = r['szFileName'].string()
        flags = int(r['ucFlags'])
        if flags & 4:
            custom += 1
        reserved_bytes = bytes(int(r['rguc_reserved'][n]) for n in range(32))
        if any(reserved_bytes):
            reserved += 1
        if '\t' in name or '\n' in name:
            raise RuntimeError('unsupported resource name')
        f.write('%s\t%d\t%d\t%d\t%d\t%s\n' % (name, int(r['type']), int(r['nIndex']), int(r['nDownloadSize']), flags, reserved_bytes.hex()))
print('HLDS manifest: resources=%d consistency=%d custom=%d reserved=%d' % (count, consistency, custom, reserved))
if custom != 0:
    raise RuntimeError('capture requires unsupported resource fields')
end
set host_client = &svs.clients[0]
set sv.num_consistency = 0
set net_message.cursize = 0
call (void)SV_SendResources(&net_message)
p net_message.cursize
dump binary memory /out/hlds-manifest-10211-v2.bin net_message.data net_message.data+net_message.cursize
quit

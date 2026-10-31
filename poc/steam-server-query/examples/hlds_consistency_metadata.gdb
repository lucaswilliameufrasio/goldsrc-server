# Export test metadata from an isolated HLDS process; no client or tickets.
python
import os
if os.path.exists('/out/hlds-consistency-metadata-10211.tsv'):
    raise RuntimeError('refusing to overwrite local metadata')
end
set breakpoint pending on
set disable-randomization off
break SV_ActivateServer
run -game cstrike -console +maxplayers 2 +sv_lan 1 +map de_dust2
finish
set $buf = (unsigned char*)malloc(32)
python
sv = gdb.parse_and_eval('sv')
spawn = int(gdb.parse_and_eval('svs.spawncount'))
inferior = gdb.selected_inferior()
buffer = int(gdb.parse_and_eval('$buf'))
checks = 0
bounds = 0
with open('/out/hlds-consistency-metadata-10211.tsv', 'x') as out:
    for index in range(int(sv['num_resources'])):
        resource = sv['resourcelist'][index]
        if not (int(resource['ucFlags']) & 128):
            continue
        raw = bytes(int(resource['rguc_reserved'][n]) for n in range(32))
        hash_bytes = bytes(int(resource['rgucMD5_hash'][n]) for n in range(4))
        decoded = raw
        if any(raw):
            bounds += 1
            inferior.write_memory(buffer, raw)
            gdb.execute('call (void)COM_UnMunge($buf,32,svs.spawncount)', to_string=True)
            decoded = bytes(inferior.read_memory(buffer, 32))
        out.write('%d\t%s\t%s\t%s\n' % (index, hash_bytes.hex(), raw.hex(), decoded.hex()))
        checks += 1
print('HLDS consistency metadata: spawn=%d checks=%d model_bounds=%d' % (spawn, checks, bounds))
end
quit

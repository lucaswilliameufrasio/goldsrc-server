# Synthetic values only. No client connection, Steam approval or network access.
set breakpoint pending on
set disable-randomization off
break SV_ActivateServer
run -game cstrike -console +maxplayers 2 +sv_lan 1 +map de_dust2
set net_from.type = NA_LOOPBACK
set $info = (char*)calloc(1, 128)
set $port = (unsigned short*)calloc(1, 2)
set $protocol = (int*)calloc(1, 4)
set $raw = (char*)calloc(1, 512)
set $key = (char*)calloc(1, 64)
python
inferior = gdb.selected_inferior()
sentinel = b'x' * 32
info = b'\\prot\\3\\raw\\synthetic\\cdkey\\' + sentinel + b'\0'
inferior.write_memory(int(gdb.parse_and_eval('$info')), info)
gdb.execute('call (int)SV_CheckKeyInfo(&net_from, $info, $port, $protocol, $raw, 512, $key)', to_string=True)
if int(gdb.parse_and_eval('$')) != 1 or int(gdb.parse_and_eval('*$protocol')) != 3:
    raise RuntimeError('HLDS rejected the synthetic key-info example')
key = bytes(inferior.read_memory(int(gdb.parse_and_eval('$key')), 64))
if key[:32] != sentinel or key[32:] != bytes(32):
    raise RuntimeError('HLDS copied an unexpected synthetic key-info value')
print('HLDS_KEYINFO_SYNTHETIC_MATCH protocol=3 input_bytes=32')
end
quit

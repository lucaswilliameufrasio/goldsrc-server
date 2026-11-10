# Synthetic 64-byte inputs only; run with an isolated network-disabled HLDS.
set breakpoint pending on
set disable-randomization off
break SV_ActivateServer
run -game cstrike -console +maxplayers 2 +sv_lan 1 +map de_dust2
set svs.clients[0].userid = 42
set svs.clients[0].userinfo[0] = 0
python
import hashlib
inferior = gdb.selected_inferior()
key = int(gdb.parse_and_eval('&svs.clients[0].hashedcdkey[0]'))
message = int(gdb.parse_and_eval('net_message.data'))
for pattern in (bytes(64), bytes(range(64)), b'x' * 32 + bytes(32), b'x' * 32 + bytes(31) + b'\1'):
    inferior.write_memory(key, pattern)
    gdb.execute('set net_message.cursize = 0')
    gdb.execute('call (void)SV_FullClientUpdate(&svs.clients[0], &net_message)', to_string=True)
    size = int(gdb.parse_and_eval('net_message.cursize'))
    if size < 16:
        raise RuntimeError('truncated client update')
    actual = bytes(inferior.read_memory(message + size - 16, 16))
    expected = hashlib.md5(pattern).digest()
    if actual != expected:
        raise RuntimeError('HLDS digest does not match 64-byte MD5')
print('HLDS_PLAYER_DIGEST_MATCH patterns=4 input_bytes=64')
end
quit

# Local-only synthetic LAN experiment. Never use real credentials or clients.
set breakpoint pending on
set disable-randomization off
break SV_ActivateServer
run -game cstrike -console +maxplayers 2 +sv_lan 1 +map de_dust2
set net_from.type = NA_LOOPBACK
call (void)Netchan_Setup(NS_SERVER, &svs.clients[0].netchan, net_from, 0, &svs.clients[0], (qboolean (*)(void *))SV_GetFragmentSize)
python
inferior = gdb.selected_inferior()
key = int(gdb.parse_and_eval('&svs.clients[0].hashedcdkey[0]'))
pattern = bytes(range(1, 65))
inferior.write_memory(key, pattern)
gdb.execute('call (void)SV_DropClient(&svs.clients[0], 0, "synthetic")', to_string=True)
if bytes(inferior.read_memory(key, 64)) != pattern:
    raise RuntimeError('normal drop changed synthetic field')
command = b'connect 48 0 "\\prot\\2\\raw\\' + b'x' * 32 + b'" "\\name\\synthetic\\*hltv\\1"\0'
gdb.execute('set $synthetic_command = (char*)calloc(1, 256)')
inferior.write_memory(int(gdb.parse_and_eval('$synthetic_command')), command)
gdb.execute('call (void)Cmd_TokenizeString($synthetic_command)', to_string=True)
class BeforeSecure(gdb.Breakpoint):
    hit = False
    def stop(self):
        self.hit = True
        return True
boundary = BeforeSecure('Steam_GSBSecure', internal=True)
print('HLDS_SYNTHETIC_COMMAND_ARGC=%s' % int(gdb.parse_and_eval('Cmd_Argc()')))
try:
    gdb.execute('call (void)SV_ConnectClient()', to_string=True)
except gdb.error:
    if not boundary.hit:
        raise
connected = int(gdb.parse_and_eval('svs.clients[0].connected'))
selected = int(gdb.parse_and_eval('host_client == &svs.clients[0]'))
result = bytes(inferior.read_memory(key, 64))
expected = b'x' * 32 + b'\0' + pattern[33:]
print('HLDS_SYNTHETIC_REUSE_SELECTED_SLOT0=%s' % bool(selected))
print('HLDS_SYNTHETIC_REUSE_REACHED_SECURE_BOUNDARY=%s' % boundary.hit)
print('HLDS_SYNTHETIC_REUSE_SLOT0_CONNECTED=%s' % bool(connected))
print('HLDS_SYNTHETIC_REUSE_FIELD_MATCH=%s' % (result == expected))
if not boundary.hit or not selected or not connected or result != expected:
    raise RuntimeError('synthetic connect did not establish expected reused slot')
end
quit

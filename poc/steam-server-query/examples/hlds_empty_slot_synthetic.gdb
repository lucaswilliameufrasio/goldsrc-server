# Isolated synthetic LAN proxy only: no real client or authentication data.
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
# A distinct previous address forces selection through SV_FindEmptySlot rather
# than the same-address reconnect branch; no packets leave the container.
gdb.execute('set svs.clients[0].netchan.remote_address.type = NA_IP')
command = b'connect 48 0 "\\prot\\2\\raw\\' + b'x' * 32 + b'" "\\name\\synthetic\\*hltv\\1"\0'
gdb.execute('set $synthetic_command = (char*)calloc(1, 256)')
inferior.write_memory(int(gdb.parse_and_eval('$synthetic_command')), command)
gdb.execute('call (void)Cmd_TokenizeString($synthetic_command)', to_string=True)
class FindEmpty(gdb.Breakpoint):
    hit = False
    def stop(self):
        self.hit = True
        return False
class BeforeSecure(gdb.Breakpoint):
    hit = False
    def stop(self):
        self.hit = True
        return True
find_empty = FindEmpty('SV_FindEmptySlot', internal=True)
boundary = BeforeSecure('Steam_GSBSecure', internal=True)
try:
    gdb.execute('call (void)SV_ConnectClient()', to_string=True)
except gdb.error:
    if not boundary.hit:
        raise
selected = bool(int(gdb.parse_and_eval('host_client == &svs.clients[0]')))
connected = bool(int(gdb.parse_and_eval('svs.clients[0].connected')))
result = bytes(inferior.read_memory(key, 64))
expected = b'x' * 32 + b'\0' + pattern[33:]
print('HLDS_EMPTY_SLOT_BRANCH=%s' % find_empty.hit)
print('HLDS_EMPTY_SLOT_SELECTED_ZERO=%s' % selected)
print('HLDS_EMPTY_SLOT_REACHED_SECURE_BOUNDARY=%s' % boundary.hit)
print('HLDS_EMPTY_SLOT_CONNECTED_FLAG=%s' % connected)
print('HLDS_EMPTY_SLOT_FIELD_MATCH=%s' % (result == expected))
if not find_empty.hit or not selected or not boundary.hit or not connected or result != expected:
    raise RuntimeError('synthetic empty-slot path did not match expectations')
end
quit

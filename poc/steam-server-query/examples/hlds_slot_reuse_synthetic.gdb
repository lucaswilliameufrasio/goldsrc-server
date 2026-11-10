# Network-isolated synthetic slot; no Steam client, ticket, or real key material.
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
after = bytes(inferior.read_memory(key, 64))
print('HLDS_NORMAL_DROP_SYNTHETIC_PRESERVED=%s' % (after == pattern))
if after != pattern:
    raise RuntimeError('normal drop changed synthetic field')
gdb.execute('call (void)Netchan_Setup(NS_SERVER, &svs.clients[0].netchan, net_from, 0, &svs.clients[0], (qboolean (*)(void *))SV_GetFragmentSize)', to_string=True)
reused = bytes(inferior.read_memory(key, 64))
print('HLDS_NETCHAN_REINIT_SYNTHETIC_PRESERVED=%s' % (reused == pattern))
if reused != pattern:
    raise RuntimeError('netchannel reinitialization changed synthetic field')
end
quit

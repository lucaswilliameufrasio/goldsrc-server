# Synthetic slot only; use an isolated HLDS without clients or network.
set breakpoint pending on
set disable-randomization off
break SV_ActivateServer
run -game cstrike -console +maxplayers 2 +sv_lan 1 +map de_dust2
python
inferior = gdb.selected_inferior()
key = int(gdb.parse_and_eval('&svs.clients[0].hashedcdkey[0]'))
initial = bytes(inferior.read_memory(key, 64))
print('HLDS_SLOT_INITIAL_ZERO=%s' % (initial == bytes(64)))
if initial != bytes(64):
    raise RuntimeError('initial synthetic slot was not zero')
pattern = bytes(range(1, 65))
inferior.write_memory(key, pattern)
gdb.execute('call (void)SV_DropClient(&svs.clients[0], 1, "synthetic")', to_string=True)
after = bytes(inferior.read_memory(key, 64))
print('HLDS_DROP_SYNTHETIC_PRESERVED=%s' % (after == pattern))
if after != pattern:
    raise RuntimeError('synthetic slot pattern changed on drop')
end
quit

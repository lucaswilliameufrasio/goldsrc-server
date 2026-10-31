# Isolated, client-free verification of Rust-generated synthetic datagrams.
set breakpoint pending on
set disable-randomization off
break SV_ActivateServer
run -game cstrike -console +maxplayers 2 +sv_lan 1 +map de_dust2
set net_from.type = NA_LOOPBACK
set svs.clients[0].netchan.remote_address = net_from
set svs.clients[0].netchan.outgoing_sequence = 1
python
fixture = open('/fixtures/rust-fragments-hlds.bin', 'rb').read()
offset = 0
inferior = gdb.selected_inferior()
for index in range(3):
    length = int.from_bytes(fixture[offset:offset + 4], 'little')
    offset += 4
    packet = fixture[offset:offset + length]
    offset += length
    if len(packet) != length or length > 1400:
        raise RuntimeError('invalid packet fixture')
    address = int(gdb.parse_and_eval('net_message.data'))
    inferior.write_memory(address, packet)
    gdb.execute('set net_message.cursize = %d' % length)
    gdb.execute('call (int)Netchan_Process(&svs.clients[0].netchan)')
    print('HLDS packet %d: incoming sequence=%d reliable=%d ready=%d' % (index + 1, int(gdb.parse_and_eval('svs.clients[0].netchan.incoming_sequence')), int(gdb.parse_and_eval('svs.clients[0].netchan.incoming_reliable_sequence')), int(gdb.parse_and_eval('svs.clients[0].netchan.incomingready[0]'))))
    if int(gdb.parse_and_eval('svs.clients[0].netchan.incoming_sequence')) != index + 1:
        raise RuntimeError('HLDS rejected packet sequence')
    if int(gdb.parse_and_eval('svs.clients[0].netchan.incoming_reliable_sequence')) != (index + 1) % 2:
        raise RuntimeError('HLDS reliable state differs from expected')
    if int(gdb.parse_and_eval('svs.clients[0].netchan.incomingready[0]')) != int(index == 2):
        raise RuntimeError('HLDS completed assembly at the wrong point')
if offset != len(fixture) or int(gdb.parse_and_eval('svs.clients[0].netchan.incomingready[0]')) != 1:
    raise RuntimeError('HLDS did not complete fragment assembly')
gdb.execute('call (int)Netchan_CopyNormalFragments(&svs.clients[0].netchan)')
if int(gdb.parse_and_eval('$')) != 1:
    raise RuntimeError('HLDS did not copy complete fragments')
size = int(gdb.parse_and_eval('net_message.cursize'))
actual = bytes(inferior.read_memory(int(gdb.parse_and_eval('net_message.data')), size))
expected = bytes([n % 256 for n in range(2300)])
if actual != expected:
    raise RuntimeError('HLDS reassembled payload differs from input')
print('HLDS_REASSEMBLY_MATCH bytes=%d packets=3' % size)
end
quit

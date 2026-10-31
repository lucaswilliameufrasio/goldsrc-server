# Synthetic three-model response; no real client or credentials.
python
import os
if os.path.exists('/out/hlds-consistency-bounds-synthetic.bin'):
    raise RuntimeError('refusing to overwrite capture')
end
set breakpoint pending on
set disable-randomization off
break SV_ActivateServer
run -game cstrike -console +maxplayers 2 +sv_lan 1 +map de_dust2
set host_client = &svs.clients[0]
set sv.num_resources = 3
set sv.num_consistency = 3
set host_client->has_force_unmodified = 1
python
for index in range(3):
    base = 'sv.resourcelist[%d]' % index
    gdb.execute('set %s.ucFlags = 128' % base)
    gdb.execute('set %s.rguc_reserved[0] = %d' % (base, index + 1))
    for pos, val in enumerate([-1.0, -2.0, -3.0, 1.0, 2.0, 3.0]):
        gdb.execute('set *(float*)&%s.rguc_reserved[%d] = %s' % (base, 1 + pos * 4, val))
    gdb.execute('call (void)COM_Munge(%s.rguc_reserved, 32, svs.spawncount)' % base)
end
set net_message.cursize = 0
call (void)MSG_WriteShort(&net_message, 0)
call (void)MSG_StartBitWriting(&net_message)
python
values = [0xbf800000, 0xc0000000, 0xc0400000, 0x3f800000, 0x40000000, 0x40400000]
for index in range(3):
    gdb.execute('call (void)MSG_WriteBits(1, 1)')
    gdb.execute('call (void)MSG_WriteBits(%d, 12)' % index)
    for v in (values if index != 2 else [0xbf800000] * 6):
        gdb.execute('call (void)MSG_WriteBits(%d, 32)' % v)
gdb.execute('call (void)MSG_WriteBits(0, 1)')
end
call (void)MSG_EndBitWriting(&net_message)
set $length = net_message.cursize - 2
set *(unsigned short*)net_message.data = $length
call (void)COM_Munge(net_message.data+2, $length, svs.spawncount)
dump binary memory /out/hlds-consistency-bounds-synthetic.bin net_message.data net_message.data+net_message.cursize
call (void)MSG_BeginReading()
call (void)SV_ParseConsistencyResponse(host_client)
python
if int(gdb.parse_and_eval('msg_badread')) != 0 or int(gdb.parse_and_eval('host_client->has_force_unmodified')) != 0:
    raise RuntimeError('HLDS did not accept synthetic bounds response')
print('HLDS accepted three synthetic model bounds, length=%d' % int(gdb.parse_and_eval('$length')))
end
quit

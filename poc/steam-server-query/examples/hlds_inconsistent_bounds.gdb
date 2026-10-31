# Observe the callback for a synthetic mismatch; no real client or approval.
set breakpoint pending on
set disable-randomization off
break SV_ActivateServer
run -game cstrike -console +maxplayers 2 +sv_lan 1 +map de_dust2
set host_client = &svs.clients[0]
set host_client->netchan.message.data = &host_client->netchan.message_buf[0]
set host_client->netchan.message.maxsize = sizeof(host_client->netchan.message_buf)
set host_client->netchan.message.cursize = 0
set sv.num_resources = 1
set sv.num_consistency = 1
set sv.resourcelist[0].ucFlags = 128
set sv.resourcelist[0].rguc_reserved[0] = 1
set *(float*)&sv.resourcelist[0].rguc_reserved[1] = -2.0
set *(float*)&sv.resourcelist[0].rguc_reserved[5] = -2.0
set *(float*)&sv.resourcelist[0].rguc_reserved[9] = -3.0
set *(float*)&sv.resourcelist[0].rguc_reserved[13] = 1.0
set *(float*)&sv.resourcelist[0].rguc_reserved[17] = 2.0
set *(float*)&sv.resourcelist[0].rguc_reserved[21] = 3.0
call (void)COM_Munge(sv.resourcelist[0].rguc_reserved, 32, svs.spawncount)
set host_client->has_force_unmodified = 1
set net_message.cursize = 0
call (void)MSG_WriteShort(&net_message, 0)
call (void)MSG_StartBitWriting(&net_message)
call (void)MSG_WriteBits(1, 1)
call (void)MSG_WriteBits(0, 12)
call (void)MSG_WriteBits(0xbf800000, 32)
call (void)MSG_WriteBits(0xc0000000, 32)
call (void)MSG_WriteBits(0xc0400000, 32)
call (void)MSG_WriteBits(0x3f800000, 32)
call (void)MSG_WriteBits(0x40000000, 32)
call (void)MSG_WriteBits(0x40400000, 32)
call (void)MSG_WriteBits(0, 1)
call (void)MSG_EndBitWriting(&net_message)
set $length = net_message.cursize - 2
set *(unsigned short*)net_message.data = $length
call (void)COM_Munge(net_message.data+2, $length, svs.spawncount)
call (void)MSG_BeginReading()
python
callback = int(gdb.parse_and_eval('gEntityInterface.pfnInconsistentFile'))
if not callback:
    raise RuntimeError('game DLL did not register callback')
gdb.execute('break *%d' % callback)
try:
    gdb.execute('call (void)SV_ParseConsistencyResponse(host_client)')
except gdb.error:
    pass
if int(gdb.parse_and_eval('$pc')) != callback:
    raise RuntimeError('game DLL inconsistent-file callback was not reached')
print('HLDS reached game DLL inconsistent-file callback for synthetic bounds mismatch')
end
finish
python
if int(gdb.parse_and_eval('$eax')) != 1:
    raise RuntimeError('unexpected game DLL callback result')
print('CS InconsistentFile returned 1 for synthetic mismatch')
end
break SV_DropClient
continue
python
if gdb.newest_frame().name() != 'SV_DropClient':
    raise RuntimeError('expected HLDS drop was not reached')
print('HLDS reached SV_DropClient after game DLL callback')
end
quit

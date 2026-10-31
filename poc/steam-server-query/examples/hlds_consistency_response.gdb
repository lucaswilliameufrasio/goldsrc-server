# Synthetic one-file response; no real client, ticket, or live server.
python
import os
if os.path.exists('/out/hlds-consistency-hash-synthetic.bin'):
    raise RuntimeError('refusing to overwrite capture')
end
set breakpoint pending on
set disable-randomization off
break SV_ActivateServer
run -game cstrike -console +maxplayers 2 +sv_lan 1 +map de_dust2
set host_client = &svs.clients[0]
set sv.num_resources = 1
set sv.num_consistency = 1
set sv.resourcelist[0].ucFlags = 128
set *(unsigned int*)&sv.resourcelist[0].rgucMD5_hash[0] = 0x12345678
set host_client->has_force_unmodified = 1
set net_message.cursize = 0
call (void)MSG_WriteShort(&net_message, 0)
call (void)MSG_StartBitWriting(&net_message)
call (void)MSG_WriteBits(1, 1)
call (void)MSG_WriteBits(0, 12)
call (void)MSG_WriteBits(0x12345678, 32)
call (void)MSG_WriteBits(0, 1)
call (void)MSG_EndBitWriting(&net_message)
set $length = net_message.cursize - 2
set *(unsigned short*)net_message.data = $length
call (void)COM_Munge(net_message.data+2, $length, svs.spawncount)
dump binary memory /out/hlds-consistency-hash-synthetic.bin net_message.data net_message.data+net_message.cursize
call (void)MSG_BeginReading()
call (void)SV_ParseConsistencyResponse(host_client)
python
if int(gdb.parse_and_eval('msg_badread')) != 0:
    raise RuntimeError('HLDS rejected synthetic consistency response')
if int(gdb.parse_and_eval('host_client->has_force_unmodified')) != 0:
    raise RuntimeError('HLDS did not clear the consistency requirement')
print('HLDS accepted synthetic hash response, length=%d' % int(gdb.parse_and_eval('$length')))
end
quit

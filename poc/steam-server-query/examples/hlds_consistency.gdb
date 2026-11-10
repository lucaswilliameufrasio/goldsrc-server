# Capture local reference output with the normal consistency list enabled.
python
import os
if os.path.exists('/out/hlds-manifest-consistency-10211.bin'):
    raise RuntimeError('refusing to overwrite local capture')
end
set breakpoint pending on
set disable-randomization off
break SV_ActivateServer
run -game cstrike -console +maxplayers 2 +sv_lan 1 +map de_dust2
finish
set host_client = &svs.clients[0]
python
if int(gdb.parse_and_eval('sv.num_consistency')) != 62:
    raise RuntimeError('unexpected consistency count')
if float(gdb.parse_and_eval('mp_consistency.value')) != 1:
    raise RuntimeError('consistency is not enabled')
if int(gdb.parse_and_eval('host_client->proxy')) != 0:
    raise RuntimeError('proxy client suppresses consistency')
end
set net_message.cursize = 0
call (void)SV_SendResources(&net_message)
python
if int(gdb.parse_and_eval('host_client->has_force_unmodified')) != 1:
    raise RuntimeError('HLDS did not request consistency response')
print('HLDS consistency enabled; output bytes=%d' % int(gdb.parse_and_eval('net_message.cursize')))
end
dump binary memory /out/hlds-manifest-consistency-10211.bin net_message.data net_message.data+net_message.cursize
quit

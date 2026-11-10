# Capture synthetic, uncompressed resource entries in an isolated HLDS process.
python
import os
if os.path.exists('/out/hlds-resources-synthetic.bin'):
    raise RuntimeError('refusing to overwrite existing resource capture')
end
set breakpoint pending on
set disable-randomization off
break SV_ActivateServer
run -game cstrike -console +maxplayers 2 +sv_lan 1 +map de_dust2
set host_client = &svs.clients[0]
set sv.num_resources = 2
call (char*)strcpy(sv.resourcelist[0].szFileName,"sound/test.wav")
set sv.resourcelist[0].type = t_sound
set sv.resourcelist[0].nIndex = 1
set sv.resourcelist[0].nDownloadSize = 1234
set sv.resourcelist[0].ucFlags = 0
call (char*)strcpy(sv.resourcelist[1].szFileName,"models/test.mdl")
set sv.resourcelist[1].type = t_model
set sv.resourcelist[1].nIndex = 12
set sv.resourcelist[1].nDownloadSize = 4567
set sv.resourcelist[1].ucFlags = 1
set net_message.cursize = 0
call (void)SV_SendResources(&net_message)
p net_message.cursize
dump binary memory /out/hlds-resources-synthetic.bin net_message.data net_message.data+net_message.cursize
quit

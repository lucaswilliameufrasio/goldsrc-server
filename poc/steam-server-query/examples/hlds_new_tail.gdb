# Synthetic inputs only. Run in a network-isolated HLDS container without clients.
python
import os
if os.path.exists('/out/hlds-new-tail-synthetic-v2.bin'):
    raise RuntimeError('refusing to overwrite the existing capture')
end
set breakpoint pending on
set disable-randomization off
break SV_ActivateServer
run -game cstrike -console +maxplayers 2 +sv_lan 1 +map de_dust2
set $u = (UserMsg*)calloc(2,sizeof(UserMsg))
set $u[0].iMsg = 64
set $u[0].iSize = -1
call (char*)strcpy($u[0].szName,"Probe")
set $u[0].next = &$u[1]
set $u[1].iMsg = 65
set $u[1].iSize = 5
call (char*)strcpy($u[1].szName,"Test")
set sv_gpNewUserMsgs = $u
set svs.clients[0].userid = 42
call (char*)strcpy(svs.clients[0].userinfo,"\\name\\tester\\_secret\\fake\\rate\\25000")
set net_message.cursize = 0
call (void)SV_SendUserReg(&net_message)
call (void)MSG_WriteByte(&net_message,9)
call (void)MSG_WriteString(&net_message,"fullserverinfo \"\\hostname\\test\"\n")
call (void)SV_FullClientUpdate(&svs.clients[0], &net_message)
p net_message.cursize
dump binary memory /out/hlds-new-tail-synthetic-v2.bin net_message.data net_message.data+net_message.cursize
quit

# Synthetic PM_Move wall-impact probe only; not a client or gameplay capture.
# Run in an isolated network-disabled container with the local HLDS tree mounted
# read-only. The input is a bounded direct call into the stock CS game DLL.
set pagination off
set confirm off
set breakpoint pending on
set disable-randomization off
break SV_ActivateServer
run -game cstrike -console +maxplayers 2 +sv_lan 1 +map de_dust2

# The engine normally initializes these pointers during SV_RunCmd. This probe
# uses the loaded, typed playermove_t and explicitly supplies its world-only
# physent because no client command or session is involved.
set $pm = '../cstrike/pm_shared/pm_shared.c'::pmove
set '../engine/pmove.c'::pmove = $pm
set $pm->numphysent = 1
set $pm->physents[0].model = sv.worldmodel
set $pm->physents[0].solid = 4
set $pm->physents[0].player = 0
set $pm->physents[0].info = 0
set $pm->physents[0].origin[0] = 0
set $pm->physents[0].origin[1] = 0
set $pm->physents[0].origin[2] = 0

# One stock map spawn, a 50 ms synthetic MOVETYPE_FLY case, and no side effects.
set $pm->player_index = 0
set $pm->server = 1
set $pm->multiplayer = 1
set $pm->time = 1
set $pm->frametime = 0.05
set $pm->origin[0] = -832
set $pm->origin[1] = -704
set $pm->origin[2] = 176
set $pm->velocity[0] = 0
set $pm->velocity[1] = 1000
set $pm->velocity[2] = 0
set $pm->basevelocity[0] = 0
set $pm->basevelocity[1] = 0
set $pm->basevelocity[2] = 0
set $pm->flags = 0
set $pm->usehull = 0
set $pm->gravity = 0
set $pm->friction = 1
set $pm->movetype = 5
set $pm->onground = -1
set $pm->waterlevel = 0
set $pm->maxspeed = 320
set $pm->cmd.msec = 50
set $pm->cmd.lerp_msec = 0
set $pm->cmd.forwardmove = 0
set $pm->cmd.sidemove = 0
set $pm->cmd.upmove = 0
set $pm->cmd.buttons = 0
set $pm->numtouch = 0
set $pm->runfuncs = 0

# The reference starts before a client is admitted, so install the normal
# engine texture-trace callback which is not populated until a real SV_RunCmd.
set $pm->PM_TraceTexture = PM_SV_TraceTexture

printf "PMOVE_BEFORE origin=(%.9g,%.9g,%.9g) velocity=(%.9g,%.9g,%.9g) dt=%.9g msec=%d type=%d hull=%d gravity=%.9g physents=%d\n", $pm->origin[0], $pm->origin[1], $pm->origin[2], $pm->velocity[0], $pm->velocity[1], $pm->velocity[2], $pm->frametime, $pm->cmd.msec, $pm->movetype, $pm->usehull, $pm->movevars->gravity, $pm->numphysent
call PM_Move($pm, 1)
printf "PMOVE_AFTER origin=(%.9g,%.9g,%.9g) velocity=(%.9g,%.9g,%.9g) flags=%d ground=%d water=%d touch=%d plane=(%.9g,%.9g,%.9g)\n", $pm->origin[0], $pm->origin[1], $pm->origin[2], $pm->velocity[0], $pm->velocity[1], $pm->velocity[2], $pm->flags, $pm->onground, $pm->waterlevel, $pm->numtouch, $pm->touchindex[0].plane.normal[0], $pm->touchindex[0].plane.normal[1], $pm->touchindex[0].plane.normal[2]
quit

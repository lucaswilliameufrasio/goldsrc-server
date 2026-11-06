# Synthetic points only. Run an isolated, network-disabled reference HLDS.
set breakpoint pending on
set disable-randomization off
break SV_ActivateServer
run -game cstrike -console +maxplayers 2 +sv_lan 1 +map de_dust2
set $point = (float*)calloc(3, sizeof(float))
python
points = [(0, 0, 0), (0, 0, 128), (100, 100, 100), (-500, 1000, 0), (4096, 4096, 4096)]
for hull in range(4):
    for p in points:
        for i in range(3):
            gdb.execute('set $point[%d] = %d' % (i, p[i]))
        result = gdb.parse_and_eval('(int)SV_HullPointContents(&sv.worldmodel->hulls[%d], sv.worldmodel->hulls[%d].firstclipnode, $point)' % (hull, hull))
        print('HLDS_HULL_POINT hull=%d point=%s,%s,%s contents=%d' % (hull, *p, int(result)))
end
quit

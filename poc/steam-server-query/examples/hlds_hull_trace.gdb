# Synthetic world-model traces only. Run the reference server without network.
set breakpoint pending on
set disable-randomization off
break SV_ActivateServer
run -game cstrike -console +maxplayers 2 +sv_lan 1 +map de_dust2
set $start = (float*)calloc(3, sizeof(float))
set $end = (float*)calloc(3, sizeof(float))
set $trace = (trace_t*)calloc(1, sizeof(trace_t))
python
segments = [((-500, 1000, 0), (0, 0, 0)), ((0, 0, 0), (-500, 1000, 0)), ((-500, 1000, 0), (-500, 1000, 0)), ((0, 0, 0), (0, 0, 0)), ((100, 100, 100), (-500, 1000, 0)), ((-500, 1000, 0), (100, 100, 100))]
for hull in range(4):
    mins = []
    maxs = []
    for axis in range(3):
        mins.append(float(gdb.parse_and_eval('sv.worldmodel->hulls[%d].clip_mins[%d]' % (hull, axis))))
        maxs.append(float(gdb.parse_and_eval('sv.worldmodel->hulls[%d].clip_maxs[%d]' % (hull, axis))))
    print('HLDS_HULL_BOUNDS hull=%d mins=%s maxs=%s' % (hull, mins, maxs))
for hull in (0, 1, 2, 3):
    for start, end in segments:
        gdb.execute('call (void*)memset($trace, 0, sizeof(trace_t))', to_string=True)
        gdb.execute('set $trace->fraction = 1.0')
        gdb.execute('set $trace->allsolid = 1')
        for i in range(3):
            gdb.execute('set $start[%d] = %d' % (i, start[i]))
            gdb.execute('set $end[%d] = %d' % (i, end[i]))
            gdb.execute('set $trace->endpos[%d] = %d' % (i, end[i]))
        gdb.execute('call (int)SV_RecursiveHullCheck(&sv.worldmodel->hulls[%d], sv.worldmodel->hulls[%d].firstclipnode, 0.0f, 1.0f, $start, $end, $trace)' % (hull, hull), to_string=True)
        fraction = float(gdb.parse_and_eval('$trace->fraction'))
        flags = [int(gdb.parse_and_eval('$trace->' + name)) for name in ('allsolid', 'startsolid', 'inopen', 'inwater')]
        endpos = [float(gdb.parse_and_eval('$trace->endpos[%d]' % i)) for i in range(3)]
        normal = [float(gdb.parse_and_eval('$trace->plane.normal[%d]' % i)) for i in range(3)]
        dist = float(gdb.parse_and_eval('$trace->plane.dist'))
        print('HLDS_HULL_TRACE hull=%d start=%s end=%s fraction=%.9g flags=%s endpos=%s normal=%s dist=%.9g' % (hull, start, end, fraction, flags, endpos, normal, dist))
end
quit

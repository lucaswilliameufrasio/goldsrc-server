set breakpoint pending on
set disable-randomization off
break SV_ActivateServer
run -game cstrike -console +maxplayers 2 +sv_lan 1 +map de_dust2
set $mins = (float*)calloc(3, sizeof(float))
set $maxs = (float*)calloc(3, sizeof(float))
set $offset = (float*)calloc(3, sizeof(float))
set $start = (float*)calloc(3, sizeof(float))
set $end = (float*)calloc(3, sizeof(float))
python
cases = [
    ('point', (0, 0, 0), (0, 0, 0)),
    ('standing', (-16, -16, -36), (16, 16, 36)),
    ('crouched', (-16, -16, -18), (16, 16, 18)),
    ('large', (-32, -32, -32), (32, 32, 32)),
    ('width8', (-4, -4, -36), (4, 4, 36)),
    ('width9', (-4.5, -4.5, -36), (4.5, 4.5, 36)),
    ('width36', (-18, -18, -18), (18, 18, 18)),
    ('width37', (-18.5, -18.5, -36), (18.5, 18.5, 36)),
    ('height37', (-16, -16, -18.5), (16, 16, 18.5)),
]
for name, mins, maxs in cases:
    for axis in range(3):
        gdb.execute('set $mins[%d] = %f' % (axis, mins[axis]))
        gdb.execute('set $maxs[%d] = %f' % (axis, maxs[axis]))
    gdb.execute('set $selected = (hull_t*)SV_HullForEntity(sv.edicts, $mins, $maxs, $offset)')
    hull_index = -1
    for index in range(4):
        expression = '(void*)$selected == (void*)&sv.worldmodel->hulls[%d]' % index
        if int(gdb.parse_and_eval(expression)) != 0:
            hull_index = index
            break
    offset = [float(gdb.parse_and_eval('$offset[%d]' % axis)) for axis in range(3)]
    print('HLDS_ENTITY_HULL name=%s index=%d offset=%s' % (name, hull_index, offset))

saved_origin = [float(gdb.parse_and_eval('sv.edicts->v.origin[%d]' % axis)) for axis in range(3)]
translation = (10, -20, 30)
start = (-500, 1000, 0)
end = (0, 0, 0)
player_mins = (-16, -16, -36)
player_maxs = (16, 16, 36)
for axis in range(3):
    gdb.execute('set sv.edicts->v.origin[%d] = %d' % (axis, translation[axis]))
    gdb.execute('set $mins[%d] = %d' % (axis, player_mins[axis]))
    gdb.execute('set $maxs[%d] = %d' % (axis, player_maxs[axis]))
    gdb.execute('set $start[%d] = %d' % (axis, start[axis] + translation[axis]))
    gdb.execute('set $end[%d] = %d' % (axis, end[axis] + translation[axis]))
gdb.execute('set $trace_result = SV_ClipMoveToEntity(sv.edicts, $start, $mins, $maxs, $end)')
fraction = float(gdb.parse_and_eval('$trace_result.fraction'))
endpos = [float(gdb.parse_and_eval('$trace_result.endpos[%d]' % axis)) for axis in range(3)]
flags = [int(gdb.parse_and_eval('$trace_result.' + name)) for name in ('allsolid', 'startsolid', 'inopen', 'inwater')]
normal = [float(gdb.parse_and_eval('$trace_result.plane.normal[%d]' % axis)) for axis in range(3)]
distance = float(gdb.parse_and_eval('$trace_result.plane.dist'))
print('HLDS_TRANSLATED_BSP_TRACE fraction=%.9g endpos=%s flags=%s normal=%s dist=%.9g' % (fraction, endpos, flags, normal, distance))
for axis in range(3):
    gdb.execute('set sv.edicts->v.origin[%d] = %f' % (axis, saved_origin[axis]))
end
quit

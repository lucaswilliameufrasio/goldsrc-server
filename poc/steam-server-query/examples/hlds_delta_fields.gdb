# Run only against an isolated HLDS process without clients or network access.
# /out must be an ignored local directory; exclusive creation prevents overwrite.
set breakpoint pending on
set disable-randomization off
break SV_ActivateServer
run -game cstrike -console +maxplayers 2 +sv_lan 1 +map de_dust2
python
p = gdb.parse_and_eval('g_sv_delta')
f = open('/out/hlds-delta-fields-build10211.tsv', 'x')
while int(p) != 0:
    node = p.dereference()
    d = node['delta'].dereference()
    count = int(d['fieldCount'])
    f.write('schema\t%s\t%d\n' % (node['name'].string(), count))
    for i in range(count):
        item = d['pdd'][i]
        f.write('field\t%s\t%d\t%d\t%d\t%d\t%.9g\t%.9g\n' % (item['fieldName'].string(), int(item['fieldType']) & 0xffffffff, int(item['fieldOffset']), int(item['fieldSize']), int(item['significant_bits']), float(item['premultiply']), float(item['postmultiply'])))
    p = node['next']
f.close()
end
quit

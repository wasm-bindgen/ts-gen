# Pivots a results.jsonl (the last run of each case) into a markdown table of ns/op per profile.
import json,sys
rows=[json.loads(l) for l in open(sys.argv[1])]
last={}
order=[]
for r in rows:
    k=(r['profile'],r['case'])
    if r['case'] not in order: order.append(r['case'])
    last[k]=r
profs=[p for p in ['linear','linear-single','gc','gc-single','gc-wtf16','gc-hoststr','host'] if any(k[0]==p for k in last)]
print('| case | '+' | '.join(profs)+' |')
print('|---|'+'---:|'*len(profs))
for c in order:
    cells=[]
    for p in profs:
        r=last.get((p,c))
        cells.append('' if r is None else (f"{r['ns']:.1f}" if r['ns']<1e4 else f"{r['ns']/1000:.0f}µ"))
    print(f'| {c} | '+' | '.join(cells)+' |')

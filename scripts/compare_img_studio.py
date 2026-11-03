"""Read-only comparison of Studio model facts and local Scope records.

Uses SQLite snapshots including WAL; never reads prompts, credentials or images.
Optional --baseline lists pre-test request IDs in a rollback metadata JSON file.
"""
import argparse
from collections import defaultdict
from decimal import Decimal
import json
from pathlib import Path
import sqlite3


def connect(path):
    connection = sqlite3.connect(Path(path).resolve().as_uri() + '?mode=ro', uri=True)
    connection.execute('BEGIN')
    return connection


def compare(studio_db, scope_db, baseline_ids=()):
    studio, scope = connect(studio_db), connect(scope_db)
    events = [json.loads(r[0]) for r in studio.execute('SELECT event FROM usage_events')]
    measurements = [json.loads(r[0]) for r in studio.execute('SELECT measurement FROM model_usage')]
    own = json.loads(scope.execute("SELECT value FROM meta WHERE key='device_id'").fetchone()[0])
    records = [json.loads(r[0]) for r in scope.execute("SELECT payload FROM records WHERE device_id=? AND json_extract(payload,'$.harness')='gpt-img-studio' AND json_extract(payload,'$.deleted')=0", (own,))]
    by_id = {r['id']: r for r in records}
    by_component = {m['id']: m for m in measurements}
    failures = []
    keys = ('model','kind','usageAvailable','tokens','cacheBreakdownKnown','cacheWriteApplicable','textInputTokens','imageInputTokens','estimatedCostUsd','estimateStatus','pricingSnapshot')
    for event in events:
        for i, component in enumerate(event['components']):
            native = f"{event['id']}:component:{i}"
            m = by_component.get(native)
            if m is None or any(m.get(k) != component.get(k) for k in keys):
                failures.append({'id':native,'issue':'source projection differs from original component'})
    models = defaultdict(lambda: {'measurements':0,'totalInput':0,'output':0,'upperBounds':0,'unknownUsage':0,'estimatedCostUsd':Decimal(0)})
    expected_ids = set()
    new = []
    for m in measurements:
        identity = f"gpt-img-studio:{m['deviceId']}:{m['id']}"
        expected_ids.add(identity)
        r = by_id.get(identity)
        if r is None:
            failures.append({'id':identity,'issue':'not collected'})
            continue
        source = r.get('modelUsage', {})
        expected = {k:m.get(k) for k in ('requestId','operation','kind','usageAvailable','cacheBreakdownKnown','cacheWriteApplicable','textInputTokens','imageInputTokens','estimatedCostUsd','estimateStatus','statusCode','requestDurationMs')}
        expected['sourceDeviceId'] = m['deviceId']
        provider = m.get('provider')
        if provider is not None:
            expected['provider'] = {k:provider[k] for k in ('id','name','kind')}
            service = provider.get('apiService', 'unknown')
            expected['provider']['apiService'] = service
            route = f"gpt-img-studio:{provider['id']}" if service == 'unknown' else f"gpt-img-studio:{service}:{provider['id']}"
            if r.get('provider') != route:
                failures.append({'id':identity,'issue':'API provider route differs'})
        expected['tokens'] = {k:m['tokens'].get(k) for k in ('totalInput','input','output','cacheRead','cacheWrite','reasoning')}
        price = m.get('pricingSnapshot')
        if price is not None:
            price = {k:v for k,v in price.items() if k in ('source','currency','unit','rates','asOf','serviceTier','contextTier') and v is not None}
            price['rates'] = {k:v for k,v in price['rates'].items() if v != ''}
        expected['pricingSnapshot'] = price
        t = m['tokens']
        known = m['cacheBreakdownKnown']
        projection = {'input':(t.get('input') if known else t.get('totalInput')) or 0, 'output':t.get('output') or 0,'cacheRead':(t.get('cacheRead') or 0) if known else 0,'cacheWrite':(t.get('cacheWrite') or 0) if known else 0,'cacheWrite1h':0,'reasoning':t.get('reasoning') or 0}
        expected_method = 'source_estimated' if m.get('estimatedCostUsd') is not None else 'unknown'
        if source != expected or r['tokens'] != projection or r['model'] != m['model'] or r['occurredAt'] != m['occurredAt'] or r['count'] != 0 or r.get('timing') is not None or r['cost']['amountUsd'] != m.get('estimatedCostUsd') or r['cost']['method'] != expected_method:
            failures.append({'id':identity,'issue':'collected facts differ'})
        if m['requestId'] not in baseline_ids:
            new.append(m)
            totals = models[m['model']]
            totals['measurements'] += 1
            totals['totalInput'] += t.get('totalInput') or 0
            totals['output'] += t.get('output') or 0
            totals['upperBounds'] += m['estimateStatus'] == 'upper_bound'
            totals['unknownUsage'] += not m['usageAvailable']
            totals['estimatedCostUsd'] += Decimal(m.get('estimatedCostUsd') or '0')
    if set(by_id) != expected_ids:
        failures.append({'issue':'record identity sets differ','extra':len(set(by_id)-expected_ids),'missing':len(expected_ids-set(by_id))})
    for totals in models.values():
        totals['estimatedCostUsd'] = str(totals['estimatedCostUsd'])
    new_requests = [e for e in events if e['id'] not in baseline_ids]
    report = {'sourceRequests':len(events),'sourceModelUsages':len(measurements),'collectedModelUsages':len(records),'failures':failures,'newRequests':len(new_requests),'newModelUsages':len(new),'models':dict(models),'newInvocations':[{'id':e['id'],'operation':e['operation'],'protocol':e['protocol'],'statusCode':e['statusCode'],'modelUsages':len(e['components'])} for e in new_requests],'scopeSchema':scope.execute('PRAGMA user_version').fetchone()[0],'studioQuickCheck':studio.execute('PRAGMA quick_check').fetchone()[0],'scopeQuickCheck':scope.execute('PRAGMA quick_check').fetchone()[0]}
    studio.close(); scope.close()
    return report


if __name__ == '__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--studio-db',required=True);p.add_argument('--scope-db',required=True)
    p.add_argument('--baseline');p.add_argument('--report',required=True)
    args=p.parse_args()
    baseline=json.loads(Path(args.baseline).read_text(encoding='utf-8'))['baselineRequestIds'] if args.baseline else []
    report=compare(args.studio_db,args.scope_db,baseline)
    Path(args.report).write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(report,ensure_ascii=False,indent=2))
    raise SystemExit(bool(report['failures']) or report['studioQuickCheck']!='ok' or report['scopeQuickCheck']!='ok')

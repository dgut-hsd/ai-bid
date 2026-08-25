# -*- coding: utf-8 -*-
import json, re, time, requests
from collections import Counter, OrderedDict

key = None
for line in open('.env', encoding='utf-8'):
    s = line.strip()
    if s.startswith('DASHSCOPE_API_KEY'):
        key = s.split('=', 1)[1].strip().strip('"')
URL = 'https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions'
MODEL = 'qwen3.6-flash'

findings = []
for p in ['output/findings/MAOMING_mutated_findings.json',
          'output/findings/ERZHONGYI_mutated_findings.json']:
    ds = json.load(open(p, encoding='utf-8'))
    doc = 'MAOMING' if 'MAOMING' in p else 'ERZHONGYI'
    for f in ds:
        findings.append((doc, f))
print('总 finding 数:', len(findings), flush=True)

# 同原文去重：只按"原文核心句"裁决一次（不看 risk_type，不看 reason）
def core_key(q):
    q = q or ''
    m = re.search(r'[^。；\n]*(?:另加|仅限|指定|不得|须不低于|酌情|注册地为|注册地|加分|品牌|注册资本|只接受|唯一)[^。；\n]*', q)
    core = m.group(0) if m else q
    return re.sub(r'\s+', '', core)[:120]

SYS = (
    '你是政府采购招标文件审计的独立复核员。给定【招标文件原文】和一条【待核风险类型】，'
    '你需要仅依据原文的字面内容，判断"原文是否确实构成该类违规"。三选一：\n'
    '- support：原文明确、逐字包含该类违规的硬性表述（例如风险类型是"地域限制"，原文写了"注册地为某地加X分"），足以直接推出违规。\n'
    '- refute：原文与该风险类型相反，或原文写明的是合法合规、无歧视、无排他的做法（含"符合/不低于法定/无倾向"这类表述）。\n'
    '- insufficient：原文是中性、正当、常规的要求，无法仅凭字面推出该违规。\n\n'
    '硬规则：\n'
    '1. 从中性/正当要求推断出歧视或排他，一律 insufficient。\n'
    '2. 原文没有明确违规定语（如"仅限/指定/不得/须为/注册地+加分/酌情分且不量化"）就不得判 support。\n'
    '3. 严禁脑补、联想，判断必须落到原文具体字句。\n\n'
    '只输出一行 JSON：{"verdict":"support|refute|insufficient","reason":"一句话，引用原文关键句"}'
)

def verdict_for(quote, risk):
    body = {
        'model': MODEL,
        'messages': [
            {'role':'system','content':SYS},
            {'role':'user','content': '【招标文件原文】\n' + (quote or '')[:800] + '\n\n【待核风险类型】\n' + (risk or '')},
        ],
        'temperature': 0, 'max_tokens': 160,
    }
    headers = {'Authorization':'Bearer '+key, 'Content-Type':'application/json'}
    last = None
    for attempt in range(3):
        try:
            r = requests.post(URL, headers=headers, json=body, timeout=60)
            last = r.status_code
            if r.status_code == 200:
                c = r.json()['choices'][0]['message']['content'].strip()
                m = re.search(r'\{.*\}', c, re.S)
                if m:
                    o = json.loads(m.group(0))
                    return o.get('verdict','?'), o.get('reason','')
                return '?', c[:100]
        except Exception:
            pass
        time.sleep(2 * (attempt + 1))
    return 'ERR', 'HTTP ' + str(last)

# 分组去重：key = core_key(source_quote)（不含 risk_type）
groups = OrderedDict()
for doc, f in findings:
    k = core_key(f.get('source_quote'))
    groups.setdefault(k, []).append((doc, f))

print('去重后独立裁决组数:', len(groups), flush=True)
cache = {}
results = []
for gi, (k, items) in enumerate(groups.items()):
    if k in cache:
        v, reason = cache[k]
    else:
        doc0, f0 = items[0]
        v, reason = verdict_for(f0.get('source_quote'), f0.get('risk_type'))
        cache[k] = (v, reason)
        print('[%d/%d] 裁决 %s -> %s' % (gi+1, len(groups), f0.get('risk_type'), v), flush=True)
        time.sleep(0.2)
    for doc, f in items:
        results.append({
            'doc': doc, 'risk_id': f.get('risk_id'), 'risk_type': f.get('risk_type'),
            'agent': f.get('agent'), 'severity': f.get('severity'),
            'confidence': f.get('confidence'), 'verdict': v, 'verifier_reason': reason,
            'source_quote': (f.get('source_quote') or '')[:200],
        })

json.dump(results, open('output/findings/verifier_offline_result_v2.json', 'w', encoding='utf-8'), ensure_ascii=False, indent=1)
c = Counter(r['verdict'] for r in results)
print('\n=== v2 三分类统计 ===', dict(c), flush=True)

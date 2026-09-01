# -*- coding: utf-8 -*-
import json, re, time, requests
from collections import Counter

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

SYS = (
    '你是一名严格的政府采购招标文件审计员，任务只有一个：尽最大努力"证伪"下面这条违规发现，'
    '也就是找出它不成立的理由，而不是替它辩护。\n\n'
    '你会收到【招标文件原文】和【审计员的违规结论】。请仅依据原文的字面内容，判断结论是否成立，三选一：\n'
    '- support：原文明确、逐字包含违规定语句（如"仅限/指定…品牌""注册地为…加分""注册资本须不低于…""酌情给分且不设量化"等硬性要求），足以直接推出该违规。\n'
    '- refute：原文明确与该结论相反（例如原文写的是合法合规、无歧视的做法）。\n'
    '- insufficient：原文是中性表述或正当要求，不足以推断出该违规（例如"2小时内响应""提供承诺函"之类，不能据此推断"地域歧视/排他"）。\n\n'
    '硬性规则：\n'
    '1. 从中性/正当要求推断出歧视或排他性结论的，一律 insufficient。\n'
    '2. 结论措辞出现"可能、疑似、潜在、值得关注、隐含、倾向"而原文没有明确违规定语的，一律 insufficient。\n'
    '3. 只有原文能"字面、逐字"看出违规，才判 support。严禁脑补、严禁联想。\n'
    '4. 每一步判断都要能指到原文的具体字句。\n\n'
    '只输出一行 JSON，禁止任何多余文字：{"verdict":"support|refute|insufficient","reason":"一句话，引用原文关键句"}'
)

def clean_reason(r):
    r = (r or '').split(chr(0x1F4CE))[0]
    r = re.sub(r'\s+', ' ', r).strip()
    return r[:400]

def verify(f):
    quote = (f.get('source_quote') or '')[:800]
    risk = f.get('risk_type') or ''
    reason = clean_reason(f.get('reason') or '')
    user = '【招标文件原文】\n' + quote + '\n\n【审计员的违规结论】\n风险类型：' + risk + '\n审计员论证：' + reason
    body = {
        'model': MODEL,
        'messages': [{'role': 'system', 'content': SYS}, {'role': 'user', 'content': user}],
        'temperature': 0, 'max_tokens': 200,
    }
    headers = {'Authorization': 'Bearer ' + key, 'Content-Type': 'application/json'}
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
                    return {'verdict': o.get('verdict', '?'), 'reason': o.get('reason', '')}
                return {'verdict': '?', 'reason': c[:120]}
        except Exception:
            pass
        time.sleep(2 * (attempt + 1))
    return {'verdict': 'ERR', 'reason': 'HTTP ' + str(last)}

results = []
for i, (doc, f) in enumerate(findings):
    v = verify(f)
    rec = {
        'doc': doc, 'risk_id': f.get('risk_id'), 'risk_type': f.get('risk_type'),
        'agent': f.get('agent'), 'severity': f.get('severity'),
        'confidence': f.get('confidence'), 'verdict': v['verdict'],
        'verifier_reason': v['reason'], 'source_quote': (f.get('source_quote') or '')[:200],
    }
    results.append(rec)
    print('[%d/%d] %s %s %s -> %s' % (i + 1, len(findings), doc, rec['risk_id'], rec['risk_type'], v['verdict']), flush=True)
    time.sleep(0.3)

json.dump(results, open('output/findings/verifier_offline_result.json', 'w', encoding='utf-8'), ensure_ascii=False, indent=1)
c = Counter(r['verdict'] for r in results)
print('\n=== 三分类统计 ===', dict(c), flush=True)

# -*- coding: utf-8 -*-
import json, re, time, requests

key = None
for line in open('.env', encoding='utf-8'):
    s = line.strip()
    if s.startswith('DASHSCOPE_API_KEY'):
        key = s.split('=', 1)[1].strip().strip('"')
URL = 'https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions'
MODEL = 'qwen3.6-flash'

SYS = """你是政府采购招标文件审核的独立复核员。给定【招标文件原文】和【待核风险类型】，仅依据原文字面内容，判断"原文是否确实构成该类违规"。三选一：
- support：原文明确、逐字包含该类违规的硬性表述，足以直接推出违规。
- refute：原文与该风险类型相反，或原文写明的是合法合规、无歧视、无排他的做法。
- insufficient：原文是中性、正当、常规的要求，无法仅凭字面推出该违规。

硬规则：
1. 从中性/正当要求推断出歧视或排他，一律 insufficient。
2. 原文没有明确违规定语就不得判 support。
3. 严禁脑补、联想，判断必须落到原文具体字句。

【已由人工复核的判例，供你校准】：
1. "供应商注册地为茂名市的，每提供一个业绩另加1分" → support（注册地直接作差异化加分，构成地域歧视）
2. "仅限华润、林德、空气产品等品牌，其他品牌不得分" → support（明确指定品牌且排斥同等产品）
3. "酌情给分，最高2分，且不设具体量化标准" → support（明确主观、未量化）
4. "本采购包不接受联合体投标" → refute（采购人有权不接受联合体，不构成排斥供应商）
5. "本项目气体产品不允许采购进口产品" → insufficient（不允许进口是本国产品政策的合规方向，非违规）
6. "给予1%-5%的价格扣除，具体比例根据重要性确定" → refute（1-5%是法定政策区间，非标准不明确）
7. "广东省内的电子认证服务机构签发的CA数字证书" → insufficient（省域CA属行业惯例，不足以判定地域歧视）

只输出一行 JSON，禁止任何多余文字：{"verdict":"support|refute|insufficient","reason":"一句话"}"""

def verdict_for(quote, risk):
    body = {'model': MODEL,
            'messages':[{'role':'system','content':SYS},
                        {'role':'user','content':'【招标文件原文】\n'+(quote or '')[:800]+'\n\n【待核风险类型】\n'+(risk or '')}],
            'temperature':0, 'max_tokens':160}
    headers = {'Authorization':'Bearer '+key, 'Content-Type':'application/json'}
    last=None
    for attempt in range(5):
        try:
            r = requests.post(URL, headers=headers, json=body, timeout=60)
            last=r.status_code
            if r.status_code==200:
                c=r.json()['choices'][0]['message']['content'].strip()
                m=re.search(r'\{.*\}', c, re.S)
                if m:
                    o=json.loads(m.group(0)); return o.get('verdict','?'), o.get('reason','')
                return '?', c[:80]
        except Exception:
            pass
        time.sleep(3*(attempt+1))
    return 'ERR', 'HTTP '+str(last)

v3 = json.load(open('output/findings/verifier_offline_result_v3.json', encoding='utf-8'))
errs = [r for r in v3 if r['verdict']=='ERR']
print('ERR 条数:', len(errs), flush=True)
# 按 (doc,risk_id) 去重后重跑；同名 risk_id 用第一条的原文
seen = {}
for r in errs:
    k = (r['doc'], r['risk_id'])
    if k not in seen:
        seen[k] = r

for (doc, rid), r0 in seen.items():
    v, reason = verdict_for(r0['source_quote'], r0['risk_type'])
    print('重跑 %s %s %s -> %s' % (doc, rid, r0['risk_type'], v), flush=True)
    time.sleep(1)
    for r in v3:
        if r['doc']==doc and r['risk_id']==rid:
            r['verdict'] = v
            r['verifier_reason'] = reason

json.dump(v3, open('output/findings/verifier_offline_result_v3.json','w',encoding='utf-8'), ensure_ascii=False, indent=1)
from collections import Counter
c = Counter(r['verdict'] for r in v3)
print('\n=== v3 修复后统计 ===', dict(c), flush=True)

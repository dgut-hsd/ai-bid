# -*- coding: utf-8 -*-
"""EvidenceVerifier 验收分析：裁决分布 + 降级效果 + 注入错误真阳性对照。"""
import json, os, sys
from collections import Counter

def load(path):
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    if isinstance(data, dict):
        for k in ("findings", "results", "risks", "items"):
            if k in data:
                return data[k]
        return list(data.values()) if data else []
    return data

def main():
    doc = sys.argv[1] if len(sys.argv) > 1 else "MAOMING_mutated"
    here = os.path.dirname(os.path.abspath(__file__))
    path = os.path.join(here, "findings", f"{doc}_findings.json")
    findings = load(path)
    print(f"文档: {doc}  总 finding 数: {len(findings)}")
    if not findings:
        print("(空)")
        return

    vc = Counter(f.get("evidence_verdict") for f in findings)
    print(f"evidence_verdict 分布: {dict(vc)}")
    sev = Counter(f.get("severity") for f in findings)
    print(f"severity 分布: {dict(sev)}")

    downgraded = [f for f in findings if f.get("evidence_verdict") in ("refute","insufficient")]
    downgraded_info = sum(1 for f in downgraded if f.get("severity") == "info")
    print(f"refute/insufficient 共 {len(downgraded)} 条，其中降级为 info: {downgraded_info}")

    # ── 注入错误对照（MAOMING 3 条）──
    print("\n════ 注入错误对照 ════")
    injections = {
      "R-C04 地域加分(真违规,应 support)": lambda q: "注册地为茂名" in q or "注册地" in q or "另加" in q,
      "R-C05 规模门槛(真违规,引擎此前漏检)": lambda q: "注册资本" in q or "500万" in q,
      "D 权重和105(确定性错误,引擎此前漏检)": lambda q: False,  # 依赖 risk_type/reason
    }
    for name, pred in injections.items():
        hits = [f for f in findings if pred(str(f.get("source_quote","")))]
        if name.startswith("D") or name.startswith("R-C05"):
            # 权重和靠 risk_type/reason 匹配
            if "105" in name:
                hits = [f for f in findings if ("105" in str(f.get("reason","")) or "105" in str(f.get("suggestion","")) or "权重" in str(f.get("risk_type","")))]
            elif "注册" in name:
                hits = [f for f in findings if "注册资本" in str(f.get("source_quote","")) or "500万" in str(f.get("source_quote",""))]
        if hits:
            for f in hits[:4]:
                print(f"  [{name}] -> {f.get('risk_id')} | verdict={f.get('evidence_verdict')} | sev={f.get('severity')} | {str(f.get('source_quote'))[:60]}")
        else:
            print(f"  [{name}] -> 未在 findings 中找到（引擎漏检，与 EvidenceVerifier 无关）")

    # ── support 明细（真阳性候选）──
    print("\n════ support（保留）明细 ════")
    sup = [f for f in findings if f.get("evidence_verdict") == "support"]
    for f in sup:
        print(f"  [{f.get('risk_id')}] {f.get('severity')} {f.get('risk_type')} | {str(f.get('source_quote'))[:70]}")

    # ── refute/insufficient 摘录 ──
    print("\n════ refute / insufficient 摘录（前 20）════")
    for f in downgraded[:20]:
        print(f"  [{f.get('risk_id')}] {f.get('evidence_verdict'):12s} -> {f.get('severity'):6s} | {f.get('risk_type')} | {str(f.get('verifier_reason'))[:60]}")

    none = [f for f in findings if f.get("evidence_verdict") is None]
    print(f"\n未核验（None，LLM 失败/超时）: {len(none)} 条")

    tag_pass = sum(1 for f in findings if "EvidenceVerify] 通过" in f.get("reason","").replace("✅","通过"))
    tag_fail = sum(1 for f in findings if "EvidenceVerify]" in f.get("reason","") and "通过" not in f.get("reason","").replace("✅","通过"))
    print(f"[EvidenceVerify] 标记: 通过 {sum(1 for f in findings if '✅' in f.get('reason',''))} 条, 未通过 {sum(1 for f in findings if '❓' in f.get('reason',''))} 条")

if __name__ == "__main__":
    main()

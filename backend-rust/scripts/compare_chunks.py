"""Compare chunk quality against original PDF."""
import json
import sys

# Try to load PDF library
try:
    import fitz  # PyMuPDF
    HAS_PDF = True
except ImportError:
    try:
        import pdfplumber
        HAS_PDF = True
    except ImportError:
        HAS_PDF = False
        print("WARNING: No PDF library available (PyMuPDF or pdfplumber)")

def load_chunks(path):
    with open(path, 'r', encoding='utf-8') as f:
        return json.load(f)

def analyze_chunk_quality(data, name):
    print(f"\n{'='*70}")
    print(f"### {name}")
    print(f"{'='*70}")

    chunks = data['chunks']
    stats = data['stats']
    config = data['config']

    # Basic stats
    print(f"\n--- 基本统计 ---")
    print(f"总chunks: {stats['total_chunks']}")
    print(f"类型分布: {stats['type_counts']}")
    print(f"总字符数: {stats['total_chars']}")
    print(f"平均chunk大小: {stats['avg_chunk_size']:.1f} 字符")
    print(f"最大chunk: {stats['max_chunk_size']} 字符")
    print(f"最小chunk: {stats['min_chunk_size']} 字符")

    # Quality issues
    issues = []

    # 1. Tiny chunks (< 50 chars)
    tiny = [(c['chunk_id'], len(c['text']), c['text'][:60]) for c in chunks if len(c['text']) < 50]
    if tiny:
        issues.append(f"❌ 过短chunks (<50字符): {len(tiny)}个")
        for tid, tlen, ttxt in tiny:
            print(f"   [{tid}] len={tlen}: [{ttxt}]")

    # 2. Unclassified content
    unclassified = [c for c in chunks if '未归类' in str(c['section_path'])]
    if unclassified:
        issues.append(f"⚠️ 未归类内容: {len(unclassified)}个chunks")
        for uc in unclassified:
            print(f"   [{uc['chunk_id']}] pages {uc['page_start']}-{uc['page_end']}: {uc['section_path']}")

    # 3. Split chunk quality
    splits = [c for c in chunks if c['chunk_type'].get('type') == 'Split']
    if splits:
        # Group by section_path to check pairs
        from collections import defaultdict
        split_groups = defaultdict(list)
        for s in splits:
            key = tuple(s['section_path'])
            split_groups[key].append(s)

        bad_splits = 0
        for key, group in split_groups.items():
            if len(group) >= 2:
                # Sort by part
                group.sort(key=lambda x: x['chunk_type']['part'])
                for i in range(len(group)-1):
                    p1 = group[i]
                    p2 = group[i+1]
                    p1_end = p1['text'][-60:]
                    p2_start = p2['text'][:60]
                    # Check if overlap makes sense
                    # The overlap should be p1's tail appearing in p2's head
                    overlap_ok = any(p1['text'][-j:] in p2['text'] for j in range(30, min(200, len(p1['text']))))
                    if not overlap_ok:
                        bad_splits += 1
        issues.append(f"ℹ️ Split chunks: {len(splits)}个 ({len(split_groups)}组)")

    # 4. Section path depth analysis
    from collections import Counter
    depth_counter = Counter()
    weird_paths = []
    for c in chunks:
        depth = len(c['section_path'])
        depth_counter[depth] += 1
        # Check for content-like section paths (scoring criteria, etc.)
        for seg in c['section_path']:
            if any(kw in seg for kw in ['0.0;', '1.0;', '得分', '分）', '；）']):
                weird_paths.append((c['chunk_id'], c['section_path']))
                break

    if weird_paths:
        issues.append(f"❌ section_path包含评分内容: {len(weird_paths)}个chunks")
        for wid, wp in weird_paths[:3]:
            print(f"   [{wid}]: {wp}")

    # 5. Check if section titles are proper vs content-like
    path_terms = Counter()
    for c in chunks:
        for seg in c['section_path']:
            path_terms[seg] += 1

    # Very long section path segments (likely content, not titles)
    long_segs = [(seg, cnt) for seg, cnt in path_terms.items() if len(seg) > 50]
    if long_segs:
        issues.append(f"⚠️ 过长section_path段 (>50字符): {len(long_segs)}个")
        for seg, cnt in long_segs[:5]:
            print(f'   [{cnt}x] "{seg[:80]}..."')

    # Summary
    print(f"\n--- 质量问题汇总 ---")
    if issues:
        for iss in issues:
            print(f"  {iss}")
    else:
        print("  ✅ 未发现明显质量问题")

    return issues

# Load both files
tsinghua = load_chunks('output/chunks/清华大学深圳国际研究生院智慧校园项目公开招标文件_chunks.json')
dongguan = load_chunks('output/chunks/智慧教室环境改造工程_chunks.json')

issues_ts = analyze_chunk_quality(tsinghua, "清华大学智慧校园招标文件")
issues_dg = analyze_chunk_quality(dongguan, "东莞理工学院智慧教室改造")

print(f"\n{'='*70}")
print("### 对比总结")
print(f"{'='*70}")
print(f"文件1 (清华): {tsinghua['stats']['total_chunks']} chunks, 平均{tsinghua['stats']['avg_chunk_size']:.0f}字符, 最小{tsinghua['stats']['min_chunk_size']}字符")
print(f"文件2 (东莞): {dongguan['stats']['total_chunks']} chunks, 平均{dongguan['stats']['avg_chunk_size']:.0f}字符, 最小{dongguan['stats']['min_chunk_size']}字符")

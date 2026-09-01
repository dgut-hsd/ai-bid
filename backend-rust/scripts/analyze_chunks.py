#!/usr/bin/env python3
"""Comprehensive chunk quality analysis for both bid documents."""
import json
import os

base = r"d:\10_Work\03_Team_Competition\ai-bid"

with open(os.path.join(base, "output/chunks/清华大学深圳国际研究生院智慧校园项目公开招标文件_chunks.json"), "r", encoding="utf-8") as f:
    tsinghua = json.load(f)

with open(os.path.join(base, "output/chunks/智慧教室环境改造工程_chunks.json"), "r", encoding="utf-8") as f:
    zhihui = json.load(f)

with open(os.path.join(base, "output/raw_json/清华大学深圳国际研究生院智慧校园项目公开招标文件_raw.json"), "r", encoding="utf-8") as f:
    tsinghua_raw = json.load(f)

with open(os.path.join(base, "output/raw_json/智慧教室环境改造工程_raw.json"), "r", encoding="utf-8") as f:
    zhihui_raw = json.load(f)

def char_count(text):
    return len(text)

def analyze_chunks(name, data, raw_data):
    chunks = data["chunks"]
    config = data["config"]
    print(f"\n{'='*70}")
    print(f"  {name}")
    print(f"{'='*70}")

    print(f"\n── 1. Size Compliance ──")
    oversized = []
    tiny = []
    for c in chunks:
        sz = char_count(c["text"])
        cid = c["chunk_id"]
        if sz > config["split_max_len"]:
            oversized.append((cid, sz))
        if sz < config["min_chunk_size"]:
            tiny.append((cid, sz, c["section_path"]))

    print(f"  超大 (>1500): {len(oversized)}")
    for o in oversized:
        print(f"    ❌ {o[0]}: {o[1]} chars")
    print(f"  碎片 (<50): {len(tiny)}")
    for t in tiny:
        print(f"    ❌ {t[0]}: {t[1]} chars, path={t[2]}")

    print(f"\n── 2. Size Distribution ──")
    ranges = [
        ("<50 (碎片)", 0, 49),
        ("50-120 (短)", 50, 120),
        ("120-500 (正常)", 120, 500),
        ("500-1000 (长)", 500, 1000),
        ("1000-1500 (超长)", 1000, 1500),
        (">1500 (违规)", 1501, 99999),
    ]
    for label, lo, hi in ranges:
        count = sum(1 for c in chunks if lo <= char_count(c["text"]) <= hi)
        pct = count / len(chunks) * 100
        print(f"  {label}: {count} ({pct:.1f}%)")

    print(f"\n── 3. Chunk Type Distribution ──")
    leaf_cnt = sum(1 for c in chunks if c["chunk_type"].get("type") == "Leaf")
    merged_cnt = sum(1 for c in chunks if c["chunk_type"].get("type") == "Merged")
    split_cnt = sum(1 for c in chunks if c["chunk_type"].get("type") == "Split")
    print(f"  Leaf: {leaf_cnt}, Merged: {merged_cnt}, Split: {split_cnt}")
    adjacent_merge = sum(1 for c in chunks if c["chunk_type"].get("rule") == "adjacent_merge")
    tiny_merge = sum(1 for c in chunks if c["chunk_type"].get("rule") == "tiny_merge")
    print(f"  adjacent_merge: {adjacent_merge}, tiny_merge: {tiny_merge}")
    merged_chunks = [c for c in chunks if c["chunk_type"].get("type") == "Merged"]
    if merged_chunks:
        avg_child = sum(c["chunk_type"].get("child_count", 0) for c in merged_chunks) / len(merged_chunks)
        avg_merged_size = sum(char_count(c["text"]) for c in merged_chunks) / len(merged_chunks)
        print(f"  平均合并子节点数: {avg_child:.1f}")
        print(f"  Merged 平均大小: {avg_merged_size:.1f}")

    print(f"\n── 4. Split Chunk Analysis ──")
    split_chunks = [c for c in chunks if c["chunk_type"].get("type") == "Split"]
    split_groups = {}
    for c in split_chunks:
        path_key = " > ".join(c["section_path"])
        if path_key not in split_groups:
            split_groups[path_key] = []
        split_groups[path_key].append(c)

    print(f"  Split 总数: {len(split_chunks)} ({len(split_groups)} 组)")
    for path, group in split_groups.items():
        group.sort(key=lambda c: c["chunk_type"].get("part", 0))
        parts = [c["chunk_type"].get("part") for c in group]
        total = group[0]["chunk_type"].get("total", 0)
        sizes = [char_count(c["text"]) for c in group]
        print(f"    Path: {path[:80]}...")
        print(f"    Parts: {parts}, Total={total}, 实际={len(group)}")
        print(f"    Sizes: {sizes}")
        for i in range(len(group)-1):
            t1 = group[i]["text"]
            t2 = group[i+1]["text"]
            overlap_chars = 200
            end_of_first = t1[-overlap_chars:] if len(t1) >= overlap_chars else t1
            # Check for overlap: last N chars of part i should appear in part i+1
            found_overlap = False
            for check_len in [200, 100, 50, 20]:
                if len(t1) >= check_len and len(t2) >= check_len:
                    tail = t1[-check_len:]
                    if tail in t2:
                        found_overlap = True
                        print(f"    Overlap part{i+1}→part{i+2}: ✅ (~{check_len} chars)")
                        break
            if not found_overlap:
                # Try char-by-char
                overlap_count = 0
                min_len = min(len(end_of_first), len(t2))
                for j in range(1, min_len+1):
                    if end_of_first[-j:] == t2[:j]:
                        overlap_count = j
                print(f"    Overlap part{i+1}→part{i+2}: {'✅' if overlap_count >= 20 else '⚠️'} ({overlap_count} chars)")

    print(f"\n── 5. Page Coverage ──")
    covered_pages = set()
    for c in chunks:
        for p in range(c["page_start"], c["page_end"] + 1):
            covered_pages.add(p)
    total_pages = len(raw_data.get("pages", []))
    all_pages = set(range(total_pages))
    missing_pages = sorted(all_pages - covered_pages)
    print(f"  PDF pages: {total_pages}, Covered: {len(covered_pages)}, Coverage: {len(covered_pages)/total_pages*100:.1f}%")
    if missing_pages:
        groups = []
        start = missing_pages[0]; end = missing_pages[0]
        for p in missing_pages[1:]:
            if p == end + 1: end = p
            else: groups.append((start, end)); start = p; end = p
        groups.append((start, end))
        print(f"  Missing ({len(missing_pages)}): {groups}")
        for grp in groups:
            for p in range(grp[0], grp[1]+1):
                page = raw_data["pages"][p]
                texts = [b.get("text", "")[:60] for b in page.get("blocks", [])]
                combined = " | ".join(texts)[:150]
                print(f"    Page {p}: {combined}...")
    else:
        print(f"  No missing pages ✅")

    print(f"\n── 6. Block ID ──")
    no_bid = [c["chunk_id"] for c in chunks if not c["source_block_ids"]]
    print(f"  无block_id: {len(no_bid)}")
    raw_ids = set()
    for p in raw_data.get("pages", []):
        for b in p.get("blocks", []):
            raw_ids.add(b.get("id", ""))
    chunk_ids_set = set()
    for c in chunks:
        for bid in c["source_block_ids"]:
            chunk_ids_set.add(bid)
    print(f"  Raw unique blocks: {len(raw_ids)}, In chunks: {len(chunk_ids_set)}")
    # Shared blocks
    block_to_chunks = {}
    for c in chunks:
        for bid in c["source_block_ids"]:
            if bid not in block_to_chunks: block_to_chunks[bid] = []
            block_to_chunks[bid].append(c["chunk_id"])
    shared = sum(1 for b, cl in block_to_chunks.items() if len(cl) > 1)
    pct = shared / len(block_to_chunks) * 100 if block_to_chunks else 0
    print(f"  跨chunk共享: {shared} ({pct:.1f}%)")

    print(f"\n── 7. Section Path ──")
    empty_p = [c["chunk_id"] for c in chunks if not c["section_path"]]
    print(f"  空路径: {len(empty_p)}")
    depth_dist = {}
    for c in chunks:
        d = len(c["section_path"])
        depth_dist[d] = depth_dist.get(d, 0) + 1
    for d in sorted(depth_dist):
        print(f"    Depth {d}: {depth_dist[d]}")
    print(f"  Max depth: {max(depth_dist.keys()) if depth_dist else 0}")

    print(f"\n── 8. Embed Text ──")
    empty_emb = [c["chunk_id"] for c in chunks if not c.get("embed_text")]
    no_bracket = [c["chunk_id"] for c in chunks if c.get("embed_text") and not c["embed_text"].startswith("【")]
    print(f"  空embed: {len(empty_emb)}, 无【】前缀: {len(no_bracket)}")
    samples = [c for c in chunks if c.get("embed_text") and c["embed_text"].startswith("【")]
    if samples:
        print(f"  样例: {samples[0]['embed_text'][:100]}...")

    print(f"\n── 9. Table Analysis ──")
    table_pages = []
    tables_found = 0
    for pi, page in enumerate(raw_data.get("pages", [])):
        page_tables = page.get("tables", [])
        if page_tables:
            tables_found += len(page_tables)
            table_pages.append(pi)
    print(f"  Raw JSON表格: {tables_found}, 含表格页面: {len(table_pages)}")

    # Check for | separated content in blocks (text-based tables)
    pipe_blocks = 0
    pipe_texts = []
    for pi, page in enumerate(raw_data.get("pages", [])):
        for b in page.get("blocks", []):
            text = b.get("text", "")
            if "|" in text and len(text) > 20:
                pipe_blocks += 1
                pipe_texts.append((pi, text[:150]))
    print(f"  含|分隔符的blocks: {pipe_blocks}")
    if pipe_texts:
        for pt in pipe_texts[:5]:
            print(f"    Page {pt[0]}: {pt[1][:100]}...")

    # Table content coverage
    if table_pages:
        print(f"\n── 10. Table Content Coverage ──")
        full_retain = 0
        partial_retain = 0
        full_loss = 0
        for tp in table_pages:
            page = raw_data["pages"][tp]
            page_tables = page.get("tables", [])
            # Extract table text content
            table_texts = set()
            for t in page_tables:
                rows = t.get("rows", [])
                for row in rows:
                    if isinstance(row, list):
                        for cell in row:
                            if isinstance(cell, str):
                                table_texts.add(cell.strip())
                    elif isinstance(row, dict):
                        cells = row.get("cells", [])
                        if isinstance(cells, list):
                            for cell in cells:
                                if isinstance(cell, str):
                                    table_texts.add(cell.strip())
                                elif isinstance(cell, dict):
                                    table_texts.add(cell.get("text", "").strip())

            # Find chunks covering this page
            covering = [c for c in chunks if c["page_start"] <= tp <= c["page_end"]]
            chunk_text = " ".join(c["text"] for c in covering)

            # Count how many table cell texts appear in chunks
            found = 0
            for tt in table_texts:
                if tt and len(tt) >= 2 and tt in chunk_text:
                    found += 1

            total_cells = len([t for t in table_texts if t and len(t) >= 2])
            if total_cells == 0:
                full_loss += 1
                status = "N/A (empty)"
            elif found == total_cells:
                full_retain += 1
                status = "✅ FULL"
            elif found > 0:
                partial_retain += 1
                status = f"⚠️ PARTIAL ({found}/{total_cells})"
            else:
                full_loss += 1
                status = f"❌ LOST (0/{total_cells})"

            if total_cells > 0:
                print(f"  Page {tp}: {status}")

        total_with_content = full_retain + partial_retain + full_loss
        print(f"  完整保留: {full_retain} ({full_retain/total_with_content*100:.1f}%)" if total_with_content else "")
        print(f"  部分保留: {partial_retain} ({partial_retain/total_with_content*100:.1f}%)" if total_with_content else "")
        print(f"  完全丢失: {full_loss} ({full_loss/total_with_content*100:.1f}%)" if total_with_content else "")

    # Orphan analysis
    print(f"\n── 11. Orphan Chunk ──")
    ch0 = next((c for c in chunks if c["chunk_id"] == "ch_000"), None)
    if ch0:
        print(f"  ch_000 path: {ch0['section_path']}, size: {char_count(ch0['text'])} chars")
        print(f"  page: {ch0['page_start']}-{ch0['page_end']}, type: {ch0['chunk_type']}")
        if char_count(ch0['text']) > 1500:
            print(f"  ⚠️ EXCEEDS split_max_len (1500)!")

    return {
        "chunks": chunks, "oversized": oversized, "tiny": tiny,
        "table_pages": table_pages, "tables_found": tables_found,
        "pipe_blocks": pipe_blocks, "merged_subtypes": {"adjacent": adjacent_merge, "tiny": tiny_merge},
    }

# ── Run ──
print("=" * 70)
print("  DOUBLE BID CHUNK QUALITY VERIFICATION")
print("=" * 70)
ts_result = analyze_chunks("清华大学深圳国际研究生院智慧校园项目", tsinghua, tsinghua_raw)
zh_result = analyze_chunks("智慧教室环境改造工程", zhihui, zhihui_raw)

# ── Specific Claims ──
print(f"\n{'='*70}")
print(f"  SPECIFIC CLAIM VERIFICATION")
print(f"{'='*70}")

# P0-1
print(f"\n── P0-1: Orphan ch_000 bypassing split (清华大学) ──")
for c in ts_result["oversized"]:
    print(f"  ❌ {c[0]}: {c[1]} chars — confirms orphan NOT split")
if not ts_result["oversized"]:
    print(f"  No oversized — claim invalid for current data")

# P1-1
print(f"\n── P1-1: Tiny chunks stuck (merge_tiny_chunks one-direction) ──")
ts_chunks = ts_result["chunks"]
for target_id in ["ch_051", "ch_114"]:
    c = next((cc for cc in ts_chunks if cc["chunk_id"] == target_id), None)
    if c:
        sz = char_count(c["text"])
        path = c["section_path"]
        idx = ts_chunks.index(c)
        prev_path = ts_chunks[idx-1]["section_path"] if idx > 0 else []
        next_path = ts_chunks[idx+1]["section_path"] if idx < len(ts_chunks)-1 else []
        same_prev = path[0] == prev_path[0] if path and prev_path else False
        same_next = path[0] == next_path[0] if path and next_path else False
        print(f"  {target_id}: {sz} chars, path={path}")
        print(f"    Prev top-path match: {same_prev}, Next top-path match: {same_next}")
        if not same_prev and same_next and sz < 50:
            print(f"    ✅ BUG CONFIRMED: forward merge fails but backward would work")

# P1-2: Check format placeholder pages
print(f"\n── P1-2: Format placeholder pages in 清华大学 ──")
placeholder_pages = [60, 61, 62, 63, 64, 66, 71, 72, 73, 74]
for p in placeholder_pages:
    if p < len(tsinghua_raw["pages"]):
        page = tsinghua_raw["pages"][p]
        blocks = page.get("blocks", [])
        texts = [b.get("text", "") for b in blocks]
        combined = " | ".join(texts)[:150]
        # Check coverage
        covering = [c["chunk_id"] for c in ts_chunks if c["page_start"] <= p <= c["page_end"]]
        print(f"  Page {p}: '{combined}' → covered by: {covering}")

# P0-2: Table loss
print(f"\n── P0-2: Table Detection Rate ──")
print(f"  智慧教室: {zh_result['tables_found']} tables on {len(zh_result['table_pages'])} pages")
print(f"  清华大学: {ts_result['tables_found']} tables on {len(ts_result['table_pages'])} pages")
print(f"  清华大学 |分隔 blocks: {ts_result['pipe_blocks']}")

# Check sections for both docs
print(f"\n── Sections Output Check ──")
sections_dir = os.path.join(base, "output/sections")
for fname in os.listdir(sections_dir):
    if fname.endswith("_sections.json"):
        with open(os.path.join(sections_dir, fname), "r", encoding="utf-8") as f:
            sec = json.load(f)
        print(f"  {fname}: {sec.get('stats', {})}")
        # Check orphan blocks
        orphan_blocks = sec.get("stats", {}).get("orphan_blocks", 0)
        if orphan_blocks > 0:
            print(f"    Orphan blocks: {orphan_blocks}")

print(f"\n{'='*70}")
print(f"  VERIFICATION COMPLETE")
print(f"{'='*70}")

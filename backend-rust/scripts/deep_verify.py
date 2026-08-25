#!/usr/bin/env python3
"""Deep verification of specific claims."""
import json, os, re

base = r"d:\10_Work\03_Team_Competition\ai-bid"

with open(os.path.join(base, "output/chunks/清华大学深圳国际研究生院智慧校园项目公开招标文件_chunks.json"), "r", encoding="utf-8") as f:
    tsinghua = json.load(f)
with open(os.path.join(base, "output/chunks/智慧教室环境改造工程_chunks.json"), "r", encoding="utf-8") as f:
    zhihui = json.load(f)
with open(os.path.join(base, "output/raw_json/清华大学深圳国际研究生院智慧校园项目公开招标文件_raw.json"), "r", encoding="utf-8") as f:
    ts_raw = json.load(f)
with open(os.path.join(base, "output/raw_json/智慧教室环境改造工程_raw.json"), "r", encoding="utf-8") as f:
    zh_raw = json.load(f)

# ── 1. Deep dive into ch_115 (清华大学) ──
print("=" * 70)
print("  1. ch_115 CONTENT ANALYSIS (claimed no-coverage pages)")
print("=" * 70)
ts_chunks = tsinghua["chunks"]
ch115 = next((c for c in ts_chunks if c["chunk_id"] == "ch_115"), None)
if ch115:
    text = ch115["text"]
    print(f"  ch_115 page range: {ch115['page_start']}-{ch115['page_end']}")
    print(f"  ch_115 path: {ch115['section_path']}")
    print(f"  ch_115 size: {len(text)} chars")
    print(f"  ch_115 type: {ch115['chunk_type']}")

    # What pages are in the text?
    page_mentions = re.findall(r'第(\d+)页共78页', text)
    print(f"  Page markers in text: {page_mentions}")

    # Show the text structure - header per page
    # Split by common headers
    sections = text.split('\n\n')
    print(f"  Number of \\n\\n sections: {len(sections)}")
    for i, s in enumerate(sections[:10]):
        print(f"  [{i}] {s[:120]}...")
    if len(sections) > 10:
        print(f"  ... and {len(sections)-10} more sections")

    # Check if specific format pages are included
    format_keywords = ["格式自拟", "投标人情况介绍", "同类项目业绩", "项目执行团队",
                       "相关资质", "自主知识产权", "重要事项说明", "技术方案与组织实施",
                       "质量（完成时间）保障", "售后服务方案", "其他需要提供的技术资料"]
    for kw in format_keywords:
        found = kw in text
        print(f"  '{kw}' in ch_115: {'✅' if found else '❌'}")

# ── 2. Check page-by-page coverage gaps in 清华大学 ──
print(f"\n{'='*70}")
print(f"  2. PAGE-BY-PAGE CONTENT GAP ANALYSIS (清华大学)")
print(f"{'='*70}")
# For each page 60-64, 66, 71-74, check what raw blocks exist
# and what chunks cover them
check_pages = [60, 61, 62, 63, 64, 66, 71, 72, 73, 74]
for p in check_pages:
    page = ts_raw["pages"][p]
    blocks = page.get("blocks", [])
    total_block_text = "\n".join(b.get("text","") for b in blocks)
    total_block_len = len(total_block_text)

    # Which chunks cover this page?
    covering = [c for c in ts_chunks if c["page_start"] <= p <= c["page_end"]]
    chunk_texts = {}
    for c in covering:
        # Check if the page-specific text appears in chunk text
        for b in blocks:
            bt = b.get("text", "")
            if bt and len(bt) > 5:
                if bt in c["text"]:
                    chunk_texts[c["chunk_id"]] = chunk_texts.get(c["chunk_id"], 0) + len(bt)

    print(f"  Page {p}: {len(blocks)} blocks, {total_block_len} chars")
    for ct_id, matched in chunk_texts.items():
        pct = matched / total_block_len * 100 if total_block_len > 0 else 0
        print(f"    → {ct_id}: {matched}/{total_block_len} chars matched ({pct:.0f}%)")

# ── 3. Pipe separator blocks in 清华大学 raw ──
print(f"\n{'='*70}")
print(f"  3. PIPE SEPARATOR BLOCKS (清华大学 raw)")
print(f"{'='*70}")
pipe_found = 0
for pi, page in enumerate(ts_raw["pages"]):
    for b in page.get("blocks", []):
        text = b.get("text", "")
        if "|" in text and len(text) > 10:
            pipe_found += 1
            if pipe_found <= 10:
                print(f"  Page {pi}, block {b['id']}: '{text[:150]}...'")
print(f"  Total blocks with |: {pipe_found}")

# Also check for table-like patterns: 序号, 品目, etc
table_keywords = ["序号", "品目", "采购标的", "数量", "技术规格", "分项预算", "报价明细"]
for kw in table_keywords:
    count = 0
    for pi, page in enumerate(ts_raw["pages"]):
        for b in page.get("blocks", []):
            if kw in b.get("text", ""):
                count += 1
    print(f"  Blocks containing '{kw}': {count}")

# ── 4. Deep table analysis for 智慧教室 ──
print(f"\n{'='*70}")
print(f"  4. TABLE CELL-TEXT COVERAGE DETAIL (智慧教室)")
print(f"{'='*70}")
table_pages = []
for pi, page in enumerate(zh_raw["pages"]):
    if page.get("tables", []):
        table_pages.append(pi)

full_loss_pages = []
partial_pages = []
full_retain_pages = []

for tp in table_pages:
    page = zh_raw["pages"][tp]
    page_tables = page.get("tables", [])

    # Collect all unique cell texts from tables
    table_cells = set()
    for t in page_tables:
        for row in t.get("rows", []):
            if isinstance(row, list):
                for cell in row:
                    if isinstance(cell, str) and len(cell.strip()) >= 2:
                        table_cells.add(cell.strip())

    # Collect all block text
    block_text_all = "\n".join(b.get("text","") for b in page.get("blocks", []))

    # Get chunk texts covering this page
    covering = [c for c in zhihui["chunks"] if c["page_start"] <= tp <= c["page_end"]]
    chunk_text_all = "\n".join(c["text"] for c in covering)

    # Check each cell
    found_in_block = 0
    found_in_chunk = 0
    for cell in table_cells:
        if cell in block_text_all:
            found_in_block += 1
        if cell in chunk_text_all:
            found_in_chunk += 1

    total = len(table_cells)
    if total == 0:
        continue

    block_pct = found_in_block / total * 100
    chunk_pct = found_in_chunk / total * 100

    if chunk_pct == 0:
        full_loss_pages.append(tp)
    elif chunk_pct == 100:
        full_retain_pages.append(tp)
    else:
        partial_pages.append(tp)

    if chunk_pct < 30:  # Show detail for significant losses
        print(f"  Page {tp}: {total} unique cells → block_text={found_in_block}({block_pct:.0f}%), chunk_text={found_in_chunk}({chunk_pct:.0f}%)")
        # Show sample cells
        lost_cells = [c for c in table_cells if c not in chunk_text_all]
        if lost_cells:
            print(f"    Sample lost cells: {lost_cells[:5]}")

print(f"\n  Full retain: {len(full_retain_pages)} ({len(full_retain_pages)/len(table_pages)*100:.1f}%)")
print(f"  Partial: {len(partial_pages)} ({len(partial_pages)/len(table_pages)*100:.1f}%)")
print(f"  Full loss: {len(full_loss_pages)} ({len(full_loss_pages)/len(table_pages)*100:.1f}%)")
print(f"  Full loss pages: {full_loss_pages}")

# ── 5. Verify the report's table structure loss claim ──
print(f"\n{'='*70}")
print(f"  5. TABLE STRUCTURE LOSS (智慧教室)")
print(f"{'='*70}")
# Check if table cells are merging together in block text
# Look at pages where tables exist but cells are merged
for tp in [8, 9, 35, 57, 58, 59, 60]:  # pages with many cells lost
    if tp >= len(zh_raw["pages"]):
        continue
    page = zh_raw["pages"][tp]
    tables = page.get("tables", [])
    blocks = page.get("blocks", [])
    if tables:
        # Get first table's first row (header)
        first_table = tables[0]
        rows = first_table.get("rows", [])
        if rows:
            header_cells = [c if isinstance(c, str) else str(c) for c in rows[0]]
            header_str = " | ".join(header_cells)
            print(f"  Page {tp} Table header: {header_str[:120]}")

        # Find the corresponding block text
        for b in blocks:
            bt = b.get("text", "")
            # Check if block text appears to be merged table cells
            flat_cells = []
            for row in rows:
                if isinstance(row, list):
                    flat_cells.extend([c if isinstance(c, str) else str(c) for c in row])
            flat_text = "".join(flat_cells)
            # Check if this block contains merged cell text
            if any(len(c) >= 3 and c in bt for c in flat_cells):
                print(f"    Block text (table cells merged): '{bt[:150]}...'")
                break
        else:
            print(f"  Page {tp}: no blocks found with table cell content")

# ── 6. Verify adjacency of ch_051/ch_114 neighbors ──
print(f"\n{'='*70}")
print(f"  6. NEIGHBOR CHUNK CONTENT (清华大学 ch_050-ch_052, ch_113-ch_115)")
print(f"{'='*70}")
for c in ts_chunks:
    if c["chunk_id"] in ["ch_049", "ch_050", "ch_051", "ch_052", "ch_053",
                           "ch_112", "ch_113", "ch_114", "ch_115"]:
        print(f"  {c['chunk_id']}: size={len(c['text'])}, path={c['section_path']}, page={c['page_start']}-{c['page_end']}")
        print(f"    text: {c['text'][:200]}...")

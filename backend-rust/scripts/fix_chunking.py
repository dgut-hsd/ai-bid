import re

with open('src/services/chunking_service.rs', 'r', encoding='utf-8') as f:
    lines = f.readlines()

# Step 1: Apply page_start → body_page_start in non-test code only
test_start_line = None
for i, line in enumerate(lines):
    if '#[cfg(test)]' in line:
        test_start_line = i
        break

result = []
for i, line in enumerate(lines):
    if test_start_line is None or i < test_start_line:
        # Non-test code: apply replacements
        line = line.replace('section.page_start', 'section.body_page_start')
        line = line.replace('section.page_end', 'section.body_page_end')
        line = line.replace('leaf.page_start', 'leaf.body_page_start')
        line = line.replace('leaf.page_end', 'leaf.body_page_end')
        line = line.replace('|s| s.page_start', '|s| s.body_page_start')
        line = line.replace('|s| s.page_end', '|s| s.body_page_end')
    result.append(line)

# Step 2: Find Section { ... } blocks and add body_page fields if missing
# We need to track whether we're inside a Section block vs Chunk block
output = []
inside_struct = None  # 'Section' or 'Chunk' or None
struct_brace_depth = 0
ps_val = '0'
pe_val = '0'
has_body_page = False

for i, line in enumerate(result):
    # Detect struct start
    m = re.match(r'(\s*)(Section|Chunk)\s*\{', line)
    if m:
        inside_struct = m.group(2)
        struct_brace_depth = 0
        ps_val = '0'
        pe_val = '0'
        has_body_page = False

    if inside_struct:
        struct_brace_depth += line.count('{') - line.count('}')

        # Capture values
        pm = re.search(r'page_start:\s*(\d+)', line)
        if pm:
            ps_val = pm.group(1)
        pm = re.search(r'page_end:\s*(\d+)', line)
        if pm:
            pe_val = pm.group(1)

        if 'body_page_start' in line:
            has_body_page = True

        # When we find page_end and it's a Section struct missing body_page
        if inside_struct == 'Section' and re.search(r'page_end:\s*\d+', line) and not has_body_page:
            # Check forward: are body_page fields already coming?
            fwd = ''.join(result[i+1:min(len(result), i+6)])
            if 'body_page_start' not in fwd:
                indent = ' ' * (len(line) - len(line.lstrip()))
                output.append(line)
                output.append(f'{indent}body_page_start: {ps_val},\n')
                output.append(f'{indent}body_page_end: {pe_val},\n')
                has_body_page = True  # prevent adding again for same struct
                if struct_brace_depth <= 0:
                    inside_struct = None
                continue

        if struct_brace_depth <= 0:
            inside_struct = None

    output.append(line)

with open('src/services/chunking_service.rs', 'w', encoding='utf-8') as f:
    f.writelines(output)

print('Done')

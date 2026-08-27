import { describe, it, expect } from 'vitest';
import { mapBBoxEntries } from './bboxMapping';

describe('mapBBoxEntries', () => {
  it('maps camelCase pageWidth from Java response', () => {
    const result = mapBBoxEntries([
      {
        blockId: 'b_0_0',
        page: 0,
        pageWidth: 612,
        bbox: { x0: 1, top: 2, x1: 3, bottom: 4 },
      },
    ]);
    expect(result).toEqual([
      { x0: 1, top: 2, x1: 3, bottom: 4, pageWidth: 612, page: 1 },
    ]);
  });

  it('falls back to snake_case page_width when camelCase absent', () => {
    const result = mapBBoxEntries([
      { page: 2, page_width: 700, bbox: { x0: 0, top: 0, x1: 10, bottom: 10 } },
    ]);
    expect(result[0].pageWidth).toBe(700);
    expect(result[0].page).toBe(3);
  });

  it('defaults pageWidth to 595 when neither field present', () => {
    const result = mapBBoxEntries([
      { page: 0, bbox: { x0: 0, top: 0, x1: 0, bottom: 0 } },
    ]);
    expect(result[0].pageWidth).toBe(595);
  });

  it('returns empty array for non-array / empty input', () => {
    expect(mapBBoxEntries(null)).toEqual([]);
    expect(mapBBoxEntries(undefined)).toEqual([]);
    expect(mapBBoxEntries({})).toEqual([]);
    expect(mapBBoxEntries('x')).toEqual([]);
  });
});
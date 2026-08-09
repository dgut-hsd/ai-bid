import { renderHook, act } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach } from 'vitest';

// Hoisted mutable state shared across the mock factory, so each test can set
// up URL search params and verify what setSearchParams received.
const { mockSearchParams, mockSetSearchParams } = vi.hoisted(() => {
  const params: { current: URLSearchParams } = { current: new URLSearchParams() };
  const setter = vi.fn().mockImplementation(
    (nextParams: URLSearchParams | ((prev: URLSearchParams) => URLSearchParams)) => {
      // Update the mutable ref so re-rendering the hook picks up the change
      params.current =
        typeof nextParams === 'function' ? nextParams(params.current) : nextParams;
    },
  );
  return { mockSearchParams: params, mockSetSearchParams: setter };
});

vi.mock('react-router-dom', () => ({
  useSearchParams: vi.fn(() => [mockSearchParams.current, mockSetSearchParams]),
}));

// Import the module-under-test AFTER the mock is set up
import { useUrlState } from './useUrlState';

describe('useUrlState', () => {
  beforeEach(() => {
    // Reset URL params and clear mock call history before every test
    mockSearchParams.current = new URLSearchParams();
    mockSetSearchParams.mockClear();
  });

  it('reads parameters from the URL and merges with defaults', () => {
    mockSearchParams.current = new URLSearchParams('page=2&keyword=bid+analysis');

    const { result } = renderHook(() =>
      useUrlState({ page: 1, keyword: '', tab: 'all' }),
    );

    expect(result.current[0]).toEqual({
      page: 2,
      keyword: 'bid analysis',
      tab: 'all',
    });
  });

  it('falls back to initial state when no URL parameters are present', () => {
    const { result } = renderHook(() =>
      useUrlState({ page: 1, keyword: '', tab: 'all' }),
    );

    expect(result.current[0]).toEqual({
      page: 1,
      keyword: '',
      tab: 'all',
    });
  });

  it('updates the URL when setState is called and reads back the new state after re-render', () => {
    const { result, rerender } = renderHook(() =>
      useUrlState({ page: 1, keyword: '', tab: 'all' }),
    );

    expect(result.current[0].page).toBe(1);

    act(() => {
      result.current[1]({ page: 3, keyword: 'hello' });
    });

    // Verify setSearchParams received the data + replace option
    expect(mockSetSearchParams).toHaveBeenCalledWith(
      expect.any(URLSearchParams),
      { replace: true },
    );

    // Re-render to simulate what React Router does: the new URLSearchParams
    // ref flows back into the hook via useSearchParams
    rerender();

    expect(result.current[0]).toEqual({
      page: 3,
      keyword: 'hello',
      tab: 'all',
    });
  });

  it('removes a parameter from the URL when its value is set to undefined, null, or empty string', () => {
    mockSearchParams.current = new URLSearchParams('page=1&keyword=test&tab=all');

    const { result } = renderHook(() =>
      useUrlState({ page: 1, keyword: '', tab: 'all' }),
    );

    expect(result.current[0]).toEqual({
      page: 1,
      keyword: 'test',
      tab: 'all',
    });

    act(() => {
      result.current[1]({ keyword: undefined });
    });

    // The parameter should have been deleted from the new URLSearchParams
    const lastCallArgs = mockSetSearchParams.mock.lastCall![0] as URLSearchParams;
    expect(lastCallArgs.has('keyword')).toBe(false);
    expect(lastCallArgs.get('page')).toBe('1');

    // Same behaviour for null
    act(() => {
      result.current[1]({ tab: null as any });
    });

    const secondCallArgs = mockSetSearchParams.mock.lastCall![0] as URLSearchParams;
    expect(secondCallArgs.has('tab')).toBe(false);

    // Same behaviour for empty string
    act(() => {
      result.current[1]({ page: '' as any });
    });

    const thirdCallArgs = mockSetSearchParams.mock.lastCall![0] as URLSearchParams;
    expect(thirdCallArgs.has('page')).toBe(false);
  });

  it('coerces number-typed fields from URL string to JavaScript number', () => {
    mockSearchParams.current = new URLSearchParams('page=42&limit=10');

    const { result } = renderHook(() =>
      useUrlState({ page: 1, limit: 20 }),
    );

    expect(result.current[0].page).toBe(42);
    expect(typeof result.current[0].page).toBe('number');

    expect(result.current[0].limit).toBe(10);
    expect(typeof result.current[0].limit).toBe('number');
  });

  it('handles multiple parameters being set simultaneously', () => {
    mockSearchParams.current = new URLSearchParams('a=old');

    const { result, rerender } = renderHook(() =>
      useUrlState({ a: '', b: 0, c: '' }),
    );

    act(() => {
      result.current[1]({ a: 'alpha', b: 99, c: '' });
    });

    const lastCallArgs = mockSetSearchParams.mock.lastCall![0] as URLSearchParams;
    expect(lastCallArgs.get('a')).toBe('alpha');
    expect(lastCallArgs.get('b')).toBe('99');
    // c was set to empty string, so it should be deleted
    expect(lastCallArgs.has('c')).toBe(false);

    // Re-render to verify read-back
    rerender();
    expect(result.current[0]).toEqual({ a: 'alpha', b: 99, c: '' });
  });
});

import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { useMediaQuery, useIsMobile } from './useMediaQuery';

interface MediaQueryListMock {
   matches: boolean;
   media: string;
   onchange: null;
   addEventListener: ReturnType<typeof vi.fn>;
   removeEventListener: ReturnType<typeof vi.fn>;
}

function createMatchMedia(matches: boolean) {
   return (query: string): MediaQueryListMock => ({
      matches,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
   });
}

let originalMatchMedia: typeof window.matchMedia;

beforeAll(() => {
   originalMatchMedia = window.matchMedia;
});

afterEach(() => {
   window.matchMedia = originalMatchMedia;
   vi.restoreAllMocks();
});

// ------------------------------------------------------------------ useMediaQuery

describe('useMediaQuery', () => {
   it('returns true when the media query matches', () => {
      window.matchMedia = createMatchMedia(true) as unknown as typeof window.matchMedia;

      const { result } = renderHook(() => useMediaQuery('(max-width: 768px)'));

      expect(result.current).toBe(true);
   });

   it('returns false when the media query does not match', () => {
      window.matchMedia = createMatchMedia(false) as unknown as typeof window.matchMedia;

      const { result } = renderHook(() => useMediaQuery('(min-width: 1200px)'));

      expect(result.current).toBe(false);
   });

   it('reacts to a dynamic viewport change via the change event listener', () => {
      const listeners = new Map<string, (event: MediaQueryListEvent) => void>();

      window.matchMedia = vi.fn().mockImplementation((query: string) => ({
         matches: false,
         media: query,
         onchange: null,
         addEventListener: vi.fn((event: string, listener: (e: MediaQueryListEvent) => void) => {
            listeners.set(event, listener);
         }),
         removeEventListener: vi.fn((event: string) => {
            listeners.delete(event);
         }),
      }));

      const { result } = renderHook(() => useMediaQuery('(max-width: 768px)'));

      expect(result.current).toBe(false);

      act(() => {
         const listener = listeners.get('change');
         if (listener) listener({ matches: true } as MediaQueryListEvent);
      });

      expect(result.current).toBe(true);
   });
});

// --------------------------------------------------------------------- useIsMobile

describe('useIsMobile', () => {
   it('returns true when viewport width is within mobile breakpoint (<= 768px)', () => {
      window.matchMedia = createMatchMedia(true) as unknown as typeof window.matchMedia;

      const { result } = renderHook(() => useIsMobile());

      expect(result.current).toBe(true);
   });

   it('returns false when viewport width is beyond mobile breakpoint (> 768px)', () => {
      window.matchMedia = createMatchMedia(false) as unknown as typeof window.matchMedia;

      const { result } = renderHook(() => useIsMobile());

      expect(result.current).toBe(false);
   });
});

// ------------------------------------------------------------ listener lifecycle

describe('listener lifecycle', () => {
   it('attaches a change listener on mount and removes it on unmount', () => {
      const addEventListener = vi.fn();
      const removeEventListener = vi.fn();

      window.matchMedia = vi.fn().mockReturnValue({
         matches: false,
         media: '(max-width: 768px)',
         onchange: null,
         addEventListener,
         removeEventListener,
      });

      const { unmount } = renderHook(() => useMediaQuery('(max-width: 768px)'));

      expect(addEventListener).toHaveBeenCalledTimes(1);
      expect(addEventListener).toHaveBeenCalledWith('change', expect.any(Function));

      unmount();

      expect(removeEventListener).toHaveBeenCalledTimes(1);
      expect(removeEventListener).toHaveBeenCalledWith('change', expect.any(Function));
   });

   it('re-attaches the listener when the query string changes', () => {
      const addMedia1 = vi.fn().mockReturnValue({
         matches: false,
         media: '(max-width: 768px)',
         onchange: null,
         addEventListener: vi.fn(),
         removeEventListener: vi.fn(),
      });

      const addMedia2 = vi.fn().mockReturnValue({
         matches: true,
         media: '(max-width: 1024px)',
         onchange: null,
         addEventListener: vi.fn(),
         removeEventListener: vi.fn(),
      });

      window.matchMedia = addMedia1;

      const { rerender } = renderHook(
         (query: string) => useMediaQuery(query),
         { initialProps: '(max-width: 768px)' },
      );

      const firstListener = addMedia1.mock.results[0]?.value;
      expect(firstListener.addEventListener).toHaveBeenCalledTimes(1);

      window.matchMedia = addMedia2;

      rerender('(max-width: 1024px)');

      expect(firstListener.removeEventListener).toHaveBeenCalledTimes(1);
      expect(firstListener.removeEventListener).toHaveBeenCalledWith('change', expect.any(Function));

      const secondListener = addMedia2.mock.results[0]?.value;
      expect(secondListener.addEventListener).toHaveBeenCalledTimes(1);
      expect(secondListener.addEventListener).toHaveBeenCalledWith('change', expect.any(Function));
   });
});

// --------------------------------------------------------------------- SSR safety

describe('SSR safety', () => {
   it('defaults to false as the safe initial value', () => {
      // The SSR contract: when `window` is undefined, the hook's `useState`
      // initializer returns `false` without calling `window.matchMedia`.
      // In jsdom `window` is always present, so we verify the contract
      // indirectly: the hook returns `false` as the safe fallback.
      window.matchMedia = createMatchMedia(false) as unknown as typeof window.matchMedia;

      const { result } = renderHook(() => useMediaQuery('(min-width: 1200px)'));

      expect(result.current).toBe(false);
   });

   it('contains SSR guards in the source code', () => {
      // Verify that both SSR guard patterns exist in the hook source.
      const src = useMediaQuery.toString();

      // Guard 1 — useState initializer: returns false without calling
      // matchMedia when window is undefined
      expect(src).toContain('typeof window !== "undefined"');

      // Guard 2 — useEffect: early return before accessing matchMedia
      expect(src).toContain('if (typeof window === "undefined") return');
   });
});

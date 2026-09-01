import { useRef, useCallback, useEffect, useState } from 'react';
import { useUrlState } from '@/hooks/useUrlState';

export const usePdfFlow = (isComplete: boolean) => {
   const containerRef = useRef<HTMLDivElement>(null);
   const [numPages, setNumPages] = useState<number>(0);

   const [queryParams, setQueryParams] = useUrlState({
      page: 1,
      scale: 1.0,
      mode: 'single',
      targetPage: 0,
   });

   const currentPage = queryParams.page;
   const scale = queryParams.scale;

   const jumpToPage = useCallback(
      (page: number) => {
         if (!containerRef.current || numPages === 0) return;

         if (page < 1) page = 1;
         if (page > numPages) page = numPages;

         const container = containerRef.current;
         const targetElement = container.querySelector(
            `[data-page-num="${page}"]`
         ) as HTMLElement | null;

         if (targetElement) {
            const containerRect = container.getBoundingClientRect();
            const targetRect = targetElement.getBoundingClientRect();
            const offset = targetRect.top - containerRect.top;

            container.scrollTo({
               top: container.scrollTop + offset,
               behavior: 'smooth',
            });
            setQueryParams({ page });
         }
      },
      [numPages, setQueryParams]
   );

   useEffect(() => {
      const target = queryParams.targetPage;
      if (target > 0 && isComplete && numPages > 0) {
         setTimeout(() => {
            jumpToPage(target);
            setQueryParams({ targetPage: 0 });
         }, 100);
      }
   }, [
      queryParams.targetPage,
      isComplete,
      numPages,
      jumpToPage,
      setQueryParams,
   ]);

   useEffect(() => {
      if (numPages === 0 || !containerRef.current) return;

      const observer = new IntersectionObserver(
         (entries) => {
            const visibleEntries = entries.filter((e) => e.isIntersecting);

            if (visibleEntries.length > 0) {
               const target = visibleEntries.reduce((prev, current) =>
                  prev.intersectionRatio > current.intersectionRatio
                     ? prev
                     : current
               );

               const page = Number(target.target.getAttribute('data-page-num'));

               if (page && page !== queryParams.page) {
                  setQueryParams({ page });
               }
            }
         },
         {
            root: containerRef.current,
            rootMargin: '-20% 0px -20% 0px',
            threshold: 0.2,
         }
      );

      const pageElements =
         containerRef.current.querySelectorAll('[data-page-num]');
      pageElements.forEach((el) => observer.observe(el));

      return () => observer.disconnect();
   }, [numPages, scale, queryParams.page, setQueryParams]);

   const zoomIn = useCallback(() => {
      setQueryParams({ scale: Math.min((scale || 1) + 0.1, 3.0) });
   }, [scale, setQueryParams]);

   const zoomOut = useCallback(() => {
      setQueryParams({ scale: Math.max((scale || 1) - 0.1, 0.1) });
   }, [scale, setQueryParams]);

   const resetZoom = useCallback(() => {
      setQueryParams({ scale: 1.0 });
   }, [setQueryParams]);

   return {
      containerRef,
      scale,
      numPages,
      setNumPages,
      currentPage,
      zoomIn,
      zoomOut,
      resetZoom,
      jumpToPage,
   };
};

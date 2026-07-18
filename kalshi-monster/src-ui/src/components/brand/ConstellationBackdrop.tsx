import { useEffect, useRef } from 'react';

type Star = {
  x: number;
  y: number;
  r: number;
  phase: number;
  speed: number;
  drift: number;
};

/**
 * Ambient constellation field behind the app shell — slow-drifting
 * gold embers, a few linked by hairline constellation arcs, each
 * twinkling on its own phase. Rendered on a canvas, DPR-aware, and
 * frozen into a static field when the user prefers reduced motion.
 */
export function ConstellationBackdrop() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    let raf = 0;
    let stars: Star[] = [];
    let links: Array<[number, number]> = [];
    let w = 0;
    let h = 0;

    const seed = () => {
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      w = window.innerWidth;
      h = window.innerHeight;
      canvas.width = Math.floor(w * dpr);
      canvas.height = Math.floor(h * dpr);
      canvas.style.width = `${w}px`;
      canvas.style.height = `${h}px`;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

      const count = Math.min(110, Math.floor((w * h) / 16000));
      stars = Array.from({ length: count }, () => ({
        x: Math.random() * w,
        y: Math.random() * h,
        r: 0.5 + Math.random() * 1.3,
        phase: Math.random() * Math.PI * 2,
        speed: 0.25 + Math.random() * 0.75,
        drift: 0.02 + Math.random() * 0.05,
      }));

      // Link a sparse subset into constellation segments.
      links = [];
      for (let i = 0; i < stars.length; i += 1) {
        for (let j = i + 1; j < stars.length; j += 1) {
          const a = stars[i];
          const b = stars[j];
          const dx = a.x - b.x;
          const dy = a.y - b.y;
          const dist = Math.hypot(dx, dy);
          if (dist < 130 && Math.random() < 0.16) {
            links.push([i, j]);
          }
        }
      }
    };

    const paint = (t: number) => {
      ctx.clearRect(0, 0, w, h);

      for (const [ia, ib] of links) {
        const a = stars[ia];
        const b = stars[ib];
        ctx.beginPath();
        ctx.moveTo(a.x, a.y);
        ctx.lineTo(b.x, b.y);
        ctx.strokeStyle = 'rgba(212, 175, 55, 0.05)';
        ctx.lineWidth = 0.6;
        ctx.stroke();
      }

      for (const s of stars) {
        const tw = reduced ? 0.6 : 0.35 + 0.65 * (0.5 + 0.5 * Math.sin(s.phase + t * 0.001 * s.speed));
        ctx.beginPath();
        ctx.arc(s.x, s.y, s.r, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(232, 205, 130, ${0.32 * tw})`;
        ctx.fill();
        if (!reduced) {
          s.x += s.drift;
          if (s.x > w + 4) s.x = -4;
        }
      }

      if (!reduced) raf = requestAnimationFrame(paint);
    };

    seed();
    paint(0);
    if (!reduced) raf = requestAnimationFrame(paint);

    const onResize = () => {
      seed();
      if (reduced) paint(0);
    };
    window.addEventListener('resize', onResize);
    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener('resize', onResize);
    };
  }, []);

  return <canvas ref={canvasRef} className="constellationBackdrop" aria-hidden="true" />;
}

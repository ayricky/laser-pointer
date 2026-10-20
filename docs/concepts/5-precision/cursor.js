/* Precision reticle. Same shared-engine contract, finer ticks and less visual weight.
   All demo stages use this renderer. Device-pixel snapping preserves 1px edges. */
(function () {
  'use strict';
  window.PrecisionCursor = {
    size: 24,
    draw(c, x, y, s) {
      const dpr = s.dpr || 1;
      const snap = v => (Math.round(v * dpr) + .5) / dpr;
      c.globalAlpha *= Math.min(1, s.ignite);
      c.translate(snap(x), snap(y));
      const gap = Math.round(5 - s.press), end = gap + 2;
      c.beginPath();
      c.moveTo(-end, 0); c.lineTo(-gap, 0); c.moveTo(gap, 0); c.lineTo(end, 0);
      c.moveTo(0, -end); c.lineTo(0, -gap); c.moveTo(0, gap); c.lineTo(0, end);
      c.lineCap = 'butt';
      c.strokeStyle = '#ffffffd9'; c.lineWidth = 3; c.stroke();
      c.strokeStyle = '#121318c0'; c.lineWidth = 1; c.stroke();
      c.beginPath(); c.arc(0, 0, 3.2, 0, Math.PI * 2);
      c.fillStyle = '#11131899'; c.fill();
      c.beginPath(); c.arc(0, 0, 2.5, 0, Math.PI * 2);
      c.fillStyle = s.color; c.fill();
      c.beginPath(); c.arc(-.25, -.25, .8, 0, Math.PI * 2);
      c.fillStyle = '#fff'; c.fill();
    }
  };
})();

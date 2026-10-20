/* Ember: the shared engine's dot, with a tighter white core and warm bloom.
   Drawing uses the shared engine's single demand-driven animation loop. */
(function () {
  'use strict';
  window.EmberCursor = {
    size: 48,
    draw(c, x, y, s) {
      c.translate(x, y); c.scale(s.ignite, s.ignite);
      const radius = 16 - s.press * 4;
      const bloom = c.createRadialGradient(0, 0, 0, 0, 0, radius);
      bloom.addColorStop(0, s.color + 'b8');
      bloom.addColorStop(.25, s.color + '60');
      bloom.addColorStop(1, s.color + '00');
      c.fillStyle = bloom; c.beginPath(); c.arc(0, 0, radius, 0, Math.PI * 2); c.fill();
      c.fillStyle = '#16161999'; c.beginPath(); c.arc(0, 0, 4.6, 0, Math.PI * 2); c.fill();
      c.fillStyle = s.color; c.beginPath(); c.arc(0, 0, 4, 0, Math.PI * 2); c.fill();
      c.fillStyle = '#fff7ed'; c.beginPath(); c.arc(-.25, -.35, 1.7 + s.press * .4, 0, Math.PI * 2); c.fill();
    }
  };
})();

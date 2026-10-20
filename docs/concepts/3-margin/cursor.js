/* A paper-friendly ring. Same centre / hotspot at rest and on press.
   Uses the shared engine's demand-driven animation; no additional rAF. */
(function () {
  window.MarginCursor = { size: 40, draw: function (c, x, y, s) {
    c.translate(x, y); c.scale(Math.max(0, s.ignite), Math.max(0, s.ignite));
    var r = 8.5 - 3 * s.press;
    c.beginPath(); c.arc(0, 0, r, 0, Math.PI * 2);
    c.strokeStyle = '#faf8f4ee'; c.lineWidth = 4; c.stroke();
    c.strokeStyle = '#252522'; c.lineWidth = 1.4; c.stroke();
    c.beginPath(); c.arc(0, 0, 2.1 + s.press * .4, 0, Math.PI * 2);
    c.fillStyle = s.color; c.fill();
  } };
})();

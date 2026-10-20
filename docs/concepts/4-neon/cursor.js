/* Neon reticle. Uses only the shared engine's on-demand animation clock. */
(function () {
  window.NeonCursor = { size: 56, draw: function (c, x, y, s) {
    c.translate(x, y);
    const ignition = Math.min(1.18, Math.max(0, s.ignite));
    c.scale(ignition, ignition);
    c.rotate(s.press * Math.PI / 4);
    const r = 8.5 - s.press * 3, outer = r + 5;
    c.lineCap = 'round';
    c.shadowColor = s.color; c.shadowBlur = 9;
    c.strokeStyle = s.color; c.lineWidth = 2;
    c.beginPath(); c.arc(0, 0, r, 0, Math.PI * 2); c.stroke();
    c.shadowBlur = 0;
    c.strokeStyle = '#ffffffdc'; c.lineWidth = .65;
    c.beginPath(); c.arc(0, 0, r - 1, 0, Math.PI * 2); c.stroke();
    c.strokeStyle = s.color; c.lineWidth = 1.5;
    c.beginPath();
    for (let i = 0; i < 4; i++) {
      const a = i * Math.PI / 2;
      c.moveTo(Math.cos(a) * outer, Math.sin(a) * outer);
      c.lineTo(Math.cos(a) * (outer + 3), Math.sin(a) * (outer + 3));
    }
    c.stroke();
    c.fillStyle = '#fff'; c.beginPath(); c.arc(0, 0, 1.2 + s.press * .4, 0, Math.PI * 2); c.fill();
    if (s.ignite > 0 && s.ignite < 1) {
      c.globalAlpha = Math.sin(Math.PI * s.ignite) * .7;
      c.strokeStyle = '#fff'; c.lineWidth = 1;
      c.beginPath(); c.arc(0, 0, 8 + s.ignite * 15, 0, Math.PI * 2); c.stroke();
    }
  }};
})();

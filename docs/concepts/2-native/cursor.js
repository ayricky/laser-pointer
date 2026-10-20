/* Fluent dot. Public renderer for the shared Laser engine; CSS-pixel dimensions. */
window.NativeCursor = { size: 40, draw: function(c,x,y,s) {
  c.translate(x,y); c.scale(Math.max(0,s.ignite),Math.max(0,s.ignite));
  var radius=4.3-s.press*.8;
  var halo=c.createRadialGradient(0,0,2,0,0,14-s.press*3);
  halo.addColorStop(0,s.color+'70'); halo.addColorStop(1,s.color+'00');
  c.fillStyle=halo; c.beginPath(); c.arc(0,0,14,0,Math.PI*2); c.fill();
  c.shadowColor='#0b152c99'; c.shadowBlur=4; c.shadowOffsetY=1;
  c.beginPath(); c.arc(0,0,radius,0,Math.PI*2); c.fillStyle=s.color; c.fill();
  c.lineWidth=1.5; c.strokeStyle='#fff'; c.stroke(); c.shadowColor='transparent';
  c.fillStyle='#ffffffa6'; c.beginPath(); c.arc(-1,-1,1.05,0,Math.PI*2); c.fill();
} };

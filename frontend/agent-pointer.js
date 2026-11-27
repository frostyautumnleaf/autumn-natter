// Draw the browser agent's pointer without taking over the system pointer.
function autumnAgentPlacePointer(x, y) {
  if (!Number.isFinite(x) || !Number.isFinite(y) || !document.documentElement) { return; }
  var marker = document.getElementById('autumn-agent-pointer');
  if (!marker) {
    marker = document.createElement('div');
    marker.id = 'autumn-agent-pointer';
    marker.setAttribute('aria-hidden', 'true');
    marker.style.position = 'fixed';
    marker.style.zIndex = '2147483647';
    marker.style.setProperty('pointer-events', 'none', 'important');
    var art = marker.attachShadow({ mode: 'open' });
    art.innerHTML = '<span>Agent<i></i></span>' +
      '<svg width="17" height="22" viewBox="0 0 17 22" aria-hidden="true">' +
      '<path fill="#ff982e" stroke="#241303" stroke-width="1.4" d="M1 1v16l4-4 3 7 3-1-3-7h6z"/></svg>';
    Object.assign(art.querySelector('span').style, {
      position: 'absolute', padding: '3px 7px', borderRadius: '7px', background: '#ff982e',
      color: '#241303', font: '700 11px/1.2 system-ui,sans-serif'
    });
    Object.assign(art.querySelector('i').style, {
      position: 'absolute', width: '0', height: '0', border: '4px solid transparent'
    });
    Object.assign(art.querySelector('svg').style, { display: 'block', overflow: 'visible', filter: 'drop-shadow(0 1px 2px #0008)' });
    document.documentElement.appendChild(marker);
  }
  marker.style.left = Math.max(0, Math.min(x, innerWidth - 2)) + 'px';
  marker.style.top = Math.max(0, Math.min(y, innerHeight - 2)) + 'px';
  var below = y < 25;
  var right = x > innerWidth - 55;
  var label = marker.shadowRoot.querySelector('span').style;
  var tail = marker.shadowRoot.querySelector('i').style;
  label.top = below ? '100%' : 'auto';
  label.bottom = below ? 'auto' : 'calc(100% + 4px)';
  label.left = right ? 'auto' : '0';
  label.right = right ? '0' : 'auto';
  tail.top = below ? 'auto' : '100%';
  tail.bottom = below ? '100%' : 'auto';
  tail.left = right ? 'auto' : '7px';
  tail.right = right ? '7px' : 'auto';
  tail.borderTop = below ? '0' : '4px solid #ff982e';
  tail.borderBottom = below ? '4px solid #ff982e' : '0';
}

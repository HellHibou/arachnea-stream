(function(){{
  const payload = {{
    html: {html_expression},
    userAgent: navigator.userAgent || ''
  }};
  const eventName = {event_name};
  if (window.__TAURI__ && window.__TAURI__.event && window.__TAURI__.event.emit) {{
    window.__TAURI__.event.emit(eventName, payload);
  }} else if (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke) {{
    window.__TAURI_INTERNALS__.invoke('plugin:event|emit', {{ event: eventName, payload }});
  }}
}})();
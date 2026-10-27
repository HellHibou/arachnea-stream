
(() => {
    const record = (url) => {
        try {
            sessionStorage.setItem('chaser-cf-blocked-popup', String(url || '').slice(0, 512));
        } catch (_) {}
    };
    const original = window.open;
    if (typeof original === 'function') {
        window.open = function (url, target, features) {
            record(url);
            return null;
        };
    }
    document.addEventListener('click', (event) => {
        const anchor = event.target && event.target.closest ? event.target.closest('a') : null;
        if (anchor && anchor.target === '_blank') {
            record(anchor.href);
            event.preventDefault();
            event.stopPropagation();
        }
    }, true);
})();

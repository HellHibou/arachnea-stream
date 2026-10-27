(function() {
            if (window.turnstile && typeof window.turnstile.reset === 'function') {
                try { window.turnstile.reset(); } catch (e) {}
            }
            var el = document.querySelector('[name="cf-response"]');
            if (el) { el.value = ''; }
            return true;
        })()
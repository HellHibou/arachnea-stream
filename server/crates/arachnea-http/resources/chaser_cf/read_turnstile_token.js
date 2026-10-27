(function() {
            if (window.turnstile && typeof window.turnstile.getResponse === 'function') {
                var t = window.turnstile.getResponse();
                if (t && t.length > 10) return t;
            }
            var el = document.querySelector('[name="cf-response"]');
            if (el && el.value && el.value.length > 10) return el.value;
            return null;
        })()
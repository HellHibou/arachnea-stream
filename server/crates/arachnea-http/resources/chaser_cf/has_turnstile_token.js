(function() {
        if (window.turnstile && typeof window.turnstile.getResponse === 'function') {
            var t = window.turnstile.getResponse();
            if (t && t.length > 10) return true;
        }
        var el = document.querySelector('[name="cf-response"], [name="cf-turnstile-response"]');
        if (el && el.value && el.value.length > 10) return true;
        return false;
    })()
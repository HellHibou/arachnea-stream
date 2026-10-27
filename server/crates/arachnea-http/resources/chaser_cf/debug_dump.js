(function() {
        var frames = [];
        document.querySelectorAll('iframe').forEach(function(f) {
            frames.push({ src: f.src || f.getAttribute('src') || '(none)', cls: f.className || '' });
        });
        return JSON.stringify({
            iframes: frames,
            turnstile: !!document.querySelector('#turnstile-wrapper, .cf-turnstile'),
            cfInputs: !!document.querySelector('input[name="cf-turnstile-response"], input[name="cf-response"]')
        });
    })()
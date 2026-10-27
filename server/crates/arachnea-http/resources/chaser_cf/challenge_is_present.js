(function() {
        if (document.querySelector('iframe[src*="challenges.cloudflare.com"]')) return true;
        if (document.querySelector('input[name="cf-turnstile-response"], input[name="cf-response"]')) return true;
        if (document.querySelector('#turnstile-wrapper, .cf-turnstile')) return true;
        return false;
    })()
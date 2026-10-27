(function() {
        var candidates = [
            '.ctp-checkbox-label',
            '.ctp-checkbox',
            '#turnstile-wrapper label',
            'input[type="checkbox"]'
        ];
        for (var i = 0; i < candidates.length; i++) {
            var el = document.querySelector(candidates[i]);
            if (el) {
                var r = el.getBoundingClientRect();
                return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
            }
        }
        return null;
    })()
//! Cloudflare challenge inspection, clearance polling and pointer interaction.

use super::*;

/// Applies HTTP proxy credentials to a page when the proxy requires auth.
///
/// # Arguments
///
/// - `page`: Browser page that will send proxy requests.
/// - `proxy`: Optional proxy configuration; credentials are applied only when
///   both username and password are present.
///
/// # Errors
///
/// Returns `ChaserCfFailure` when browser authentication setup fails.
pub(super) async fn setup_proxy_auth(
    page: &Page,
    proxy: Option<&ProxyConfig>,
) -> Result<(), ArachneaHttpError> {
    if let Some(proxy) = proxy {
        if let (Some(username), Some(password)) = (&proxy.username, &proxy.password) {
            page.authenticate(Credentials {
                username: username.clone(),
                password: password.clone(),
            })
            .await
            .map_err(|err| ArachneaHttpError::ChaserCfFailure(format!("proxy auth: {err}")))?;
        }
    }
    Ok(())
}

/// Polls until a `cf_clearance` cookie appears or the timeout expires.
///
/// For the first `CLEARANCE_PASSIVE_WAIT_MS` milliseconds nothing is done — the
/// managed challenge JS runs its invisible PoW/fingerprint in this window.
/// Polling the DOM during that phase causes timing anomalies that raise the bot
/// score. After the passive window a Turnstile widget, if present, is clicked
/// through its closed shadow root.
///
/// This function returns on timeout or success; it does not report which
/// occurred. Callers must inspect the resulting cookies.
///
/// # Arguments
///
/// - `page`: Raw page used to read cookies.
/// - `chaser`: High-level page used for challenge interaction.
/// - `timeout_seconds`: Maximum polling duration in seconds.
pub(super) async fn wait_for_clearance(page: &Page, chaser: &ChaserPage, timeout_seconds: u64) {
    let started = Instant::now();
    let timeout = Duration::from_secs(timeout_seconds);
    let mut last_click = started - crate::HTTP_CHASER_INITIAL_CLICK_OFFSET;

    loop {
        if has_clearance_cookie(page).await {
            tokio::time::sleep(crate::HTTP_CHASER_CLEARANCE_ERROR_POLL_INTERVAL).await;
            return;
        }

        if started.elapsed() >= timeout {
            return;
        }

        if started.elapsed().as_millis() as u64 >= CLEARANCE_PASSIVE_WAIT_MS
            && last_click.elapsed().as_millis() as u64 >= CLEARANCE_CLICK_INTERVAL_MS
        {
            try_click_challenge(chaser).await;
            last_click = Instant::now();
        }

        tokio::time::sleep(crate::HTTP_CHASER_CLEARANCE_POLL_INTERVAL).await;
    }
}

/// Returns true if the browser has a `cf_clearance` cookie for any domain.
///
/// Cookie-read failures are treated as absence.
///
/// # Arguments
///
/// - `page`: Browser page whose cookie jar is inspected.
pub(super) async fn has_clearance_cookie(page: &Page) -> bool {
    page.get_cookies()
        .await
        .map(|cookies| cookies.iter().any(|c| c.name == "cf_clearance"))
        .unwrap_or(false)
}

/// Prints a snapshot of the main document (iframes, turnstile markers) when
/// `CHASER_CF_DEBUG` is set, to pinpoint where the injected captcha lives.
///
/// This function is a no-op unless `CHASER_CF_DEBUG` is present in the
/// environment.
///
/// # Arguments
///
/// - `chaser`: Page whose main document is inspected.
/// - `stage`: Diagnostic label printed with the snapshot.
pub(super) async fn debug_dump(chaser: &ChaserPage, stage: &str) {
    if std::env::var_os("CHASER_CF_DEBUG").is_none() {
        return;
    }
    let script = crate::HTTP_CHASER_SCRIPT_DEBUG_DUMP;
    let info = chaser
        .evaluate(script)
        .await
        .ok()
        .flatten()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "(unreadable)".to_string());
    eprintln!("[chaser-cf][{stage}] main-document snapshot: {info}");
}

/// Collects human-readable descriptions of every `<iframe>` element in the CDP
/// DOM tree, including whether its content document was exposed (OOPIF iframes
/// have no content document in the parent tree).
///
/// # Arguments
///
/// - `node`: Root of the subtree to inspect.
/// - `out`: Destination for one description per iframe.
pub(super) fn collect_iframes(node: &Node, out: &mut Vec<String>) {
    if node.node_name.eq_ignore_ascii_case("iframe") {
        let src = attr_value(node, "src").unwrap_or("(none)");
        let content = if node.content_document.is_some() {
            "exposed"
        } else {
            "not-exposed"
        };
        let doc_url = node.document_url.as_deref().unwrap_or("(none)");
        out.push(format!(
            "src={src:?} content_doc={content} doc_url={doc_url:?} frame_id={:?}",
            node.frame_id
        ));
    }
    for child in node.children.as_deref().unwrap_or(&[]) {
        collect_iframes(child, out);
    }
    for sr in node.shadow_roots.as_deref().unwrap_or(&[]) {
        for child in sr.children.as_deref().unwrap_or(&[]) {
            collect_iframes(child, out);
        }
    }
    if let Some(content) = node.content_document.as_deref() {
        collect_iframes(content, out);
    }
}

/// Returns true when a Cloudflare challenge iframe or Turnstile widget has been
/// injected into the page by a page action, meaning the captcha needs solving.
///
/// Script failures and unexpected results are treated as absent.
///
/// # Arguments
///
/// - `chaser`: Page inspected for a challenge widget.
pub(super) async fn challenge_is_present(chaser: &ChaserPage) -> bool {
    let script = crate::HTTP_CHASER_SCRIPT_CHALLENGE_IS_PRESENT;
    chaser
        .evaluate(script)
        .await
        .ok()
        .flatten()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

/// Returns true when a solved Turnstile token is readable from the main page.
///
/// Script failures and unexpected results are treated as absence.
///
/// # Arguments
///
/// - `chaser`: Page inspected for a solved token.
pub(super) async fn has_turnstile_token(chaser: &ChaserPage) -> bool {
    let script = crate::HTTP_CHASER_SCRIPT_HAS_TURNSTILE_TOKEN;
    chaser
        .evaluate(script)
        .await
        .ok()
        .flatten()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

/// Polls until the captcha injected into the page by a page action is solved,
/// or the timeout expires.
///
/// Completion is signalled by a Turnstile token, or by the challenge widget
/// being removed from the DOM (which sites do once the solve callback fires).
/// A `cf_clearance` cookie is *not* a reliable signal here: the browser
/// session may already hold one from a previous run via the session cache,
/// which would short-circuit the wait without ever solving the injected
/// widget. While waiting, the challenge is clicked automatically (see
/// [`wait_for_clearance`] for the passive window).
///
/// # Arguments
///
/// - `page`: Raw page used to keep the solver foregrounded.
/// - `chaser`: High-level page used to inspect and click the challenge.
/// - `timeout`: Maximum polling duration.
pub(super) async fn wait_for_injected_captcha(page: &Page, chaser: &ChaserPage, timeout: Duration) {
    let started = Instant::now();
    let mut last_click = started - crate::HTTP_CHASER_INITIAL_CLICK_OFFSET;
    let mut last_front = started - crate::HTTP_CHASER_INITIAL_FRONT_OFFSET;
    let mut absent_since: Option<Instant> = None;

    loop {
        if has_turnstile_token(chaser).await {
            info!(target: "arachnea_http::engine::chaser_cf", "chaser-cf: captcha solved, turnstile token present");
            return;
        }
        if challenge_is_present(chaser).await {
            absent_since = None;
        } else {
            let absent = absent_since.unwrap_or(Instant::now());
            absent_since = Some(absent);
            if absent.elapsed() >= crate::HTTP_CHASER_CHALLENGE_ABSENCE_GRACE {
                info!(target: "arachnea_http::engine::chaser_cf", "chaser-cf: captcha solved, widget removed from page");
                return;
            }
        }
        if started.elapsed() >= timeout {
            warn!(target: "arachnea_http::engine::chaser_cf",
                elapsed_ms = started.elapsed().as_millis() as u64,
                "chaser-cf: captcha solve timed out"
            );
            return;
        }

        if last_front.elapsed() >= crate::HTTP_CHASER_FOREGROUND_POLL_INTERVAL {
            page.bring_to_front().await.ok();
            last_front = Instant::now();
        }
        if started.elapsed().as_millis() as u64 >= CLEARANCE_PASSIVE_WAIT_MS
            && last_click.elapsed().as_millis() as u64 >= CLEARANCE_CLICK_INTERVAL_MS
        {
            try_click_challenge(chaser).await;
            last_click = Instant::now();
        }

        tokio::time::sleep(crate::HTTP_CHASER_INJECTED_POLL_INTERVAL).await;
    }
}

/// Clicks the Turnstile challenge element by traversing its closed shadow root
/// via CDP, using a human-like cursor arc.
///
/// Three layouts are handled:
/// 1. The Cloudflare challenge page itself — the checkbox lives in a shadow
///    root of the page.
/// 2. A `turnstile.render(...)` widget injected into the main frame after a
///    page action (e.g. clicking a button) — the widget container owns the
///    `cf-turnstile-response` input and the shadow host is a descendant.
/// 3. A captcha injected into an `<iframe>` after a page action — the frame
///    owner element is located and the checkbox inside the frame is clicked at
///    its viewport position.
///
/// CDP failures are silently ignored.
///
/// # Arguments
///
/// - `chaser`: Page containing the challenge.
pub(super) async fn try_click_challenge(chaser: &ChaserPage) {
    let page = chaser.raw_page();

    let doc = match page
        .execute(GetDocumentParams {
            depth: Some(-1),
            pierce: Some(true),
        })
        .await
    {
        Ok(r) => r,
        Err(_) => return,
    };
    let root = &doc.result.root;

    if std::env::var_os("CHASER_CF_DEBUG").is_some() {
        let mut frames = Vec::new();
        collect_iframes(root, &mut frames);
        eprintln!(
            "[chaser-cf][try_click_challenge] cdp iframes: {}",
            if frames.is_empty() {
                "(none)".to_string()
            } else {
                frames.join(" | ")
            }
        );
        eprintln!(
            "[chaser-cf][try_click_challenge] turnstile checkbox node: {:?}",
            find_turnstile_checkbox(root)
        );
        eprintln!(
            "[chaser-cf][try_click_challenge] challenge frame: {:?}",
            find_challenge_frame_node(root)
                .and_then(|node| attr_value(node, "src").map(str::to_owned))
        );
    }

    // Widget rendered directly in the main frame (either the Cloudflare
    // challenge page itself or a `turnstile.render(...)` widget injected by a
    // page action). The checkbox is inside a closed shadow root; click it at
    // the checkbox position.
    if let Some(target_id) = find_turnstile_checkbox(root) {
        info!(target: "arachnea_http::engine::chaser_cf",
            target_id = ?target_id,
            "chaser-cf: turnstile checkbox surface found, clicking"
        );
        if click_turnstile_surface(page, target_id).await {
            return;
        }
    }

    // Fallback: the Turnstile widget itself is an iframe served by
    // `challenges.cloudflare.com` rendered inside the shadow root. Clicking the
    // iframe at the checkbox position toggles the checkbox.
    if let Some(iframe_id) = find_turnstile_iframe(root) {
        info!(target: "arachnea_http::engine::chaser_cf",
            target_id = ?iframe_id,
            "chaser-cf: turnstile widget iframe found, clicking"
        );
        if click_turnstile_surface(page, iframe_id).await {
            return;
        }
    }

    // Captcha injected inside a Cloudflare iframe: click the frame owner
    // element at the checkbox position so the coordinates stay valid in the
    // main viewport (cross-frame node IDs are not clickable).
    if let Some(frame_node) = find_challenge_frame_node(root) {
        info!(target: "arachnea_http::engine::chaser_cf",
            src = attr_value(frame_node, "src").unwrap_or("(none)"),
            "chaser-cf: challenge iframe found, clicking frame owner"
        );
        click_challenge_frame(page, frame_node).await;
    }
}

/// Clicks the Turnstile widget surface at the checkbox position (near its left
/// edge, vertically centered), where the checkbox circle is rendered. This
/// works both for a precise checkbox element and for the widget `<iframe>`
/// itself.
///
/// Returns `false` when the box model is unavailable or too small; click
/// failures are ignored.
///
/// # Arguments
///
/// - `page`: Browser page receiving the click.
/// - `target_id`: Node whose surface is clicked.
pub(super) async fn click_turnstile_surface(page: &Page, target_id: NodeId) -> bool {
    let box_model = match page
        .execute(GetBoxModelParams {
            node_id: Some(target_id),
            backend_node_id: None,
            object_id: None,
        })
        .await
    {
        Ok(r) => r,
        Err(_) => return false,
    };

    let content = box_model.result.model.content.inner();
    if content.len() < 8 {
        return false;
    }

    let left = content[0];
    let top = content[1];
    let width = content[2] - content[0];
    let height = content[5] - content[1];
    if width < 20.0 || height < 20.0 {
        return false;
    }

    let cx = left + (width * 0.5).min(26.0);
    let cy = top + height / 2.0;

    human_click(page, cx, cy).await;
    true
}

/// Clicks the Cloudflare challenge checkbox hosted by the iframe `frame_node`.
///
/// The checkbox position is resolved inside the frame's own execution context
/// when available, otherwise it falls back to the top-left corner of the widget
/// where the Turnstile checkbox is rendered.
///
/// Returns without clicking when the iframe box model cannot be obtained.
///
/// # Arguments
///
/// - `page`: Browser page receiving the click.
/// - `frame_node`: Iframe owner node.
pub(super) async fn click_challenge_frame(page: &Page, frame_node: &Node) {
    let box_model = match page
        .execute(GetBoxModelParams {
            node_id: Some(frame_node.node_id),
            backend_node_id: None,
            object_id: None,
        })
        .await
    {
        Ok(r) => r,
        Err(_) => return,
    };

    let content = box_model.result.model.content.inner();
    if content.len() < 8 {
        return;
    }

    // Content-box top-left corner of the iframe in main-viewport coordinates.
    let origin_x = content[0];
    let origin_y = content[1];

    let (bx, by) = match challenge_checkbox_point(page, frame_node).await {
        Some(point) => point,
        None => fallback_checkbox_offset(),
    };

    human_click(page, origin_x + bx, origin_y + by).await;
}

/// Locates the Turnstile checkbox inside `frame_node`'s document and returns
/// its center relative to the frame's viewport.
///
/// Returns `None` when the frame has no frame id or execution context, or when
/// the locator script cannot produce a point.
///
/// # Arguments
///
/// - `page`: Browser page containing the frame.
/// - `frame_node`: Iframe owner node.
pub(super) async fn challenge_checkbox_point(page: &Page, frame_node: &Node) -> Option<(f64, f64)> {
    let frame_id = frame_node.frame_id.clone()?;
    let context_id = page
        .frame_execution_context(frame_id)
        .await
        .ok()
        .flatten()?;

    let script = crate::HTTP_CHASER_SCRIPT_CHALLENGE_CHECKBOX_POINT;

    let params = EvaluateParams::builder()
        .expression(script)
        .context_id(context_id)
        .return_by_value(true)
        .build()
        .ok()?;
    let result = page.evaluate_expression(params).await.ok()?;
    let point: Option<FrameClickPoint> = result.into_value().ok()?;
    point.map(|point| (point.x, point.y))
}

/// Fallback checkbox offset used when the frame's execution context cannot be
/// reached: the Turnstile checkbox is rendered at the top-left of its iframe.
pub(super) fn fallback_checkbox_offset() -> (f64, f64) {
    (18.0, 18.0)
}

/// Walks the CDP DOM tree and returns the frame owner node hosting a Cloudflare
/// challenge iframe.
///
/// # Arguments
///
/// - `node`: Root of the subtree to inspect.
///
/// # Returns
///
/// The first matching iframe node, including inside shadow roots and exposed
/// content documents.
pub(super) fn find_challenge_frame_node(node: &Node) -> Option<&Node> {
    if is_challenge_frame(node) {
        return Some(node);
    }
    for child in node.children.as_deref().unwrap_or(&[]) {
        if let Some(found) = find_challenge_frame_node(child) {
            return Some(found);
        }
    }
    for sr in node.shadow_roots.as_deref().unwrap_or(&[]) {
        for child in sr.children.as_deref().unwrap_or(&[]) {
            if let Some(found) = find_challenge_frame_node(child) {
                return Some(found);
            }
        }
    }
    if let Some(content) = node.content_document.as_deref() {
        if let Some(found) = find_challenge_frame_node(content) {
            return Some(found);
        }
    }
    None
}

/// Returns whether the node is an `<iframe>` that hosts a Cloudflare challenge.
///
/// # Arguments
///
/// - `node`: Node to classify.
///
/// # Returns
///
/// `true` for an iframe whose `src`, document URL, or exposed content points at
/// `challenges.cloudflare.com`, or whose content contains a challenge response
/// input.
pub(super) fn is_challenge_frame(node: &Node) -> bool {
    if !node.node_name.eq_ignore_ascii_case("iframe") {
        return false;
    }
    // The `src` attribute carries the challenge URL even for cross-origin
    // OOPIF iframes, whose content document is not exposed in the parent DOM
    // tree and whose `document_url` is the parent document's URL.
    if let Some(src) = attr_value(node, "src") {
        if src.contains("challenges.cloudflare.com") {
            return true;
        }
    }
    if let Some(url) = node.document_url.as_deref() {
        if url.contains("challenges.cloudflare.com") {
            return true;
        }
    }
    if let Some(content) = node.content_document.as_deref() {
        if content
            .document_url
            .as_deref()
            .is_some_and(|url| url.contains("challenges.cloudflare.com"))
        {
            return true;
        }
        if subtree_has_response_input(content) {
            return true;
        }
    }
    false
}

/// Returns whether the subtree contains a Cloudflare Turnstile or managed
/// challenge response input.
///
/// # Arguments
///
/// - `node`: Root of the subtree to inspect.
pub(super) fn subtree_has_response_input(node: &Node) -> bool {
    if node.node_name.eq_ignore_ascii_case("input")
        && attr_value(node, "name")
            .is_some_and(|name| name == "cf-turnstile-response" || name == "cf-chl-response")
    {
        return true;
    }
    for child in node.children.as_deref().unwrap_or(&[]) {
        if subtree_has_response_input(child) {
            return true;
        }
    }
    for sr in node.shadow_roots.as_deref().unwrap_or(&[]) {
        for child in sr.children.as_deref().unwrap_or(&[]) {
            if subtree_has_response_input(child) {
                return true;
            }
        }
    }
    if let Some(content) = node.content_document.as_deref() {
        if subtree_has_response_input(content) {
            return true;
        }
    }
    false
}

/// Returns the value of the named attribute when present.
///
/// Attribute names are matched exactly and case-sensitively.
///
/// # Arguments
///
/// - `node`: Node carrying attributes.
/// - `wanted`: Attribute name.
pub(super) fn attr_value<'a>(node: &'a Node, wanted: &str) -> Option<&'a str> {
    node.attributes
        .as_deref()
        .unwrap_or(&[])
        .chunks(2)
        .find(|pair| pair.len() == 2 && pair[0] == wanted)
        .and_then(|pair| pair.get(1))
        .map(String::as_str)
}

/// Dispatches a human-like click at `(tx, ty)` using a ghost-cursor style cubic
/// Bezier arc from a random off-screen origin.
///
/// Mouse movement and click failures are ignored.
///
/// # Arguments
///
/// - `page`: Browser page receiving pointer events.
/// - `tx`: Target viewport x coordinate.
/// - `ty`: Target viewport y coordinate.
pub(super) async fn human_click(page: &Page, tx: f64, ty: f64) {
    // Compute all random values in a synchronous block so the thread RNG is
    // dropped before any await point (ThreadRng is !Send).
    let (tx, ty, curve_points, post_pause_ms) = {
        let mut rng = rand::rng();

        let tx = tx + rng.random_range(-5.0..=5.0_f64);
        let ty = ty + rng.random_range(-4.0..=4.0_f64);

        // Ghost-cursor style cubic Bezier from a random off-screen origin.
        let p0x = tx + rng.random_range(-200.0..=-60.0_f64);
        let p0y = ty + rng.random_range(-120.0..=120.0_f64);
        let p1x = p0x + (tx - p0x) * rng.random_range(0.2..0.5_f64) + rng.random_range(-30.0..30.0);
        let p1y = p0y + (ty - p0y) * rng.random_range(0.1..0.4_f64) + rng.random_range(-40.0..40.0);
        let p2x = p0x + (tx - p0x) * rng.random_range(0.5..0.8_f64) + rng.random_range(-20.0..20.0);
        let p2y = p0y + (ty - p0y) * rng.random_range(0.5..0.9_f64) + rng.random_range(-20.0..20.0);

        let steps: u8 = rng.random_range(12..22);
        let mut points: Vec<(f64, f64, u64)> = Vec::with_capacity(steps as usize);
        for i in 1..=steps {
            let t = i as f64 / steps as f64;
            let u = 1.0 - t;
            let bx =
                u * u * u * p0x + 3.0 * u * u * t * p1x + 3.0 * u * t * t * p2x + t * t * t * tx;
            let by =
                u * u * u * p0y + 3.0 * u * u * t * p1y + 3.0 * u * t * t * p2y + t * t * t * ty;
            let speed = (4.0 * t * (1.0 - t)).max(0.1);
            let step_ms = (rng.random_range(8.0..22.0_f64) / speed) as u64;
            points.push((bx, by, step_ms.min(80)));
        }

        let post_pause_ms = rng.random_range(40..120_u64);
        (tx, ty, points, post_pause_ms)
    };

    for (bx, by, step_ms) in curve_points {
        let _ = page.move_mouse(Point::new(bx, by)).await;
        tokio::time::sleep(Duration::from_millis(step_ms)).await;
    }

    tokio::time::sleep(Duration::from_millis(post_pause_ms)).await;
    let _ = page.click(Point::new(tx, ty)).await;
}

/// Walks the CDP DOM tree and returns the NodeId to click for the Turnstile
/// checkbox.
///
/// Two layouts are handled:
/// 1. The Cloudflare challenge page itself, where the shadow host also owns
///    the `cf-turnstile-response` input.
/// 2. An explicit `turnstile.render('#xf_lock', ...)` widget injected by a page
///    action, where the container holds the response input and the shadow host
///    is a nested descendant.
///
/// A precise checkbox element (`ctp-checkbox` / `input[type=checkbox]`) inside
/// any closed shadow root is preferred, since those class names are only used
/// by Turnstile. Otherwise the widget container — the node that owns the
/// `cf-turnstile-response` input — is located and its visible shadow-root
/// surface is returned (the whole Turnstile surface toggles on click).
///
/// # Arguments
///
/// - `node`: Root of the subtree to inspect.
///
/// # Returns
///
/// The preferred checkbox node id, or the widget surface when no precise
/// checkbox is found.
pub(super) fn find_turnstile_checkbox(node: &Node) -> Option<NodeId> {
    if let Some(id) = find_precise_checkbox(node) {
        return Some(id);
    }
    if let Some(container) = find_widget_container(node) {
        return checkbox_surface_within(container);
    }
    None
}

/// Returns the id of any Turnstile widget `<iframe>` (whose src is served by
/// `challenges.cloudflare.com`) present anywhere in the tree, including inside
/// closed shadow roots. Clicking the widget iframe toggles its checkbox.
///
/// # Arguments
///
/// - `node`: Root of the subtree to inspect.
pub(super) fn find_turnstile_iframe(node: &Node) -> Option<NodeId> {
    if node.node_name.eq_ignore_ascii_case("iframe")
        && attr_value(node, "src")
            .map(|src| src.contains("challenges.cloudflare.com"))
            .unwrap_or(false)
    {
        return Some(node.node_id);
    }
    for child in node.children.as_deref().unwrap_or(&[]) {
        if let Some(id) = find_turnstile_iframe(child) {
            return Some(id);
        }
    }
    for sr in node.shadow_roots.as_deref().unwrap_or(&[]) {
        for sr_child in sr.children.as_deref().unwrap_or(&[]) {
            if let Some(id) = find_turnstile_iframe(sr_child) {
                return Some(id);
            }
        }
    }
    if let Some(content) = node.content_document.as_deref() {
        if let Some(id) = find_turnstile_iframe(content) {
            return Some(id);
        }
    }
    None
}

/// Returns the id of the first Turnstile checkbox element found in any closed
/// shadow root of the tree.
///
/// # Arguments
///
/// - `node`: Root of the subtree to inspect.
pub(super) fn find_precise_checkbox(node: &Node) -> Option<NodeId> {
    for sr in node.shadow_roots.as_deref().unwrap_or(&[]) {
        for sr_child in sr.children.as_deref().unwrap_or(&[]) {
            if let Some(id) = checkbox_node_in_subtree(sr_child) {
                return Some(id);
            }
        }
    }
    for child in node.children.as_deref().unwrap_or(&[]) {
        if let Some(id) = find_precise_checkbox(child) {
            return Some(id);
        }
    }
    if let Some(content) = node.content_document.as_deref() {
        if let Some(id) = find_precise_checkbox(content) {
            return Some(id);
        }
    }
    None
}

/// Returns the node whose direct children include the hidden
/// `cf-turnstile-response` input — the Turnstile widget container.
///
/// # Arguments
///
/// - `node`: Root of the subtree to inspect.
pub(super) fn find_widget_container(node: &Node) -> Option<&Node> {
    let has_input = node.children.as_deref().unwrap_or(&[]).iter().any(|child| {
        child.node_name.eq_ignore_ascii_case("input")
            && attr_value(child, "name") == Some("cf-turnstile-response")
    });
    if has_input {
        return Some(node);
    }
    for child in node.children.as_deref().unwrap_or(&[]) {
        if let Some(found) = find_widget_container(child) {
            return Some(found);
        }
    }
    if let Some(content) = node.content_document.as_deref() {
        if let Some(found) = find_widget_container(content) {
            return Some(found);
        }
    }
    None
}

/// Returns the visible shadow-root surface inside the widget container's
/// subtree (the element whose box toggles the Turnstile checkbox).
///
/// Hidden nodes and stylesheet/link elements are skipped.
///
/// # Arguments
///
/// - `node`: Root of the subtree to inspect.
pub(super) fn checkbox_surface_within(node: &Node) -> Option<NodeId> {
    for sr in node.shadow_roots.as_deref().unwrap_or(&[]) {
        for sr_child in sr.children.as_deref().unwrap_or(&[]) {
            if !is_style_element(sr_child) && !is_hidden_node(sr_child) {
                return Some(sr_child.node_id);
            }
        }
    }
    for child in node.children.as_deref().unwrap_or(&[]) {
        if let Some(id) = checkbox_surface_within(child) {
            return Some(id);
        }
    }
    None
}

/// Returns the node id if `node` or one of its descendants looks like the
/// Turnstile checkbox element.
///
/// # Arguments
///
/// - `node`: Root of the subtree to inspect.
pub(super) fn checkbox_node_in_subtree(node: &Node) -> Option<NodeId> {
    if is_checkbox_node(node) {
        return Some(node.node_id);
    }
    for child in node.children.as_deref().unwrap_or(&[]) {
        if let Some(id) = checkbox_node_in_subtree(child) {
            return Some(id);
        }
    }
    for sr in node.shadow_roots.as_deref().unwrap_or(&[]) {
        for sr_child in sr.children.as_deref().unwrap_or(&[]) {
            if let Some(id) = checkbox_node_in_subtree(sr_child) {
                return Some(id);
            }
        }
    }
    None
}

/// Returns whether the node is the Turnstile checkbox surface.
///
/// # Arguments
///
/// - `node`: Node to classify.
pub(super) fn is_checkbox_node(node: &Node) -> bool {
    if node.node_name.eq_ignore_ascii_case("input") && attr_value(node, "type") == Some("checkbox")
    {
        return true;
    }
    attr_value(node, "class")
        .map(|class| class.contains("ctp-checkbox"))
        .unwrap_or(false)
}

/// Returns whether the node is a `<style>` or `<link>` element.
///
/// # Arguments
///
/// - `node`: Node to classify.
pub(super) fn is_style_element(node: &Node) -> bool {
    node.node_name.eq_ignore_ascii_case("style") || node.node_name.eq_ignore_ascii_case("link")
}

/// Returns whether the node is hidden via an inline `display: none` style.
///
/// # Arguments
///
/// - `node`: Node to classify.
pub(super) fn is_hidden_node(node: &Node) -> bool {
    node.attributes
        .as_deref()
        .unwrap_or(&[])
        .chunks(2)
        .any(|p| p.len() == 2 && p[0] == "style" && p[1].contains("display: none"))
}

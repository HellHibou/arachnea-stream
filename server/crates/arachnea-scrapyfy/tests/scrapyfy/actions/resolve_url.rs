use super::*;

#[test]
fn apply_wraps_resolved_urls_when_proxy_is_enabled() {
    let params = HashMap::from([(
        HTTP_PROXY_PUBLIC_PATH_PARAM.to_string(),
        "/arachnea/api/proxy".to_string(),
    )]);

    let values = apply(
        vec!["/images/poster.jpg".to_string()],
        "https://french-anime.com/animes/",
        &params,
        true,
        &HashMap::new(),
        &[],
        None,
    );

    assert_eq!(
        values,
        vec!["/arachnea/api/proxy/https://french-anime.com/images/poster.jpg"]
    );
}

#[test]
fn apply_keeps_direct_urls_when_proxy_is_disabled() {
    let params = HashMap::from([(
        HTTP_PROXY_PUBLIC_PATH_PARAM.to_string(),
        "/arachnea/api/proxy".to_string(),
    )]);

    let values = apply(
        vec!["/images/poster.jpg".to_string()],
        "https://french-anime.com/animes/",
        &params,
        false,
        &HashMap::new(),
        &[],
        None,
    );

    assert_eq!(values, vec!["https://french-anime.com/images/poster.jpg"]);
}

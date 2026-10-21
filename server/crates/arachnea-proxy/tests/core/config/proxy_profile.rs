use super::*;

/// Verifies that new profile names serialize while legacy names still parse.
#[test]
fn proxy_profile_accepts_new_and_legacy_names() {
    let current = serde_json::to_string(&ProxyProfile::Direct).unwrap();
    let legacy: ProxyProfile = serde_json::from_str("\"direct\"").unwrap();
    let current_single: ProxyProfile = serde_json::from_str("\"single_forwarder\"").unwrap();
    let legacy_single: ProxyProfile = serde_json::from_str("\"single_proxy\"").unwrap();

    assert_eq!(current, "\"system_relay\"");
    assert_eq!(legacy, ProxyProfile::Direct);
    assert_eq!(current_single, ProxyProfile::SingleProxy);
    assert_eq!(legacy_single, ProxyProfile::SingleProxy);
}

use omicsops_agent::provider::ProviderRequest;
use serde_json::json;

#[test]
fn provider_replay_is_optional_and_preserves_typed_call_ids() {
    let old =
        json!({"system":"fixture","messages":[],"tools":[],"require_strict_json_fallback":false});
    let parsed: ProviderRequest = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), old);
    let mut native = old;
    native["replay"] = json!([
        {"kind":"tool_call","call":{"call_id":"call_1","tool_id":"project.read","arguments":{"path":"nonce.txt"}}},
        {"kind":"tool_result","call_id":"call_1","output":"nonce-fixture"}
    ]);
    let parsed: ProviderRequest = serde_json::from_value(native.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), native);
}

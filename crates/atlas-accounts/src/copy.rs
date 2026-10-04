//! Account messages, pending catalog integration in copy-audit deliverable 2.
use serde_json::{Value, json};

pub fn msg(key: &str, params: Value) -> Value {
    let fallback = match key {
        "account.error.unauthorized" => "Sign in to continue.",
        "account.error.bad_credentials" => "The e-mail address or password is incorrect.",
        "account.error.invalid" => "Check the required fields and their values, then try again.",
        "account.error.not_found" => "This item was not found.",
        "account.error.forbidden" => "You do not have permission to change this item.",
        "account.error.conflict" => {
            "This change conflicts with an existing item. Reload and check it before trying again."
        }
        "account.error.rate_limited" => "Too many attempts. Try again later.",
        "account.error.unavailable" => "Accounts are unavailable. You can still search without signing in.",
        "account.error.internal" => "We could not complete this account request. Try again later.",
        "account.export.credentials" => {
            "Password hashes and session tokens are excluded from this download. We store only a password hash and a fingerprint of each session token."
        }
        "account.export.public_data" => {
            "Condition records and their sources are public data, separate from your account. Saved items include the IDs of the records they link to."
        }
        "account.export.network_data" => "Your account does not store IP addresses or browser details.",
        _ => panic!("unknown account copy key: {key}"),
    };
    json!({"key": key, "params": params, "fallback": fallback})
}

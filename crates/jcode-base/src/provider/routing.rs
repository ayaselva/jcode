pub(crate) fn anthropic_oauth_route_availability(model: &str) -> (bool, String) {
    if model.ends_with("[1m]") {
        match crate::usage::extra_usage_state() {
            Some(true) => {}
            Some(false) => return (false, "requires extra usage".to_string()),
            None => return (true, "extra usage not yet verified".to_string()),
        }
    }
    if model.contains("opus") && !crate::auth::claude::is_max_subscription() {
        (false, "requires Max subscription".to_string())
    } else {
        (true, String::new())
    }
}

pub(crate) fn anthropic_api_key_route_availability(model: &str) -> (bool, String) {
    if model.ends_with("[1m]") {
        match crate::usage::extra_usage_state() {
            Some(true) => (true, String::new()),
            Some(false) => (false, "requires extra usage".to_string()),
            None => (true, "extra usage not yet verified".to_string()),
        }
    } else {
        (true, String::new())
    }
}

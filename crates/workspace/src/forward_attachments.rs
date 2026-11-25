//! Tracks which connection keys currently hold a forward-engine host
//! attachment, so each `attach_host` is paired with exactly one `detach_host`.

use std::collections::HashMap;

#[derive(Default)]
pub(crate) struct ForwardAttachments(HashMap<String, String>);

impl ForwardAttachments {
    /// True only if `key` was newly inserted (caller must call `attach_host`).
    pub(crate) fn attach(&mut self, key: &str, host_id: &str) -> bool {
        self.0
            .insert(key.to_string(), host_id.to_string())
            .is_none()
    }

    /// The host id if `key` was attached (caller must call `detach_host`).
    pub(crate) fn detach(&mut self, key: &str) -> Option<String> {
        self.0.remove(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_detach_pairs_once() {
        let mut a = ForwardAttachments::default();
        assert!(a.attach("k", "h"));
        assert!(!a.attach("k", "h"));
        assert_eq!(a.detach("k").as_deref(), Some("h"));
        assert_eq!(a.detach("k"), None);
        assert_eq!(a.detach("unknown"), None);
        assert!(a.attach("k", "h"));
    }
}

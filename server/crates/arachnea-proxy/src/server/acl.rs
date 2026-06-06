use std::net::IpAddr;

use ipnet::IpNet;

/// Client network ACL for proxy listeners.
#[derive(Clone, Debug)]
pub struct ClientAcl {
    allow: Vec<IpNet>,
    deny: Vec<IpNet>,
}

impl ClientAcl {
    /// Creates an ACL from allow and deny networks.
    ///
    /// # Parameters
    ///
    /// - `allow`: Networks that may use the listener when the allowlist is not empty.
    /// - `deny`: Networks that are always denied.
    ///
    /// # Returns
    ///
    /// Client ACL ready for listener checks.
    pub fn new(allow: Vec<IpNet>, deny: Vec<IpNet>) -> Self {
        Self { allow, deny }
    }

    /// Returns true if a client IP may use the server.
    ///
    /// # Parameters
    ///
    /// - `ip`: Client IP address to check.
    ///
    /// # Returns
    ///
    /// `true` when the IP is allowed by the ACL.
    pub fn allows(&self, ip: IpAddr) -> bool {
        if self.deny.iter().any(|net| net.contains(&ip)) {
            return false;
        }
        if self.allow.is_empty() {
            return true;
        }
        self.allow.iter().any(|net| net.contains(&ip))
    }
}

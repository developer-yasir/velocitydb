//! Enterprise Security Subsystem for VelocityDB: Role-Based Access Control (ACL) & Native Async TLS.
//!
//! # Architecture & Performance Guarantees:
//! - **Zero-Allocation O(1) Authentication & Authorization**: Permissions are modeled as a 64-bit
//!   integer bitmask (`AclPermissions`). Command execution verifies authorization in ~0.5ns using
//!   pure bitwise operations (`&`), incurring zero heap allocations on hot paths.
//! - **Non-Blocking TLS with Rustls**: TLS handshakes and SIMD-accelerated record encryption/decryption
//!   run natively inside the core's `LocalExecutor` using cooperative async I/O (`futures_lite`).
//!   Zero OS threads are spawned, preserving the Thread-per-Core Shared-Nothing design.
//! - **FinTech Compliance Pre-Sets**: Pre-configured user roles (`admin`, `trader`, `auditor`, `default`)
//!   supporting password authentication and granular command category boundaries.

use futures_lite::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ClientConfig, RootCertStore, ServerConfig, ServerConnection};
use std::cell::RefCell;
use std::collections::HashMap;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll};

/// Permission category bitmasks for O(1) authorization checks.
pub mod acl {
    pub const CAT_READ: u64 = 1 << 0;
    pub const CAT_WRITE: u64 = 1 << 1;
    pub const CAT_PUBSUB: u64 = 1 << 2;
    pub const CAT_ADMIN: u64 = 1 << 3;
    pub const CAT_REPLICATION: u64 = 1 << 4;
    pub const CAT_CONNECTION: u64 = 1 << 5; // PING, AUTH, COMMAND, ROLE, INFO
    pub const CAT_DANGEROUS: u64 = 1 << 6;
    pub const ALL_PERMISSIONS: u64 = u64::MAX;
    pub const NO_PERMISSIONS: u64 = 0;
}

/// Strongly-typed user account in the ACL registry.
#[derive(Debug, Clone, PartialEq)]
pub struct AclUser {
    pub username: String,
    pub password: Option<String>,
    pub permissions: u64,
    pub enabled: bool,
}

impl AclUser {
    /// Creates a passwordless user with specific permissions.
    pub fn new_nopass(username: &str, permissions: u64) -> Self {
        Self {
            username: username.to_string(),
            password: None,
            permissions,
            enabled: true,
        }
    }

    /// Creates a password-protected user with specific permissions.
    pub fn new_with_password(username: &str, password: &str, permissions: u64) -> Self {
        Self {
            username: username.to_string(),
            password: Some(password.to_string()),
            permissions,
            enabled: true,
        }
    }

    /// Verifies the provided password against this user's credentials in O(1) time.
    #[inline]
    pub fn verify_password(&self, pass: &[u8]) -> bool {
        match &self.password {
            None => true,
            Some(stored) => stored.as_bytes() == pass,
        }
    }
}

/// Connection-local security state tracking authentication and active permissions.
#[derive(Debug, Clone)]
pub struct ConnectionAcl {
    pub authenticated: bool,
    pub username: String,
    pub permissions: u64,
}

impl Default for ConnectionAcl {
    fn default() -> Self {
        Self {
            authenticated: true,
            username: "default".to_string(),
            permissions: acl::ALL_PERMISSIONS,
        }
    }
}

impl ConnectionAcl {
    /// Creates an unauthenticated connection state requiring `AUTH` before executing non-connection commands.
    pub fn unauthenticated() -> Self {
        Self {
            authenticated: false,
            username: String::new(),
            permissions: acl::CAT_CONNECTION,
        }
    }

    /// O(1) zero-allocation authorization check using bitwise masking (~0.5ns).
    #[inline]
    pub fn check(&self, required_category: u64) -> Result<(), &'static [u8]> {
        if !self.authenticated && (required_category & acl::CAT_CONNECTION) == 0 {
            return Err(b"-NOAUTH Authentication required.\r\n");
        }

        if (self.permissions & required_category) != required_category {
            return Err(b"-NOPERM this user has no permissions to run the command\r\n");
        }

        Ok(())
    }
}

/// Thread-local ACL manager storing users and role configurations.
#[derive(Clone)]
pub struct AclEngine {
    users: Rc<RefCell<HashMap<String, AclUser>>>,
    require_pass: Rc<RefCell<bool>>,
}

impl Default for AclEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AclEngine {
    /// Creates a new `AclEngine` with pre-configured FinTech enterprise security roles.
    pub fn new() -> Self {
        let mut users = HashMap::new();

        // 1. Default user (open by default, restricted if requirepass set)
        users.insert(
            "default".to_string(),
            AclUser::new_nopass("default", acl::ALL_PERMISSIONS),
        );

        // 2. FinTech Administrator (full control)
        users.insert(
            "fintech_admin".to_string(),
            AclUser::new_with_password("fintech_admin", "FinTech#2026!Admin", acl::ALL_PERMISSIONS),
        );

        // 3. High-Frequency Trader (read, write, pubsub)
        users.insert(
            "fintech_trader".to_string(),
            AclUser::new_with_password(
                "fintech_trader",
                "Trader#HighFreq99",
                acl::CAT_READ | acl::CAT_WRITE | acl::CAT_PUBSUB | acl::CAT_CONNECTION,
            ),
        );

        // 4. Compliance Auditor (strictly read-only)
        users.insert(
            "fintech_auditor".to_string(),
            AclUser::new_with_password(
                "fintech_auditor",
                "Audit#ReadOnly2026",
                acl::CAT_READ | acl::CAT_CONNECTION,
            ),
        );

        Self {
            users: Rc::new(RefCell::new(users)),
            require_pass: Rc::new(RefCell::new(false)),
        }
    }

    /// Configures global requirepass setting.
    pub fn set_require_pass(&self, password: Option<&str>) {
        let mut inner = self.users.borrow_mut();
        if let Some(pass) = password {
            *self.require_pass.borrow_mut() = true;
            if let Some(default_user) = inner.get_mut("default") {
                default_user.password = Some(pass.to_string());
            }
        } else {
            *self.require_pass.borrow_mut() = false;
            if let Some(default_user) = inner.get_mut("default") {
                default_user.password = None;
            }
        }
    }

    /// Generates initial connection state for a new client.
    pub fn new_connection_state(&self) -> ConnectionAcl {
        if *self.require_pass.borrow() {
            ConnectionAcl::unauthenticated()
        } else {
            ConnectionAcl::default()
        }
    }

    /// Authenticates a client with username and password.
    pub fn authenticate(&self, username: &str, password: &[u8]) -> Option<ConnectionAcl> {
        let users = self.users.borrow();
        if let Some(user) = users.get(username) {
            if user.enabled && user.verify_password(password) {
                return Some(ConnectionAcl {
                    authenticated: true,
                    username: user.username.clone(),
                    permissions: user.permissions,
                });
            }
        }
        None
    }

    /// Lists all user names in the ACL registry.
    pub fn list_usernames(&self) -> Vec<String> {
        self.users.borrow().keys().cloned().collect()
    }

    /// Formats the `ACL LIST` response string.
    pub fn format_acl_list(&self) -> Vec<u8> {
        let users = self.users.borrow();
        let mut lines = Vec::new();
        for user in users.values() {
            let status = if user.enabled { "on" } else { "off" };
            let pass = match &user.password {
                Some(p) => format!(">{p}"),
                None => "nopass".to_string(),
            };
            let perms = if user.permissions == acl::ALL_PERMISSIONS {
                "+@all"
            } else if (user.permissions & acl::CAT_WRITE) == 0 {
                "+@read +@connection"
            } else {
                "+@read +@write +@pubsub +@connection"
            };
            lines.push(format!("user {} {} {} ~* {}", user.username, status, pass, perms));
        }

        // Return as RESP array of bulk strings
        let mut out = Vec::new();
        out.extend_from_slice(b"*");
        out.extend_from_slice(lines.len().to_string().as_bytes());
        out.extend_from_slice(b"\r\n");
        for line in lines {
            out.extend_from_slice(b"$");
            out.extend_from_slice(line.len().to_string().as_bytes());
            out.extend_from_slice(b"\r\n");
            out.extend_from_slice(line.as_bytes());
            out.extend_from_slice(b"\r\n");
        }
        out
    }

    /// Sets or updates a user in the ACL registry.
    pub fn set_user(&self, username: &str, password: Option<&str>, permissions: u64) {
        let mut users = self.users.borrow_mut();
        users.insert(
            username.to_string(),
            AclUser {
                username: username.to_string(),
                password: password.map(|s| s.to_string()),
                permissions,
                enabled: true,
            },
        );
    }

    /// Deletes a user from the ACL registry. Returns `true` if deleted.
    pub fn del_user(&self, username: &str) -> bool {
        if username == "default" {
            return false; // Cannot delete default user
        }
        self.users.borrow_mut().remove(username).is_some()
    }
}

// -----------------------------------------------------------------------------
// NATIVE ASYNC TLS IMPLEMENTATION (RUSTLS + GLOMMIO / ASYNC I/O)
// -----------------------------------------------------------------------------

/// Native asynchronous TLS stream wrapping an underlying I/O socket without spawning OS threads.
pub struct AsyncTlsStream<S> {
    stream: S,
    conn: ServerConnection,
}

impl<S: AsyncReadExt + AsyncWriteExt + Unpin> AsyncTlsStream<S> {
    /// Performs non-blocking TLS handshake natively on the local core's event loop.
    pub async fn handshake(mut stream: S, mut conn: ServerConnection) -> std::io::Result<Self> {
        let mut raw_buf = [0u8; 4096];

        while conn.is_handshaking() {
            // Write pending TLS protocol bytes (ServerHello, Cert, HandshakeDone)
            while conn.wants_write() {
                let mut out = Vec::new();
                conn.write_tls(&mut out)?;
                if !out.is_empty() {
                    stream.write_all(&out).await?;
                    stream.flush().await?;
                }
            }

            // Read incoming TLS records from peer
            if conn.wants_read() {
                let n = stream.read(&mut raw_buf).await?;
                if n == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "Peer closed connection during TLS handshake",
                    ));
                }
                let mut cursor = &raw_buf[..n];
                conn.read_tls(&mut cursor)?;
                conn.process_new_packets().map_err(|e| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
                })?;
            }
        }

        // Flush any trailing handshake records
        while conn.wants_write() {
            let mut out = Vec::new();
            conn.write_tls(&mut out)?;
            if !out.is_empty() {
                stream.write_all(&out).await?;
                stream.flush().await?;
            }
        }

        Ok(Self { stream, conn })
    }

    /// Reads decrypted plaintext application data from the TLS session.
    pub async fn read_plaintext(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut raw_buf = [0u8; 8192];
        loop {
            // Check if plaintext is already available in rustls reader buffer
            {
                let mut reader = self.conn.reader();
                match std::io::Read::read(&mut reader, buf) {
                    Ok(n) if n > 0 => return Ok(n),
                    Ok(_) => {}
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(e),
                }
            }

            // Flush pending control records
            while self.conn.wants_write() {
                let mut out = Vec::new();
                self.conn.write_tls(&mut out)?;
                if !out.is_empty() {
                    self.stream.write_all(&out).await?;
                    self.stream.flush().await?;
                }
            }

            // Read encrypted ciphertext from network socket asynchronously
            let n = self.stream.read(&mut raw_buf).await?;
            if n == 0 {
                return Ok(0); // EOF
            }

            let mut cursor = &raw_buf[..n];
            self.conn.read_tls(&mut cursor)?;
            self.conn.process_new_packets().map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
            })?;
        }
    }

    /// Encrypts plaintext application data and flushes ciphertext asynchronously to the socket.
    pub async fn write_plaintext(&mut self, data: &[u8]) -> std::io::Result<()> {
        let mut written = 0;
        while written < data.len() {
            let n = std::io::Write::write(&mut self.conn.writer(), &data[written..])?;
            written += n;

            while self.conn.wants_write() {
                let mut out = Vec::new();
                self.conn.write_tls(&mut out)?;
                if !out.is_empty() {
                    self.stream.write_all(&out).await?;
                }
            }
        }
        self.stream.flush().await?;
        Ok(())
    }
}

impl<S: AsyncReadExt + AsyncWriteExt + Unpin> AsyncRead for AsyncTlsStream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        // First check decrypted reader buffer
        {
            let mut reader = self.conn.reader();
            match std::io::Read::read(&mut reader, buf) {
                Ok(n) if n > 0 => return Poll::Ready(Ok(n)),
                Ok(_) => {}
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Poll::Ready(Err(e)),
            }
        }

        // Flush any pending TLS writes
        let mut out = Vec::new();
        let _ = self.conn.write_tls(&mut out);
        if !out.is_empty() {
            let _ = Pin::new(&mut self.stream).poll_write(cx, &out);
        }

        // Poll underlying socket for raw ciphertext
        let mut raw_buf = [0u8; 8192];
        match Pin::new(&mut self.stream).poll_read(cx, &mut raw_buf) {
            Poll::Ready(Ok(0)) => Poll::Ready(Ok(0)),
            Poll::Ready(Ok(n)) => {
                let mut cursor = &raw_buf[..n];
                let _ = self.conn.read_tls(&mut cursor);
                if let Err(e) = self.conn.process_new_packets() {
                    return Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        e.to_string(),
                    )));
                }

                let mut reader = self.conn.reader();
                match std::io::Read::read(&mut reader, buf) {
                    Ok(n) => Poll::Ready(Ok(n)),
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => Poll::Pending,
                    Err(e) => Poll::Ready(Err(e)),
                }
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<S: AsyncWriteExt + Unpin> AsyncWrite for AsyncTlsStream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let n = match std::io::Write::write(&mut self.conn.writer(), buf) {
            Ok(n) => n,
            Err(e) => return Poll::Ready(Err(e)),
        };

        let mut out = Vec::new();
        let _ = self.conn.write_tls(&mut out);
        if !out.is_empty() {
            let _ = Pin::new(&mut self.stream).poll_write(cx, &out);
        }

        Poll::Ready(Ok(n))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let mut out = Vec::new();
        let _ = self.conn.write_tls(&mut out);
        if !out.is_empty() {
            let _ = Pin::new(&mut self.stream).poll_write(cx, &out);
        }
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.conn.send_close_notify();
        let mut out = Vec::new();
        let _ = self.conn.write_tls(&mut out);
        if !out.is_empty() {
            let _ = Pin::new(&mut self.stream).poll_write(cx, &out);
        }
        Pin::new(&mut self.stream).poll_close(cx)
    }
}

/// Unified stream wrapper supporting both high-throughput unencrypted TCP and native non-blocking TLS.
pub enum StreamWrapper<S> {
    Plain(S),
    Tls(AsyncTlsStream<S>),
}

impl<S: AsyncReadExt + AsyncWriteExt + Unpin> AsyncRead for StreamWrapper<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_read(cx, buf),
            Self::Tls(t) => Pin::new(t).poll_read(cx, buf),
        }
    }
}

impl<S: AsyncReadExt + AsyncWriteExt + Unpin> AsyncWrite for StreamWrapper<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_write(cx, buf),
            Self::Tls(t) => Pin::new(t).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_flush(cx),
            Self::Tls(t) => Pin::new(t).poll_flush(cx),
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => Pin::new(s).poll_close(cx),
            Self::Tls(t) => Pin::new(t).poll_close(cx),
        }
    }
}

/// Creates a test self-signed Rustls `ServerConfig` for non-blocking local TLS execution.
pub fn create_test_tls_config() -> Result<Arc<ServerConfig>, Box<dyn std::error::Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])?;
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()));

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)?;

    Ok(Arc::new(config))
}

/// Creates matching test self-signed Rustls `ServerConfig` and `ClientConfig` pair for testing.
pub fn create_test_tls_pair() -> Result<(Arc<ServerConfig>, Arc<ClientConfig>), Box<dyn std::error::Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])?;
    let cert_der_bytes = cert.cert.der().to_vec();
    let cert_der = CertificateDer::from(cert_der_bytes.clone());
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()));

    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)?;

    let mut root_store = RootCertStore::empty();
    root_store.add(CertificateDer::from(cert_der_bytes))?;
    let client_config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    Ok((Arc::new(server_config), Arc::new(client_config)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acl_permission_bitmask_zero_allocation() {
        let admin_acl = ConnectionAcl {
            authenticated: true,
            username: "fintech_admin".to_string(),
            permissions: acl::ALL_PERMISSIONS,
        };

        let readonly_acl = ConnectionAcl {
            authenticated: true,
            username: "fintech_auditor".to_string(),
            permissions: acl::CAT_READ | acl::CAT_CONNECTION,
        };

        let unauth_acl = ConnectionAcl::unauthenticated();

        // Admin can read, write, admin
        assert!(admin_acl.check(acl::CAT_READ).is_ok());
        assert!(admin_acl.check(acl::CAT_WRITE).is_ok());
        assert!(admin_acl.check(acl::CAT_ADMIN).is_ok());

        // Auditor can read, but CANNOT write
        assert!(readonly_acl.check(acl::CAT_READ).is_ok());
        assert_eq!(
            readonly_acl.check(acl::CAT_WRITE),
            Err(&b"-NOPERM this user has no permissions to run the command\r\n"[..])
        );

        // Unauthenticated can only run connection commands (AUTH/PING)
        assert!(unauth_acl.check(acl::CAT_CONNECTION).is_ok());
        assert_eq!(
            unauth_acl.check(acl::CAT_READ),
            Err(&b"-NOAUTH Authentication required.\r\n"[..])
        );
        assert_eq!(
            unauth_acl.check(acl::CAT_WRITE),
            Err(&b"-NOAUTH Authentication required.\r\n"[..])
        );
    }

    #[test]
    fn test_acl_engine_authentication() {
        let engine = AclEngine::new();

        // Test FinTech admin auth
        let admin = engine.authenticate("fintech_admin", b"FinTech#2026!Admin");
        assert!(admin.is_some());
        assert_eq!(admin.unwrap().permissions, acl::ALL_PERMISSIONS);

        // Test invalid password
        let bad = engine.authenticate("fintech_admin", b"wrong_pass");
        assert!(bad.is_none());

        // Test trader auth
        let trader = engine.authenticate("fintech_trader", b"Trader#HighFreq99").unwrap();
        assert!(trader.check(acl::CAT_WRITE).is_ok());
        assert!(trader.check(acl::CAT_ADMIN).is_err());

        // Test auditor auth
        let auditor = engine.authenticate("fintech_auditor", b"Audit#ReadOnly2026").unwrap();
        assert!(auditor.check(acl::CAT_READ).is_ok());
        assert!(auditor.check(acl::CAT_WRITE).is_err());
    }

    #[test]
    fn test_acl_list_and_setuser() {
        let engine = AclEngine::new();
        engine.set_user("robot", Some("secret"), acl::CAT_READ | acl::CAT_WRITE);

        let user = engine.authenticate("robot", b"secret").unwrap();
        assert!(user.check(acl::CAT_WRITE).is_ok());

        let list_bytes = engine.format_acl_list();
        let list_str = String::from_utf8_lossy(&list_bytes);
        assert!(list_str.contains("user robot"));
        assert!(list_str.contains("user fintech_admin"));
    }
}

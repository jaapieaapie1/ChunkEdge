//! An ergonomic API for Minecraft's [cookies] during the login and
//! configuration phases of the connection.
//!
//! A cookie is an arbitrary `key -> bytes` blob the server stores *on the
//! client*. Cookies survive server transfers and are cleared when the client
//! disconnects normally, which makes them the canonical way to carry state
//! between servers in a network.
//!
//! Because the login and configuration phases run inside the networking task
//! before a [`Client`] entity exists, this handle is the only way to touch
//! cookies there. It is handed to users through
//! [`NetworkCallbacks::login_cookies`] and [`NetworkCallbacks::configure`].
//! For the play phase, use the ECS API on [`Client`] instead
//! (`Client::store_cookie` / `Client::request_cookie`).
//!
//! [cookies]: https://minecraft.wiki/w/Java_Edition_protocol#Store_Cookie
//! [`Client`]: chunkedge_server::client::Client
//! [`NetworkCallbacks::login_cookies`]: crate::NetworkCallbacks::login_cookies
//! [`NetworkCallbacks::configure`]: crate::NetworkCallbacks::configure

use std::marker::PhantomData;

use chunkedge_binary::Bounded;
use chunkedge_protocol::packets::configuration::StoreCookieS2c;
use chunkedge_protocol::Ident;
use tracing::warn;

use crate::packet_io::PacketIo;

/// The maximum size, in bytes, of a cookie payload as defined by the protocol.
pub const MAX_COOKIE_SIZE: usize = 5120;

mod sealed {
    pub trait Sealed {}
}

/// Marker for the **Login** connection phase. Cookies can be read with
/// [`Cookies::get`] but not written — the protocol has no login-phase
/// `StoreCookie` packet.
pub enum Login {}

/// Marker for the **Configuration** connection phase. Cookies can be both read
/// ([`Cookies::get`]) and written ([`Cookies::set`] / [`Cookies::remove`]).
pub enum Configuration {}

impl sealed::Sealed for Login {}
impl sealed::Sealed for Configuration {}

/// A phase of the connection during which cookies may be accessed. Sealed: only
/// [`Login`] and [`Configuration`] implement it.
pub trait CookiePhase: sealed::Sealed {}

impl CookiePhase for Login {}
impl CookiePhase for Configuration {}

/// An asynchronous handle for reading and writing client-side cookies during
/// the login or configuration phase.
///
/// The phase is encoded in the type parameter `P` so that write operations are
/// only available where the protocol allows them: [`set`](Cookies::set) and
/// [`remove`](Cookies::remove) exist solely on `Cookies<Configuration>`, so
/// attempting to write a cookie during login is a compile error rather than a
/// runtime protocol violation.
pub struct Cookies<'a, P: CookiePhase> {
    io: &'a mut PacketIo,
    _phase: PhantomData<fn() -> P>,
}

impl<'a, P: CookiePhase> Cookies<'a, P> {
    pub(crate) fn new(io: &'a mut PacketIo) -> Self {
        Self {
            io,
            _phase: PhantomData,
        }
    }
}

impl<'a> Cookies<'a, Login> {
    /// Request a cookie from the client and await its response.
    ///
    /// Returns `Ok(None)` if the client holds no cookie under `key`.
    pub async fn get(&mut self, key: Ident<&str>) -> anyhow::Result<Option<Vec<u8>>> {
        use chunkedge_protocol::packets::login::{CookieRequestS2c, CookieResponseC2s};

        self.io
            .send_packet(&CookieRequestS2c { key: key.into() })
            .await?;

        let resp: CookieResponseC2s = self.io.recv_packet().await?;
        Ok(finish(key, &resp.key, resp.payload))
    }
}

impl<'a> Cookies<'a, Configuration> {
    /// Request a cookie from the client and await its response.
    ///
    /// Returns `Ok(None)` if the client holds no cookie under `key`.
    pub async fn get(&mut self, key: Ident<&str>) -> anyhow::Result<Option<Vec<u8>>> {
        use chunkedge_protocol::packets::configuration::{CookieRequestS2c, CookieResponseC2s};

        self.io
            .send_packet(&CookieRequestS2c { key: key.into() })
            .await?;

        let resp: CookieResponseC2s = self.io.recv_packet().await?;
        Ok(finish(key, &resp.key, resp.payload))
    }

    /// Store a cookie (`key -> payload`) on the client.
    ///
    /// Returns an error if `payload` exceeds [`MAX_COOKIE_SIZE`] bytes.
    pub async fn set(&mut self, key: Ident<&str>, payload: &[u8]) -> anyhow::Result<()> {
        anyhow::ensure!(
            payload.len() <= MAX_COOKIE_SIZE,
            "cookie payload of {} bytes exceeds the {MAX_COOKIE_SIZE} byte limit",
            payload.len(),
        );

        self.io
            .send_packet(&StoreCookieS2c {
                key: key.into(),
                payload: Bounded(payload),
            })
            .await
    }

    /// Remove a cookie from the client by storing an empty payload under `key`.
    ///
    /// Minecraft has no dedicated delete operation; an empty blob is the
    /// idiomatic erase.
    pub async fn remove(&mut self, key: Ident<&str>) -> anyhow::Result<()> {
        self.set(key, &[]).await
    }
}

/// Validate the responded key and own the returned payload bytes, dropping the
/// borrow of the IO frame buffer.
fn finish(
    requested: Ident<&str>,
    responded: &Ident<std::borrow::Cow<str>>,
    payload: Option<Bounded<&[u8], MAX_COOKIE_SIZE>>,
) -> Option<Vec<u8>> {
    if responded.as_str() != requested.as_str() {
        warn!(
            requested = requested.as_str(),
            responded = responded.as_str(),
            "cookie response key does not match the requested key",
        );
    }

    payload.map(|b| b.0.to_vec())
}

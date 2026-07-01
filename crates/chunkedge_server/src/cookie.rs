//! Play-phase access to Minecraft's [cookies] — arbitrary `key -> bytes` blobs
//! stored on the client that survive server transfers.
//!
//! Storing and requesting cookies is done through methods on [`Client`]
//! ([`Client::store_cookie`] / [`Client::request_cookie`]). A client's reply to
//! a request is surfaced as a [`CookieResponseMessage`].
//!
//! For the login and configuration phases (before a [`Client`] entity exists),
//! use the `Cookies` handle in `chunkedge_network` instead.
//!
//! [cookies]: https://minecraft.wiki/w/Java_Edition_protocol#Store_Cookie

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use chunkedge_binary::Bounded;
use chunkedge_protocol::packets::play::{CookieRequestS2c, CookieResponseC2s, StoreCookieS2c};
use chunkedge_protocol::{Ident, WritePacket};

use crate::client::Client;
use crate::event_loop::{EventLoopPreUpdate, PacketMessage};

/// The maximum size, in bytes, of a cookie payload as defined by the protocol.
pub const MAX_COOKIE_SIZE: usize = 5120;

pub struct CookiePlugin;

impl Plugin for CookiePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<CookieResponseMessage>()
            .add_systems(EventLoopPreUpdate, handle_cookie_response);
    }
}

/// Emitted when a client replies to a [`Client::request_cookie`] call.
///
/// `payload` is `None` when the client holds no cookie under `key`.
#[derive(Message, Clone, Debug)]
pub struct CookieResponseMessage {
    pub client: Entity,
    pub key: Ident<String>,
    pub payload: Option<Vec<u8>>,
}

impl Client {
    /// Store a cookie (`key -> payload`) on the client.
    ///
    /// Payloads larger than [`MAX_COOKIE_SIZE`] bytes are rejected by the
    /// client; in debug builds this is asserted.
    pub fn store_cookie(&mut self, key: Ident<&str>, payload: &[u8]) {
        debug_assert!(
            payload.len() <= MAX_COOKIE_SIZE,
            "cookie payload of {} bytes exceeds the {MAX_COOKIE_SIZE} byte limit",
            payload.len(),
        );

        self.write_packet(&StoreCookieS2c {
            key: key.into(),
            payload: Bounded(payload),
        });
    }

    /// Ask the client to send back the cookie stored under `key`. The reply
    /// arrives as a [`CookieResponseMessage`].
    pub fn request_cookie(&mut self, key: Ident<&str>) {
        self.write_packet(&CookieRequestS2c { key: key.into() });
    }
}

fn handle_cookie_response(
    mut packets: MessageReader<PacketMessage>,
    mut messages: MessageWriter<CookieResponseMessage>,
) {
    for packet in packets.read() {
        if let Some(pkt) = packet.decode::<CookieResponseC2s>() {
            messages.write(CookieResponseMessage {
                client: packet.client,
                key: pkt.key.into(),
                payload: pkt.payload.map(|b| b.0.to_vec()),
            });
        }
    }
}

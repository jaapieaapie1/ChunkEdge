#![allow(clippy::type_complexity)]

//! Demonstrates Minecraft's cookies — arbitrary `key -> bytes` blobs stored on
//! the client that survive server transfers.
//!
//! - During the **configuration** phase, [`MyCallbacks::configure`] reads a
//!   `chunkedge:visits` cookie, increments it, and writes it back. Reconnect
//!   and the counter keeps climbing, proving the value round-trips to the
//!   client.
//! - During the **play** phase, [`greet_clients`] asks each joining client for
//!   the same cookie via [`Client::request_cookie`], and [`log_cookies`] prints
//!   the reply delivered as a [`CookieResponseMessage`].

use chunkedge::client::{VisibleChunkLayer, VisibleEntityLayers};
use chunkedge::network::{async_trait, Configuration, Cookies, Login};
use chunkedge::prelude::*;

const SPAWN_Y: i32 = 64;

/// The cookie key used throughout this example. A cookie key is a namespaced
/// identifier.
const VISITS_KEY: Ident<&str> = ident!("chunkedge:visits");

fn main() {
    App::new()
        .insert_resource(NetworkSettings {
            connection_mode: ConnectionMode::Offline,
            callbacks: MyCallbacks.into(),
            ..Default::default()
        })
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                init_clients,
                greet_clients,
                log_cookies,
                despawn_disconnected_clients,
            ),
        )
        .run();
}

struct MyCallbacks;

#[async_trait]
impl NetworkCallbacks for MyCallbacks {
    async fn login_cookies(
        &self,
        _shared: &SharedNetworkState,
        cookies: &mut Cookies<'_, Login>,
        info: &NewClientInfo,
    ) -> Result<(), Text> {
        // Cookies are read-only during login (the protocol has no login-phase
        // store packet). Reading one here is the place to, e.g., validate a
        // transfer token before letting the client proceed.
        match cookies.get(VISITS_KEY).await {
            Ok(Some(bytes)) => {
                println!("[login] {} arrives with a visits cookie ({} bytes)", info.username, bytes.len());
            }
            Ok(None) => println!("[login] {} arrives with no visits cookie", info.username),
            Err(e) => println!("[login] failed to read cookie: {e:#}"),
        }

        Ok(())
    }

    async fn configure(
        &self,
        _shared: &SharedNetworkState,
        cookies: &mut Cookies<'_, Configuration>,
        info: &NewClientInfo,
    ) -> Result<(), Text> {
        // Read the current visit count (defaulting to 0), increment it, and
        // store it back on the client.
        let visits = match cookies.get(VISITS_KEY).await {
            Ok(Some(bytes)) => std::str::from_utf8(&bytes)
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0),
            Ok(None) => 0,
            Err(e) => {
                return Err(format!("failed to read visits cookie: {e}").color(Color::RED));
            }
        };

        let visits = visits + 1;
        println!("[configure] {} has now connected {visits} time(s)", info.username);

        if let Err(e) = cookies.set(VISITS_KEY, visits.to_string().as_bytes()).await {
            return Err(format!("failed to store visits cookie: {e}").color(Color::RED));
        }

        Ok(())
    }
}

fn setup(
    mut commands: Commands,
    server: Res<Server>,
    dimensions: Res<DimensionTypeRegistry>,
    biomes: Res<BiomeRegistry>,
) {
    let mut layer = LayerBundle::new(ident!("overworld"), &dimensions, &biomes, &server);

    for z in -5..5 {
        for x in -5..5 {
            layer.chunk.insert_chunk([x, z], UnloadedChunk::new());
        }
    }

    for z in -25..25 {
        for x in -25..25 {
            layer
                .chunk
                .set_block([x, SPAWN_Y, z], BlockState::GRASS_BLOCK);
        }
    }

    commands.spawn(layer);
}

fn init_clients(
    mut clients: Query<
        (
            &mut EntityLayerId,
            &mut VisibleChunkLayer,
            &mut VisibleEntityLayers,
            &mut Position,
            &mut GameMode,
        ),
        Added<Client>,
    >,
    layers: Query<Entity, (With<ChunkLayer>, With<EntityLayer>)>,
) {
    for (mut layer_id, mut visible_chunk_layer, mut visible_entity_layers, mut pos, mut game_mode) in
        &mut clients
    {
        let layer = layers.single().unwrap();

        layer_id.0 = layer;
        visible_chunk_layer.0 = layer;
        visible_entity_layers.0.insert(layer);
        pos.set([0.0, f64::from(SPAWN_Y) + 1.0, 0.0]);
        *game_mode = GameMode::Creative;
    }
}

/// Ask each joining client for the visits cookie (play-phase read).
fn greet_clients(mut clients: Query<&mut Client, Added<Client>>) {
    for mut client in &mut clients {
        client.request_cookie(VISITS_KEY);
    }
}

/// Surface the client's reply.
fn log_cookies(mut messages: MessageReader<CookieResponseMessage>) {
    for msg in messages.read() {
        match &msg.payload {
            Some(bytes) => {
                let value = String::from_utf8_lossy(bytes);
                println!("[play] cookie {} = {value}", msg.key);
            }
            None => println!("[play] cookie {} is not set", msg.key),
        }
    }
}

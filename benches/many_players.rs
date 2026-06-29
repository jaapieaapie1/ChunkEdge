use std::time::Duration;

use bevy_app::prelude::*;
use chunkedge::client::{ViewDistance, VisibleChunkLayer, VisibleEntityLayers};
use chunkedge::entity::{EntityLayerId, Position};
use chunkedge::keepalive::KeepaliveSettings;
use chunkedge::layer::chunk::UnloadedChunk;
use chunkedge::layer::LayerBundle;
use chunkedge::math::DVec3;
use chunkedge::network::NetworkPlugin;
use chunkedge::protocol::movement_flags::MovementFlags;
use chunkedge::protocol::packets::play::{MovePlayerPosRotC2s, SwingC2s};
use chunkedge::registry::{BiomeRegistry, DimensionTypeRegistry};
use chunkedge::testing::create_mock_client;
use chunkedge::{ident, ChunkPos, DefaultPlugins, Hand, Server, ServerSettings};
use chunkedge_server::CompressionThreshold;
use divan::Bencher;
use rand::RngExt;

#[divan::bench]
fn many_players(bencher: Bencher) {
    run_many_players(bencher, 3000, 16, 16);
}

#[divan::bench]
fn many_players_spread_out(bencher: Bencher) {
    run_many_players(bencher, 3000, 8, 200);
}

fn run_many_players(bencher: Bencher, client_count: usize, view_dist: u8, world_size: i32) {
    let mut app = App::new();

    app.insert_resource(ServerSettings {
        compression_threshold: CompressionThreshold(256),
        ..Default::default()
    });

    app.insert_resource(KeepaliveSettings {
        period: Duration::MAX,
    });

    app.add_plugins(DefaultPlugins.build().disable::<NetworkPlugin>());

    app.update(); // Initialize plugins.

    let _dimension_types = app.world().resource::<DimensionTypeRegistry>();
    let mut layer = LayerBundle::new(
        ident!("overworld"),
        app.world().resource::<DimensionTypeRegistry>(),
        app.world().resource::<BiomeRegistry>(),
        app.world().resource::<Server>(),
    );

    for z in -world_size..world_size {
        for x in -world_size..world_size {
            layer
                .chunk
                .insert_chunk(ChunkPos::new(x, z), UnloadedChunk::new());
        }
    }

    let layer = app.world_mut().spawn(layer).id();

    let mut clients = vec![];

    // Spawn a bunch of clients in at random initial positions in the instance.
    for i in 0..client_count {
        let (bundle, helper) = create_mock_client(format!("client_{i}"));

        let mut rng = rand::rng();
        let x = rng.random_range(-f64::from(world_size) * 16.0..=f64::from(world_size) * 16.0);
        let z = rng.random_range(-f64::from(world_size) * 16.0..=f64::from(world_size) * 16.0);

        let id = app
            .world_mut()
            .spawn((bundle, Position::new(DVec3::new(x, 64.0, z))))
            .id();

        let mut entity = app.world_mut().entity_mut(id);
        entity.get_mut::<VisibleChunkLayer>().unwrap().0 = layer;
        entity
            .get_mut::<VisibleEntityLayers>()
            .unwrap()
            .0
            .insert(layer);
        entity.get_mut::<EntityLayerId>().unwrap().0 = layer;
        entity.get_mut::<ViewDistance>().unwrap().set(view_dist);

        clients.push((id, helper));
    }

    let mut query = app.world_mut().query::<&mut Position>();

    app.update();

    for (_, helper) in &mut clients {
        helper.confirm_initial_pending_teleports();
    }

    app.update();

    bencher.bench_local(|| {
        let mut rng = rand::rng();

        // Move the clients around randomly. They'll cross chunk borders and cause
        // interesting things to happen.
        for (id, helper) in &mut clients {
            let pos = query.get(app.world_mut(), *id).unwrap().get();

            let offset = DVec3::new(
                rng.random_range(-1.0..=1.0),
                0.0,
                rng.random_range(-1.0..=1.0),
            );

            helper.send(&MovePlayerPosRotC2s {
                position: pos + offset,
                yaw: rng.random_range(0.0..=360.0),
                pitch: rng.random_range(0.0..=360.0),
                flags: MovementFlags::new().with_on_ground(true),
            });

            helper.send(&SwingC2s { hand: Hand::Main });
        }

        drop(rng);

        app.update(); // The important part.

        for (_, helper) in &mut clients {
            helper.clear_received();
        }
    });
}

use azalea_core::{
    aabb::AABB,
    direction::Direction,
    hit_result::{BlockHitResult, EntityHitResult, HitResult},
    position::Vec3,
};
use azalea_entity::{
    Attributes, Dead, LocalEntity, LookDirection, Physics, Position,
    dimensions::EntityDimensions,
    metadata::{ArmorStandMarker, Marker},
    view_vector,
};
use azalea_physics::{
    clip::{BlockShapeType, ClipContext, FluidPickType},
    collision::entity_collisions::{PhysicsQuery, get_entities},
};
use azalea_world::{Instance, InstanceContainer, InstanceName};
use bevy_ecs::prelude::*;
use derive_more::{Deref, DerefMut};

/// A component that contains the block or entity that the player is currently
/// looking at.
#[doc(alias("looking at", "looking at block", "crosshair"))]
#[derive(Component, Clone, Debug, Deref, DerefMut)]
pub struct HitResultComponent(HitResult);

#[allow(clippy::type_complexity)]
pub fn update_hit_result_component(
    mut commands: Commands,
    mut query: Query<
        (
            Entity,
            Option<&mut HitResultComponent>,
            &Position,
            &EntityDimensions,
            &LookDirection,
            &InstanceName,
            &Attributes,
        ),
        With<LocalEntity>,
    >,
    instance_container: Res<InstanceContainer>,
) {
    for (
        entity,
        hit_result_ref,
        position,
        dimensions,
        look_direction,
        world_name,
        attributes,
    ) in &mut query
    {
        let block_pick_range = attributes.block_interaction_range.calculate();

        let eye_position = position.up(dimensions.eye_height.into());

        let Some(world_lock) = instance_container.get(world_name) else {
            continue;
        };
        let world = world_lock.read();

        let hit_result = pick(PickOpts {
            look_direction: *look_direction,
            eye_position,
            world: &world,
            block_pick_range,
        });
        if let Some(mut hit_result_ref) = hit_result_ref {
            **hit_result_ref = hit_result;
        } else {
            commands
                .entity(entity)
                .insert(HitResultComponent(hit_result));
        }
    }
}

pub type PickableEntityQuery<'world, 'state, 'a> = Query<
    'world,
    'state,
    Option<&'a ArmorStandMarker>,
    (Without<Dead>, Without<Marker>, Without<LocalEntity>),
>;

pub struct PickOpts<'a> {
    look_direction: LookDirection,
    eye_position: Vec3,
    world: &'a Instance,
    block_pick_range: f64,
}

/// Get the block or entity that a player would be looking at if their eyes were
/// at the given direction and position.
///
/// If you need to get the block/entity the player is looking at right now, use
/// [`HitResultComponent`].
///
/// Also see [`pick_block`].
pub fn pick(opts: PickOpts<'_>) -> HitResult {
    // vanilla does extra math here to calculate the pick result in between ticks by
    // interpolating, but since clients can still only interact on exact ticks, that
    // isn't relevant for us.

    let mut max_range = opts.block_pick_range;

    let block_hit_result = pick_block(
        opts.look_direction,
        opts.eye_position,
        &opts.world.chunks,
        max_range,
    );

    filter_hit_result(
        HitResult::Block(block_hit_result),
        opts.eye_position,
        opts.block_pick_range,
    )
}

fn filter_hit_result(hit_result: HitResult, eye_position: Vec3, range: f64) -> HitResult {
    let location = hit_result.location();
    if !location.closer_than(eye_position, range) {
        let direction = Direction::nearest(location - eye_position);
        HitResult::new_miss(location, direction, location.into())
    } else {
        hit_result
    }
}

/// Get the block that a player would be looking at if their eyes were at the
/// given direction and position.
///
/// Also see [`pick`].
pub fn pick_block(
    look_direction: LookDirection,
    eye_position: Vec3,
    chunks: &azalea_world::ChunkStorage,
    pick_range: f64,
) -> BlockHitResult {
    let view_vector = view_vector(look_direction);
    let end_position = eye_position + (view_vector * pick_range);

    azalea_physics::clip::clip(
        chunks,
        ClipContext {
            from: eye_position,
            to: end_position,
            block_shape_type: BlockShapeType::Outline,
            fluid_pick_type: FluidPickType::None,
        },
    )
}

struct PickEntityOpts<'world, 'state, 'a, 'b> {
    source_entity: Entity,
    eye_position: Vec3,
    end_position: Vec3,
    world: &'a azalea_world::Instance,
    pick_range_squared: f64,
    predicate: &'a dyn Fn(Entity) -> bool,
    aabb: &'a AABB,
    physics_query: &'a PhysicsQuery<'world, 'state, 'b>,
}

// port of getEntityHitResult
fn pick_entity(opts: PickEntityOpts) -> Option<EntityHitResult> {
    let mut picked_distance_squared = opts.pick_range_squared;
    let mut result = None;

    for (candidate, candidate_aabb) in get_entities(
        opts.world,
        Some(opts.source_entity),
        opts.aabb,
        opts.predicate,
        opts.physics_query,
    ) {
        // TODO: if the entity is "REDIRECTABLE_PROJECTILE" then this should be 1.0.
        // azalea needs support for entity tags first for this to be possible. see
        // getPickRadius in decompiled minecraft source
        let candidate_pick_radius = 0.;
        let candidate_aabb = candidate_aabb.inflate_all(candidate_pick_radius);
        let clip_location = candidate_aabb.clip(opts.eye_position, opts.end_position);

        if candidate_aabb.contains(opts.eye_position) {
            if picked_distance_squared >= 0. {
                result = Some(EntityHitResult {
                    location: clip_location.unwrap_or(opts.eye_position),
                    entity: candidate,
                });
                picked_distance_squared = 0.;
            }
        } else if let Some(clip_location) = clip_location {
            let distance_squared = opts.eye_position.distance_squared_to(clip_location);
            if distance_squared < picked_distance_squared || picked_distance_squared == 0. {
                // TODO: don't pick the entity we're riding on
                // if candidate_root_vehicle == entity_root_vehicle {
                //     if picked_distance_squared == 0. {
                //         picked_entity = Some(candidate);
                //         picked_location = Some(clip_location);
                //     }
                // } else {
                result = Some(EntityHitResult {
                    location: clip_location,
                    entity: candidate,
                });
                picked_distance_squared = distance_squared;
            }
        }
    }

    result
}

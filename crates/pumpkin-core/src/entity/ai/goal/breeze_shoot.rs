use pumpkin_data::entity::EntityType;
use pumpkin_data::sound::{Sound, SoundCategory};
use pumpkin_util::math::vector3::Vector3;
use std::sync::{Arc, atomic::Ordering};

use crate::entity::{
    Entity,
    ai::goal::{Controls, Goal},
    mob::Mob,
    projectile::{
        ThrownItemEntity,
        wind_charge::{WIND_CHARGE_GRAVITY, WindChargeEntity},
    },
};

/// Ticks the breeze winds up before the charge leaves it.
const CHARGE_UP_TICKS: i32 = 20;
/// Ticks between the end of one shot and the start of the next wind-up.
const COOLDOWN_TICKS: i32 = 40;
/// Closest and farthest distance (in blocks) the breeze shoots from.
const MIN_RANGE: f64 = 4.0;
const MAX_RANGE: f64 = 15.0;
/// Blocks per tick the wind charge travels at.
const SHOOT_SPEED: f64 = 0.7;

/// Shoots breeze wind charges at the target, with a wind-up and a cooldown.
///
/// Approximation of vanilla's `Shoot` behaviour: vanilla also long-jumps and
/// strafes between volleys, which is not implemented here.
#[derive(Default)]
pub struct BreezeShootGoal {
    charge_time: i32,
    /// Entity tick count before which no new shot may start.
    cooldown_until: i32,
}

impl Goal for BreezeShootGoal {
    fn can_start(&mut self, mob: &dyn Mob) -> bool {
        let mob_entity = mob.get_mob_entity();
        // `can_start` only runs on full selector passes (every other tick), so the
        // cooldown is measured against the entity tick count rather than counted down.
        if mob_entity
            .living_entity
            .entity
            .tick_count
            .load(Ordering::Relaxed)
            < self.cooldown_until
        {
            return false;
        }
        let Some(target) = mob_entity.get_target() else {
            return false;
        };
        let target_entity = target.get_entity();
        if !target_entity.is_alive() {
            return false;
        }
        let distance = mob_entity
            .living_entity
            .entity
            .pos
            .load()
            .squared_distance_to_vec(&target_entity.pos.load())
            .sqrt();
        (MIN_RANGE..=MAX_RANGE).contains(&distance) && mob.has_line_of_sight(target_entity)
    }

    fn should_continue(&mut self, mob: &dyn Mob) -> bool {
        self.charge_time > 0
            && mob
                .get_mob_entity()
                .get_target()
                .is_some_and(|target| target.get_entity().is_alive())
    }

    fn start(&mut self, _mob: &dyn Mob) {
        self.charge_time = CHARGE_UP_TICKS;
    }

    fn stop(&mut self, _mob: &dyn Mob) {
        self.charge_time = 0;
    }

    fn should_run_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, mob: &dyn Mob) {
        let mob_entity = mob.get_mob_entity();
        let Some(target) = mob_entity.get_target() else {
            self.charge_time = 0;
            return;
        };

        mob_entity
            .look_control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .look_at_entity_with_range(&target, 30.0, 30.0);

        self.charge_time -= 1;
        if self.charge_time > 0 {
            return;
        }
        self.charge_time = 0;
        self.cooldown_until = mob_entity
            .living_entity
            .entity
            .tick_count
            .load(Ordering::Relaxed)
            + COOLDOWN_TICKS;

        if mob.has_line_of_sight(target.get_entity()) {
            shoot_wind_charge(&mob_entity.living_entity.entity, target.get_entity());
        }
    }

    fn controls(&self) -> Controls {
        Controls::LOOK
    }
}

fn shoot_wind_charge(breeze: &Entity, target: &Entity) {
    let world = breeze.world.load_full();
    let breeze_pos = breeze.pos.load();
    let spawn_pos = Vector3::new(
        breeze_pos.x,
        breeze_pos.y + f64::from(breeze.entity_dimension.load().height) * 0.5,
        breeze_pos.z,
    );
    let target_pos = target.pos.load();
    let aim = Vector3::new(
        target_pos.x - spawn_pos.x,
        target_pos.y + f64::from(target.entity_dimension.load().height) / 3.0 - spawn_pos.y,
        target_pos.z - spawn_pos.z,
    );
    let length = aim.length();
    if length < 1.0E-6 {
        return;
    }

    world.play_sound_fine(
        Sound::EntityBreezeShoot,
        SoundCategory::Hostile,
        &breeze_pos,
        1.5,
        1.0,
    );

    let base_entity = Entity::from_uuid(
        uuid::Uuid::new_v4(),
        world.clone(),
        spawn_pos,
        &EntityType::BREEZE_WIND_CHARGE,
    );
    let thrown = ThrownItemEntity::owned(base_entity, breeze, WIND_CHARGE_GRAVITY);
    thrown.entity.set_velocity(aim * (SHOOT_SPEED / length));
    world.spawn_entity(Arc::new(WindChargeEntity::new_breeze(thrown)));
}

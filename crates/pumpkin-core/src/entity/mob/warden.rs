use std::sync::{
    Arc, Weak,
    atomic::{AtomicI32, Ordering},
};

use pumpkin_data::BlockState;
use pumpkin_data::{
    damage::DamageType,
    effect::StatusEffect,
    entity::{EntityPose, EntityType},
    particle::Particle,
    potion::Effect,
    sound::{Sound, SoundCategory},
};
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::{GameMode, math::vector3::Vector3};

use crate::entity::{
    Entity, EntityBase,
    ai::goal::{
        Controls, Goal, active_target::ActiveTargetGoal, look_around::RandomLookAroundGoal,
        look_at_entity::LookAtEntityGoal, melee_attack::MeleeAttackGoal, swim::SwimGoal,
        wander_around::WanderAroundGoal,
    },
    mob::{Mob, MobEntity},
};
use crate::world::World;

const EMERGE_DURATION: i32 = 134;
const DARKNESS_DISPLAY_LIMIT: i32 = 200;
const DARKNESS_DURATION: i32 = 260;
const SONIC_BOOM_MIN_DISTANCE_SQ: f64 = 4.0;
const SONIC_BOOM_MAX_DISTANCE_SQ: f64 = 225.0;
const SONIC_BOOM_CHARGE_TICKS: i32 = 34;
const SONIC_BOOM_COOLDOWN_TICKS: i32 = 40;
const SONIC_BOOM_DAMAGE: f32 = 10.0;

pub struct WardenEntity {
    pub mob_entity: MobEntity,
    emerge_ticks: Arc<AtomicI32>,
}

struct EmergeGoal {
    emerge_ticks: Arc<AtomicI32>,
}

impl Goal for EmergeGoal {
    fn can_start(&mut self, _mob: &dyn Mob) -> bool {
        self.emerge_ticks.load(Ordering::Relaxed) > 0
    }

    fn start(&mut self, mob: &dyn Mob) {
        mob.get_mob_entity().steered().navigator().stop();
    }

    fn stop(&mut self, mob: &dyn Mob) {
        let entity = &mob.get_mob_entity().living_entity.entity;
        if entity.pose.load() == EntityPose::Emerging {
            entity.set_pose(EntityPose::Standing);
        }
    }

    fn tick(&mut self, _mob: &dyn Mob) {
        self.emerge_ticks.fetch_sub(1, Ordering::Relaxed);
    }

    fn should_run_every_tick(&self) -> bool {
        true
    }

    fn can_stop(&self) -> bool {
        false
    }

    fn controls(&self) -> Controls {
        Controls::MOVE | Controls::LOOK | Controls::JUMP | Controls::TARGET
    }
}

/// Vanilla `SonicBoom` behaviour: charge, then a 10 damage ranged hit that ignores armor and shields.
#[derive(Default)]
struct SonicBoomGoal {
    charge: i32,
    /// Entity `tick_count` at which the cooldown expires (counted in server ticks).
    cooldown_until: i32,
    done: bool,
}

impl SonicBoomGoal {
    fn in_range(mob: &dyn Mob) -> Option<Arc<dyn EntityBase>> {
        let target = mob.get_mob_entity().get_target()?;
        let target_entity = target.get_entity();
        if !target_entity.is_alive() {
            return None;
        }
        let distance_sq = mob
            .get_entity()
            .pos
            .load()
            .squared_distance_to_vec(&target_entity.pos.load());
        (SONIC_BOOM_MIN_DISTANCE_SQ..=SONIC_BOOM_MAX_DISTANCE_SQ)
            .contains(&distance_sq)
            .then_some(target)
    }
}

impl Goal for SonicBoomGoal {
    fn can_start(&mut self, mob: &dyn Mob) -> bool {
        if mob.get_entity().tick_count.load(Ordering::Relaxed) < self.cooldown_until {
            return false;
        }
        Self::in_range(mob).is_some_and(|target| mob.has_line_of_sight(target.get_entity()))
    }

    fn should_continue(&mut self, mob: &dyn Mob) -> bool {
        !self.done
            && Self::in_range(mob).is_some_and(|target| mob.has_line_of_sight(target.get_entity()))
    }

    fn start(&mut self, mob: &dyn Mob) {
        self.done = false;
        self.charge = SONIC_BOOM_CHARGE_TICKS;
        mob.get_mob_entity().steered().navigator().stop();
        mob.play_entity_sound(Sound::EntityWardenSonicCharge, 3.0, 1.0);
    }

    fn stop(&mut self, _mob: &dyn Mob) {
        self.charge = 0;
    }

    fn tick(&mut self, mob: &dyn Mob) {
        let Some(target) = Self::in_range(mob) else {
            return;
        };
        mob.get_mob_entity()
            .look_control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .look_at_entity_with_range(&target, 30.0, 30.0);

        self.charge -= 1;
        if self.charge > 0 {
            return;
        }

        let entity = mob.get_entity();
        let world = entity.world.load();
        let origin = entity.pos.load().add_raw(0.0, entity.get_eye_height(), 0.0);
        let target_entity = target.get_entity();
        let aim = target_entity
            .pos
            .load()
            .add_raw(0.0, target_entity.get_eye_height() * 0.5, 0.0);
        let step = aim.sub(&origin);
        let length = step.length();
        if length > 0.0 {
            let step = step.multiply(1.0 / length, 1.0 / length, 1.0 / length);
            for i in 1..length.floor() as i32 {
                let point = origin.add(&step.multiply(f64::from(i), f64::from(i), f64::from(i)));
                world.spawn_particle(
                    point,
                    Vector3::new(0.0, 0.0, 0.0),
                    0.0,
                    1,
                    Particle::SonicBoom,
                );
            }
        }
        world.play_sound_fine(
            Sound::EntityWardenSonicBoom,
            SoundCategory::Hostile,
            &entity.pos.load(),
            3.0,
            1.0,
        );
        // SONIC_BOOM has the bypasses_armor and bypasses_shield damage type tags.
        target.damage_with_context(
            target.as_ref(),
            SONIC_BOOM_DAMAGE,
            DamageType::SONIC_BOOM,
            None,
            Some(entity),
            Some(entity),
        );
        self.cooldown_until = entity.tick_count.load(Ordering::Relaxed) + SONIC_BOOM_COOLDOWN_TICKS;
        self.charge = 0;
        self.done = true;
    }

    fn should_run_every_tick(&self) -> bool {
        true
    }

    fn controls(&self) -> Controls {
        Controls::MOVE | Controls::LOOK
    }
}

impl WardenEntity {
    pub fn new(entity: Entity) -> Arc<Self> {
        let mob_entity = MobEntity::new(entity);
        let emerge_ticks = Arc::new(AtomicI32::new(0));
        let warden = Self {
            mob_entity,
            emerge_ticks: emerge_ticks.clone(),
        };
        let mob_arc = Arc::new(warden);
        let mob_weak: Weak<dyn Mob> = {
            let mob_arc: Arc<dyn Mob> = mob_arc.clone();
            Arc::downgrade(&mob_arc)
        };

        {
            let goal_selector = &mob_arc.mob_entity.goals_selector;

            goal_selector.add_goal(0, Box::new(EmergeGoal { emerge_ticks }));
            goal_selector.add_goal(0, Box::new(SwimGoal::default()));
            goal_selector.add_goal(3, Box::new(SonicBoomGoal::default()));
            goal_selector.add_goal(4, Box::new(MeleeAttackGoal::new(1.0, true)));
            goal_selector.add_goal(5, Box::new(WanderAroundGoal::new(0.5)));
            goal_selector.add_goal(
                6,
                LookAtEntityGoal::with_default(mob_weak.clone(), &EntityType::PLAYER, 8.0),
            );
            goal_selector.add_goal(7, Box::new(RandomLookAroundGoal::default()));

            let target_selector = &mob_arc.mob_entity.target_selector;
            target_selector.add_goal(
                1,
                ActiveTargetGoal::with_default(&mob_arc.mob_entity, &EntityType::PLAYER, true),
            );
        };

        mob_arc
    }

    pub fn emerge(&self) {
        let entity = &self.mob_entity.living_entity.entity;
        entity.set_pose(EntityPose::Emerging);
        self.emerge_ticks.store(EMERGE_DURATION, Ordering::Relaxed);
        entity.world.load().play_sound_fine(
            Sound::EntityWardenAgitated,
            SoundCategory::Hostile,
            &entity.pos.load(),
            5.0,
            1.0,
        );
    }

    pub fn apply_darkness_around(world: &World, position: Vector3<f64>, radius: f64) {
        for player in world.get_nearby_players(position, radius) {
            if !matches!(
                player.gamemode.load(),
                GameMode::Survival | GameMode::Adventure
            ) {
                continue;
            }
            let current = player.living_entity.get_effect(&StatusEffect::DARKNESS);
            if current.is_some_and(|effect| {
                effect.duration < 0 || effect.duration >= DARKNESS_DISPLAY_LIMIT
            }) {
                continue;
            }
            let darkness = Effect {
                effect_type: &StatusEffect::DARKNESS,
                duration: DARKNESS_DURATION,
                amplifier: 0,
                ambient: false,
                show_particles: false,
                show_icon: false,
                blend: true,
            };
            player.send_effect(&darkness);
            player.living_entity.add_effect(darkness);
        }
    }
}

impl Mob for WardenEntity {
    fn mob_is_pushable(&self) -> bool {
        let pose = self.mob_entity.living_entity.entity.pose.load();
        pose != EntityPose::Digging
            && pose != EntityPose::Emerging
            && self.mob_entity.living_entity.is_pushable()
    }

    fn mob_play_step_sound(&self, _pos: &BlockPos, _state: &BlockState) {
        self.play_entity_sound(Sound::EntityWardenStep, 10.0, 1.0);
    }

    fn get_mob_entity(&self) -> &MobEntity {
        &self.mob_entity
    }

    fn check_spawn_obstruction(&self, world: &World) -> bool {
        let entity = &self.mob_entity.living_entity.entity;
        let bounding_box = entity.bounding_box.load();
        !world.contains_any_liquid(bounding_box)
            && world.get_entities_at_box(&bounding_box).is_empty()
            && world.is_space_empty(bounding_box)
    }
}

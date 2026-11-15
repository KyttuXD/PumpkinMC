use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Weak};

use pumpkin_data::entity::EntityType;

use crate::entity::ai::behavior::hunting::{self, Hunt};
use crate::entity::ai::control::flying_move_control::FlyingMoveControl;
use crate::entity::ai::target_match::TargetMatch;
use crate::entity::mob::raider;
use crate::entity::{
    Entity, EntityBase,
    ai::goal::{
        look_around::RandomLookAroundGoal, look_at_entity::LookAtEntityGoal,
        melee_attack::MeleeAttackGoal, swim::SwimGoal, wander_around::WanderAroundGoal,
    },
    mob::{Mob, MobEntity},
};

const VEX_HUNTS: &[Hunt] = &[Hunt::new(3, TargetMatch::Type(&EntityType::PLAYER), true)];

pub struct VexEntity {
    pub mob_entity: MobEntity,
    /// Ticks left before the vex disappears. -1 means it never expires (vanilla limited life).
    life_ticks: AtomicI32,
}

impl VexEntity {
    pub fn new(entity: Entity) -> Arc<Self> {
        let mob_entity = MobEntity::new(entity);
        // Vanilla vexes fly: no gravity, and a flying move control instead of ground steering.
        mob_entity.living_entity.entity.set_has_no_gravity(true);
        *mob_entity
            .move_control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Box::new(FlyingMoveControl::new(20, true));
        let vex = Self {
            mob_entity,
            life_ticks: AtomicI32::new(-1),
        };
        let mob_arc = Arc::new(vex);
        let mob_weak: Weak<dyn Mob> = {
            let mob_arc: Arc<dyn Mob> = mob_arc.clone();
            Arc::downgrade(&mob_arc)
        };

        {
            let goal_selector = &mob_arc.mob_entity.goals_selector;

            goal_selector.add_goal(0, Box::new(SwimGoal::default()));
            goal_selector.add_goal(4, Box::new(MeleeAttackGoal::new(1.0, true)));
            goal_selector.add_goal(5, Box::new(WanderAroundGoal::new(1.0)));
            goal_selector.add_goal(
                6,
                LookAtEntityGoal::with_default(mob_weak.clone(), &EntityType::PLAYER, 8.0),
            );
            goal_selector.add_goal(7, Box::new(RandomLookAroundGoal::default()));

            let target_selector = &mob_arc.mob_entity.target_selector;
            target_selector.add_goal(1, raider::revenge_goal());
            // TODO: VexCopyOwnerTargetGoal at 2.
            hunting::apply(target_selector, &mob_arc.mob_entity, VEX_HUNTS);
        };

        mob_arc
    }
}

impl VexEntity {
    /// Vanilla `Vex.setLimitedLife`: the evoker gives summoned vexes 30 to 120 seconds.
    pub fn set_limited_life(&self, ticks: i32) {
        self.life_ticks.store(ticks, Ordering::Relaxed);
    }
}

impl Mob for VexEntity {
    fn get_mob_entity(&self) -> &MobEntity {
        &self.mob_entity
    }

    fn get_mob_gravity(&self) -> f64 {
        0.0
    }

    fn travel(&self, caller: &dyn EntityBase) -> bool {
        self.mob_entity
            .living_entity
            .travel_flying_with_speed(caller, 0.2);
        true
    }

    fn base_tick(&self) {
        let life = self.life_ticks.load(Ordering::Relaxed);
        if life > 0 {
            self.life_ticks.store(life - 1, Ordering::Relaxed);
            if life == 1 {
                self.mob_entity.living_entity.entity.remove();
            }
        }
    }
}

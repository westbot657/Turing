use crate::data::types::{Quat, Vec3};

pub trait GameObject {
    fn get_position(&self) -> Vec3;
    fn get_rotation(&self) -> Quat;
}

#[repr(C)]
#[derive(Clone)]
pub struct GameObjectData {
    position: Vec3,
    rotation: Quat,
    scale: Vec3,
}

impl GameObjectData {
    pub fn default() -> Self {
        GameObjectData {
            position: Vec3::zero(),
            rotation: Quat::identity(),
            scale: Vec3::splat(1.0)
        }
    }
}

#[repr(C)]
#[derive(Clone)]
pub struct ColorNote {
    data: GameObjectData
}

#[repr(C)]
#[derive(Clone)]
pub struct BombNote {
    data: GameObjectData
}

#[repr(C)]
#[derive(Clone)]
pub struct Arc {
    data: GameObjectData
}

#[repr(C)]
#[derive(Clone)]
pub struct ChainHeadNote {
    data: GameObjectData
}

#[repr(C)]
#[derive(Clone)]
pub struct ChainLinkNote {
    data: GameObjectData
}

#[repr(C)]
#[derive(Clone)]
pub struct ChainNote {
    data: GameObjectData
}

#[repr(C)]
#[derive(Clone)]
pub struct Wall {
    data: GameObjectData
}

#[repr(C)]
#[derive(Clone)]
pub struct Saber {
    data: GameObjectData
}

// "Player" as a *game object* refers to head position stuff
#[repr(C)]
#[derive(Clone)]
pub struct Player {
    data: GameObjectData
}




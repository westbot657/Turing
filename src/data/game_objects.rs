use glam::{Vec3, Quat};
use crate::data::types::Color;

pub trait GameObject {
    fn get_position(&self) -> Vec3;
    fn get_rotation(&self) -> Quat;
}


#[repr(C)]
#[derive(Debug, Clone)]
pub struct TransformData {
    position: Vec3,
    rotation: Quat,
    scale: Vec3,
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct BeatmapObjectData {
    color: Color,
    njs: f32
}


#[repr(C)]
#[derive(Clone)]
pub struct ColorNote {
    transform: TransformData,
    data: BeatmapObjectData,
}

#[repr(C)]
#[derive(Clone)]
pub struct BombNote {
    transform: TransformData,
    data: BeatmapObjectData,
}

#[repr(C)]
#[derive(Clone)]
pub struct Arc {
    transform: TransformData,
    data: BeatmapObjectData,
}

#[repr(C)]
#[derive(Clone)]
pub struct ChainHeadNote {
    transform: TransformData,
    data: BeatmapObjectData,
}

#[repr(C)]
#[derive(Clone)]
pub struct ChainLinkNote {
    transform: TransformData,
    data: BeatmapObjectData,
}

#[repr(C)]
#[derive(Clone)]
pub struct ChainNote {
    transform: TransformData,
    data: BeatmapObjectData,
}

#[repr(C)]
#[derive(Clone)]
pub struct Wall {
    transform: TransformData,
    data: BeatmapObjectData,
}



#[repr(C)]
#[derive(Clone)]
pub struct Saber {
    transform: TransformData,
    color: Color,
}

// "Player" as a *game object* refers to head position stuff
#[repr(C)]
#[derive(Clone)]
pub struct Player {
    transform: TransformData
}




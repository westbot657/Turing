use glam::{Vec3, Quat};

pub trait GameObject {
    fn get_position(&self) -> Vec3;
    fn get_rotation(&self) -> Quat;
}


#[repr(C)]
#[derive(Clone, Copy)]
pub struct ColorNote {
    reference_id: i32
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BombNote {
    reference_id: i32
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Arc {
    reference_id: i32
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ChainHeadNote {
    reference_id: i32
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ChainLinkNote {
    reference_id: i32
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ChainNote {
    reference_id: i32
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Wall {
    reference_id: i32
}



#[repr(C)]
#[derive(Clone, Copy)]
pub struct Saber {
    reference_id: i32
}

// "Player" as a *game object* refers to head position stuff
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Player {
    transform: TransformData
}




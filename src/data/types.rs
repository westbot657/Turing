

#[repr("C")]
#[derive(Copy, Clone)]
pub struct Vec3 {
    x: f32,
    y: f32,
    z: f32,
}

#[repr("C")]
#[derive(Copy, Clone)]
pub struct Vec4 {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
}

#[repr("C")]
#[derive(Copy, Clone)]
pub struct Quat {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
}

#[repr("C")]
#[derive(Copy, Clone)]
pub struct Color {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}


impl Vec3 {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Vec3 { x, y, z }
    }

    pub fn zero() -> Self {
        Vec3 { x: 0.0, y: 0.0, z: 0.0 }
    }

    pub fn splat(v: f32) -> Self {
        Vec3 { x: v, y: v, z: v }
    }
}

impl Quat {
    pub fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Quat { x, y, z, w }
    }

    pub fn identity() -> Self {
        Quat { x: 0.0, y: 0.0, z: 0.0, w: 1.0 }
    }
}



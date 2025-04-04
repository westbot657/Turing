use std::ffi::c_void;
use glam::{Vec3, Quat};

pub trait GameObject {
    fn get_position(&self) -> Vec3;
    fn get_rotation(&self) -> Quat;
}

macro_rules! ptr_type {
    ( $name:ident ) => {
        #[repr(C)]
        #[derive(Copy, Clone, Debug)]
        pub struct $name {
            ptr: usize
        }
    };
}

ptr_type!(ColorNote);
ptr_type!(BombNote);
ptr_type!(Arc);
ptr_type!(ChainHeadNote);
ptr_type!(ChainLinkNote);
ptr_type!(ChainNote);
ptr_type!(Wall);
ptr_type!(Saber);
ptr_type!(Player);




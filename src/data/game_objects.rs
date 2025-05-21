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
            pub ptr: usize
        }
    };
}

ptr_type!(Object);
ptr_type!(FuncRef);




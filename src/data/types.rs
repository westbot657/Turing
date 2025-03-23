use std::ops::Deref;

// Unique data types
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Color {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl Color {
    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Color {
        Color { r, g, b, a }
    }

    pub fn zero() -> Color {
        Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }
    }

    pub fn black() -> Color {
        Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }
    }

    pub fn white() -> Color {
        Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }
    }
}



// Wrapper types for better C# compat

#[repr(C)]
pub struct Boolean {
    pub b: u8,
}

impl Boolean {
    pub fn from_u8(b: u8) -> Boolean { Boolean { b } }
    pub fn from_bool(b: bool) -> Boolean { Boolean { b: if b {1} else {0} } }
}

impl Deref for Boolean {
    type Target = bool;
    fn deref(&self) -> Self::Target { self.b != 0 }
}


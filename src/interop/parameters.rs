use std::any::Any;
use std::ffi::{c_char, c_void, CStr, CString};
use crate::data::game_objects::*;
use crate::interop::parameters::params::{get_type, get_value, Param, ParamData, ParamType};

unsafe trait CSharpConvertible {
    type Raw;
    fn into_cs(self) -> Self::Raw;
    unsafe fn from_cs(raw: Self::Raw) -> Self;
    fn as_ptr(&self) -> *const c_void;
}

macro_rules! convertible {
    (
        $strct:ty => $raw:ty;
        to C: $self:tt => $to_cs:block
        from C: $name:tt => $from_cs:block
    ) => {
        convertible!(
            $strct => $raw;
            to C: $self => $to_cs
            from C: $name => $from_cs
            to ptr: $self => {
                Box::into_raw(Box::new($self.into_cs())) as *const c_void
            }
        );
    };
    (
        $strct:ty => $raw:ty;
        to C: $self:tt => $to_cs:block
        from C: $name:tt => $from_cs:block
        to ptr: $self2:tt => $to_ptr:block
    ) => {
        unsafe impl CSharpConvertible for $strct {

            type Raw = $raw;

            fn into_cs($self) -> Self::Raw $to_cs

            unsafe fn from_cs($name: Self::Raw) -> Self $from_cs

            fn as_ptr(&$self2) -> *const c_void $to_ptr


        }

    };
    ($strct:ty) => {
        convertible!(
            $strct => $strct;
            to C: self => { self }
            from C: raw => { raw }
        );
    }
}


convertible!(
    String => *const c_char;
    to C: self => {
        CString::new(self).unwrap().into_raw()
    }
    from C: raw => {
        CStr::from_ptr(raw).to_string_lossy().to_string()
    }
    to ptr: self => {
        self.clone().into_cs() as *const c_void
    }
);

convertible!(i8);
convertible!(i16);
convertible!(i32);
convertible!(i64);

convertible!(u8);
convertible!(u16);
convertible!(u32);
convertible!(u64);

convertible!(f32);
convertible!(f64);

convertible!(bool);

convertible!(ColorNote);
convertible!(BombNote);
convertible!(Arc);
convertible!(ChainHeadNote);
convertible!(ChainLinkNote);
convertible!(ChainNote);
convertible!(Wall);
convertible!(Saber);
convertible!(Player);

/// a list of either of these patterns, which can be mixed:
/// type_name type enum_constant
/// or
/// type_name type - data_name enum_constant
macro_rules! param_def {
    ( $( $ty:tt | $val:literal ),* $(,)? ) => {
        mod params {
            use crate::data::game_objects::*;
            use crate::interop::parameters::CSharpConvertible;

            #[allow(non_camel_case_types)]
            #[repr(u32)]
            #[derive(Debug, Copy, Clone)]
            pub enum ParamType {
                $( $ty = $val ),*
            }

            #[allow(non_snake_case)]
            #[repr(C)]
            #[derive(Clone, Copy)]
            pub union ParamData {
                $( $ty: <$ty as CSharpConvertible>::Raw ),*
            }

            #[allow(non_camel_case_types)]
            pub enum Param {
                $( $ty(ParamData) ),*
            }

            pub fn get_type(value: &Param) -> ParamType {
                match value {
                    $( Param::$ty(_) => ParamType::$ty, )*
                }
            }

            pub fn get_value(value: &Param) -> Box<dyn std::any::Any> {
                match value {
                    $( Param::$ty(x) => Box::new(x.clone()), )*
                }
            }
        }
    };
}

param_def! {

    i8     | 0,
    i16    | 1,
    i32    | 2,
    i64    | 3,
    u8     | 4,
    u16    | 5,
    u32    | 6,
    u64    | 7,
    f32    | 8,
    f64    | 9,
    bool   | 10,
    String | 11,

    ColorNote     | 100,
    BombNote      | 101,
    Arc           | 102,
    ChainHeadNote | 103,
    ChainLinkNote | 104,
    ChainNote     | 105,
    Wall          | 106,
    Saber         | 107,
    Player        | 108,

}

#[repr(C)]
pub struct CParam {
    pub data_type: ParamType,
    pub data: ParamData,
}

#[repr(C)]
pub struct CParams {
    param_count: u32,
    param_ptr_array_ptr: *mut *mut CParam,
}


impl CParam {
    pub fn new(data_type: ParamType, data: ParamData) -> Self {
        CParam { data_type, data }
    }
}

pub struct Parameters {
    params: Vec<(ParamType, ParamData)>,
}

impl Parameters {
    pub fn new() -> Self {
        Self {
            params: Vec::new(),
        }
    }

    pub fn push(&mut self, value: Param) {

        let typ = get_type(&value);

        let mut v = get_value(&value);
        let val = v.downcast_mut::<ParamData>().unwrap();

        let data = ParamData::try_from(*val).unwrap();
        self.params.push((
            typ,
            data
        ));

    }

    pub fn pack(self) -> CParams {

        let mut params = Vec::new();

        for (tp, param) in self.params {
            let c_param = CParam {
                data_type: tp,
                data: param,
            };
            params.push(c_param);
        }

        CParams {
            param_count: params.len() as u32,
            param_ptr_array_ptr: params.as_mut_ptr() as *mut *mut CParam,
        }

    }

}


use std::any::Any;
use std::ffi::{c_char, c_void, CStr, CString};
use std::fmt::{write, Display, Formatter};
use glam::{Quat, Vec3};
use crate::data::game_objects::*;
use crate::interop::parameters::params::{get_type, get_value, remap_data, Param, ParamData, ParamType, Parameters};

pub unsafe trait CSharpConvertible {
    type Raw;
    fn into_cs(self) -> Self::Raw;
    unsafe fn from_cs(raw: Self::Raw) -> Self;
}

macro_rules! convertible {
    (
        $strct:ty => $raw:ty;
        to C: $self:tt => $to_cs:block
        from C: $name:tt => $from_cs:block
    ) => {
        unsafe impl CSharpConvertible for $strct {

            type Raw = $raw;

            fn into_cs($self) -> Self::Raw $to_cs

            unsafe fn from_cs($name: Self::Raw) -> Self $from_cs


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

convertible!(Vec3);
convertible!(Quat);

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
    ( $ds:tt $( $ty:tt = $val:literal ),* $(,)? ) => {
        pub mod params {
            use crate::data::game_objects::*;
            use glam::*;
            use crate::interop::parameters::CSharpConvertible;
            use std::fmt::{write, Display, Formatter};

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
                $( pub $ty: <$ty as CSharpConvertible>::Raw ),*
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

            macro_rules! remap_data {
                ( $ds tp:tt, $ds dt:ident, $ds c_param: ident ) => {
                    match $ds tp {
                        $( ParamType::$ty => ParamData { $ty : *Box::from_raw($ds dt as *mut <$ty as CSharpConvertible>::Raw) } ),*
                    }
                };
            }

            pub(crate) use remap_data;

            pub struct Parameters {
                pub params: Vec<(ParamType, ParamData)>,
            }

            impl Display for Parameters {
                fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                    let mut vals = Vec::new();

                    for (typ, data) in &self.params {
                        let mut val;

                        unsafe {
                            match typ {
                                $( ParamType::$ty => val = format!("Param::{}({:?})", stringify!($ty), data.$ty) ),*
                            }
                        }

                        vals.push(format!("{:#?} : {}", typ, val));
                    }

                    write!(f, "Parameters: [{}]", vals.join(", "))
                }
            }

        }
    };
}

param_def! {$ // < this is here for internal macro creation, don't remove or replace it
    i8     = 0,
    i16    = 1,
    i32    = 2,
    i64    = 3,
    u8     = 4,
    u16    = 5,
    u32    = 6,
    u64    = 7,
    f32    = 8,
    f64    = 9,
    bool   = 10,
    String = 11,

    ColorNote     = 100,
    BombNote      = 101,
    Arc           = 102,
    ChainHeadNote = 103,
    ChainLinkNote = 104,
    ChainNote     = 105,
    Wall          = 106,
    Saber         = 107,
    Player        = 108,

    Vec3  = 200,
    Quat = 201

}

#[repr(C)]
#[derive(Debug)]
pub struct CParam {
    pub data_type: ParamType,
    pub data: *const ParamData,
}

#[repr(C)]
#[derive(Debug)]
pub struct CParams {
    param_count: u32,
    param_ptr_array_ptr: *mut *mut CParam,
}


impl CParam {
    pub fn new(data_type: ParamType, data: *const ParamData) -> Self {
        CParam { data_type, data }
    }
}



impl Parameters {
    pub fn new() -> Self {
        Self {
            params: Vec::new(),
        }
    }

    pub fn size(&self) -> usize {
        self.params.len()
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
                data: Box::into_raw(Box::new(param)),
            };
            params.push(Box::into_raw(Box::new(c_param)));
        }

        CParams {
            param_count: params.len() as u32,
            param_ptr_array_ptr: params.as_mut_ptr() as *mut *mut CParam,
        }

    }

    pub unsafe fn unpack(c_params: CParams) -> Self {

        let mut params = Vec::new();

        let c_param_ptrs = std::slice::from_raw_parts(
            c_params.param_ptr_array_ptr,
            c_params.param_count as usize,
        );

        for &param_ptr in c_param_ptrs {

            let c_param = &*param_ptr;

            let data_type = c_param.data_type;

            let raw_ptr = c_param.data;

            let raw_value = remap_data!(data_type, raw_ptr, c_param);

            params.push((data_type, raw_value));

        }

        Self {
            params
        }
    }

}

#[macro_export]
macro_rules! get_parameter {
    ( $params:expr, $t:tt, $index:expr) => {
        {
            if ($index >= $params.size()) {
                Err("index out of bounds".to_owned())
            } else {
                let raw = $params.params[$index];
                if raw.0 as u32 != crate::params::ParamType::$t as u32 {
                    Err("parameter at that position is not of expected type".to_owned())
                }
                else {
                    let p = crate::params::Param::$t(unsafe { raw.1 });
                    match p {
                        crate::params::Param::$t(x) => Ok(unsafe { $t::from_cs( x.$t ) }),
                        _ => Err("parameter at that position is not of expected type".to_owned())
                    }
                }
            }
        }
    };
}

#[macro_export]
macro_rules! get_return {
    ( $params:expr, $t:tt, $index:expr) => {
        {
            let raw = $params.params[$index];
            if raw.0 as u32 != crate::params::ParamType::$t as u32 {
                Err::<$t, String>("wrong value type was returned".to_owned())
            }
            else {
                let p = crate::params::Param::$t(unsafe { raw.1 });
                match p {
                    crate::params::Param::$t(x) => Ok(unsafe { $t::from_cs( x.$t ) }),
                    _ => Err("wrong value type was returned".to_owned())
                }
            }

        }
    };
}

#[macro_export]
macro_rules! push_parameter {
    ( $params:ident , $t:tt : $value:expr ) => {
        let csharp_value = crate::params::ParamData { $t: crate::interop::parameters::CSharpConvertible::into_cs($value) };
        $params.push(crate::params::Param::$t(csharp_value));
    };
}



#[cfg(test)]
mod parameters_tests {
    use crate::data::game_objects::ColorNote;
    use crate::interop::parameters::{CSharpConvertible, Parameters};
    use crate::interop::parameters::params::{Param, ParamData, ParamType};

    #[test]
    fn test_c_params() {

        let mut p = Parameters::new();

        let note = ColorNote { ptr: 0 };
        push_parameter!(p, ColorNote: note);

        let note2 = ColorNote { ptr: 1 };
        push_parameter!(p, ColorNote: note2);

        let f = 134.23f32;
        push_parameter!(p, f32: f);

        let s = "test string".to_owned();
        push_parameter!(p, String: s);

        println!("initial: {}", p);

        let packed = p.pack();

        println!("packed: {:?}", packed);

        let unpacked = unsafe { Parameters::unpack(packed) };

        println!("unpacked: {}", unpacked);

        let note_unpacked = get_parameter!(unpacked, ColorNote, 0);

        println!("note: {:?}", note_unpacked);

        let str_unpacked = get_parameter!(unpacked, String, 3);
        println!("str: {:?}", str_unpacked);

        let error = get_parameter!(unpacked, f64, 2);
        println!("error: {:?}", error);

    }

}


use std::any::Any;
use std::ffi::{c_char, c_void, CStr, CString};
use glam::{Quat, Vec2, Vec3, Vec4};
use crate::data::{game_objects::*, types::Color};
use crate::interop::parameters::params::{free_data, get_param_data, get_type, pack_value, remap_data, Param, ParamData, ParamDataRaw, ParamType, Parameters};

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


#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct RsString {
    pub ptr: *mut c_char,
}

convertible!(
    String => RsString;
    to C: self => {
        RsString { ptr: CString::new(self).unwrap().into_raw() }
    }
    from C: raw => {
        CString::from_raw(raw.ptr).to_string_lossy().to_string() // takes full ownership
        // CStr::from_ptr(raw.ptr).to_string_lossy().to_string() // copies data
    }
);

#[derive(Debug, Clone)]
pub struct InteropError {
    pub error_type: String,
    pub message: String,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct InteroperableError {
    pub type_ptr: *mut c_char,
    pub error_message: *mut c_char,
}

convertible!(
    InteropError => InteroperableError;
    to C: self => {
        InteroperableError {
            type_ptr: CString::new(self.error_type).unwrap().into_raw(),
            error_message: CString::new(self.message).unwrap().into_raw()
        }
    }
    from C: raw => {
        let et = CString::from_raw(raw.type_ptr).to_string_lossy().to_string();
        let m = CString::from_raw(raw.error_message).to_string_lossy().to_string();

        Self {
            error_type: et,
            message: m
        }
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

convertible!(Vec2);
convertible!(Vec3);
convertible!(Vec4);
convertible!(Quat);
convertible!(Color);

convertible!(ColorNote);
convertible!(BombNote);
convertible!(Arc);
convertible!(ChainHeadNote);
convertible!(ChainLinkNote);
convertible!(ChainNote);
convertible!(Wall);
convertible!(Saber);
convertible!(Player);


macro_rules! param_def {
    ( $ds:tt $( $ty:tt = $val:literal ),* $(,)? ) => {
        pub mod params {
            use crate::data::game_objects::*;
            use glam::*;
            use crate::interop::parameters::{CSharpConvertible, CParams, CParam};
            use std::fmt::{write, Display, Formatter};
            use crate::data::types::Color;
            use crate::InteropError;

            #[allow(non_camel_case_types)]
            #[repr(u32)]
            #[derive(Debug, Copy, Clone)]
            pub enum ParamType {
                $( $ty = $val ),*
            }

            #[allow(non_snake_case)]
            // #[derive(Clone, Copy)]
            pub union ParamData {
                $( pub $ty: std::mem::ManuallyDrop<$ty> ),*
            }

            #[allow(non_snake_case)]
            #[repr(C)]
            // #[derive(Clone)]
            pub union ParamDataRaw {
                $( pub $ty: <$ty as CSharpConvertible>::Raw ),*
            }



            #[allow(non_camel_case_types)]
            pub enum Param {
                $( $ty(ParamData) ),*
            }

            pub fn get_param_data(param: Param) -> ParamData {
                match param {
                    $( Param::$ty(d) => d ),*
                }
            }

            pub fn get_type(value: &Param) -> ParamType {
                match value {
                    $( Param::$ty(_) => ParamType::$ty, )*
                }
            }

            pub fn pack_value(tp: ParamType, value: ParamData) -> CParam {
                match tp {
                    $(
                        ParamType::$ty => {
                            CParam {
                                data_type: tp,
                                data: Box::into_raw(Box::new( ParamDataRaw { $ty: $ty::into_cs(unsafe { std::mem::ManuallyDrop::into_inner(value.$ty) }) }))
                            }
                        },
                    )*
                }

                // match value {
                //     $( Param::$ty(x) => Box::new($ty::to_cs(x)), )*
                // }
            }

            macro_rules! remap_data {
                ( $ds tp:tt, $ds dt:ident, $ds c_param: ident ) => {
                    match $ds tp {
                        $( ParamType::$ty => ParamData { $ty : std::mem::ManuallyDrop::new($ty::from_cs(*($ds dt as *mut <$ty as CSharpConvertible>::Raw))) } ),*
                    }
                };
            }

            macro_rules! free_data {
                ( $ds tp:tt, $ds dt:ident, $ds c_param: ident ) => {
                    match $ds tp {
                        $( ParamType::$ty => { $ty::from_cs(*Box::from_raw($ds dt as *mut <$ty as CSharpConvertible>::Raw)); } )*
                    }
                };
            }

            pub(crate) use {remap_data, free_data};

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

    Vec2  = 200,
    Vec3  = 201,
    Vec4  = 202,
    Quat  = 203,
    Color = 204,

    InteropError = 900

}

#[macro_export]
macro_rules! get_parameter {
    ( $params:expr, $t:tt, $index:expr) => {
        {
            if ($index >= $params.size()) {
                Err("index out of bounds".to_owned())
            } else {
                let raw = $params.params.remove($index);
                if raw.0 as u32 != crate::params::ParamType::$t as u32 {
                    Err("parameter at that position is not of expected type".to_owned())
                }
                else {
                    let p = crate::params::Param::$t(unsafe { raw.1 });
                    match p {
                        crate::params::Param::$t(x) => Ok(unsafe { std::mem::ManuallyDrop::into_inner(x.$t.clone()) }),
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
            let raw = $params.params.remove($index);
            if raw.0 as u32 != crate::params::ParamType::$t as u32 {
                Err::<std::mem::ManuallyDrop<$t>, String>("wrong value type was returned".to_owned())
            }
            else {
                let p = crate::params::Param::$t(unsafe { raw.1 });
                match p {
                    crate::params::Param::$t(x) => Ok(unsafe { x.$t }),
                    _ => Err("wrong value type was returned".to_owned())
                }
            }

        }
    };
}

#[repr(C)]
#[derive(Debug)]
pub struct CParam {
    pub data_type: ParamType,
    pub data: *const ParamDataRaw,
}

#[repr(C)]
#[derive(Debug)]
pub struct CParams {
    param_count: u32,
    param_ptr_array_ptr: *mut *mut CParam,
}


impl CParam {
    pub fn new(data_type: ParamType, data: *const ParamDataRaw) -> Self {
        CParam { data_type, data }
    }
}

impl Parameters {

    pub fn new() -> Self {
        Self {
            params: Vec::new()
        }
    }

    pub fn size(&self) -> usize {
        self.params.len()
    }

    pub fn push(&mut self, value: Param) {
        let typ = get_type(&value);

        let data = get_param_data(value);

        self.params.push((typ, data));

    }

    pub fn pack(self) -> CParams {
        let mut c_param_ptrs: Vec<*mut CParam> = self.params.into_iter()
            .map(|(tp, param)| {
                let c_param = pack_value(tp, param);

                Box::into_raw(Box::new(c_param))
            }).collect();

        let param_count = c_param_ptrs.len() as u32;

        let param_ptr_array = c_param_ptrs.as_mut_ptr();

        std::mem::forget(c_param_ptrs);

        CParams {
            param_count,
            param_ptr_array_ptr: param_ptr_array,
        }

    }

    /// unpacks the CParams struct into Parameters, and then deallocates the CParams object
    pub unsafe fn unpack(c_params: &CParams) -> Self {

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

        // Self::free(c_params); // is this safe? idk

        Self {
            params
        }
    }

    /// checks for an error at parameter index 0
    pub fn check_error(&mut self) -> Option<InteropError> {
        let res = get_parameter!(self, InteropError, 0);

        if let Ok(err) = res {
            Some(err)
        } else {
            None
        }

    }


    pub unsafe fn free_cs(c_params: CParams) {}

    /// free a CParams object. C# calls to this function to free it's RsParams struct instances
    pub unsafe fn free(c_params: &CParams) {

        // take ownership of the pointed data
        let c_params = Vec::from_raw_parts(
            c_params.param_ptr_array_ptr,
            c_params.param_count as usize,
            c_params.param_count as usize,
        );

        for param in c_params {
            // take ownership of each parameter
            let param = Box::from_raw(param);

            let data_type = param.data_type;
            let raw_ptr = param.data;
            // take ownership of each parameter's data
            free_data!(data_type, raw_ptr, c_param);

        }
    }

}


macro_rules! push_parameter {
    ( $params:expr, $typ:ident: $obj:expr ) => {
        $params.push(crate::params::Param::$typ( crate::params::ParamData { $typ: std::mem::ManuallyDrop::new($obj) } ))
    };
}


#[cfg(test)]
mod parameters_tests {
    use crate::data::game_objects::ColorNote;
    use crate::interop::parameters::{CSharpConvertible, InteropError, Parameters};
    use crate::interop::parameters::params::Param;

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

        let mut unpacked = unsafe { Parameters::unpack(&packed) };

        println!("unpacked: {}", unpacked);

        let note_unpacked = get_parameter!(unpacked, ColorNote, 0);

        println!("note: {:?}", note_unpacked);

        let str_unpacked = get_parameter!(unpacked, String, 2);
        println!("str: {:?}", str_unpacked);

        let error = get_parameter!(unpacked, f64, 1);
        println!("error: {:?}", error);

    }

    #[test]
    fn test_error() {
        let mut p = Parameters::new();

        let err = InteropError {
            error_type: "Test Error".to_string(),
            message: "hello".to_string(),
        };

        push_parameter!(p, InteropError: err);

        let packed = p.pack();

        println!("packed: {:?}", packed);

        let mut unpacked = unsafe { Parameters::unpack(&packed) };

        println!("unpacked: {}", unpacked);


        let error = unpacked.check_error();

        println!("error: {:?}", error);

    }

}
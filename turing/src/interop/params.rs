use smallvec::SmallVec;
use std::ffi::{CStr, CString, c_char, c_void};
use std::fmt::Display;
use std::marker::PhantomData;
use std::mem;
use std::ops::{Deref, DerefMut};
use anyhow::{anyhow, Result};
use glam::{Mat2, Mat3, Mat4, Quat, Vec2, Vec3, Vec4};
use num_enum::TryFromPrimitive;
use serde::{Deserialize, Serialize};
use crate::ExternalFunctions;
use crate::interop::string::RustString;
use crate::interop::types::ExtString;


#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, TryFromPrimitive)]
pub enum DataType {
    I8 = 1,
    I16 = 2,
    I32 = 3,
    I64 = 4,
    U8 = 5,
    U16 = 6,
    U32 = 7,
    U64 = 8,
    F32 = 9,
    F64 = 10,
    Bool = 11,
    RustString = 12,
    ExtString  = 13,
    Object = 14,
    RustError = 15,
    ExtError  = 16,
    Void = 17,
    Vec2 = 18,
    Vec3 = 19,
    RustVec4 = 20,
    ExtVec4  = 21,
    RustQuat = 22,
    ExtQuat  = 23,
    RustMat4 = 24,
    ExtMat4  = 25,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, TryFromPrimitive)]
pub enum FreeableDataType {
    ExtVec4  = DataType::ExtVec4 as u32,
    ExtQuat  = DataType::ExtQuat as u32,
    ExtMat4  = DataType::ExtMat4 as u32,
}

impl FreeableDataType {
    pub unsafe fn free_ptr(&self, ptr: *mut c_void) {
        unsafe {
            match self {
                FreeableDataType::ExtVec4 => { drop(Box::from_raw(ptr as *mut Vec4)); }
                FreeableDataType::ExtQuat => { drop(Box::from_raw(ptr as *mut Quat)); }
                FreeableDataType::ExtMat4 => { drop(Box::from_raw(ptr as *mut Mat4)); }
            }
        }
    }
}

trait InnerFfiType {
    const STRING: DataType;
    const ERROR: DataType;
    const VEC4: DataType;
    const QUAT: DataType;
    const MAT4: DataType;
}

struct RustTypes;
struct ExtTypes;

impl InnerFfiType for RustTypes {
    const STRING: DataType = DataType::RustString;
    const ERROR: DataType = DataType::RustError;
    const VEC4: DataType = DataType::RustVec4;
    const QUAT: DataType = DataType::RustQuat;
    const MAT4: DataType = DataType::RustMat4;
}

impl InnerFfiType for ExtTypes {
    const STRING: DataType = DataType::ExtString;
    const ERROR: DataType = DataType::ExtError;
    const VEC4: DataType = DataType::ExtVec4;
    const QUAT: DataType = DataType::ExtQuat;
    const MAT4: DataType = DataType::ExtMat4;
}


impl Display for DataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            DataType::I8 => "I8",
            DataType::I16 => "I16",
            DataType::I32 => "I32",
            DataType::I64 => "I64",
            DataType::U8 => "U8",
            DataType::U16 => "U16",
            DataType::U32 => "U32",
            DataType::U64 => "U64",
            DataType::F32 => "F32",
            DataType::F64 => "F64",
            DataType::Bool => "BOOL",
            DataType::RustString => "RUST_STRING",
            DataType::ExtString => "EXT_STRING",
            DataType::Object => "OBJECT",
            DataType::RustError => "RUST_ERROR",
            DataType::ExtError => "EXT_ERROR",
            DataType::Void => "VOID",
            DataType::Vec2 => "VEC2",
            DataType::Vec3 => "VEC3",
            DataType::RustVec4 => "RUST_VEC4",
            DataType::ExtVec4 => "EXT_VEC4",
            DataType::RustQuat => "RUST_QUAT",
            DataType::ExtQuat => "EXT_QUAT",
            DataType::RustMat4 => "RUST_MAT4",
            DataType::ExtMat4 => "EXT_MAT4",
        };
        write!(f, "{}", s)
    }
}

impl DataType {
    /// Checks if the ParamType is valid.
    pub fn is_valid(&self) -> bool {
        DataType::try_from(*self as u32).is_ok()
    }

    pub fn is_valid_param_type(&self) -> bool {
        !matches!(
            self,
            DataType::RustError
            | DataType::ExtError
            | DataType::Void
        )
    }

    pub fn is_valid_return_type(&self) -> bool {
        !matches!(
            self,
            DataType::RustError
            | DataType::ExtError
        )
    }


}

#[derive(Debug, Clone, PartialEq)]
pub enum Param {
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    Bool(bool),
    String(CString),
    Object(*const c_void),
    Error(RustString),
    Void,
    Vec2(Vec2),
    Vec3(Vec3),
    Vec4(Vec4),
    Quat(Quat),
    Mat4(Mat4),
}


impl Param {

    pub fn to_rs_param(self) -> FfiParam {
        self.into_param_inner::<RustTypes>()
    }
    pub fn to_ext_param(self) -> FfiParam {
        self.into_param_inner::<ExtTypes>()
    }
    
    #[rustfmt::skip]
    fn into_param_inner<T: InnerFfiType>(self) -> FfiParam {
        match self {
            Param::I8(x) => FfiParam { type_id: DataType::I8, value: RawParam { i8: x } },
            Param::I16(x) => FfiParam { type_id: DataType::I16, value: RawParam { i16: x } },
            Param::I32(x) => FfiParam { type_id: DataType::I32, value: RawParam { i32: x } },
            Param::I64(x) => FfiParam { type_id: DataType::I64, value: RawParam { i64: x } },
            Param::U8(x) => FfiParam { type_id: DataType::U8, value: RawParam { u8: x } },
            Param::U16(x) => FfiParam { type_id: DataType::U16, value: RawParam { u16: x } },
            Param::U32(x) => FfiParam { type_id: DataType::U32, value: RawParam { u32: x } },
            Param::U64(x) => FfiParam { type_id: DataType::U64, value: RawParam { u64: x } },
            Param::F32(x) => FfiParam { type_id: DataType::F32, value: RawParam { f32: x } },
            Param::F64(x) => FfiParam { type_id: DataType::F64, value: RawParam { f64: x } },
            Param::Bool(x) => FfiParam { type_id: DataType::Bool, value: RawParam { bool: x } },
            // allocated via CString, must be freed via CString::from_raw
            Param::String(x) => FfiParam { type_id: T::STRING, value: RawParam { string: CString::new(x).unwrap().into_raw() } },
            Param::Object(x) => FfiParam { type_id: DataType::Object, value: RawParam { object: x } },
            Param::Error(x) => FfiParam { type_id: T::ERROR, value: RawParam { error: x.into_cstring().into_raw() } },
            Param::Void => FfiParam { type_id: DataType::Void, value: RawParam { void: () } },
            Param::Vec2(v) => FfiParam { type_id: DataType::Vec2, value: RawParam { vec2: v } },
            Param::Vec3(v) => FfiParam { type_id: DataType::Vec3, value: RawParam { vec3: v } },
            Param::Vec4(v) => FfiParam { type_id: T::VEC4, value: RawParam { vec4: Box::into_raw(Box::new(v)) } },
            Param::Quat(q) => FfiParam { type_id: T::QUAT, value: RawParam { quat: Box::into_raw(Box::new(q)) } },
            Param::Mat4(m) => FfiParam { type_id: T::MAT4, value: RawParam { mat4: Box::into_raw(Box::new(m)) } },
        }
    }

    pub fn to_result<T: FromParam>(self) -> Result<T> {
        T::from_param(self)
    }

}

pub trait FromParam: Sized {
    fn from_param(param: Param) -> Result<Self>;
}
macro_rules! deref_param {
    ( $param:expr, $case:tt ) => {
        match $param {
            Param::$case(v) => Ok(v),
            Param::Error(e) => Err(anyhow!("{}", e)),
            _ => Err(anyhow!("Incorrect data type"))
        }
    };
    ( $tp:ty => $case:tt ) => {
        impl FromParam for $tp {
            fn from_param(param: Param) -> Result<Self> {
                deref_param!(param, $case)
            }
        }
    }
}
deref_param! { i8     => I8     }
deref_param! { i16    => I16    }
deref_param! { i32    => I32    }
deref_param! { i64    => I64    }
deref_param! { u8     => U8     }
deref_param! { u16    => U16    }
deref_param! { u32    => U32    }
deref_param! { u64    => U64    }
deref_param! { f32    => F32    }
deref_param! { f64    => F64    }
deref_param! { bool   => Bool   }
deref_param! { CString => String }
deref_param! { Vec2   => Vec2   }
deref_param! { Vec3   => Vec3   }
deref_param! { Vec4   => Vec4   }
deref_param! { Quat   => Quat   }
deref_param! { Mat4   => Mat4   }
impl FromParam for () {
    fn from_param(param: Param) -> Result<Self> {
        match param {
            Param::Void => Ok(()),
            Param::Error(e) => Err(anyhow!("{}", e)),
            _ => Err(anyhow!("Incorrect data type"))
        }
    }
}
impl FromParam for String {
    fn from_param(param: Param) -> Result<Self> {
        match param {
            Param::String(s) => Ok(s.to_string_lossy().into_owned()),
            Param::Error(e) => Err(anyhow!("{}", e)),
            _ => Err(anyhow!("Incorrect data type"))
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Params {
    // SmallVec will spill onto the heap if there are more than 4 params
    pub(crate) params: SmallVec<[Param; 4]>,
}

impl Params {
    pub fn new() -> Self {
        Self {
            params: Default::default(),
        }
    }

    pub fn of_size(size: u32) -> Self {
        Self {
            params: SmallVec::with_capacity(size as usize),
        }
    }

    pub fn push(&mut self, param: Param) {
        self.params.push(param);
    }

    pub fn set(&mut self, index: u32, param: Param) {
        self.params[index as usize] = param;
    }

    pub fn get(&self, idx: usize) -> Option<&Param> {
        self.params.get(idx)
    }

    pub fn len(&self) -> u32 {
        self.params.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }

    pub fn to_ffi<Ext>(self) -> FfiParams<Ext> where Ext: ExternalFunctions {
        FfiParams::from_params(self.params)
    }
}

impl IntoIterator for Params {
    type Item = Param;
    type IntoIter = smallvec::IntoIter<[Param; 4]>;

    fn into_iter(self) -> Self::IntoIter {
        self.params.into_iter()
    }
}

impl Deref for Params {
    type Target = SmallVec<[Param; 4]>;

    fn deref(&self) -> &Self::Target {
        &self.params
    }
}

impl DerefMut for Params {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.params
    }
}

/// C repr of ffi data
#[repr(C)]
pub union RawParam {
    i8: i8,
    i16: i16,
    i32: i32,
    i64: i64,
    u8: u8,
    u16: u16,
    u32: u32,
    u64: u64,
    f32: f32,
    f64: f64,
    bool: bool,
    string: *const c_char,
    object: *const c_void,
    error: *const c_char,
    void: (),
    vec2: Vec2,
    vec3: Vec3,
    vec4: *const Vec4,
    quat: *const Quat,
    mat2: *const Mat2,
    mat3: *const Mat3,
    mat4: *const Mat4,
}

/// C tagged repr of ffi data
#[repr(C)]
pub struct FfiParam {
    pub type_id: DataType,
    pub value: RawParam,
}

/// A collection of FfiParams.
/// Can be converted to/from Params.
/// Will free allocated resources on drop.
pub struct FfiParams<Ext: ExternalFunctions> {
    pub params: SmallVec<[FfiParam; 4]>,
    marker: PhantomData<Ext>,
}


impl<Ext> Drop for FfiParams<Ext> where Ext: ExternalFunctions {
    fn drop(&mut self) {
        if self.params.is_empty() {
            return;
        }

        // Convert the inner params without moving fields out of `self`
        let params: Result<_> = mem::take(&mut self.params)
            .into_iter()
            .map(|p| p.into_param::<Ext>())
            .collect();

        if let Ok(params) = params {
            // drop the converted Params so any allocated resources are freed
            drop(Params { params });
        }
    }
}

impl<Ext> Default for FfiParams<Ext> where Ext: ExternalFunctions {
    fn default() -> Self {
        Self::empty()
    }
}

impl<Ext> FfiParams<Ext> where Ext: ExternalFunctions {
    pub fn empty() -> Self {
        Self { params: SmallVec::new(), marker: PhantomData }
    }

    /// Creates FfiParams from a vector of Params.
    pub fn from_params<T>(params: T) -> Self where T: IntoIterator<Item = Param> {
        let ffi_params = params.into_iter().map(|p| p.to_rs_param()).collect();
        Self { params: ffi_params, marker: PhantomData }
    }

    /// Creates FfiParams from an FfiParamArray with 'static lifetime.
    pub fn from_ffi_array(array: FfiParamArray<'static>) -> Result<Self> {
        if array.ptr.is_null() || array.count == 0 {
            return Ok(Self::default());
        }
        unsafe {
            let raw_vec =
                std::ptr::slice_from_raw_parts_mut(array.ptr as *mut FfiParam, array.count as usize);
            let raw_vec = Box::from_raw(raw_vec);

            // take ownership of the raw_vec
            let owned = raw_vec.into_vec();


            Ok(Self {
                params: SmallVec::from_vec(owned),
                marker: PhantomData,
            })
        }
    }

    /// Converts FfiParams back into Params.
    pub fn to_params(mut self) -> Result<Params> {
        // take the inner SmallVec to avoid moving a field out of a Drop type
        let params: Result<_> = mem::take(&mut self.params)
            .into_iter()
            .map(|p| p.into_param::<Ext>())
            .collect();
        Ok(Params { params: params? })
    }

    /// Creates an FfiParamArray from the FfiParams.
    pub fn as_ffi_array<'a>(&'a self) -> FfiParamArray<'a> {
        FfiParamArray::<'a> {
            count: self.params.len() as u32,
            ptr: self.params.as_ptr(),
            marker: PhantomData,
        }
    }

    /// Leaks the FfiParams into an FfiParamArray with 'static lifetime.
    /// Caller is responsible for freeing the memory. 
    /// Freeing is possible by converting back via FfiParams::from_ffi_array and dropping the FfiParams.
    pub fn leak(mut self) -> FfiParamArray<'static> {
        let boxed_slice = mem::take(&mut self.params).into_boxed_slice();
        let count = boxed_slice.len() as u32;
        let ptr = Box::into_raw(boxed_slice) as *const FfiParam;

        FfiParamArray {
            count,
            ptr,
            marker: PhantomData,
        }
    }
}

/// C repr of an array of FfiParams.
/// Does not own the memory, just a view.
/// Can be converted to Params.
#[repr(C)]
#[derive(Clone)]
pub struct FfiParamArray<'a> {
    pub count: u32,
    pub ptr: *const FfiParam,
    pub marker: PhantomData<&'a ()>,
}

impl<'a> FfiParamArray<'a> {
    /// Creates an empty FfiParamArray.
    pub fn empty() -> Self {
        Self {
            count: 0,
            ptr: std::ptr::null(),
            marker: PhantomData,
        }
    }

    /// Clones the parameters from the FfiParamArray without taking ownership.
    /// Does not free any memory.
    pub fn as_params<Ext: ExternalFunctions>(&'a self) -> Result<Params> {
        if self.ptr.is_null() || self.count == 0 {
            return Ok(Params::default());
        }

        unsafe {
            let raw_slice =
                std::ptr::slice_from_raw_parts(self.ptr as *mut FfiParam, self.count as usize);
            let slice = &*raw_slice;

            let result = slice.iter()
                .map(|p| p.as_param::<Ext>())
                .collect::<Result<_>>()?;
            Ok(Params { params: result })
        }
    }

    pub fn as_slice(&'a self) -> &'a [FfiParam] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.count as usize) }
    }
}

impl FfiParam {
    pub fn into_param<Ext: ExternalFunctions>(self) -> Result<Param> {
        macro_rules! unbox {
            ($tok:tt) => { unsafe { *Box::from_raw(self.value.$tok as *mut _) } };
        }
        macro_rules! deref {
            ( $typ:tt ( $tok:tt ) ) => {
                {
                    let x = unsafe { &*self.value.$tok }.clone();
                    unsafe { <Ext>::free_of_type(self.value.$tok as *mut c_void, FreeableDataType::$typ) };
                    x
                }
            };
        }
        Ok(match self.type_id {
            DataType::I8 => Param::I8(unsafe { self.value.i8 }),
            DataType::I16 => Param::I16(unsafe { self.value.i16 }),
            DataType::I32 => Param::I32(unsafe { self.value.i32 }),
            DataType::I64 => Param::I64(unsafe { self.value.i64 }),
            DataType::U8 => Param::U8(unsafe { self.value.u8 }),
            DataType::U16 => Param::U16(unsafe { self.value.u16 }),
            DataType::U32 => Param::U32(unsafe { self.value.u32 }),
            DataType::U64 => Param::U64(unsafe { self.value.u64 }),
            DataType::F32 => Param::F32(unsafe { self.value.f32 }),
            DataType::F64 => Param::F64(unsafe { self.value.f64 }),
            DataType::Bool => Param::Bool(unsafe { self.value.bool }),
            DataType::RustString => Param::String(unsafe {
                CString::from_raw(self.value.string as *mut c_char)
            }),
            DataType::ExtString => {
                Param::String(unsafe { ExtString::<Ext>::from(self.value.string).to_owned() })
            }
            DataType::Object => Param::Object(unsafe { self.value.object }),
            DataType::RustError => Param::Error(unsafe {
                CString::from_raw(self.value.error as *mut c_char)
                    .into()
            }),
            DataType::ExtError => {
                Param::Error(unsafe { ExtString::<Ext>::from(self.value.error).into_string().into() })
            }
            DataType::Void => Param::Void,
            DataType::Vec2 => Param::Vec2(unsafe { self.value.vec2 }),
            DataType::Vec3 => Param::Vec3(unsafe { self.value.vec3 }),
            DataType::RustVec4 => Param::Vec4(unbox!(vec4)),
            DataType::ExtVec4 => Param::Vec4(deref!(ExtVec4(vec4))),
            DataType::RustQuat => Param::Quat(unbox!(quat)),
            DataType::ExtQuat => Param::Quat(deref!(ExtQuat(quat))),
            DataType::RustMat4 => Param::Mat4(unbox!(mat4)),
            DataType::ExtMat4 => Param::Mat4(deref!(ExtMat4(mat4))),
        })
    }

    pub fn as_param<Ext: ExternalFunctions>(&self) -> Result<Param> {
        macro_rules! unbox {
            ($tok:tt) => { deref!($tok) };
        }
        macro_rules! deref {
            ($tok:tt) => { unsafe { &*self.value.$tok }.clone() };
        }
        Ok(match self.type_id {
            DataType::I8 => Param::I8(unsafe { self.value.i8 }),
            DataType::I16 => Param::I16(unsafe { self.value.i16 }),
            DataType::I32 => Param::I32(unsafe { self.value.i32 }),
            DataType::I64 => Param::I64(unsafe { self.value.i64 }),
            DataType::U8 => Param::U8(unsafe { self.value.u8 }),
            DataType::U16 => Param::U16(unsafe { self.value.u16 }),
            DataType::U32 => Param::U32(unsafe { self.value.u32 }),
            DataType::U64 => Param::U64(unsafe { self.value.u64 }),
            DataType::F32 => Param::F32(unsafe { self.value.f32 }),
            DataType::F64 => Param::F64(unsafe { self.value.f64 }),
            DataType::Bool => Param::Bool(unsafe { self.value.bool }),
            DataType::RustString => Param::String(unsafe {
                CStr::from_ptr(self.value.string)
                    .to_owned()
            }),
            DataType::ExtString => {
                Param::String(unsafe { ExtString::<Ext>::from(self.value.string).to_owned() })
            }
            DataType::Object => Param::Object(unsafe { self.value.object }),
            DataType::RustError => Param::Error(unsafe {
                CStr::from_ptr(self.value.error)
                    .into()
            }),
            DataType::ExtError => {
                Param::Error(unsafe { ExtString::<Ext>::from(self.value.error).into_cstring().into() })
            }
            DataType::Void => Param::Void,
            DataType::Vec2 => Param::Vec2(unsafe { self.value.vec2 }),
            DataType::Vec3 => Param::Vec3(unsafe { self.value.vec3 }),
            DataType::RustVec4 => Param::Vec4(unbox!(vec4)),
            DataType::ExtVec4 => Param::Vec4(deref!(vec4)),
            DataType::RustQuat => Param::Quat(unbox!(quat)),
            DataType::ExtQuat => Param::Quat(deref!(quat)),
            DataType::RustMat4 => Param::Mat4(unbox!(mat4)),
            DataType::ExtMat4 => Param::Mat4(deref!(mat4)),
        })
    }

}

impl From<Param> for FfiParam {
    fn from(value: Param) -> Self {
        value.to_rs_param()
    }
}


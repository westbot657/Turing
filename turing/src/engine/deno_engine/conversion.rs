use deno_core::{FromV8, ToV8, op2, v8::BigInt};

use crate::interop::params::Param;

pub struct TuringFunctionDispatch(String, Vec<Param>);

impl<'a> ToV8<'a> for Param {
    type Error = std::convert::Infallible;

    fn to_v8<'i>(
        self,
        scope: &mut deno_core::v8::PinScope<'a, 'i>,
    ) -> Result<deno_core::v8::Local<'a, deno_core::v8::Value>, <Param as ToV8::<'a>>::Error> {
        match self {
            Param::String(s) => s.to_v8(scope).map_err(|e| e.into()),
            Param::I8(i) => i.to_v8(scope).map_err(|e| e.into()),
            Param::I16(i) => i.to_v8(scope).map_err(|e| e.into()),
            Param::I32(i) => i.to_v8(scope).map_err(|e| e.into()),
            Param::I64(i) => Ok(BigInt::new_from_i64(scope, i).cast()),
            Param::U8(u) => u.to_v8(scope).map_err(|e| e.into()),
            Param::U16(u) => u.to_v8(scope).map_err(|e| e.into()),
            Param::U32(u) => u.to_v8(scope).map_err(|e| e.into()),
            Param::U64(u) => Ok(BigInt::new_from_u64(scope, u).cast()),
            Param::F32(f) => f.to_v8(scope).map_err(|e| e.into()),
            Param::F64(f) => Ok(deno_core::v8::Number::new(scope, f as f64).cast()),
            Param::Bool(b) => b.to_v8(scope).map_err(|e| e.into()),
            Param::Object(o) => {
                todo!()
            },
            Param::Error(_) => todo!(),
            Param::Void => todo!(),
        }
    }
}
impl<'a> FromV8<'a> for Param {
    type Error = std::convert::Infallible;
    
    fn from_v8<'i>(
        scope: &mut deno_core::v8::PinScope<'a, 'i>,
        value: deno_core::v8::Local<'a, deno_core::v8::Value>,
      ) -> Result<Self, <Self as FromV8::<'a>>::Error> {
        if value.is_string() {
            let s = String::from_v8(scope, value).unwrap();
            Ok(Param::String(s))
        } else if value.is_big_int() {
            let bi = deno_core::v8::Local::<BigInt>::try_from(value).unwrap();
            let u = bi.u64_value().0;
            Ok(Param::U64(u))
        } else {
            unimplemented!()
        }
    }


}

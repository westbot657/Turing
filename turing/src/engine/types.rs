use anyhow::anyhow;
use crate::interop::params::{DataType, FfiParam, FfiParamArray};

pub type ScriptCallback = extern "C" fn(FfiParamArray) -> FfiParam;


#[derive(Clone)]
pub struct ScriptFnMetadata {
    pub capability: Option<String>,
    pub callback: ScriptCallback,
    pub param_types: Vec<DataType>,
    pub param_type_names: Vec<(String, String)>,
    pub return_type: Vec<DataType>,
    pub return_type_names: Vec<String>,
    pub doc_comment: Option<String>,
}

impl ScriptFnMetadata {
    pub fn new(capability: Option<String>, callback: ScriptCallback, doc_comment: Option<String>) -> Self {
        Self {
            capability,
            callback,
            param_types: Vec::new(),
            param_type_names: Vec::new(),
            return_type: Vec::new(),
            return_type_names: Vec::new(),
            doc_comment,
        }
    }

    /// May error if DataType is not a valid parameter type
    pub fn add_param_type(&mut self, p: DataType, param_name: impl ToString) -> anyhow::Result<&mut Self> {
         if !p.is_valid_param_type() {
            return Err(anyhow!("DataType '{}' is not a valid parameter type", p))
         }
        self.param_types.push(p);
        self.param_type_names.push((param_name.to_string(), p.as_spec_param_type()?.to_string()));

        Ok(self)
    }

    /// May error if DataType is not a valid parameter type
    pub fn add_param_type_named(&mut self, p: DataType, param_name: String, type_name: String) -> anyhow::Result<&mut Self> {
        if !p.is_valid_param_type() {
            return Err(anyhow!("DataType '{}' is not a valid parameter type", p))
        }
        self.param_types.push(p);
        self.param_type_names.push((param_name, type_name));

        Ok(self)
    }

    /// May error if DataType is not a valid return type
    pub fn add_return_type(&mut self, r: DataType) -> anyhow::Result<&mut Self> {
        if !r.is_valid_return_type() {
            return Err(anyhow!("DataType '{}' is not a valid return type", r))
        }
        self.return_type.push(r);
        self.return_type_names.push(r.as_spec_return_type()?.to_string());
        Ok(self)
    }

    /// May error if DataType is not a valid return type
    pub fn add_return_type_named(&mut self, r: DataType, type_name: String) -> anyhow::Result<&mut Self> {
        if !r.is_valid_return_type() {
            return Err(anyhow!("DataType '{}' is not a valid return type", r))
        }
        self.return_type.push(r);
        self.return_type_names.push(type_name);
        Ok(self)
    }


}

impl DataType {
    fn as_spec_param_type(&self) -> anyhow::Result<&'static str> {
        Ok(match self {
            DataType::I8 => "i8",
            DataType::I16 => "i16",
            DataType::I32 => "i32",
            DataType::I64 => "i64",
            DataType::U8 => "u8",
            DataType::U16 => "u16",
            DataType::U32 => "u32",
            DataType::U64 => "u64",
            DataType::F32 => "f32",
            DataType::F64 => "f64",
            DataType::Bool => "bool",
            DataType::RustString | DataType::ExtString => "&str",
            DataType::Object => return Err(anyhow!("Cannot derive type name from 'Object'")),
            DataType::RustError | DataType::ExtError => return Err(anyhow!("Error is not a valid param type")),
            DataType::Void => return Err(anyhow!("Void is not a valid param type")),
            DataType::Vec2 => "Vec2",
            DataType::Vec3 => "Vec3",
            DataType::RustVec4 | DataType::ExtVec4 => "Vec4",
            DataType::RustQuat | DataType::ExtQuat => "Quat",
            DataType::RustMat4 | DataType::ExtMat4 => "Mat4",
        })
    }

    fn as_spec_return_type(&self) -> anyhow::Result<&'static str> {
        Ok(match self {
            DataType::I8 => "i8",
            DataType::I16 => "i16",
            DataType::I32 => "i32",
            DataType::I64 => "i64",
            DataType::U8 => "u8",
            DataType::U16 => "u16",
            DataType::U32 => "u32",
            DataType::U64 => "u64",
            DataType::F32 => "f32",
            DataType::F64 => "f64",
            DataType::Bool => "bool",
            DataType::RustString | DataType::ExtString => "String",
            DataType::Object => return Err(anyhow!("Cannot derive type name from 'Object'")),
            DataType::RustError | DataType::ExtError => return Err(anyhow!("Error is not a valid param type")),
            DataType::Void => "void",
            DataType::Vec2 => "Vec2",
            DataType::Vec3 => "Vec3",
            DataType::RustVec4 | DataType::ExtVec4 => "Vec4",
            DataType::RustQuat | DataType::ExtQuat => "Quat",
            DataType::RustMat4 | DataType::ExtMat4 => "Mat4",
        })
    }
}


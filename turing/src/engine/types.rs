use crate::interop::params::{DataType, FfiParam, FfiParamArray};
use anyhow::anyhow;
use convert_case::{Case, Casing};

pub type ScriptCallback = extern "C" fn(FfiParamArray) -> FfiParam;

// Represents the name of a type used in parameter or return type lists
pub type DataTypeName = String;

#[derive(Clone)]
pub struct ScriptFnParameter {
    pub name: String,
    pub data_type: DataType,
    pub data_type_name: DataTypeName,
}

#[derive(Clone)]
pub struct ScriptFnMetadata {
    pub capability: String,
    pub callback: ScriptCallback,
    pub param_types: Vec<ScriptFnParameter>,
    pub return_type: Vec<(DataType, DataTypeName)>,
    pub doc_comment: Option<String>,
}

impl ScriptFnMetadata {
    pub fn new(
        capability: String,
        callback: ScriptCallback,
        doc_comment: Option<String>,
    ) -> Self {
        Self {
            capability,
            callback,
            param_types: Vec::new(),
            return_type: Vec::new(),
            doc_comment,
        }
    }

    /// May error if DataType is not a valid parameter type
    pub fn add_param_type(
        &mut self,
        p: DataType,
        param_name: impl ToString,
    ) -> anyhow::Result<&mut Self> {
        if !p.is_valid_param_type() {
            return Err(anyhow!("DataType '{}' is not a valid parameter type", p));
        }
        self.param_types.push(ScriptFnParameter {
            name: param_name.to_string(),
            data_type: p,
            data_type_name: p.as_spec_param_type()?.to_string(),
        });

        Ok(self)
    }

    /// May error if DataType is not a valid parameter type
    pub fn add_param_type_named(
        &mut self,
        p: DataType,
        param_name: String,
        type_name: String,
    ) -> anyhow::Result<&mut Self> {
        if !p.is_valid_param_type() {
            return Err(anyhow!("DataType '{}' is not a valid parameter type", p));
        }
        self.param_types.push(ScriptFnParameter {
            name: param_name,
            data_type: p,
            data_type_name: type_name,
        });

        Ok(self)
    }

    /// May error if DataType is not a valid return type
    pub fn add_return_type(&mut self, r: DataType) -> anyhow::Result<&mut Self> {
        if !r.is_valid_return_type() {
            return Err(anyhow!("DataType '{}' is not a valid return type", r));
        }
        self.return_type.push((r, r.as_spec_return_type()?.to_string()));
        Ok(self)
    }

    /// May error if DataType is not a valid return type
    pub fn add_return_type_named(
        &mut self,
        r: DataType,
        type_name: String,
    ) -> anyhow::Result<&mut Self> {
        if !r.is_valid_return_type() {
            return Err(anyhow!("DataType '{}' is not a valid return type", r));
        }
        self.return_type.push((r, type_name));
        Ok(self)
    }

    /// Determines if function is an instance method
    pub fn is_instance_method(fn_name: &str) -> bool {
        fn_name.contains(".")
    }

    /// Determines if function is a static method
    pub fn is_static_method(fn_name: &str) -> bool {
        !Self::is_instance_method(fn_name) && fn_name.contains("::")
    }

    /// Converts function name to internal representation
    /// e.g. `Class::functionName` becomes `capability_class_function_name`
    pub fn as_internal_name(&self, fn_name: &str) -> String {
        format!(
            "_{}_{}",
            self.capability.to_case(Case::Snake),
            fn_name.to_case(Case::Snake).replace("::", "__").replace(".", "__")
        )
    }
}

impl DataType {
    /// Returns the corresponding type name for a DataType when used as a parameter type in the function spec
    /// The function spec uses Rust type names for simplicity, but some DataTypes like String and Object require special handling
    /// 
    /// This is not the same type name used in the engine's internal type system. For example, both `RustString` and `ExtString` are represented as `&str` in the function spec,
    /// but they are handled uniquely in the engine's internal type system.
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
            DataType::RustError | DataType::ExtError => {
                return Err(anyhow!("Error is not a valid param type"));
            }
            DataType::Void => return Err(anyhow!("Void is not a valid param type")),
            DataType::Vec2 => "Vec2",
            DataType::Vec3 => "Vec3",
            DataType::RustVec4 | DataType::ExtVec4 => "Vec4",
            DataType::RustQuat | DataType::ExtQuat => "Quat",
            DataType::RustMat4 | DataType::ExtMat4 => "Mat4",
            DataType::RustU32Buffer | DataType::ExtU32Buffer => "&Vu32"
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
            DataType::RustError | DataType::ExtError => {
                return Err(anyhow!("Error is not a valid param type"));
            }
            DataType::Void => "void",
            DataType::Vec2 => "Vec2",
            DataType::Vec3 => "Vec3",
            DataType::RustVec4 | DataType::ExtVec4 => "Vec4",
            DataType::RustQuat | DataType::ExtQuat => "Quat",
            DataType::RustMat4 | DataType::ExtMat4 => "Mat4",
            DataType::RustU32Buffer | DataType::ExtU32Buffer => "Vu32"
        })
    }
}

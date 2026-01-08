use anyhow::anyhow;
use crate::interop::params::{DataType, FfiParam, FfiParamArray, Param};

pub type ScriptCallback = extern "C" fn(FfiParamArray) -> FfiParam;

extern "C" fn null_fn(_: FfiParamArray) -> FfiParam { Param::Void.into() }
fn null_ptr() -> ScriptCallback { null_fn }

#[derive(Clone)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ScriptFnMetadata {
    pub capability: String,
    #[serde(skip, default="null_ptr")]
    pub callback: ScriptCallback,
    pub param_types: Vec<DataType>,
    pub return_type: Vec<DataType>,
    pub signature: String,
    pub doc_comment: Option<String>,
}

impl ScriptFnMetadata {
    pub fn new(capability: impl ToString, callback: ScriptCallback, signature: impl ToString, doc_comment: Option<String>) -> Self {
        Self {
            capability: capability.to_string(),
            callback,
            param_types: Vec::new(),
            return_type: Vec::new(),
            signature: signature.to_string(),
            doc_comment,
        }
    }

    /// May error if DataType is not a valid parameter type
    pub fn add_param_type(&mut self, p: DataType) -> anyhow::Result<&mut Self> {
         if !p.is_valid_param_type() {
            return Err(anyhow!("DataType '{}' is not a valid parameter type", p))
        }
        self.param_types.push(p);

        Ok(self)
    }

    /// May error if DataType is not a valid return type
    pub fn add_return_type(&mut self, r: DataType) -> anyhow::Result<&mut Self> {
        if !r.is_valid_return_type() {
            return Err(anyhow!("DataType '{}' is not a valid return type", r))
        }
        self.return_type.push(r);
        Ok(self)
    }

}
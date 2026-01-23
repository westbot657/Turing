use anyhow::anyhow;
use crate::interop::params::{DataType, FfiParam, FfiParamArray};

pub type ScriptCallback = extern "C" fn(FfiParamArray) -> FfiParam;


#[derive(Clone)]
pub struct ScriptFnMetadata {
    pub capability: Option<String>,
    pub callback: ScriptCallback,
    pub param_types: Vec<DataType>,
    pub return_type: Vec<DataType>,
    pub signature: String,
    pub doc_comment: Option<String>,
}

impl ScriptFnMetadata {
    pub fn new(capability: Option<String>, callback: ScriptCallback, signature: String, doc_comment: Option<String>) -> Self {
        Self {
            capability,
            callback,
            param_types: Vec::new(),
            return_type: Vec::new(),
            signature,
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
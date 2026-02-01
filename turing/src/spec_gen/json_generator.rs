use convert_case::{Case, Casing};
use rustc_hash::FxHashMap;
use serde::Serialize;

use crate::{engine::types::{DataTypeName, ScriptFnMetadata}, interop::{params::DataType, types::Semver}};
use anyhow::Result;

#[derive(Debug, Serialize)]
pub struct SpecClass {
    pub is_opaque: bool,
    pub capability: String,
    pub api_version: Option<Semver>,
    
    pub functions: Vec<SpecMethod>,
}

#[derive(Debug, Serialize)]
pub struct SpecMethod {
    pub name: String,
    pub internal_name: String,
    pub doc_comment: Option<String>,

    pub return_type: DataType,
    pub return_type_name: Option<DataTypeName>,
    pub param_types: Vec<SpecParam>,

    pub is_instance_method: bool,
    pub is_static_method: bool,
}

#[derive(Debug, Serialize)]
pub struct SpecParam {
    pub name: String,
    pub data_type_name: DataTypeName,
    pub data_type: DataType,
}

#[derive(Debug, Serialize)]
pub struct SpecMap {
    pub specs: FxHashMap<String, SpecClass>,
}

pub fn generate_specs_json(
    metadata: &FxHashMap<String, ScriptFnMetadata>,
    api_versions: &FxHashMap<String, Semver>,
) -> Result<SpecMap> {
    let mut specs = FxHashMap::default();

    for (name, data) in metadata {
        let class_name;
        let func_name;
        let mut is_opaque = false;

        if ScriptFnMetadata::is_instance_method(name) { // methods
            let names = name.splitn(2, ".").collect::<Vec<&str>>();
            class_name = names[0].to_case(Case::Pascal);
            func_name = names[1].to_case(Case::Snake);
            is_opaque = true;
        } else if ScriptFnMetadata::is_static_method(name) { // functions
            let names = name.splitn(2, "::").collect::<Vec<&str>>();
            class_name = names[0].to_case(Case::Pascal);
            func_name = names[1].to_case(Case::Snake);
        } else { // globals
            class_name = "Global".to_string();
            func_name = name.to_case(Case::Snake);
        }

        let spec_class = specs.entry(class_name.clone()).or_insert(SpecClass {
            is_opaque: false,
            functions: Vec::new(),
            capability: data.capability.clone(),
            api_version: api_versions.get(&data.capability).cloned(),
        });

        if name.contains(".") {
            spec_class.is_opaque |= is_opaque;
        }

        let is_instance_method = ScriptFnMetadata::is_instance_method(name);
        let is_static_method = !is_instance_method && ScriptFnMetadata::is_static_method(name);
        spec_class.functions.push(SpecMethod {
            name: func_name,
            internal_name: data.as_internal_name(name),
            doc_comment: data.doc_comment.clone(),
            
            is_instance_method,
            is_static_method,

            return_type: data.return_type.first().map_or(DataType::Void, |v| v.0),
            return_type_name: data.return_type.first().map(|v| v.1.clone()),
            param_types: data.param_types.iter().map(|p| SpecParam { 
                name: p.name.clone(),
                data_type_name: p.data_type_name.clone(),
                data_type: p.data_type,
             }).collect(),
        });
    }

    Ok(SpecMap { specs })
}

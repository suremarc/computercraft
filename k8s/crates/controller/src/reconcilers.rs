use k8s_openapi::{
    api::core::v1::ObjectReference, apimachinery::pkg::apis::meta::v1::OwnerReference,
};

use super::{Error, Result};

pub mod cluster;
pub mod gateway;

pub(crate) fn owner_ref_from_object_ref(object_ref: &ObjectReference) -> Result<OwnerReference> {
    Ok(OwnerReference {
        api_version: object_ref
            .api_version
            .clone()
            .ok_or_else(|| Error::MissingField)?,
        kind: object_ref.kind.clone().ok_or_else(|| Error::MissingField)?,
        name: object_ref.name.clone().ok_or_else(|| Error::MissingField)?,
        uid: object_ref.uid.clone().ok_or_else(|| Error::MissingField)?,
        ..Default::default()
    })
}

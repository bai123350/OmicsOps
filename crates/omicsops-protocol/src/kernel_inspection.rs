//! Ephemeral, content-free metadata for an already running kernel.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::KernelLanguageV4;

pub const KERNEL_INSPECTION_MAX_OBJECTS: usize = 64;
pub const KERNEL_INSPECTION_MAX_BINDINGS: usize = 4_096;
pub const KERNEL_INSPECTION_MAX_RESPONSE_BYTES: usize = 16 * 1_024;
pub const KERNEL_INSPECTION_MAX_NAME_BYTES: usize = 256;
pub const KERNEL_INSPECTION_MAX_SUMMARY_BYTES: usize = 128;
pub const KERNEL_INSPECTION_MAX_TYPE_LABEL_BYTES: usize = 128;
pub const KERNEL_INSPECTION_MAX_DIMENSIONS: usize = 8;
pub const KERNEL_INSPECTION_TIMEOUT_SECONDS: u64 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectKernelRequestV4 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub run_id: Uuid,
    pub language: KernelLanguageV4,
    pub expected_session_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KernelInspectionStatusV4 {
    Ready,
    NotStarted,
    Busy,
    StaleSession,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ObjectWire")]
pub struct KernelObjectMetadataV4 {
    pub name: String,
    pub type_label: String,
    pub dimensions: Option<Vec<u64>>,
    pub element_count: Option<u64>,
    pub summary: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ObjectWire {
    name: String,
    type_label: String,
    dimensions: Option<Vec<u64>>,
    element_count: Option<u64>,
    summary: String,
}

impl TryFrom<ObjectWire> for KernelObjectMetadataV4 {
    type Error = KernelInspectionValidationErrorV4;
    fn try_from(value: ObjectWire) -> Result<Self, Self::Error> {
        let object = Self {
            name: value.name,
            type_label: value.type_label,
            dimensions: value.dimensions,
            element_count: value.element_count,
            summary: value.summary,
        };
        validate_object(&object)?;
        Ok(object)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PayloadWire")]
pub struct KernelInspectionPayloadV4 {
    pub objects: Vec<KernelObjectMetadataV4>,
    pub truncated: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PayloadWire {
    objects: Vec<KernelObjectMetadataV4>,
    truncated: bool,
}

impl TryFrom<PayloadWire> for KernelInspectionPayloadV4 {
    type Error = KernelInspectionValidationErrorV4;
    fn try_from(value: PayloadWire) -> Result<Self, Self::Error> {
        let payload = Self {
            objects: value.objects,
            truncated: value.truncated,
        };
        validate_inspection_payload(&payload)?;
        Ok(payload)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "InspectionWire")]
pub struct KernelInspectionV4 {
    pub project_id: Uuid,
    pub conversation_id: Uuid,
    pub run_id: Uuid,
    pub language: KernelLanguageV4,
    pub session_id: Option<Uuid>,
    pub status: KernelInspectionStatusV4,
    pub objects: Vec<KernelObjectMetadataV4>,
    pub truncated: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectionWire {
    project_id: Uuid,
    conversation_id: Uuid,
    run_id: Uuid,
    language: KernelLanguageV4,
    session_id: Option<Uuid>,
    status: KernelInspectionStatusV4,
    objects: Vec<KernelObjectMetadataV4>,
    truncated: bool,
}

impl TryFrom<InspectionWire> for KernelInspectionV4 {
    type Error = KernelInspectionValidationErrorV4;
    fn try_from(value: InspectionWire) -> Result<Self, Self::Error> {
        let response = Self {
            project_id: value.project_id,
            conversation_id: value.conversation_id,
            run_id: value.run_id,
            language: value.language,
            session_id: value.session_id,
            status: value.status,
            objects: value.objects,
            truncated: value.truncated,
        };
        response.validate()?;
        Ok(response)
    }
}

impl KernelInspectionV4 {
    /// The host calls this after adding the verified UI scope to a worker payload.
    pub fn validate(&self) -> Result<(), KernelInspectionValidationErrorV4> {
        validate_objects(&self.objects)?;
        validate_size(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum KernelInspectionValidationErrorV4 {
    #[error("too many kernel objects")]
    TooManyObjects,
    #[error("kernel inspection exceeds the response budget")]
    TooLarge,
    #[error("invalid kernel object dimensions")]
    InvalidDimension,
    #[error("invalid kernel metadata text")]
    InvalidText,
}

fn validate_object(
    object: &KernelObjectMetadataV4,
) -> Result<(), KernelInspectionValidationErrorV4> {
    for (text, limit, required) in [
        (&object.name, KERNEL_INSPECTION_MAX_NAME_BYTES, true),
        (
            &object.type_label,
            KERNEL_INSPECTION_MAX_TYPE_LABEL_BYTES,
            true,
        ),
        (&object.summary, KERNEL_INSPECTION_MAX_SUMMARY_BYTES, false),
    ] {
        if text.len() > limit || (required && text.is_empty()) || text.chars().any(char::is_control)
        {
            return Err(KernelInspectionValidationErrorV4::InvalidText);
        }
    }
    if object
        .dimensions
        .as_ref()
        .is_some_and(|dimensions| dimensions.len() > KERNEL_INSPECTION_MAX_DIMENSIONS)
    {
        return Err(KernelInspectionValidationErrorV4::InvalidDimension);
    }
    Ok(())
}

fn validate_objects(
    objects: &[KernelObjectMetadataV4],
) -> Result<(), KernelInspectionValidationErrorV4> {
    if objects.len() > KERNEL_INSPECTION_MAX_OBJECTS {
        return Err(KernelInspectionValidationErrorV4::TooManyObjects);
    }
    objects.iter().try_for_each(validate_object)
}

fn validate_size(value: &impl Serialize) -> Result<(), KernelInspectionValidationErrorV4> {
    if serde_json::to_vec(value)
        .map_err(|_| KernelInspectionValidationErrorV4::TooLarge)?
        .len()
        > KERNEL_INSPECTION_MAX_RESPONSE_BYTES
    {
        return Err(KernelInspectionValidationErrorV4::TooLarge);
    }
    Ok(())
}

pub fn validate_inspection_payload(
    payload: &KernelInspectionPayloadV4,
) -> Result<(), KernelInspectionValidationErrorV4> {
    validate_objects(&payload.objects)?;
    validate_size(payload)
}
